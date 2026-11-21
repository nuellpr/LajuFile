use super::*;

impl LajuFileApp {
    pub(super) fn handle_add(&mut self, request: AddRequest) {
        let prompt = self
            .snapshot
            .as_ref()
            .map(|snapshot| should_prompt_for_folder(&snapshot.settings, true))
            .unwrap_or(false);
        if prompt && request.save_dir.is_none() {
            self.pending_add = Some(request);
            self.folder_picker = Some(FolderPicker::new(self.default_save_dir()));
            return;
        }
        self.send(Command::Add(request));
    }

    pub(super) fn handle_actions(&mut self, ctx: &egui::Context, actions: Vec<Action>) {
        for action in actions {
            match action {
                Action::Select(id) => {
                    self.selected = Some(id);
                    self.selected_ids.clear();
                    self.selected_ids.insert(id);
                }
                Action::SelectRow { id, ctrl, shift } => {
                    let visible_ids: Vec<i64> = self
                        .snapshot
                        .as_ref()
                        .map(|snapshot| {
                            ui::main_window::visible_downloads(self, &snapshot.downloads)
                                .into_iter()
                                .map(|d| d.id)
                                .collect()
                        })
                        .unwrap_or_default();
                    apply_row_click(
                        &visible_ids,
                        &mut self.selected,
                        &mut self.selected_ids,
                        id,
                        ctrl,
                        shift,
                    );
                }
                Action::SelectAll => {
                    let visible_ids: Vec<i64> = self
                        .snapshot
                        .as_ref()
                        .map(|snapshot| {
                            ui::main_window::visible_downloads(self, &snapshot.downloads)
                                .into_iter()
                                .map(|d| d.id)
                                .collect()
                        })
                        .unwrap_or_default();
                    self.selected_ids = visible_ids.iter().copied().collect();
                    if self.selected.is_none()
                        || !self
                            .selected
                            .is_some_and(|id| self.selected_ids.contains(&id))
                    {
                        self.selected = visible_ids.first().copied();
                    }
                }
                Action::Pause(id) => self.send(Command::Pause(id)),
                Action::Resume(id) => self.send(Command::Resume(id)),
                Action::MoveToTop(id) => self.send(Command::MoveToTop(id)),
                Action::ForceStart(id) => self.send(Command::ForceStart(id)),
                Action::SetItemSpeedLimit { id, kbps } => {
                    self.send(Command::SetItemSpeedLimit { id, kbps });
                }
                Action::CancelQueueComplete => self.send(Command::CancelQueueComplete),
                Action::UpdateUrlAndResume { id, url } => {
                    self.send(Command::UpdateUrlAndResume { id, url });
                }
                Action::MoveRename { id, file_name, dir } => {
                    self.send(Command::MoveRename { id, file_name, dir });
                    self.properties_id = None;
                }
                Action::BrowseMoveDir(id) => {
                    let start = self
                        .path_for(id)
                        .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
                        .unwrap_or_else(|| self.default_save_dir());
                    self.folder_picker_target = Some(FolderTarget::MoveDownload);
                    self.folder_picker = Some(FolderPicker::new(start));
                }
                Action::BrowseCategoryDir(cat) => {
                    let start = self
                        .settings_draft
                        .category_dirs
                        .get(&cat)
                        .cloned()
                        .unwrap_or_else(|| self.settings_draft.download_dir.clone());
                    self.folder_picker_target = Some(FolderTarget::Category(cat));
                    self.folder_picker = Some(FolderPicker::new(start));
                }
                Action::VerifyChecksum(id) => {
                    if let Some(path) = self.path_for(id) {
                        let (tx, rx) = mpsc::channel();
                        std::thread::spawn(move || {
                            let res = sha256_file(&path).map_err(|e| e.to_string());
                            let _ = tx.send(res);
                        });
                        self.checksum_rx = Some((id, rx));
                        self.checksum_result = None;
                    }
                }
                Action::CopyChecksum(hash) => {
                    self.last_clipboard = Some(hash.clone());
                    ctx.copy_text(hash);
                    self.toast = Some(("SHA-256 copied".to_string(), Instant::now()));
                }
                Action::ConfirmRemove(ids, delete_file) => {
                    if !ids.is_empty() {
                        self.delete_confirm = Some((ids, delete_file));
                    }
                }
                Action::RemoveWithFile { ids, delete_file } => {
                    for id in ids {
                        self.remove_download(id, delete_file);
                    }
                }
                Action::RemoveCompleted => {
                    self.send(Command::RemoveCompleted);
                    self.completed_id = None;
                }
                Action::ShowProgress(id) => {
                    self.progress_id = Some(id);
                    if self.hidden_progress_id == Some(id) {
                        self.hidden_progress_id = None;
                    }
                }
                Action::HideProgress(id) => {
                    if self.progress_id == Some(id) {
                        self.progress_id = None;
                    }
                    self.hidden_progress_id = Some(id);
                }
                Action::OpenUpdateChecker => {
                    self.show_update_dialog = true;
                    if matches!(
                        self.update_state,
                        UpdateUiState::Idle | UpdateUiState::UpToDate(_) | UpdateUiState::Failed(_)
                    ) {
                        self.start_update_check();
                    }
                }
                Action::CheckUpdatesNow => {
                    self.start_update_check();
                }
                Action::StartSelfUpdate { url, file_name } => {
                    if let UpdateUiState::Available(info) = &self.update_state {
                        let checksums_url = info.checksums_url.clone();
                        let proxy = self.configured_proxy();
                        self.update_state = UpdateUiState::Downloading {
                            info: info.clone(),
                            downloaded: 0,
                            total: info.asset_size,
                        };
                        let (tx, rx) = mpsc::channel();
                        self.update_rx = Some(rx);
                        updater::spawn_download(url, file_name, checksums_url, proxy, tx);
                    }
                }
                Action::OpenProperties(id) => {
                    self.open_properties(id);
                }
                Action::Redownload(id) => {
                    self.send(Command::Redownload(id));
                    self.toast = Some(("Redownload queued".to_string(), Instant::now()));
                }
                Action::PauseAll => self.send(Command::PauseAll),
                Action::ResumeAll => self.send(Command::ResumeAll),
                Action::Exit => {
                    self.allow_exit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Action::Open(id) => {
                    if let Some(path) = self.path_for(id) {
                        open_file(&path);
                    }
                }
                Action::OpenWith(id) => {
                    if let Some(path) = self.path_for(id) {
                        open_with(&path);
                    }
                }
                Action::OpenFolder(id) => {
                    if let Some(path) = self.path_for(id) {
                        open_folder(&path);
                    }
                }
                Action::CopyUrl(url) => {
                    self.last_clipboard = Some(url.clone());
                    ctx.copy_text(url);
                    self.toast = Some(("URL copied".to_string(), Instant::now()));
                }
                Action::CopyPath(path) => {
                    let text = path.display().to_string();
                    self.last_clipboard = Some(text.clone());
                    ctx.copy_text(text);
                    self.toast = Some(("File path copied".to_string(), Instant::now()));
                }
                Action::FocusSearch => {
                    self.search_focus = true;
                }
                Action::ExportLinks => {
                    self.export_links(ctx);
                }
                Action::OpenAdd => {
                    self.add_draft = self.fresh_add_draft();
                    self.add_focus = true;
                    self.show_add = true;
                }
                Action::OpenBatch => {
                    self.batch_draft = self.fresh_batch_draft();
                    self.show_batch = true;
                }
                Action::OpenSettings => {
                    if let Some(snapshot) = &self.snapshot {
                        self.settings_draft = snapshot.settings.clone();
                    }
                    self.show_settings = true;
                }
                Action::OpenBrowserWizard => {
                    if !self.show_settings
                        && let Some(snapshot) = &self.snapshot
                    {
                        self.settings_draft = snapshot.settings.clone();
                    }
                    self.show_settings = false;
                    self.show_browser_wizard = true;
                }
                Action::OpenExtensionFolder => {
                    if let Some(dir) = crate::tools::extension_dir() {
                        open_file(&dir);
                    } else {
                        self.toast = Some((
                            "Extension folder not found next to LajuFile".to_string(),
                            Instant::now(),
                        ));
                    }
                }
                Action::LaunchBrowser {
                    browser_name,
                    extensions_url,
                } => {
                    if let Some(browser) = crate::tools::detect_installed_browsers()
                        .into_iter()
                        .find(|b| b.name == browser_name)
                        && let Some(exe) = browser.executable
                    {
                        let _ = std::process::Command::new(exe).spawn();
                    }
                    self.toast = Some((
                        format!("Quick-Pair JSON copied — open {extensions_url} in {browser_name}"),
                        Instant::now(),
                    ));
                }
                Action::OpenLanguagePicker => {
                    self.show_language_picker = true;
                }
                Action::SetLanguage(code) => {
                    self.language = code.clone();
                    self.settings_draft.language = code.clone();
                    crate::i18n::install_system_fonts_for_language(ctx, &code);
                    crate::i18n::set_ctx_lang(ctx, &code);
                    if let Some(snapshot) = &self.snapshot {
                        let mut settings = snapshot.settings.clone();
                        settings.language = code;
                        self.send(Command::UpdateSettings(settings));
                    }
                    let eff = crate::i18n::effective_code(&self.language);
                    let name = crate::i18n::find_language(&eff)
                        .map(|info| info.native_name)
                        .unwrap_or("English");
                    self.toast = Some((format!("Language: {name}"), Instant::now()));
                }
                Action::Add(request) => self.handle_add(request),
                Action::AddBatch(requests) => {
                    let count = requests.len();
                    for request in requests {
                        self.send(Command::Add(request));
                    }
                    if count > 0 {
                        self.toast =
                            Some((format!("Queued {count} batch downloads"), Instant::now()));
                    }
                }
                Action::LoadFormats(url) => {
                    let (reply, rx) = tokio::sync::oneshot::channel();
                    self.send(Command::VideoFormats {
                        url,
                        cookie: None,
                        reply,
                    });
                    self.formats = Some(rx);
                }
                Action::LoadHfFiles(url) => {
                    let (reply, rx) = tokio::sync::oneshot::channel();
                    self.send(Command::LoadHfFiles { url, reply });
                    self.hf_files = Some(rx);
                }
                Action::GrabPageLinks(url) => {
                    let (reply, rx) = tokio::sync::oneshot::channel();
                    self.send(Command::GrabPageLinks { url, reply });
                    self.page_links = Some(rx);
                }
                Action::OpenFolderPicker => self.open_folder_picker(),
                Action::SaveSettings(mut settings) => {
                    if let Err(err) = lajufile_core::http::validate_proxy_url(&settings.proxy_url) {
                        self.toast = Some((err, Instant::now()));
                        return;
                    }
                    normalize_settings(&mut settings);
                    sync_autostart(settings.autostart);
                    self.settings_draft = settings.clone();
                    self.theme = settings.theme.clone();
                    self.ui_scale_pct = settings.ui_scale_pct;
                    apply_theme(ctx, &self.theme);
                    apply_scale(ctx, self.ui_scale_pct);
                    self.send(Command::UpdateSettings(settings));
                }
                Action::SetScale(pct) => {
                    let clamped = pct.clamp(80, 150);
                    self.ui_scale_pct = clamped;
                    self.settings_draft.ui_scale_pct = clamped;
                    apply_scale(ctx, clamped);
                    if let Some(snapshot) = &self.snapshot {
                        let mut settings = snapshot.settings.clone();
                        settings.ui_scale_pct = clamped;
                        self.send(Command::UpdateSettings(settings));
                    }
                    self.toast = Some((format!("Interface size: {clamped}%"), Instant::now()));
                }
                Action::SetSpeedLimit(kbps) => {
                    if let Some(snapshot) = &self.snapshot {
                        let mut settings = snapshot.settings.clone();
                        settings.global_speed_limit_kbps = kbps;
                        normalize_settings(&mut settings);
                        let applied = settings.global_speed_limit_kbps;
                        self.settings_draft.global_speed_limit_kbps = applied;
                        self.custom_speed_kbps = applied.to_string();
                        self.send(Command::UpdateSettings(settings));
                        let msg = if applied == 0 {
                            "Speed limit: Unlimited".to_string()
                        } else {
                            format!("Speed limit: {}", format_speed(Some(applied * 1024)))
                        };
                        self.toast = Some((msg, Instant::now()));
                    }
                }
                Action::SetTheme(theme) => {
                    self.theme = theme.clone();
                    apply_theme(ctx, &theme);
                    if let Some(snapshot) = &self.snapshot {
                        let mut settings = snapshot.settings.clone();
                        settings.theme = theme;
                        self.send(Command::UpdateSettings(settings));
                    }
                }
                Action::TestBridge(port) => {
                    let (tx, rx) = mpsc::channel();
                    std::thread::spawn(move || {
                        let _ = tx.send(bridge::health_check(port, Duration::from_secs(2)));
                    });
                    self.bridge_test = Some(rx);
                }
                Action::RegenerateToken => {
                    self.settings_draft.bridge_token = generate_token();
                }
                Action::UpdateTools => self.send(Command::UpdateTools),
            }
        }
    }

    pub(super) fn remove_download(&mut self, id: i64, delete_file: bool) {
        self.send(Command::RemoveWithFile { id, delete_file });
        self.selected_ids.remove(&id);
        if self.selected == Some(id) {
            self.selected = self.selected_ids.iter().next().copied();
        }
        if self.progress_id == Some(id) {
            self.progress_id = None;
        }
        if self.completed_id == Some(id) {
            self.completed_id = None;
        }
        if self.properties_id == Some(id) {
            self.properties_id = None;
            self.properties_draft = None;
        }
        self.speed_history.remove(&id);
    }

    pub(super) fn export_links(&mut self, ctx: &egui::Context) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let visible = ui::main_window::visible_downloads(self, &snapshot.downloads);
        let use_selected = self.selected_ids.len() > 1;
        let mut urls = Vec::new();
        let mut seen = HashSet::new();
        for d in visible {
            if use_selected && !self.selected_ids.contains(&d.id) {
                continue;
            }
            if seen.insert(d.url.clone()) {
                urls.push(d.url.clone());
            }
        }
        if urls.is_empty() {
            self.toast = Some(("No links to export".to_string(), Instant::now()));
            return;
        }
        let content = urls.join("\n");
        let dir = self.default_save_dir();
        let _ = paths::ensure_dir(&dir);
        let out_path = dir.join("lajufile-links.txt");
        self.last_clipboard = Some(content.clone());
        ctx.copy_text(content.clone());
        match std::fs::write(&out_path, content) {
            Ok(()) => {
                self.toast = Some((
                    format!("Exported {} links to {}", urls.len(), out_path.display()),
                    Instant::now(),
                ));
            }
            Err(err) => {
                self.toast = Some((
                    format!("Copied {} links (write failed: {err})", urls.len()),
                    Instant::now(),
                ));
            }
        }
    }

