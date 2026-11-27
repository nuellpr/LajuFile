use super::*;

impl EngineState {
    pub(super) fn check_duplicate_url(&mut self, url: &str) -> Result<DuplicateAction, StoreError> {
        if self.settings.duplicate_policy == DuplicatePolicy::RenameNumbered {
            return Ok(DuplicateAction::Proceed);
        }
        let Some(existing) = self.store.active_by_url(url)? else {
            return Ok(DuplicateAction::Proceed);
        };
        if self.settings.duplicate_policy == DuplicatePolicy::ResumeExisting {
            let message = if existing.status == DownloadStatus::Paused {
                self.resume(existing.id);
                format!("Resumed {}", existing.file_name)
            } else {
                format!("Already active: {}", existing.file_name)
            };
            return Ok(DuplicateAction::Handled {
                id: existing.id,
                file_name: existing.file_name,
                size: existing.size,
                message,
            });
        }
        Ok(DuplicateAction::Rejected(Box::new(existing)))
    }

    pub(super) fn handle_add(&mut self, request: AddRequest) -> Option<String> {
        self.handle_add_inner(request, true)
    }

    pub(super) fn handle_add_inner(
        &mut self,
        request: AddRequest,
        check_duplicate: bool,
    ) -> Option<String> {
        let url = match ssrf::validate_http_url(&request.url) {
            Ok(url) => url,
            Err(error) => return Some(format!("Add failed: {error}")),
        };
        if let Err(error) = ssrf::reject_self_bridge_url(&url, self.bridge_status.port) {
            return Some(format!("Add failed: {error}"));
        }
        if let Some(ref selector) = request.video_format
            && !lajufile_core::video::is_valid_video_selector(selector)
        {
            return Some("Add failed: invalid video_format".to_string());
        }
        let normalized_url = url.to_string();

        if check_duplicate {
            match self.check_duplicate_url(&normalized_url) {
                Ok(DuplicateAction::Proceed) => {}
                Ok(DuplicateAction::Handled { message, .. }) => return Some(message),
                Ok(DuplicateAction::Rejected(existing)) => {
                    return Some(format!("Already in LajuFile: {}", existing.file_name));
                }
                Err(error) => return Some(format!("Add failed: {error}")),
            }
        }

        if request.video_format.is_some() {
            let request = AddRequest {
                url: normalized_url,
                ..request
            };
            return Some(match self.insert_video(request, check_duplicate) {
                Ok(inserted) => format!("Added {}", inserted.file_name),
                Err(InsertError::Duplicate(existing)) => {
                    format!("Already in LajuFile: {}", existing.file_name)
                }
                Err(InsertError::Failed(error)) => format!("Add failed: {error}"),
            });
        }

        let user_agent = request
            .user_agent
            .clone()
            .unwrap_or_else(|| DEFAULT_USER_AGENT.to_string());
        let authorization = self.hf_authorization_for(&url);
        let probe_request = ProbeRequest {
            url,
            options: RequestOptions {
                user_agent: Some(user_agent),
                referrer: request.referrer.clone(),
                cookie: request.cookie.clone(),
                authorization,
            },
            timeout: Duration::from_secs(20),
        };
        let client = self.client.clone();
        let events = self.events.clone();
        self.in_flight_probes += 1;
        self.runtime.spawn(async move {
            let result = probe_with_response(&client, &probe_request).await;
            let _ = events.send(Event::Probe {
                request: AddRequest {
                    url: normalized_url,
                    ..request
                },
                check_duplicate,
                result: Box::new(result),
            });
        });
        None
    }

