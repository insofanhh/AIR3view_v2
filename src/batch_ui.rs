//! Native queue UI. Worker messages never enter the active editor's event channel.
use super::*;
use crate::batch::{self, Action, Batch, State};
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Preferences {
    pub enabled: bool,
    pub auto_export: bool,
    pub title: String,
}
pub(super) struct BatchView {
    pub prefs: Preferences,
    pub active: bool,
    pub creating: bool,
    batches: Vec<Batch>,
    selected: String,
    last_poll: Instant,
    child: Option<std::process::Child>,
    owned_id: String,
    job: Option<batch::KillJob>,
    tx: mpsc::Sender<anyhow::Result<(Batch, bool)>>,
    rx: mpsc::Receiver<anyhow::Result<(Batch, bool)>>,
    excluded: std::collections::BTreeSet<String>,
    detail: String,
}
impl BatchView {
    pub fn load(data: &std::path::Path) -> Self {
        let (tx, rx) = mpsc::channel();
        let mut batches = batch::list(data);
        for b in &mut batches {
            let _ = batch::recover(data, b);
        }
        Self {
            prefs: fs::read(data.join("batch-ui.json"))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default(),
            active: batch::running(data),
            creating: false,
            selected: batches.first().map(|b| b.id.clone()).unwrap_or_default(),
            batches,
            last_poll: Instant::now(),
            child: None,
            owned_id: String::new(),
            job: None,
            tx,
            rx,
            excluded: Default::default(),
            detail: String::new(),
        }
    }
    pub fn shutdown(&mut self, data: &std::path::Path) {
        if let Some(mut child) = self.child.take() {
            if child.try_wait().ok().flatten().is_none() {
                let _ = batch::send(data, &self.owned_id, Action::Stop);
                let start = Instant::now();
                while start.elapsed() < Duration::from_secs(2) {
                    if child.try_wait().ok().flatten().is_some() {
                        return;
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                process::stop(&mut child);
            }
        }
    }
}
impl StudioApp {
    pub(super) fn poll_batch(&mut self) {
        let data = self.projects_root.parent().unwrap().to_owned();
        while let Ok(result) = self.batch_view.rx.try_recv() {
            self.batch_view.creating = false;
            match result {
                Ok((b, start)) => {
                    self.batch_view.selected = b.id.clone();
                    self.batch_view.batches.insert(0, b.clone());
                    self.history = project_store::list(&self.projects_root);
                    self.library_cache.clear();
                    self.page = Page::Batch;
                    if start {
                        self.start_batch(&b.id);
                    } else {
                        self.message("Đã tạo các dự án riêng và xếp hàng đợi.", false);
                    }
                }
                Err(e) => self.message(format!("Không tạo được hàng đợi: {e:#}"), true),
            }
        }
        if self.batch_view.last_poll.elapsed() > Duration::from_secs(1) {
            let was_active = self.batch_view.active;
            self.batch_view.active = batch::running(&data);
            self.batch_view.batches = batch::list(&data);
            if !self.batch_view.active {
                for b in &mut self.batch_view.batches {
                    let _ = batch::recover(&data, b);
                }
            }
            if let Some(child) = &mut self.batch_view.child {
                if child.try_wait().ok().flatten().is_some() {
                    self.batch_view.child = None;
                    self.batch_view.job = None;
                }
            }
            if was_active && !self.batch_view.active {
                self.history = project_store::list(&self.projects_root);
                self.library_cache.clear();
            }
            self.batch_view.last_poll = Instant::now();
        }
    }
    fn start_batch(&mut self, id: &str) {
        if self.busy || self.batch_view.active {
            self.message("Chờ tác vụ đang chạy hoàn tất hoặc tạm dừng trước.", true);
            return;
        }
        let data = self.projects_root.parent().unwrap().to_owned();
        let result = (|| -> anyhow::Result<_> {
            let b = Batch::load(&batch::root(&data, id)?.join("batch.json"))?;
            if b.items
                .iter()
                .any(|i| i.project_id == self.project.id && !i.state.complete())
            {
                if self.dirty {
                    self.save();
                    anyhow::ensure!(!self.dirty, "Chưa lưu được dự án đang mở");
                }
                self.new_project();
            }
            let path = batch::root(&data, id)?.join("settings.json");
            let mut settings = Settings::load(&path)?;
            settings.python = self.settings.python.clone();
            settings.vieneu_url = self.settings.vieneu_url.clone();
            settings.cli_path = self.settings.cli_path.clone();
            settings.cli_args = self.settings.cli_args.clone();
            settings.gemini_base = self.settings.gemini_base.clone();
            settings.gemini_key_env = self.settings.gemini_key_env.clone();
            settings.heygen_base = self.settings.heygen_base.clone();
            settings.heygen_key_env = self.settings.heygen_key_env.clone();
            settings.vieneu_key_env = self.settings.vieneu_key_env.clone();
            settings.asr_device = self.settings.asr_device.clone();
            settings.asr_compute_type = self.settings.asr_compute_type.clone();
            settings.youtube_cookie_path = self.settings.youtube_cookie_path.clone();
            settings.ffmpeg = self.settings.ffmpeg.clone();
            settings.ffprobe = self.settings.ffprobe.clone();
            settings.ytdlp = self.settings.ytdlp.clone();
            settings.youtube_node = self.settings.youtube_node.clone();
            settings.rhubarb = self.settings.rhubarb.clone();
            write_json(&path, &settings)?;
            batch::start(&data, id)
        })();
        match result {
            Ok((child, job)) => {
                self.batch_view.child = Some(child);
                self.batch_view.job = Some(job);
                self.batch_view.owned_id = id.into();
                self.batch_view.active = true;
                self.page = Page::Batch;
                self.message(
                    "Đang xử lý lần lượt từng URL; có thể xem dự án đã hoàn thành.",
                    false,
                );
            }
            Err(e) => self.message(format!("{e:#}"), true),
        }
    }
    pub(super) fn enqueue_batch(&mut self, start: bool) {
        if self.busy || self.batch_view.creating || self.batch_view.active {
            return;
        }
        let list = batch::parse_urls(&self.urls);
        if !list.invalid.is_empty() {
            self.message(
                "Sửa hoặc bỏ các dòng URL không hợp lệ trước khi tạo hàng đợi.",
                true,
            );
            return;
        }
        let urls = list
            .urls
            .into_iter()
            .filter(|u| !self.batch_view.excluded.contains(u))
            .collect::<Vec<_>>()
            .join("\n");
        if urls.is_empty() {
            self.message("Chọn ít nhất một URL.", true);
            return;
        }
        let output = if self.batch_view.prefs.auto_export {
            if self.export_location.folder().is_none() {
                let Some(folder) = rfd::FileDialog::new()
                    .set_title("Chọn thư mục xuất video hàng loạt")
                    .pick_folder()
                else {
                    return;
                };
                if let Err(e) = self.export_location.remember(folder) {
                    self.message(format!("{e:#}"), true);
                    return;
                }
            }
            self.export_location.folder().map(|p| p.to_owned())
        } else {
            None
        };
        let data = self.projects_root.parent().unwrap().to_owned();
        let _ = write_json(&data.join("batch-ui.json"), &self.batch_view.prefs);
        let template = self.project.clone();
        let settings = self.settings.clone();
        let title = self.batch_view.prefs.title.clone();
        let auto_export = self.batch_view.prefs.auto_export;
        let tx = self.batch_view.tx.clone();
        self.batch_view.creating = true;
        thread::spawn(move || {
            let result = batch::create(
                &data,
                &title,
                &urls,
                &template,
                &settings,
                auto_export,
                output,
            )
            .map(|b| (b, start));
            let _ = tx.send(result);
        });
    }
    pub(super) fn batch_mode_ui(&mut self, ui: &mut egui::Ui) {
        let old = self.batch_view.prefs.enabled;
        let mode = egui::ComboBox::from_id_salt("batch_mode")
            .width(ui.available_width().min(340.))
            .selected_text(if self.batch_view.prefs.enabled {
                "Hàng loạt · mỗi URL một dự án"
            } else {
                "Nhiều nguồn · một dự án"
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut self.batch_view.prefs.enabled,
                    false,
                    "Nhiều nguồn · một dự án",
                );
                ui.selectable_value(
                    &mut self.batch_view.prefs.enabled,
                    true,
                    "Hàng loạt · mỗi URL một dự án",
                );
            });
        #[cfg(test)]
        self.hitboxes
            .insert("batch_mode".into(), mode.response.rect);
        #[cfg(not(test))]
        let _ = mode;
        if old != self.batch_view.prefs.enabled {
            let _ = write_json(
                &self.projects_root.parent().unwrap().join("batch-ui.json"),
                &self.batch_view.prefs,
            );
        }
    }
    pub(super) fn batch_options_ui(&mut self, ui: &mut egui::Ui) {
        let old = (
            self.batch_view.prefs.auto_export,
            self.batch_view.prefs.title.clone(),
        );
        if self.batch_view.prefs.enabled {
            let _options = egui::CollapsingHeader::new(if self.batch_view.prefs.auto_export {
                "Tùy chọn đợt · tự xuất MP4"
            } else {
                "Tùy chọn đợt · dừng ở timeline"
            })
            .id_salt("batch_options")
            .default_open(false)
            .show(ui, |ui| {
                field(ui, "Tên đợt xử lý", &mut self.batch_view.prefs.title);
                let _export = ui.checkbox(
                    &mut self.batch_view.prefs.auto_export,
                    "Tự xuất MP4 sau khi hoàn thành từng dự án",
                );
                #[cfg(test)]
                self.hitboxes
                    .insert("batch_auto_export".into(), _export.rect);
                if self.batch_view.prefs.auto_export {
                    ui.horizontal_wrapped(|ui| {
                        ui.add(
                            egui::Label::new(
                                self.export_location
                                    .folder()
                                    .map(|p| p.display().to_string())
                                    .unwrap_or_else(|| "Chọn nơi lưu một lần khi bắt đầu".into()),
                            )
                            .wrap(),
                        );
                        if ui.button("Đổi nơi lưu…").clicked() {
                            self.choose_export_folder();
                        }
                    });
                }
            });
            #[cfg(test)]
            self.hitboxes
                .insert("batch_options".into(), _options.header_response.rect);
        }
        if old
            != (
                self.batch_view.prefs.auto_export,
                self.batch_view.prefs.title.clone(),
            )
        {
            let _ = write_json(
                &self.projects_root.parent().unwrap().join("batch-ui.json"),
                &self.batch_view.prefs,
            );
        }
    }
    pub(super) fn batch_sources_ui(&mut self, ui: &mut egui::Ui) {
        if ui.button("Nhập danh sách .txt…").clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("URL list", &["txt"])
                .pick_file()
            {
                match fs::read_to_string(path) {
                    Ok(text) => {
                        self.urls = text;
                        self.project.input_urls = self.urls.clone();
                    }
                    Err(e) => self.message(format!("Không đọc được danh sách: {e}"), true),
                }
            }
        }
        let list = batch::parse_urls(&self.urls);
        ui.label(format!(
            "{} URL hợp lệ · {} trùng · {} dòng sai",
            list.urls.len(),
            list.duplicates,
            list.invalid.len()
        ));
        for (line, _) in &list.invalid {
            ui.colored_label(
                Color32::LIGHT_RED,
                format!("URL ở dòng {line} không hợp lệ; sửa hoặc xóa dòng này."),
            );
        }
        let mut remove = None;
        egui::ScrollArea::vertical()
            .id_salt("batch_sources")
            .max_height(170.)
            .show(ui, |ui| {
                for (index, url) in list.urls.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let mut selected = !self.batch_view.excluded.contains(url);
                        if ui
                            .checkbox(&mut selected, format!("{}", index + 1))
                            .changed()
                        {
                            if selected {
                                self.batch_view.excluded.remove(url);
                            } else {
                                self.batch_view.excluded.insert(url.clone());
                            }
                        }
                        ui.add(egui::Label::new(url).truncate()).on_hover_text(url);
                        if ui.small_button("Xóa").clicked() {
                            remove = Some(url.clone());
                        }
                    });
                }
            });
        if let Some(url) = remove {
            self.urls = self
                .urls
                .lines()
                .filter(|s| validate_youtube_url(s).ok().as_ref() != Some(&url))
                .collect::<Vec<_>>()
                .join("\n");
            self.project.input_urls = self.urls.clone();
        }
        ui.label(RichText::new("Không dùng vùng giữ/loại của nguồn trong dự án mẫu. Muốn sửa riêng một video, mở dự án con sau khi xử lý.").size(11.).color(MUTED));
    }
    pub(super) fn batch_run_bar(&mut self, ui: &mut egui::Ui) {
        let list = batch::parse_urls(&self.urls);
        let selected = list
            .urls
            .iter()
            .filter(|u| !self.batch_view.excluded.contains(*u))
            .count();
        ui.label(format!(
            "Hàng loạt · {selected} dự án · {}–{} giây / video",
            self.project.rules.min_seconds, self.project.rules.max_seconds
        ));
        let enabled = selected > 0
            && list.invalid.is_empty()
            && !self.busy
            && !self.batch_view.active
            && !self.batch_view.creating;
        ui.horizontal_wrapped(|ui| {
            let queue = ui.add_enabled(
                enabled,
                egui::Button::new("Xếp vào hàng đợi").min_size(Vec2::new(0., 36.)),
            );
            let run = ui.add_enabled(
                enabled,
                egui::Button::new(RichText::new("Bắt đầu xử lý hàng loạt").strong())
                    .fill(BLUE)
                    .min_size(Vec2::new(0., 36.)),
            );
            #[cfg(test)]
            {
                self.hitboxes.insert("batch_enqueue".into(), queue.rect);
                self.hitboxes.insert("batch_start".into(), run.rect);
            }
            if queue.clicked() {
                self.enqueue_batch(false);
            }
            if run.clicked() {
                self.enqueue_batch(true);
            }
            if ui.button("Mở hàng đợi").clicked() {
                self.page = Page::Batch;
            }
        });
        if self.batch_view.creating {
            ui.spinner();
            ui.label("Đang tạo các dự án và lưu thiết lập…");
        }
        if self.batch_view.active {
            ui.colored_label(
                TEAL,
                "Hàng đợi đang chạy. Mở Hàng loạt để xem tiến độ hoặc tạm dừng.",
            );
        }
    }
    pub(super) fn batch_page(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.heading("Hàng loạt · mỗi URL một dự án");
            if ui.button("+ Đợt xử lý mới").clicked() {
                self.new_project();
                self.batch_view.prefs.enabled = true;
                self.page = Page::Editor;
                self.setup_open = true;
            }
        });
        if self.batch_view.batches.is_empty() {
            ui.label("Chưa có hàng đợi. Thêm danh sách URL trong Thiết lập dự án và chọn chế độ Hàng loạt.");
            return;
        }
        egui::ComboBox::from_id_salt("batch_picker")
            .width(ui.available_width().min(540.))
            .truncate()
            .selected_text(
                self.batch_view
                    .batches
                    .iter()
                    .find(|b| b.id == self.batch_view.selected)
                    .map(|b| b.title.as_str())
                    .unwrap_or("Chọn đợt xử lý"),
            )
            .show_ui(ui, |ui| {
                for b in &self.batch_view.batches {
                    ui.selectable_value(&mut self.batch_view.selected, b.id.clone(), &b.title);
                }
            });
        let Some(b) = self
            .batch_view
            .batches
            .iter()
            .find(|b| b.id == self.batch_view.selected)
            .cloned()
        else {
            return;
        };
        let complete = b.items.iter().filter(|i| i.state.complete()).count();
        let failed = b.items.iter().filter(|i| i.state.retryable()).count();
        ui.label(format!(
            "{complete}/{} hoàn thành · {failed} cần xử lý · {}",
            b.items.len(),
            match b.status.as_str() {
                "running" => "Đang chạy",
                "pausing" => "Đang chờ dừng",
                "paused" => "Đã tạm dừng",
                "blocked" => "Tạm dừng do cấu hình / tài nguyên",
                "interrupted" => "Bị gián đoạn",
                "complete" => "Đã hoàn thành",
                "complete_with_errors" => "Hoàn thành, có mục lỗi",
                _ => "Chờ chạy",
            }
        ));
        let running = self.batch_view.active && matches!(b.status.as_str(), "running" | "pausing");
        let mut action = None;
        let mut start = false;
        let mut retry_start = false;
        let mut open = None;
        ui.horizontal_wrapped(|ui| {
            if running {
                if ui.button("Tạm dừng sau video hiện tại").clicked() {
                    action = Some(Action::Pause);
                }
                if ui.button("Dừng tác vụ hiện tại").clicked() {
                    action = Some(Action::Stop);
                }
            } else {
                let run = ui.add_enabled(
                    !self.busy
                        && !self.batch_view.active
                        && b.items.iter().any(|i| {
                            matches!(
                                i.state,
                                State::Pending | State::Interrupted | State::Cancelled
                            )
                        }),
                    egui::Button::new("Bắt đầu / tiếp tục"),
                );
                #[cfg(test)]
                self.hitboxes.insert("batch_resume".into(), run.rect);
                if run.clicked() {
                    start = true;
                }
                if ui
                    .add_enabled(
                        !self.busy && !self.batch_view.active && failed > 0,
                        egui::Button::new("Thử lại các mục lỗi"),
                    )
                    .clicked()
                {
                    action = Some(Action::RetryAll);
                    retry_start = true;
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("batch_items")
            .show(ui, |ui| {
                for (index, item) in b.items.iter().enumerate() {
                    let _card = panel().inner_margin(12).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("{:02}. {}", index + 1, item.title)).strong(),
                            )
                            .wrap(),
                        );
                        ui.add(egui::Label::new(&item.url).truncate())
                            .on_hover_text(&item.url);
                        ui.horizontal_wrapped(|ui| {
                            chip(
                                ui,
                                item.state.label(),
                                if item.state.complete() {
                                    TEAL
                                } else if item.state == State::Failed {
                                    Color32::LIGHT_RED
                                } else {
                                    BLUE
                                },
                            );
                            ui.label(&item.stage);
                            if item.state == State::Running {
                                ui.spinner();
                                if let Some(at) = item.started_at {
                                    ui.label(format!(
                                        "Đã chạy {}",
                                        seconds(batch::now().saturating_sub(at) * 1000)
                                    ));
                                }
                            }
                        });
                        if !item.message.is_empty() {
                            ui.add(egui::Label::new(&item.message).wrap());
                        }
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .add_enabled(
                                    !matches!(item.state, State::Pending | State::Running)
                                        && !self.busy,
                                    egui::Button::new("Mở dự án"),
                                )
                                .clicked()
                            {
                                open =
                                    batch::project_path(self.projects_root.parent().unwrap(), item)
                                        .ok();
                            }
                            if item.state == State::Pending {
                                if ui.button("Đưa lên trước").clicked() {
                                    action = Some(Action::Up(item.id.clone()));
                                }
                                let skip = ui.button("Bỏ khỏi hàng đợi");
                                #[cfg(test)]
                                self.hitboxes
                                    .insert(format!("batch_skip_{index}"), skip.rect);
                                if skip.clicked() {
                                    action = Some(Action::Skip(item.id.clone()));
                                }
                            }
                            if item.state.retryable() && ui.button("Thử lại").clicked() {
                                action = Some(Action::Retry(item.id.clone()));
                                retry_start = !self.batch_view.active;
                            }
                            if let Some(output) = &item.output {
                                if ui.button("Mở thư mục MP4").clicked() {
                                    let _ = process::command("explorer.exe")
                                        .arg(output.parent().unwrap())
                                        .spawn();
                                }
                            }
                            if ui.button("Nhật ký").clicked() {
                                self.batch_view.detail = format!(
                                    "{}\n{}\n{}\n\n{}",
                                    item.project_id,
                                    item.url,
                                    item.message,
                                    read_tail(
                                        &batch::root(self.projects_root.parent().unwrap(), &b.id)
                                            .unwrap()
                                            .join("events.jsonl"),
                                        &item.id
                                    )
                                );
                            }
                        });
                    });
                    #[cfg(test)]
                    self.hitboxes
                        .insert(format!("batch_item_{index}"), _card.response.rect);
                    ui.add_space(8.);
                }
            });
        if let Some(action) = action {
            match batch::action(self.projects_root.parent().unwrap(), &b.id, action) {
                Ok(()) => self.batch_view.last_poll = Instant::now() - Duration::from_secs(2),
                Err(e) => self.message(format!("{e:#}"), true),
            }
        }
        if start || retry_start {
            self.start_batch(&b.id);
        }
        if let Some(path) = open {
            self.open(path);
        }
        if !self.batch_view.detail.is_empty() {
            let mut showing = true;
            egui::Window::new("Nhật ký tác vụ hàng loạt")
                .open(&mut showing)
                .resizable(true)
                .default_width(600.)
                .show(ui.ctx(), |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(450.)
                        .show(ui, |ui| {
                            ui.add(egui::Label::new(&self.batch_view.detail).wrap());
                        });
                });
            if !showing {
                self.batch_view.detail.clear();
            }
        }
    }
}
fn read_tail(path: &std::path::Path, item_id: &str) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = fs::File::open(path) else {
        return String::new();
    };
    let size = f.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = f.seek(SeekFrom::Start(size.saturating_sub(65536)));
    let mut bytes = Vec::new();
    let _ = f.read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes);
    text.lines()
        .filter_map(|line| serde_json::from_str::<batch::Progress>(line).ok())
        .filter(|e| e.item_id == item_id)
        .map(|e| format!("{}: {}", e.stage, e.message))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn batch_queue_wraps_actions_and_skip_never_deletes_a_project() {
        for width in [280., 540., 1080.] {
            let ctx = egui::Context::default();
            let mut app = StudioApp::from_context(&ctx);
            app.capture_path = None;
            let data = app.projects_root.parent().unwrap().to_owned();
            let b=batch::create(&data,"Bodycam long batch title — kiểm tra bố cục","https://youtu.be/dBy-8s6ly08\nhttps://youtu.be/r1cvbJJ5G54\nhttps://youtu.be/dNYsRcfWeLU",&app.project,&app.settings,false,None).unwrap();
            let path = batch::project_path(&data, &b.items[0]).unwrap();
            app.batch_view.selected = b.id.clone();
            app.batch_view.batches = vec![b.clone()];
            let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 900.));
            let draw = |app: &mut StudioApp, events| {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| app.batch_page(ui));
                    },
                );
            };
            for _ in 0..3 {
                draw(&mut app, vec![]);
            }
            let first = app.hitboxes["batch_item_0"];
            assert!(
                first.left() >= 0. && first.right() <= width,
                "Card extends outside {width}px: {first:?}"
            );
            let skip = app.hitboxes["batch_skip_0"];
            assert!(
                screen.contains_rect(skip),
                "Actions must wrap inside narrow panels: {skip:?}"
            );
            let pos = skip.center();
            for pressed in [true, false] {
                draw(
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            let saved =
                Batch::load(&batch::root(&data, &b.id).unwrap().join("batch.json")).unwrap();
            assert_eq!(saved.items[0].state, State::Skipped);
            assert!(
                path.is_file(),
                "Removing a queue item must keep its project files"
            );
        }
    }
    #[test]
    fn batch_run_controls_fit_and_live_batches_block_manual_jobs_and_updates() {
        for width in [280., 540., 1080.] {
            let ctx = egui::Context::default();
            let mut app = StudioApp::from_context(&ctx);
            app.capture_path = None;
            app.batch_view.prefs.enabled = true;
            app.urls="https://youtu.be/dBy-8s6ly08\nhttps://youtu.be/r1cvbJJ5G54\nhttps://youtu.be/dNYsRcfWeLU".into();
            let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 700.));
            let draw = |app: &mut StudioApp, events| {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            app.batch_mode_ui(ui);
                            app.batch_options_ui(ui);
                            app.batch_run_bar(ui);
                        });
                    },
                );
            };
            for _ in 0..3 {
                draw(&mut app, vec![]);
            }
            let pos = app.hitboxes["batch_options"].center();
            for pressed in [true, false] {
                draw(
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            for _ in 0..3 {
                draw(&mut app, vec![]);
            }
            for key in [
                "batch_mode",
                "batch_auto_export",
                "batch_enqueue",
                "batch_start",
            ] {
                assert!(
                    screen.contains_rect(app.hitboxes[key]),
                    "{key} exceeds {width}px: {:?}",
                    app.hitboxes[key]
                );
            }
            let before = serde_json::to_vec(&app.project).unwrap();
            app.batch_view.active = true;
            app.launch(Task::Automatic { resume: false });
            assert!(!app.busy && app.is_error);
            assert_eq!(before, serde_json::to_vec(&app.project).unwrap());
        }
    }
}