    pub(super) fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|input| input.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }
        const MAX_DROP_BYTES: u64 = 512 * 1024;
        let mut collected = Vec::new();
        for file in dropped {
            let path = file.path();
            if !path.as_os_str().is_empty() {
                let ext = path
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_ascii_lowercase());
                let allowed_ext = matches!(ext.as_deref(), Some("url" | "txt" | "ef2" | "md"));
                let size_ok = std::fs::metadata(path)
                    .map(|m| m.len() <= MAX_DROP_BYTES)
                    .unwrap_or(false);
                if allowed_ext
                    && size_ok
                    && let Ok(text) = std::fs::read_to_string(path)
                {
                    collected.extend(extract_urls_from_text(&text));
                }
            } else if let Ok(bytes) = file.bytes()
                && bytes.len() <= MAX_DROP_BYTES as usize
                && let Ok(text) = std::str::from_utf8(&bytes)
            {
                collected.extend(extract_urls_from_text(text));
            }
        }
        let mut unique = Vec::new();
        let mut seen = HashSet::new();
        for url in collected {
            if seen.insert(url.clone()) {
                unique.push(url);
            }
        }
        match unique.len() {
            0 => {
                self.toast = Some((
                    "Drop a .url shortcut or .txt link list with HTTP/HTTPS URLs".to_string(),
                    Instant::now(),
                ));
            }
            1 => {
                self.add_draft = self.fresh_add_draft();
                self.add_draft.url = unique.into_iter().next().unwrap_or_default();
                self.add_focus = true;
                self.show_add = true;
            }
            _ => {
                self.batch_draft = self.fresh_batch_draft();
                self.batch_draft.mode = BatchMode::UrlList;
                self.batch_draft.urls_text = unique.join("\n");
                self.show_batch = true;
            }
        }
    }

    pub(super) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            if self.delete_confirm.is_some() {
                self.delete_confirm = None;
                return;
            }
            if self.properties_id.is_some() {
                self.properties_id = None;
                return;
            }
            if self.show_browser_wizard {
                self.show_browser_wizard = false;
                return;
            }
            if self.show_language_picker {
                self.show_language_picker = false;
                return;
            }
            if self.show_update_dialog {
                self.show_update_dialog = false;
                return;
            }
            if self.completed_id.is_some() {
                self.completed_id = None;
                return;
            }
            if self.progress_id.is_some() {
                self.progress_id = None;
                return;
            }
            if self.show_speed_popover {
                self.show_speed_popover = false;
                return;
            }
            if !self.search_query.is_empty() {
                self.search_query.clear();
                return;
            }
        }

        let blocking_modal = self.show_add
            || self.show_batch
            || self.show_settings
            || self.show_browser_wizard
            || self.show_language_picker
            || self.show_update_dialog
            || self.delete_confirm.is_some()
            || self.properties_id.is_some()
            || self.completed_id.is_some()
            || self.progress_id.is_some()
            || self.capture_info.is_some()
            || self.folder_picker.is_some();
        if blocking_modal {
            return;
        }

        let mut actions = Vec::new();
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) {
            actions.push(Action::FocusSearch);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::N)) {
            actions.push(Action::OpenAdd);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::B)) {
            actions.push(Action::OpenBatch);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma)) {
            actions.push(Action::OpenSettings);
        }

        if !ctx.egui_wants_keyboard_input() {
            let visible_ids: Vec<i64> = self
                .snapshot
                .as_ref()
                .map(|snapshot| {
                    ui::main_window::visible_downloads(self, &snapshot.downloads)
                        .into_iter()
                        .map(|d| d.id)
                        .collect()
                })
                .unwrap_or_default();

            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::A)) {
                actions.push(Action::SelectAll);
            } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown))
            {
                if let Some(next_id) = step_selection(&visible_ids, self.selected, 1) {
                    self.selected = Some(next_id);
                    self.selected_ids.clear();
                    self.selected_ids.insert(next_id);
                }
            } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp))
                && let Some(prev_id) = step_selection(&visible_ids, self.selected, -1)
            {
                self.selected = Some(prev_id);
                self.selected_ids.clear();
                self.selected_ids.insert(prev_id);
            }

            let target_ids: Vec<i64> = if !self.selected_ids.is_empty() {
                visible_ids
                    .iter()
                    .copied()
                    .filter(|id| self.selected_ids.contains(id))
                    .collect()
            } else {
                self.selected.into_iter().collect()
            };

            if !target_ids.is_empty()
                && let Some(snapshot) = &self.snapshot
            {
                let anchor_id = self.selected.unwrap_or(target_ids[0]);
                if ctx.input_mut(|i| {
                    i.consume_key(egui::Modifiers::ALT, egui::Key::Enter)
                        || i.consume_key(egui::Modifiers::COMMAND, egui::Key::M)
                }) {
                    actions.push(Action::OpenProperties(anchor_id));
                } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
                    && let Some(anchor) = snapshot.downloads.iter().find(|d| d.id == anchor_id)
                {
                    if anchor.status == DownloadStatus::Done {
                        actions.push(Action::Open(anchor_id));
                    } else {
                        actions.push(Action::ShowProgress(anchor_id));
                    }
                } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space))
                {
                    let any_active = target_ids.iter().any(|id| {
                        snapshot.downloads.iter().any(|d| {
                            d.id == *id
                                && matches!(
                                    d.status,
                                    DownloadStatus::Downloading | DownloadStatus::Queued
                                )
                        })
                    });
                    for id in target_ids {
                        if any_active {
                            actions.push(Action::Pause(id));
                        } else {
                            actions.push(Action::Resume(id));
                        }
                    }
                } else if ctx
                    .input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::Delete))
                {
                    actions.push(Action::ConfirmRemove(target_ids, true));
                } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete))
                {
                    actions.push(Action::ConfirmRemove(target_ids, false));
                } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::C)) {
                    let urls: Vec<String> = target_ids
                        .iter()
                        .filter_map(|id| snapshot.downloads.iter().find(|d| d.id == *id))
                        .map(|d| d.url.clone())
                        .collect();
                    if !urls.is_empty() {
                        actions.push(Action::CopyUrl(urls.join("\n")));
                    }
                }
            }
        }

        if !actions.is_empty() {
            self.handle_actions(ctx, actions);
        }
    }
}