    pub(super) fn handle_bridge_add(
        &mut self,
        request: BridgeAddRequest,
        reply: tokio::sync::oneshot::Sender<BridgeAddOutcome>,
    ) {
        let url = match ssrf::validate_http_url(&request.url) {
            Ok(url) => url,
            Err(error) => {
                let _ = reply.send(BridgeAddOutcome::Invalid(error));
                return;
            }
        };
        if let Err(error) = ssrf::reject_self_bridge_url(&url, self.bridge_status.port) {
            let _ = reply.send(BridgeAddOutcome::Invalid(error));
            return;
        }
        let normalized_url = url.to_string();

        match self.check_duplicate_url(&normalized_url) {
            Ok(DuplicateAction::Proceed) => {}
            Ok(DuplicateAction::Handled {
                id,
                file_name,
                size,
                ..
            }) => {
                let _ = reply.send(BridgeAddOutcome::Created {
                    id,
                    filename: file_name,
                    size,
                });
                return;
            }
            Ok(DuplicateAction::Rejected(existing)) => {
                let _ = reply.send(BridgeAddOutcome::Conflict { id: existing.id });
                return;
            }
            Err(error) => {
                let _ = reply.send(BridgeAddOutcome::Internal(error.to_string()));
                return;
            }
        }

        let add = AddRequest {
            url: normalized_url,
            file_name: request.filename.clone(),
            save_dir: None,
            start_now: true,
            connections: None,
            referrer: request.referrer.clone(),
            user_agent: request.user_agent.clone(),
            cookie: request.cookie.clone(),
            video_format: request.video_format.clone(),
            video_size: request.size,
            expected_sha256: None,
        };
        if self.settings.ask_where_to_save {
            let key = self.next_capture_key;
            self.next_capture_key = self.next_capture_key.wrapping_add(1);
            let file_name = if request.video_format.is_some() {
                request
                    .filename
                    .clone()
                    .unwrap_or_else(|| video::video_file_name(&add.url))
            } else {
                resolve_file_name(request.filename.as_deref(), None, &url)
            };
            self.pending_captures.insert(
                key,
                PendingCapture {
                    request,
                    reply,
                    file_name,
                    size: None,
                    probe: None,
                    probe_response: None,
                },
            );
            self.message = Some("Waiting for a save folder".to_string());
            self.trigger_wake();
            if add.video_format.is_none() {
                let authorization = self.hf_authorization_for(&url);
                let probe_request = ProbeRequest {
                    url,
                    options: RequestOptions {
                        user_agent: add
                            .user_agent
                            .clone()
                            .or_else(|| Some(DEFAULT_USER_AGENT.to_string())),
                        referrer: add.referrer.clone(),
                        cookie: add.cookie.clone(),
                        authorization,
                    },
                    timeout: Duration::from_secs(3),
                };
                let client = self.client.clone();
                let events = self.events.clone();
                self.in_flight_probes += 1;
                self.runtime.spawn(async move {
                    let result = probe_with_response(&client, &probe_request).await;
                    let _ = events.send(Event::CaptureProbe {
                        key,
                        result: Box::new(result),
                    });
                });
            }
            return;
        }
        if add.video_format.is_some() {
            self.finish_video_add(add, reply);
            return;
        }
        self.spawn_bridge_probe(url, add, reply);
    }

    /// Replies to a `/video` request: the row exists or the caller gets the reason.
    pub(super) fn finish_video_add(
        &mut self,
        add: AddRequest,
        reply: tokio::sync::oneshot::Sender<BridgeAddOutcome>,
    ) {
        if !tools::tools_ready(&tools::tools_dir()) {
            self.ensure_tools();
            let _ = reply.send(BridgeAddOutcome::ProbeFailed(
                "the video tools are downloading, try again in a minute".to_string(),
            ));
            return;
        }
        let _ = reply.send(match self.insert_video(add, true) {
            Ok(inserted) => BridgeAddOutcome::Created {
                id: inserted.id,
                filename: inserted.file_name,
                size: inserted.size,
            },
            Err(InsertError::Duplicate(existing)) => BridgeAddOutcome::Conflict { id: existing.id },
            Err(InsertError::Failed(error)) => BridgeAddOutcome::ProbeFailed(error),
        });
    }

