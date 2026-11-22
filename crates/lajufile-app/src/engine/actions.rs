use super::*;

impl EngineState {
    pub(super) fn start_download(&mut self, download: &Download) -> bool {
        if self.active.contains_key(&download.id) {
            return false;
        }
        let url = match Url::parse(&download.url) {
            Ok(url) => url,
            Err(error) => {
                let _ = self.store.update_status(
                    download.id,
                    DownloadStatus::Error,
                    Some(&format!("invalid URL: {error}")),
                );
                self.cache_dirty = true;
                return false;
            }
        };

        let cookie = match download.cookie_enc.as_deref() {
            Some(encrypted) => match unprotect_cookie(encrypted) {
                Ok(cookie) => Some(cookie),
                Err(error) => {
                    let _ = self.store.update_status(
                        download.id,
                        DownloadStatus::Error,
                        Some(&format!("cookie: {error}")),
                    );
                    self.cache_dirty = true;
                    return false;
                }
            },
            None => None,
        };

        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.clear_pending(download.id);
        if self
            .store
            .update_status(download.id, DownloadStatus::Downloading, None)
            .is_err()
        {
            return false;
        }
        self.cache_dirty = true;

        if download.kind == "video" {
            return self.spawn_video(download, generation, cookie);
        }
        let authorization = self.hf_authorization_for(&url);
        let options = RequestOptions {
            user_agent: download
                .user_agent
                .clone()
                .or_else(|| Some(DEFAULT_USER_AGENT.to_string())),
            referrer: download.referrer.clone(),
            cookie,
            authorization,
        };
        let id = download.id;
        let events = self.events.clone();
        let limiter = self.limiter.clone();
        let item_limiter = Arc::new(RateLimiter::new(download.speed_limit_kbps));
        let worker_item_limiter = Arc::clone(&item_limiter);
        let segments = self.store.segments(id).unwrap_or_default();
        let use_segmented =
            download.ranged && download.size.is_some_and(|size| size > 0) && !segments.is_empty();
        let handle = if use_segmented {
            let spec = SegmentedSpec {
                url,
                temp_path: download.temp_path.clone(),
                final_path: download.save_path.clone(),
                size: download.size.unwrap_or(0),
                segments,
                options,
                if_range: download
                    .etag
                    .clone()
                    .or_else(|| download.last_modified.clone()),
                limiter,
                item_limiter: Some(worker_item_limiter),
                retry_delays: vec![
                    Duration::from_secs(2),
                    Duration::from_secs(5),
                    Duration::from_secs(10),
                ],
            };
            let client = self.client.clone();
            self.runtime.spawn(async move {
                let progress_events = events.clone();
                let split_events = events.clone();
                let result = download_segmented_with_splits(
                    &client,
                    &spec,
                    move |idx, segment_done, total| {
                        let _ = progress_events.send(Event::SegmentProgress {
                            id,
                            generation,
                            idx,
                            segment_done,
                            total,
                        });
                    },
                    move |segments| {
                        let _ = split_events.send(Event::SegmentsSplit {
                            id,
                            generation,
                            segments,
                        });
                    },
                )
                .await;
                match result {
                    Ok(outcome) => {
                        let _ = events.send(Event::Finished {
                            id,
                            generation,
                            result: Ok(outcome),
                        });
                    }
                    Err(SegmentedError::RangeIgnored) => {
                        let _ = events.send(Event::Fallback {
                            id,
                            generation,
                            reason: "server ignored byte ranges".to_string(),
                        });
                    }
                    Err(SegmentedError::RestartRequired) => {
                        let _ = events.send(Event::Fallback {
                            id,
                            generation,
                            reason: "resume validation failed".to_string(),
                        });
                    }
                    Err(error) => {
                        let _ = events.send(Event::Finished {
                            id,
                            generation,
                            result: Err(segmented_error(error)),
                        });
                    }
                }
            })
        } else {
            let prefetched = self.prefetched_responses.remove(&id);
            let spec = DownloadSpec {
                url,
                temp_path: download.temp_path.clone(),
                final_path: download.save_path.clone(),
                expected_size: download.size,
                options,
            };
            let client = self.client.clone();
            let retry_delays = [
                Duration::from_secs(2),
                Duration::from_secs(5),
                Duration::from_secs(10),
            ];
            self.runtime.spawn(async move {
                let result = download_single_from_response_or_retry(
                    &client,
                    prefetched,
                    &spec,
                    Some(limiter),
                    Some(worker_item_limiter),
                    &retry_delays,
                    |done| {
                        let _ = events.send(Event::Progress {
                            id,
                            generation,
                            done,
                        });
                    },
                )
                .await;
                let _ = events.send(Event::Finished {
                    id,
                    generation,
                    result,
                });
            })
        };
        self.active.insert(
            id,
            ActiveDownload {
                generation,
                handle: Some(handle),
                video: None,
                item_limiter,
            },
        );
        true
    }