    pub(super) fn spawn_bridge_probe(
        &mut self,
        url: Url,
        add: AddRequest,
        reply: tokio::sync::oneshot::Sender<BridgeAddOutcome>,
    ) {
        let authorization = self.hf_authorization_for(&url);
        let probe_request = ProbeRequest {
            url,
            options: RequestOptions {
                user_agent: add
                    .user_agent
                    .clone()
                    .or_else(|| Some(DEFAULT_USER_AGENT.to_string())),
                referrer: add.referrer.clone(),
                cookie: add.cookie.clone(),
                authorization,
            },
            timeout: Duration::from_secs(3),
        };
        let client = self.client.clone();
        let events = self.events.clone();
        self.in_flight_probes += 1;
        self.runtime.spawn(async move {
            let result = probe_with_response(&client, &probe_request).await;
            let _ = events.send(Event::BridgeProbe {
                request: add,
                reply,
                result: Box::new(result),
            });
        });
    }

    pub(super) fn capture_folder_decision(&mut self, key: u64, path: Option<PathBuf>) {
        match path {
            Some(folder) => self.capture_decision(key, folder, None, true),
            None => {
                if let Some(pending) = self.pending_captures.remove(&key) {
                    let _ = pending.reply.send(BridgeAddOutcome::Canceled);
                    self.check_queue_complete();
                }
            }
        }
    }

    pub(super) fn capture_decision(
        &mut self,
        key: u64,
        folder: PathBuf,
        file_name: Option<String>,
        start_now: bool,
    ) {
        let Some(pending) = self.pending_captures.remove(&key) else {
            return;
        };
        let Ok(url) = Url::parse(&pending.request.url) else {
            let _ = pending
                .reply
                .send(BridgeAddOutcome::Invalid("invalid URL".to_string()));
            self.check_queue_complete();
            return;
        };
        let chosen_name = file_name
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .or(pending.request.filename);
        let add = AddRequest {
            url: url.to_string(),
            file_name: chosen_name,
            save_dir: Some(folder),
            start_now,
            connections: None,
            referrer: pending.request.referrer,
            user_agent: pending.request.user_agent,
            cookie: pending.request.cookie,
            video_format: pending.request.video_format,
            video_size: pending.request.size,
            expected_sha256: None,
        };
        if add.video_format.is_some() {
            self.finish_video_add(add, pending.reply);
            self.check_queue_complete();
            return;
        }
        if let Some(probe_result) = pending.probe {
            let outcome =
                match self.insert_from_probe(add, probe_result, pending.probe_response, true) {
                    Ok(inserted) => BridgeAddOutcome::Created {
                        id: inserted.id,
                        filename: inserted.file_name,
                        size: inserted.size,
                    },
                    Err(InsertError::Duplicate(existing)) => {
                        BridgeAddOutcome::Conflict { id: existing.id }
                    }
                    Err(InsertError::Failed(error)) => BridgeAddOutcome::Internal(error),
                };
            let _ = pending.reply.send(outcome);
            self.check_queue_complete();
            return;
        }
        self.spawn_bridge_probe(url, add, pending.reply);
    }

    pub(super) fn handle_probe(
        &mut self,
        request: AddRequest,
        check_duplicate: bool,
        result: Result<ProbeResponse, ProbeError>,
    ) -> String {
        let probed = match result {
            Ok(probed) => probed,
            Err(error) => return format!("Add failed: {error}"),
        };
        match self.insert_from_probe(request, probed.result, probed.response, check_duplicate) {
            Ok(inserted) => format!("Added {}", inserted.file_name),
            Err(InsertError::Duplicate(existing)) => {
                format!("Already in LajuFile: {}", existing.file_name)
            }
            Err(InsertError::Failed(error)) => format!("Add failed: {error}"),
        }
    }

    pub(super) fn resolve_save_dir(&self, category: Category, requested: Option<&Path>) -> PathBuf {
        if let Some(dir) = requested
            && !dir.as_os_str().is_empty()
        {
            return dir.to_path_buf();
        }
        if let Some(dir) = self.settings.category_dirs.get(category.label())
            && !dir.as_os_str().is_empty()
        {
            return dir.clone();
        }
        self.settings.download_dir.clone()
    }