    pub(super) fn spawn_video(
        &mut self,
        download: &Download,
        generation: u64,
        cookie_text: Option<String>,
    ) -> bool {
        let dir = tools::tools_dir();
        if !tools::tools_ready(&dir) {
            // No download here: a test (or a fresh profile) must not hit the network.
            let _ = self.store.update_status(
                download.id,
                DownloadStatus::Error,
                Some("the video tools are not installed, install them in Settings"),
            );
            self.cache_dirty = true;
            return false;
        }
        let cookie_file = cookie_text
            .as_deref()
            .and_then(video::TempCookieFile::from_netscape);
        let js_runtime = tools::js_runtime_arg(&dir);
        let selector = download.video_format.as_deref().unwrap_or(DEFAULT_SELECTOR);
        let audio_only = is_audio_only_selector(selector);
        let started_at = std::time::SystemTime::now();
        let work_dir = video::ytdlp_work_dir(download.save_path.parent());
        let proxy = self.configured_proxy();
        let mut command = std::process::Command::new(tools::tool_path(&dir, tools::YTDLP));
        command
            .current_dir(work_dir)
            .args(video::video_args(
                selector,
                &download.save_path,
                &download.url,
                &dir,
                self.settings.video_subtitles,
                self.settings.video_chapters,
                js_runtime.as_deref(),
                cookie_file.as_ref().map(video::TempCookieFile::path),
                proxy.as_deref(),
                effective_video_speed_limit_kbps(
                    self.settings.global_speed_limit_kbps,
                    download.speed_limit_kbps,
                ),
            ))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("PYTHONUNBUFFERED", "1");
        tools::no_window(&mut command);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = self.store.update_status(
                    download.id,
                    DownloadStatus::Error,
                    Some(&format!("yt-dlp: {error}")),
                );
                self.cache_dirty = true;
                return false;
            }
        };
        let id = download.id;
        let resume_done = if download.downloaded > 0 && video::has_parts(&download.save_path) {
            download.downloaded
        } else {
            if download.downloaded > 0 {
                let _ = self.store.update_progress_batch(id, &[(0, 0)]);
                self.cache_dirty = true;
            }
            0
        };
        self.video_streams.insert(
            id,
            video::VideoStreamTracker::from_seeded(download.size, resume_done),
        );
        let stdout_join = child.stdout.take().map(|stdout| {
            let events = self.events.clone();
            thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if let Some(parsed) = video::parse_progress(&line) {
                        if video::is_auxiliary_media_file(parsed.filename.as_deref()) {
                            continue;
                        }
                        let _ = events.send(Event::VideoProgress {
                            id,
                            generation,
                            done: parsed.done,
                            total: parsed.total,
                            filename: parsed.filename,
                        });
                    }
                }
            })
        });
        let last_error = Arc::new(Mutex::new(String::new()));
        let stderr_join = child.stderr.take().map(|stderr| {
            let slot = Arc::clone(&last_error);
            thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    if let Ok(mut slot) = slot.lock() {
                        if !slot.is_empty() {
                            slot.push('\n');
                        }
                        if slot.len() < 4096 {
                            slot.push_str(line);
                        }
                    }
                }
            })
        });
        self.active.insert(
            id,
            ActiveDownload {
                generation,
                handle: None,
                video: Some(video::VideoRun {
                    child,
                    save_path: download.save_path.clone(),
                    audio_only,
                    started_at,
                    last_error,
                    stdout_join,
                    stderr_join,
                    _cookie_file: cookie_file,
                }),
                item_limiter: Arc::new(RateLimiter::new(download.speed_limit_kbps)),
            },
        );
        true
    }

    pub(super) fn poll_videos(&mut self) -> bool {
        let exited: Vec<(i64, bool)> = self
            .active
            .iter_mut()
            .filter_map(|(id, active)| {
                let status = active.video.as_mut()?.child.try_wait().ok()??;
                Some((*id, status.success()))
            })
            .collect();
        let changed = !exited.is_empty();
        for (id, success) in exited {
            let Some(mut active) = self.active.remove(&id) else {
                continue;
            };
            let Some(mut run) = active.video.take() else {
                continue;
            };
            if let Some(join) = run.stdout_join.take() {
                let _ = join.join();
            }
            if let Some(join) = run.stderr_join.take() {
                let _ = join.join();
            }
            self.flush_segment_progress(id);
            if success {
                // The row reserved its temp name before the run; a finished video leaves it empty.
                let _ = std::fs::remove_file(temp_path_for(&run.save_path));
                if let Some(actual_path) =
                    video::resolve_video_output(&run.save_path, run.audio_only, run.started_at)
                {
                    if actual_path != run.save_path
                        && let Some(actual_name) = actual_path.file_name().and_then(|n| n.to_str())
                    {
                        let actual_temp = temp_path_for(&actual_path);
                        let _ =
                            self.store
                                .update_location(id, actual_name, &actual_path, &actual_temp);
                    }
                    let actual_bytes = std::fs::metadata(&actual_path).ok().map(|m| m.len());
                    let _ = self.store.mark_video_done(id, actual_bytes, now_millis());
                    self.had_completed_in_batch = true;
                } else {
                    let _ = self.store.update_status(
                        id,
                        DownloadStatus::Error,
                        Some("yt-dlp exited without producing an output file"),
                    );
                    video::remove_parts(&run.save_path);
                    let _ = self.store.update_progress_batch(id, &[(0, 0)]);
                }
            } else {
                let raw = run
                    .last_error
                    .lock()
                    .map(|line| line.clone())
                    .unwrap_or_default();
                let message = video::format_ytdlp_error(&raw);
                let _ = self
                    .store
                    .update_status(id, DownloadStatus::Error, Some(&message));
                // Keep .part/.ytdl files so retrying resumes instead of discarding downloaded bytes.
                let _ = std::fs::remove_file(&run.save_path);
            }
            self.cache_dirty = true;
            self.video_streams.remove(&id);
            self.speeds.remove(&id);
            self.progress.remove(&id);
            self.last_persist.remove(&id);
            self.clear_pending(id);
            self.pump_queue();
            self.check_queue_complete();
        }
        changed
    }

    pub(super) fn reset_to_single(&mut self, id: i64, reason: &str) {
        let Ok(download) = self.store.get_download(id) else {
            return;
        };
        let segments = plan_segments(download.size.unwrap_or(0), false, 1);
        if self.store.replace_segments(id, &segments, 1, false).is_ok() {
            let _ = self.store.update_status(id, DownloadStatus::Queued, None);
            self.cache_dirty = true;
            self.message = Some(format!("Falling back to single stream: {reason}"));
        }
    }

    pub(super) fn pump_queue(&mut self) {
        if self.is_schedule_blocking() {
            return;
        }
        let limit = self.settings.max_concurrent.max(1) as usize;
        let mut queued: Vec<Download> = match self.store.list_downloads() {
            Ok(downloads) => downloads
                .into_iter()
                .filter(|download| {
                    download.status == DownloadStatus::Queued
                        && !self.active.contains_key(&download.id)
                })
                .collect(),
            Err(_) => return,
        };
        queued.sort_by_key(|download| download.queue_order);
        for download in queued {
            if self.active.len() >= limit {
                break;
            }
            // A row that fails before starting (bad cookie, invalid URL) must
            // not block the rest of the queue.
            self.start_download(&download);
        }
    }

    pub(super) fn pause(&mut self, id: i64) {
        self.pause_with_pump(id, true);
    }

    pub(super) fn pause_with_pump(&mut self, id: i64, pump: bool) {
        self.flush_segment_progress(id);
        if let Some(mut active) = self.active.remove(&id) {
            video::stop_video(&mut active.video);
            if let Some(handle) = active.handle.take() {
                handle.abort();
                let _ = self.runtime.block_on(handle);
            }
            self.video_streams.remove(&id);
            self.speeds.remove(&id);
            self.progress.remove(&id);
            self.last_persist.remove(&id);
        }
        self.clear_pending(id);
        if let Ok(download) = self.store.get_download(id) {
            if matches!(
                download.status,
                DownloadStatus::Downloading | DownloadStatus::Queued
            ) {
                let _ = self.store.update_status(id, DownloadStatus::Paused, None);
                self.cache_dirty = true;
            }
        }
        if pump {
            self.pump_queue();
        }
    }

    pub(super) fn resume(&mut self, id: i64) {
        if let Ok(download) = self.store.get_download(id) {
            if matches!(
                download.status,
                DownloadStatus::Paused | DownloadStatus::Error | DownloadStatus::Queued
            ) {
                let _ = self.store.update_status(id, DownloadStatus::Queued, None);
                self.cache_dirty = true;
            }
        }
        self.pump_queue();
    }

    pub(super) fn move_to_top(&mut self, id: i64) {
        if self.store.move_to_top(id).is_ok() {
            self.cache_dirty = true;
            self.message = Some("Moved download to top of queue".to_string());
        }
    }

    pub(super) fn force_start(&mut self, id: i64) {
        let _ = self.store.move_to_top(id);
        let Ok(download) = self.store.get_download(id) else {
            return;
        };
        if matches!(
            download.status,
            DownloadStatus::Paused | DownloadStatus::Error | DownloadStatus::Queued
        ) {
            let _ = self.store.update_status(id, DownloadStatus::Queued, None);
            self.cache_dirty = true;
            if let Ok(fresh) = self.store.get_download(id) {
                self.start_download(&fresh);
            }
        }
    }

    pub(super) fn set_item_speed_limit(&mut self, id: i64, kbps: u64) {
        let clamped = kbps.min(1_000_000);
        if self.store.update_speed_limit(id, clamped).is_ok() {
            if let Some(active) = self.active.get(&id) {
                active.item_limiter.set_kbps(clamped);
            }
            self.cache_dirty = true;
            self.message = Some(if clamped == 0 {
                "Download speed limit: Unlimited".to_string()
            } else {
                format!("Download speed limit: {clamped} KB/s")
            });
        }
    }

    pub(super) fn redownload(&mut self, id: i64) {
        let Ok(download) = self.store.get_download(id) else {
            return;
        };
        let cookie = match download.cookie_enc.as_deref() {
            Some(encrypted) => match unprotect_cookie(encrypted) {
                Ok(cookie) => Some(cookie),
                Err(error) => {
                    self.message = Some(format!("Redownload failed: cookie {error}"));
                    return;
                }
            },
            None => None,
        };
        let request = AddRequest {
            url: download.url,
            file_name: Some(download.file_name),
            save_dir: download.save_path.parent().map(Path::to_path_buf),
            start_now: true,
            connections: Some(download.connections.max(1)),
            referrer: download.referrer,
            user_agent: download.user_agent,
            cookie,
            video_format: download.video_format,
            video_size: download.size,
            expected_sha256: download.expected_sha256,
        };
        if let Some(text) = self.handle_add_inner(request, false) {
            self.message = Some(text);
        }
    }

    pub(super) fn update_url_and_resume(&mut self, id: i64, raw_url: String) {
        let parsed = match ssrf::validate_http_url(&raw_url) {
            Ok(url) => url,
            Err(error) => {
                self.message = Some(error);
                return;
            }
        };
        let Ok(download) = self.store.get_download(id) else {
            return;
        };
        if !matches!(
            download.status,
            DownloadStatus::Paused | DownloadStatus::Error
        ) {
            return;
        }
        if let Err(error) = self.store.update_url(id, parsed.as_str()) {
            self.message = Some(format!("Update URL failed: {error}"));
            return;
        }
        self.cache_dirty = true;
        self.message = Some("URL updated, resuming download".to_string());
        self.resume(id);
    }

    pub(super) fn move_rename(&mut self, id: i64, raw_name: String, target_dir: Option<PathBuf>) {
        let Ok(download) = self.store.get_download(id) else {
            return;
        };
        if download.status != DownloadStatus::Done {
            self.message = Some("Only completed downloads can be moved or renamed".to_string());
            return;
        }
        if !download.save_path.exists() {
            self.message = Some("Source file no longer exists on disk".to_string());
            return;
        }
        let clean_name = sanitize_file_name(raw_name.trim());
        let dir = target_dir
            .filter(|p| !p.as_os_str().is_empty())
            .or_else(|| download.save_path.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| self.settings.download_dir.clone());
        if let Err(error) = ensure_dir(&dir) {
            self.message = Some(format!("Create folder failed: {error}"));
            return;
        }
        let dest = dir.join(&clean_name);
        if dest == download.save_path {
            return;
        }
        if dest.exists() {
            self.message = Some("Destination file already exists".to_string());
            return;
        }
        if let Err(error) =
            lajufile_core::download::move_file_no_overwrite(&download.save_path, &dest)
        {
            self.message = Some(format!("Move failed: {error}"));
            return;
        }
        let temp = temp_path_for(&dest);
        if let Err(error) = self.store.update_location(id, &clean_name, &dest, &temp) {
            self.message = Some(format!("Update record failed: {error}"));
            return;
        }
        self.cache_dirty = true;
        self.message = Some(format!("Moved to {}", dest.display()));
    }

    pub(super) fn remove(&mut self, id: i64, delete_file: bool) {
        if let Some(mut active) = self.active.remove(&id) {
            video::stop_video(&mut active.video);
            if let Some(handle) = active.handle.take() {
                handle.abort();
                let _ = self.runtime.block_on(handle);
            }
        }
        let details = self
            .store
            .get_download(id)
            .ok()
            .map(|download| (download.temp_path, download.save_path, download.kind));
        if self.store.delete_download(id).is_ok() {
            self.cache_dirty = true;
            if let Some((temp_path, save_path, kind)) = details {
                let _ = std::fs::remove_file(&temp_path);
                if kind == "video" {
                    video::remove_parts(&save_path);
                }
                if delete_file {
                    let _ = std::fs::remove_file(&save_path);
                }
            }
        }
        self.video_streams.remove(&id);
        self.speeds.remove(&id);
        self.progress.remove(&id);
        self.last_persist.remove(&id);
        self.clear_pending(id);
        self.pump_queue();
    }

    pub(super) fn remove_completed(&mut self) {
        let completed: Vec<i64> = self
            .store
            .list_downloads()
            .unwrap_or_default()
            .into_iter()
            .filter(|download| download.status == DownloadStatus::Done)
            .map(|download| download.id)
            .collect();
        for id in completed {
            if self.store.delete_download(id).is_ok() {
                self.cache_dirty = true;
            }
        }
    }

    pub(super) fn pause_all(&mut self) {
        self.had_completed_in_batch = false;
        let ids: Vec<i64> = self.active.keys().copied().collect();
        let queued: Vec<i64> = self
            .store
            .list_downloads()
            .unwrap_or_default()
            .into_iter()
            .filter(|download| download.status == DownloadStatus::Queued)
            .map(|download| download.id)
            .collect();
        for id in ids.into_iter().chain(queued) {
            self.pause_with_pump(id, false);
        }
    }

    pub(super) fn resume_all(&mut self) {
        let paused: Vec<i64> = self
            .store
            .list_downloads()
            .unwrap_or_default()
            .into_iter()
            .filter(|download| {
                matches!(
                    download.status,
                    DownloadStatus::Paused | DownloadStatus::Error | DownloadStatus::Queued
                )
            })
            .map(|download| download.id)
            .collect();
        for id in paused {
            if self
                .store
                .update_status(id, DownloadStatus::Queued, None)
                .is_ok()
            {
                self.cache_dirty = true;
            }
        }
        self.pump_queue();
    }

    pub(super) fn update_settings(&mut self, mut settings: Settings) {
        settings.max_concurrent = settings.max_concurrent.clamp(1, 10);
        settings.max_connections = settings.max_connections.clamp(1, 32);
        settings.global_speed_limit_kbps = settings.global_speed_limit_kbps.min(1_000_000);
        settings.bridge_port = settings.bridge_port.clamp(1024, 65_535);
        settings.proxy_url = settings.proxy_url.trim().to_string();
        settings.last_update_check_ms = settings
            .last_update_check_ms
            .max(self.settings.last_update_check_ms);
        settings.browser_paired |= self.settings.browser_paired;
        if !is_valid_token(&settings.bridge_token) {
            settings.bridge_token = generate_token();
        }
        if settings.proxy_url != self.settings.proxy_url {
            match build_client_with_proxy(
                Duration::from_secs(10),
                Duration::from_secs(30),
                (!settings.proxy_url.is_empty()).then_some(settings.proxy_url.as_str()),
            ) {
                Ok(client) => self.client = client,
                Err(error) => {
                    self.message = Some(format!("Settings save failed: {error}"));
                    return;
                }
            }
        }
        let schedule_changed = settings.schedule_enabled != self.settings.schedule_enabled
            || settings.schedule_start != self.settings.schedule_start
            || settings.schedule_stop != self.settings.schedule_stop;
        match settings.save(&self.data_dir) {
            Ok(()) => {
                self.limiter.set_kbps(settings.global_speed_limit_kbps);
                if let Ok(mut token) = self.bridge_token.write() {
                    *token = settings.bridge_token.clone();
                }
                self.settings = settings;
                if schedule_changed {
                    self.schedule_was_active = None;
                    self.last_schedule_check = Instant::now() - Duration::from_secs(10);
                    self.tick_scheduler();
                }
                self.pump_queue();
            }
            Err(error) => {
                self.message = Some(format!("Settings save failed: {error}"));
            }
        }
    }

    pub(super) fn shutdown(&mut self) {
        if let Some(bridge) = self.bridge.take() {
            bridge.shutdown();
        }
        for (_, pending) in std::mem::take(&mut self.pending_captures) {
            let _ = pending.reply.send(BridgeAddOutcome::Canceled);
        }
        let active: Vec<i64> = self.active.keys().copied().collect();
        for id in active {
            self.flush_segment_progress(id);
            if let Some(mut download) = self.active.remove(&id) {
                video::stop_video(&mut download.video);
                if let Some(handle) = download.handle.take() {
                    handle.abort();
                }
                let _ = self.store.update_status(id, DownloadStatus::Paused, None);
                self.cache_dirty = true;
            }
            self.clear_pending(id);
        }
    }
}

fn effective_video_speed_limit_kbps(global_kbps: u64, item_kbps: u64) -> u64 {
    match (global_kbps, item_kbps) {
        (0, item) => item,
        (global, 0) => global,
        (global, item) => global.min(item),
    }
}

fn segmented_error(error: SegmentedError) -> DownloadError {
    match error {
        SegmentedError::Download(error) => error,
        SegmentedError::InvalidSegments(message) => {
            DownloadError::Io(std::io::Error::other(message))
        }
        SegmentedError::RangeIgnored | SegmentedError::RestartRequired => {
            DownloadError::Io(std::io::Error::other("segmented fallback was not handled"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::effective_video_speed_limit_kbps;

    #[test]
    fn video_speed_limit_uses_lowest_nonzero_cap() {
        assert_eq!(effective_video_speed_limit_kbps(0, 0), 0);
        assert_eq!(effective_video_speed_limit_kbps(0, 512), 512);
        assert_eq!(effective_video_speed_limit_kbps(1024, 0), 1024);
        assert_eq!(effective_video_speed_limit_kbps(1024, 256), 256);
        assert_eq!(effective_video_speed_limit_kbps(256, 1024), 256);
    }
}