    pub(super) fn insert_from_probe(
        &mut self,
        request: AddRequest,
        result: ProbeResult,
        prefetched_response: Option<reqwest::Response>,
        check_duplicate: bool,
    ) -> Result<Inserted, InsertError> {
        if check_duplicate && self.settings.duplicate_policy != DuplicatePolicy::RenameNumbered {
            match self.store.active_by_url(&request.url) {
                Ok(Some(existing)) => return Err(InsertError::Duplicate(Box::new(existing))),
                Ok(None) => {}
                Err(error) => return Err(InsertError::Failed(error.to_string())),
            }
        }

        let base_name = resolve_file_name(
            request.file_name.as_deref(),
            result.content_disposition.as_deref(),
            &result.final_url,
        );
        let category = Category::from_file_name(&base_name);
        let dir = self.resolve_save_dir(category, request.save_dir.as_deref());
        if let Err(error) = ensure_dir(&dir) {
            return Err(InsertError::Failed(error.to_string()));
        }

        let (final_path, temp_path) =
            reserve_download_paths(&dir, &base_name).map_err(InsertError::Failed)?;
        let reserved_temp = temp_path.clone();
        let file_name = final_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or(base_name);
        let connections = request
            .connections
            .unwrap_or(self.settings.max_connections)
            .clamp(1, 32);
        let segments = plan_segments(result.size.unwrap_or(0), result.ranged, connections);
        let size = result.size;

        let cookie_enc = match encrypt_cookie(request.cookie.as_deref()) {
            Ok(encrypted) => encrypted,
            Err(error) => {
                let _ = std::fs::remove_file(&reserved_temp);
                return Err(InsertError::Failed(error));
            }
        };
        let referrer = request.referrer.clone();
        let user_agent = request
            .user_agent
            .clone()
            .or_else(|| Some(DEFAULT_USER_AGENT.to_string()));

        let queue_order = match self.store.next_queue_order() {
            Ok(order) => order,
            Err(error) => {
                let _ = std::fs::remove_file(&reserved_temp);
                return Err(InsertError::Failed(error.to_string()));
            }
        };
        let status = if request.start_now {
            DownloadStatus::Queued
        } else {
            DownloadStatus::Paused
        };
        let expected_sha256 = request
            .expected_sha256
            .as_deref()
            .map(normalize_checksum_input)
            .filter(|h| !h.is_empty());
        let new = NewDownload {
            url: request.url,
            final_url: Some(result.final_url.to_string()),
            file_name: file_name.clone(),
            save_path: final_path,
            temp_path,
            size,
            status,
            category: Category::from_file_name(&file_name),
            kind: "http".to_string(),
            video_format: None,
            connections,
            ranged: result.ranged,
            etag: result.etag,
            last_modified: result.last_modified,
            cookie_enc,
            referrer,
            user_agent,
            queue_order,
            created_at: now_millis(),
            speed_limit_kbps: 0,
            expected_sha256,
            segments,
        };

        match self.store.insert_download(&new) {
            Ok(id) => {
                self.cache_dirty = true;
                if request.start_now
                    && !result.ranged
                    && let Some(response) = prefetched_response
                {
                    self.prefetched_responses.insert(id, response);
                }
                self.pump_queue();
                // Drop any unconsumed probe response if the row stayed queued/paused.
                self.prefetched_responses.remove(&id);
                Ok(Inserted {
                    id,
                    file_name,
                    size,
                })
            }
            Err(error) => {
                let _ = std::fs::remove_file(&reserved_temp);
                Err(InsertError::Failed(error.to_string()))
            }
        }
    }

    /// Queues a yt-dlp job: no probe, no temp file, no parallel ranges.
    pub(super) fn insert_video(
        &mut self,
        request: AddRequest,
        check_duplicate: bool,
    ) -> Result<Inserted, InsertError> {
        if check_duplicate && self.settings.duplicate_policy != DuplicatePolicy::RenameNumbered {
            match self.store.active_by_url(&request.url) {
                Ok(Some(existing)) => return Err(InsertError::Duplicate(Box::new(existing))),
                Ok(None) => {}
                Err(error) => return Err(InsertError::Failed(error.to_string())),
            }
        }

        let audio_only = request
            .video_format
            .as_deref()
            .is_some_and(is_audio_only_selector);
        let category = if audio_only {
            Category::Music
        } else {
            Category::Video
        };
        let dir = self.resolve_save_dir(category, request.save_dir.as_deref());
        if let Err(error) = ensure_dir(&dir) {
            return Err(InsertError::Failed(error.to_string()));
        }

        let base_name = video::resolve_video_file_name(
            request.file_name.as_deref(),
            &request.url,
            request.video_format.as_deref(),
        );
        // Reserve the temp name so two jobs queued before either file exists cannot
        // collide on one save path.
        let (save_path, temp_path) =
            reserve_download_paths(&dir, &base_name).map_err(InsertError::Failed)?;
        let reserved_temp = temp_path.clone();
        let file_name = save_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or(base_name);
        let cookie_enc = match encrypt_cookie(request.cookie.as_deref()) {
            Ok(encrypted) => encrypted,
            Err(error) => {
                let _ = std::fs::remove_file(&reserved_temp);
                return Err(InsertError::Failed(error));
            }
        };
        let queue_order = match self.store.next_queue_order() {
            Ok(order) => order,
            Err(error) => {
                let _ = std::fs::remove_file(&reserved_temp);
                return Err(InsertError::Failed(error.to_string()));
            }
        };
        let status = if request.start_now {
            DownloadStatus::Queued
        } else {
            DownloadStatus::Paused
        };
        let size = request.video_size.filter(|s| *s > 0);
        let new = NewDownload {
            url: request.url.clone(),
            final_url: Some(request.url),
            file_name: file_name.clone(),
            temp_path,
            save_path,
            size,
            status,
            category,
            kind: "video".to_string(),
            video_format: request.video_format,
            connections: 1,
            ranged: false,
            etag: None,
            last_modified: None,
            cookie_enc,
            referrer: request.referrer,
            user_agent: request.user_agent,
            queue_order,
            created_at: now_millis(),
            speed_limit_kbps: 0,
            expected_sha256: None,
            segments: vec![Segment {
                idx: 0,
                start: 0,
                end: 0,
                done: 0,
            }],
        };

        match self.store.insert_download(&new) {
            Ok(id) => {
                self.cache_dirty = true;
                self.pump_queue();
                Ok(Inserted {
                    id,
                    file_name,
                    size,
                })
            }
            Err(error) => {
                let _ = std::fs::remove_file(&reserved_temp);
                Err(InsertError::Failed(error.to_string()))
            }
        }
    }

    pub(super) fn update_tools(&mut self) {
        self.spawn_tools_install();
    }

    /// Downloads missing video tools once; video jobs work after it lands.
    pub(super) fn ensure_tools(&mut self) {
        let dir = tools::tools_dir();
        if tools::tools_ready(&dir) {
            return;
        }
        self.spawn_tools_install();
    }

    pub(super) fn configured_proxy(&self) -> Option<String> {
        let trimmed = self.settings.proxy_url.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    }

    pub(super) fn spawn_tools_install(&mut self) {
        if self.tools_installing {
            self.message = Some("Downloading tools...".to_string());
            return;
        }
        let dir = tools::tools_dir();
        let proxy = self.configured_proxy();
        self.tools_installing = true;
        self.message = Some("Downloading tools...".to_string());
        let commands = self.commands.clone();
        thread::spawn(move || {
            let result = tools::install_tools_with_proxy(&dir, proxy.as_deref());
            let _ = commands.send(Command::ToolsInstalled(result));
        });
    }

    /// Runs `yt-dlp -J` off the engine loop; the reply carries the title and qualities.
    pub(super) fn video_formats(
        &mut self,
        url: String,
        cookie: Option<String>,
        reply: tokio::sync::oneshot::Sender<Result<VideoListing, String>>,
    ) {
        let parsed = match ssrf::validate_http_url(&url) {
            Ok(parsed) => parsed,
            Err(error) => {
                let _ = reply.send(Err(error));
                return;
            }
        };
        if let Err(error) = ssrf::reject_self_bridge_url(&parsed, self.bridge_status.port) {
            let _ = reply.send(Err(error));
            return;
        }
        let dir = tools::tools_dir();
        if !tools::tools_ready(&dir) {
            self.ensure_tools();
            let _ = reply.send(Err(
                "the video tools are downloading, try again in a minute".to_string(),
            ));
            return;
        }
        let data_dir = self.data_dir.clone();
        let proxy = self.configured_proxy();
        thread::spawn(move || {
            let _ = reply.send(video::ytdlp_listing(
                &dir,
                &data_dir,
                &url,
                cookie.as_deref(),
                proxy.as_deref(),
            ));
        });
    }

    pub(super) fn hf_authorization_for(&self, url: &Url) -> Option<String> {
        let token = self.settings.hf_token.trim();
        (!token.is_empty() && huggingface::is_huggingface_host(url))
            .then(|| format!("Bearer {token}"))
    }

    pub(super) fn load_hf_files(
        &self,
        raw_url: String,
        reply: tokio::sync::oneshot::Sender<Result<Vec<huggingface::HfModelFile>, String>>,
    ) {
        let Some((repo_id, revision)) = huggingface::parse_hf_repo_url(&raw_url) else {
            let _ = reply.send(Err("Not a HuggingFace model repository URL".to_string()));
            return;
        };
        let client = self.client.clone();
        let token = (!self.settings.hf_token.trim().is_empty())
            .then(|| self.settings.hf_token.trim().to_string());
        self.runtime.spawn(async move {
            let result =
                huggingface::list_repo_files(&client, &repo_id, &revision, token.as_deref()).await;
            let _ = reply.send(result);
        });
    }

    pub(super) fn grab_page_links(
        &mut self,
        raw_url: String,
        reply: tokio::sync::oneshot::Sender<Result<Vec<String>, String>>,
    ) {
        let parsed = match Url::parse(raw_url.trim()) {
            Ok(url) => url,
            Err(_) => {
                let _ = reply.send(Err(
                    "Enter a valid http:// or https:// web page URL".to_string()
                ));
                return;
            }
        };
        if let Err(reason) = ssrf::validate_public_page_url(&parsed) {
            let _ = reply.send(Err(reason));
            return;
        }
        let proxy = (!self.settings.proxy_url.trim().is_empty())
            .then(|| self.settings.proxy_url.trim().to_string());
        if lajufile_core::video::is_playlist_url(parsed.as_str()) {
            let dir = tools::tools_dir();
            if !tools::tools_ready(&dir) {
                self.ensure_tools();
                let _ = reply.send(Err(
                    "the video tools are downloading, try again in a minute".to_string(),
                ));
                return;
            }
            thread::spawn(move || {
                let _ = reply.send(video::ytdlp_playlist_urls(
                    &dir,
                    parsed.as_str(),
                    proxy.as_deref(),
                ));
            });
            return;
        }
        self.runtime.spawn(async move {
            let _ = reply.send(ssrf::fetch_page_links(parsed, proxy).await);
        });
    }
}

/// Reserves a deduplicated save path and empty `.lajufile` sentinel so queued rows
/// cannot collide before download writes begin.
pub(super) fn reserve_download_paths(
    dir: &Path,
    base_name: &str,
) -> Result<(PathBuf, PathBuf), String> {
    for _ in 0..128 {
        let final_path = dedupe_path(dir, base_name);
        let candidate = temp_path_for(&final_path);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(_) => return Ok((final_path, candidate)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("Could not reserve a unique download filename".to_string())
}

pub(super) fn encrypt_cookie(raw: Option<&str>) -> Result<Option<Vec<u8>>, String> {
    match raw.map(str::trim).filter(|cookie| !cookie.is_empty()) {
        Some(cookie) => protect_cookie(cookie)
            .map(Some)
            .map_err(|error| format!("cookie: {error}")),
        None => Ok(None),
    }
}
