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
    dates: crate::date_filter::DateFilter,
    last_poll: Instant,
    child: Option<std::process::Child>,
    owned_id: String,
    job: Option<batch::KillJob>,
    tx: mpsc::Sender<anyhow::Result<(Batch, bool)>>,
    rx: mpsc::Receiver<anyhow::Result<(Batch, bool)>>,
    excluded: std::collections::BTreeSet<String>,
    detail: String,
    pending_export: Option<batch::ExportPlan>,
    export_checking: bool,
    export_tx: mpsc::Sender<anyhow::Result<batch::ExportPlan>>,
    export_rx: mpsc::Receiver<anyhow::Result<batch::ExportPlan>>,
}
impl BatchView {
    pub fn refresh_after_project_delete(&mut self, data: &std::path::Path) {
        self.batches = batch::list(data);
        self.pending_export = None;
        self.detail.clear();
        self.last_poll = Instant::now();
    }
    pub fn load(data: &std::path::Path) -> Self {
        let (tx, rx) = mpsc::channel();
        let (export_tx, export_rx) = mpsc::channel();
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
            dates: crate::date_filter::DateFilter::load(&data.join("batch-dates.json")),
            batches,
            last_poll: Instant::now(),
            child: None,
            owned_id: String::new(),
            job: None,
            tx,
            rx,
            excluded: Default::default(),
            detail: String::new(),
            pending_export: None,
            export_checking: false,
            export_tx,
            export_rx,
        }
    }
    fn visible_sessions(&mut self,today:crate::date_filter::Date)->Vec<String> {
        let ids:Vec<_>=self.batches.iter().filter(|b|self.dates.matches(Some(b.created_at),today)).map(|b|b.id.clone()).collect();
        if !ids.contains(&self.selected) {
            self.selected=self.batches.iter().find(|b|ids.contains(&b.id)&&matches!(b.status.as_str(),"running"|"pausing"))
                .map(|b|b.id.clone()).or_else(||ids.first().cloned()).unwrap_or_default();
        }ids
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
    pub(super) fn selected_batch(&self) -> Option<&Batch> {
        self.batch_view.batches.iter().find(|b| b.id == self.batch_view.selected)
    }
    pub(super) fn batch_export_enabled(&self) -> bool {
        !self.busy && !self.batch_view.active && !self.batch_view.creating && !self.batch_view.export_checking
            && self.selected_batch().is_some_and(|b| b.items.iter().any(|i| i.can_export()))
    }
    pub(super) fn export_batch(&mut self, item_id: Option<&str>) {
        if !self.batch_export_enabled() {
            self.message("Chờ tác vụ hiện tại hoàn tất; phiên cần có dự án sẵn sàng xuất.", true);
            return;
        }
        let Some(b) = self.selected_batch().cloned() else { return; };
        let targets: Vec<_> = b.items.iter().filter(|i| i.can_export() && item_id.is_none_or(|id| i.id == id)).collect();
        if targets.is_empty() {
            self.message("Dự án chưa sẵn sàng xuất. Hoàn thành xử lý trước.", true);
            return;
        }
        if targets.iter().any(|i| i.project_id == self.project.id) && self.dirty {
            self.save();
            if self.dirty { return; }
        }
        if self.export_location.folder().is_none() { self.choose_export_folder(); }
        let Some(folder) = self.export_location.folder().map(|p| p.to_owned()) else { return; };
        let data = self.projects_root.parent().unwrap().to_owned();
        let tx = self.batch_view.export_tx.clone();
        let item_id = item_id.map(str::to_owned);
        self.batch_view.export_checking = true;
        self.message("Đang kiểm tra dữ liệu dự án và MP4 đích…", false);
        thread::spawn(move || { let _ = tx.send(batch::preview_export(&data, &b.id, item_id.as_deref(), &folder)); });
    }
    fn confirm_batch_export(&mut self, plan: batch::ExportPlan, overwrite: bool) {
        if self.busy || self.batch_view.active || self.batch_view.creating {
            self.message("Chờ tác vụ hiện tại hoàn tất rồi bấm Xuất lại.", true);
            return;
        }
        let data = self.projects_root.parent().unwrap().to_owned();
        match batch::queue_export(&data, &plan, overwrite) {
            Ok(queued) => {
                let count = queued.run_items.as_ref().map_or(0, Vec::len);
                let skipped = plan.skipped.len();
                if let Some(saved) = self.batch_view.batches.iter_mut().find(|v| v.id == plan.batch_id) { *saved = queued; }
                self.player.stop();
                self.start_batch(&plan.batch_id);
                if self.batch_view.active {
                    self.message(format!("Đang xuất {count} dự án từ timeline đã lưu · bỏ qua {skipped} dự án chưa hợp lệ. Lưu tại {}", plan.folder.display()), false);
                }
            }
            Err(e) => self.message(format!("Không bắt đầu xuất MP4: {e:#}"), true),
        }
    }
    pub(super) fn batch_export_dialog(&mut self, ctx: &egui::Context) {
        let Some(plan) = self.batch_view.pending_export.clone() else { return; };
        let existing = plan.existing_count();
        let mut confirm = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::new("batch_export_confirmation")).show(ctx, |ui| {
            ui.set_width(540.0_f32.min(ctx.screen_rect().width() - 60.0));
            ui.heading(if existing > 0 { "MP4 đã tồn tại — xuất lại?" } else { "Kiểm tra trước khi xuất MP4" });
            ui.label(format!("{} dự án hợp lệ · {} file đã tồn tại · {} dự án bị bỏ qua", plan.targets.len(), existing, plan.skipped.len()));
            egui::ScrollArea::vertical().max_height((ctx.screen_rect().height()*0.5).min(350.)).show(ui, |ui| {
                for target in plan.targets.iter().filter(|t|t.existing.is_some()) {
                    ui.label(RichText::new(&target.title).strong());
                    ui.add(egui::Label::new(target.output.to_string_lossy()).wrap());
                }
                if !plan.skipped.is_empty() {
                    ui.separator(); ui.label(RichText::new("Không xuất các dự án sau:").strong());
                    for reason in &plan.skipped { ui.add(egui::Label::new(reason).wrap()); }
                }
            });
            ui.add_space(8.);
            if existing > 0 { ui.label("Tạo MP4 mới từ timeline hiện tại. MP4 cũ được lưu thành final.previous-….mp4 khi bản mới xuất thành công."); }
            ui.horizontal(|ui| {
                let r = ui.button("Hủy"); cancel = r.clicked();
                #[cfg(test)] self.hitboxes.insert("batch_export_cancel".into(), r.rect);
                let r = ui.add_enabled(!self.busy && !self.batch_view.active && !self.batch_view.creating,
                    egui::Button::new(if existing > 0 { "Xác nhận xuất lại".into() } else { format!("Xuất {} dự án hợp lệ", plan.targets.len()) }));
                confirm = r.clicked();
                #[cfg(test)] self.hitboxes.insert("batch_export_confirm".into(), r.rect);
            });
        });
        if cancel || modal.should_close() { self.batch_view.pending_export = None; }
        if confirm { self.batch_view.pending_export = None; self.confirm_batch_export(plan, existing > 0); }
    }
    pub(super) fn poll_batch(&mut self) {
        let data = self.projects_root.parent().unwrap().to_owned();
        while let Ok(result) = self.batch_view.export_rx.try_recv() {
            self.batch_view.export_checking = false;
            match result {
                Ok(plan) if plan.existing_count() > 0 || !plan.skipped.is_empty() => self.batch_view.pending_export = Some(plan),
                Ok(plan) => self.confirm_batch_export(plan, false),
                Err(e) => self.message(format!("Không bắt đầu xuất MP4: {e:#}"), true),
            }
        }
        while let Ok(result) = self.batch_view.rx.try_recv() {
            self.batch_view.creating = false;
            match result {
                Ok((b, start)) => {
                    self.batch_view.dates.period=crate::date_filter::Period::All;
                    self.batch_view.dates.save(&data.join("batch-dates.json"));
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
                Err(e) => self.message(format!("Không tạo được phiên hàng loạt · {}: {e:#}", crate::date_filter::timestamp(Some(batch::now()))), true),
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
            let mut b = Batch::load(&batch::root(&data, id)?.join("batch.json"))?;
            if !b.resumable() && b.items.iter().any(|i| matches!(i.state, State::Pending | State::Interrupted | State::Cancelled)) {
                b.run_items = None;
                b.save(&data)?;
            }
            if b.items
                .iter()
                .any(|i| b.scheduled(i) && i.project_id == self.project.id && !i.state.complete())
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
    fn batch_filter_ui(&mut self, ui: &mut egui::Ui) -> Vec<String> {
        ui.horizontal_wrapped(|ui| {
            ui.heading("Hàng loạt · mỗi URL một dự án");
            if ui.button("+ Phiên hàng loạt mới").clicked() {
                self.new_project();
                self.batch_view.prefs.enabled = true;
                self.page = Page::Editor;
                self.setup_open = true;
            }
        });
        ui.label(RichText::new("Lọc theo ngày tạo phiên · ngày giờ theo máy").size(12.).color(MUTED));
        let before=self.batch_view.dates.clone();
        let _date_rects=self.batch_view.dates.ui(ui,"batch_dates");
        #[cfg(test)] for (i,rect) in _date_rects.into_iter().enumerate(){self.hitboxes.insert(format!("batch_date_{i}"),rect);}
        if self.batch_view.dates!=before {self.batch_view.dates.save(&self.projects_root.parent().unwrap().join("batch-dates.json"));}
        let today=crate::date_filter::local_parts(crate::date_filter::now()).unwrap().0;
        let visible=self.batch_view.visible_sessions(today);
        let count:usize=self.batch_view.batches.iter().filter(|b|visible.contains(&b.id)).map(|b|b.items.len()).sum();
        ui.label(format!("{} / {} phiên · {count} video",visible.len(),self.batch_view.batches.len()));
        if self.batch_view.active {
            if let Some(id)=self.batch_view.batches.iter().find(|b|!visible.contains(&b.id)&&matches!(b.status.as_str(),"running"|"pausing")).map(|b|b.id.clone()) {
                ui.horizontal_wrapped(|ui|{
                    ui.colored_label(TEAL,"Có phiên đang chạy ngoài bộ lọc.");
                    if ui.button("Xem phiên đang chạy").clicked(){
                        self.batch_view.dates.period=crate::date_filter::Period::All;
                        self.batch_view.selected=id;
                        self.batch_view.dates.save(&self.projects_root.parent().unwrap().join("batch-dates.json"));
                    }
                });
            }
        }
        visible
    }
    fn batch_session_ui(&mut self, ui: &mut egui::Ui, visible: &[String]) -> Option<(Batch, Option<Action>, bool, bool)> {
        ui.label(RichText::new("Phiên chạy").strong());
        if self.batch_view.batches.is_empty() {
            ui.add(egui::Label::new("Chưa có hàng đợi. Thêm danh sách URL trong Thiết lập dự án và chọn chế độ Hàng loạt.").wrap());
            return None;
        }
        if visible.is_empty(){
            ui.add(egui::Label::new("Không có phiên trong khoảng ngày này. Chọn khoảng khác hoặc Bỏ lọc.").wrap());
            return None;
        }
        let _picker=egui::ComboBox::from_id_salt("batch_picker")
            .width(ui.available_width().min(540.))
            .truncate()
            .selected_text(
                self.batch_view
                    .batches
                    .iter()
                    .find(|b| b.id == self.batch_view.selected)
                    .map(session_label)
                    .unwrap_or_else(||"Chọn phiên chạy".into()),
            )
            .show_ui(ui, |ui| {
                ui.set_max_width(540.);
                ui.style_mut().wrap_mode=Some(egui::TextWrapMode::Truncate);
                for b in self.batch_view.batches.iter().filter(|b|visible.contains(&b.id)) {
                    let label=session_label(b);
                    ui.selectable_value(&mut self.batch_view.selected,b.id.clone(),&label).on_hover_text(label);
                }
            });
        #[cfg(test)] self.hitboxes.insert("batch_session_picker".into(),_picker.response.rect);
        let b = self
            .batch_view
            .batches
            .iter()
            .find(|b| b.id == self.batch_view.selected)
            .cloned()?;
        ui.add(egui::Label::new(RichText::new(&b.title).strong()).truncate()).on_hover_text(session_label(&b));
        let complete = b.items.iter().filter(|i| i.state.complete()).count();
        let exported = b.items.iter().filter(|i| i.state == State::Exported).count();
        let failed = b.items.iter().filter(|i| i.state.retryable()).count();
        ui.add(egui::Label::new(format!(
            "{complete}/{} hoàn thành · {exported} đã xuất MP4 · {failed} cần xử lý · {}",
            b.items.len(),
            session_status(&b),
        )).wrap());
        let running = self.batch_view.active && matches!(b.status.as_str(), "running" | "pausing");
        let mut action = None;
        let mut start = false;
        let mut retry_start = false;
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
        Some((b,action,start,retry_start))
    }
    pub(super) fn batch_page(&mut self, ui: &mut egui::Ui) {
        let width=ui.available_width();
        let selected=if width>=700. {
            ui.horizontal_top(|ui| {
                let left_width=(width-ui.spacing().item_spacing.x)*0.46;
                let left=ui.allocate_ui_with_layout(Vec2::new(left_width,0.),egui::Layout::top_down(egui::Align::Min),|ui| {
                    ui.set_width(left_width);
                    self.batch_filter_ui(ui)
                });
                #[cfg(test)] self.hitboxes.insert("batch_header_left".into(),left.response.rect);
                let right_width=width-left_width-ui.spacing().item_spacing.x;
                let right=ui.allocate_ui_with_layout(Vec2::new(right_width,0.),egui::Layout::top_down(egui::Align::Min),|ui| {
                    ui.set_width(right_width);
                    self.batch_session_ui(ui,&left.inner)
                });
                #[cfg(test)] self.hitboxes.insert("batch_header_right".into(),right.response.rect);
                right.inner
            }).inner
        }else{
            let visible=self.batch_filter_ui(ui);
            self.batch_session_ui(ui,&visible)
        };
        let Some((b,mut action,start,mut retry_start))=selected else {return;};
        if b.items.is_empty() {
            ui.label(RichText::new("Phiên này không còn dự án.").color(MUTED));
        }
        let mut open=None;
        let mut export_item=None;
        ui.add_space(8.);
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
                                if item.export_only && item.state == State::Pending { "Chờ xuất MP4" }
                                else if item.export_only && item.state == State::Running { "Đang xuất MP4" }
                                else { item.state.label() },
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
                            if let Some(message)=crate::diagnostics::friendly_error(&item.message) {
                                let _response=ui.add(egui::Label::new(RichText::new(message).color(Color32::LIGHT_RED)).wrap());
                                #[cfg(test)] self.hitboxes.insert(format!("batch_error_{index}"),_response.rect);
                                egui::CollapsingHeader::new("Chi tiết kỹ thuật / thư mục cần kiểm tra").id_salt(&item.id).show(ui,|ui|{
                                    if let Ok(path)=batch::project_path(self.projects_root.parent().unwrap(),item) {
                                        ui.add(egui::Label::new(format!("Dự án/cache: {}",path.parent().unwrap_or(&path).display())).wrap());
                                    }
                                    if let Some(folder)=&b.output_folder {ui.add(egui::Label::new(format!("Nơi xuất MP4: {}",folder.display())).wrap());}
                                    ui.add(egui::Label::new(crate::diagnostics::safe_text(&item.message,&[])).wrap());
                                });
                            }else{ui.add(egui::Label::new(&item.message).wrap());}
                        }
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().interact_size.y = 30.;
                            let export = ui.add_enabled(
                                !self.busy && !self.batch_view.active && !self.batch_view.creating && !self.batch_view.export_checking && item.can_export(),
                                egui::Button::new(if item.state == State::Exported { "Xuất lại" } else { "Xuất" }).min_size(Vec2::new(64.,30.)),
                            ).on_hover_text("Xuất MP4 riêng dự án này từ timeline đã lưu.")
                                .on_disabled_hover_text("Hoàn thành xử lý và chờ tác vụ đang chạy kết thúc trước khi xuất.");
                            #[cfg(test)] {
                                self.hitboxes.insert(format!("batch_export_{index}"),export.rect);
                            }
                            if export.clicked() { export_item=Some(item.id.clone()); }
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
        if let Some(id) = export_item { self.export_batch(Some(&id)); }
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
fn session_label(b:&Batch)->String {
    format!("{} · #{} · {} · {} video · {}",crate::date_filter::timestamp(Some(b.created_at)),b.id.chars().take(6).collect::<String>(),b.title,b.items.len(),session_status(b))
}
fn session_status(b:&Batch)->&'static str {
    match b.status.as_str() {
        "running"=>"Đang chạy","pausing"=>"Đang chờ dừng","paused"=>"Đã tạm dừng",
        "blocked"=>"Tạm dừng do cấu hình / tài nguyên","interrupted"=>"Bị gián đoạn",
        "complete"=>"Đã hoàn thành","complete_with_errors"=>"Hoàn thành, có mục lỗi",_=>"Chờ chạy",
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
    fn library_confirmed_delete_refreshes_session_rows_and_survives_restart() {
        let ctx = egui::Context::default();
        let mut app = StudioApp::from_context(&ctx); app.capture_path = None;
        ctx.style_mut(|style| style.animation_time = 0.);
        let data = app.projects_root.parent().unwrap().to_owned();
        let b = batch::create(&data, "Delete sync UI", "https://youtu.be/dBy-8s6ly08\nhttps://youtu.be/r1cvbJJ5G54",
            &app.project, &app.settings, false, None).unwrap();
        let target = b.items[0].clone();
        let path = batch::project_path(&data, &target).unwrap();
        app.project = Project::load(&path).unwrap();
        app.batch_view = BatchView::load(&data);
        app.history = project_store::list(&app.projects_root);
        app.page = Page::Projects;
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.,900.));
        let draw = |app:&mut StudioApp, events| {
            let _ = ctx.run(egui::RawInput{screen_rect:Some(screen),events,..Default::default()},|ctx| app.draw(ctx));
        };
        let click = |app:&mut StudioApp,key:&str| {
            let pos=app.hitboxes[key].center();
            for pressed in [true,false] {
                draw(app,vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed,modifiers:egui::Modifiers::NONE}]);
            }
            draw(app,vec![]);
        };
        draw(&mut app,vec![]); draw(&mut app,vec![]);
        let delete_key = format!("delete_{}",target.project_id);
        click(&mut app,&delete_key);
        for key in ["delete_cancel","delete_confirm"] {assert!(screen.contains_rect(app.hitboxes[key]));}
        click(&mut app,"delete_cancel");
        assert!(path.exists());
        assert_eq!(app.selected_batch().unwrap().items.len(),2);
        click(&mut app,&delete_key);
        app.dirty=true;
        click(&mut app,"delete_confirm");
        assert!(!path.parent().unwrap().exists());
        assert!(!app.is_error && !app.dirty && app.pending_delete.is_none());
        let selected=app.selected_batch().unwrap();
        assert_eq!(selected.items.len(),1);
        assert_eq!(selected.items[0].project_id,b.items[1].project_id);
        let reloaded=BatchView::load(&data);
        assert_eq!(reloaded.batches[0].items.len(),1);
        app.page=Page::Batch; app.hitboxes.clear(); draw(&mut app,vec![]); draw(&mut app,vec![]);
        assert!(app.hitboxes.contains_key("batch_item_0"));
        assert!(!app.hitboxes.contains_key("batch_item_1"));
        app.last_save=Instant::now()-Duration::from_secs(3); draw(&mut app,vec![]);
        assert!(!path.parent().unwrap().exists(),"Autosave must not recreate the deleted project");
    }

    #[test]
    fn missing_project_preflight_runs_in_background_and_never_starts_a_worker() {
        let ctx = egui::Context::default();
        let mut app = StudioApp::from_context(&ctx); app.capture_path = None;
        let data = app.projects_root.parent().unwrap().to_owned();
        let mut b = batch::create(&data,"Deleted project QA","https://youtu.be/dBy-8s6ly08",&app.project,&app.settings,false,None).unwrap();
        b.items[0].state=State::Ready; b.save(&data).unwrap();
        let path=batch::project_path(&data,&b.items[0]).unwrap(); fs::remove_file(&path).unwrap();
        let output=tempfile::tempdir().unwrap(); app.export_location.remember(output.path().to_owned()).unwrap();
        app.batch_view.selected=b.id.clone(); app.batch_view.batches=vec![b.clone()];
        let before=fs::read(batch::root(&data,&b.id).unwrap().join("batch.json")).unwrap();
        app.export_batch(None);
        assert!(app.batch_view.export_checking && !app.batch_export_enabled());
        let started=Instant::now();
        while app.batch_view.export_checking && started.elapsed()<Duration::from_secs(10) {app.poll_batch();thread::sleep(Duration::from_millis(10));}
        assert!(!app.batch_view.export_checking);
        assert!(app.is_error && app.notice.contains("Không đọc được dữ liệu dự án"));
        assert!(!app.batch_view.active && app.batch_view.child.is_none());
        assert!(!path.exists() && !path.parent().unwrap().join("batch-owner.lock").exists());
        assert_eq!(fs::read(batch::root(&data,&b.id).unwrap().join("batch.json")).unwrap(),before);
    }
    #[test]
    fn export_confirmation_fits_screen_and_cancel_keeps_output_and_queue_unchanged() {
        for width in [540.,1080.,1440.] {
            let ctx = egui::Context::default();
            let mut app = StudioApp::from_context(&ctx); app.capture_path = None;
            let temp = tempfile::tempdir().unwrap();
            let output = temp.path().join("final.mp4"); fs::write(&output,b"keep existing video").unwrap();
            app.batch_view.pending_export = Some(batch::ExportPlan {
                batch_id:"confirmation-test".into(), item_id:None, folder:temp.path().to_owned(),
                targets:vec![batch::ExportTarget {item_id:"item".into(), title:"Video đã xuất".into(),output:output.clone(),
                    existing:Some(batch::ExportFile {path:output.clone(),bytes:19,modified_ns:0})}],
                skipped:vec!["Video 2: Không đọc được dữ liệu dự án: C:/projects/missing/project.json. Khôi phục thư mục dự án hoặc tạo lại từ URL; MP4 cũ vẫn được giữ.".into()],
            });
            let screen = Rect::from_min_size(Pos2::ZERO,Vec2::new(width,720.));
            let draw = |app:&mut StudioApp,events| {ctx.run(egui::RawInput {screen_rect:Some(screen),events,..Default::default()},|ctx|app.batch_export_dialog(ctx))};
            let mut text=String::new();
            for _ in 0..3 { let out=draw(&mut app,vec![]); text=out.shapes.iter().filter_map(|s|if let egui::Shape::Text(t)=&s.shape{Some(t.galley.text())}else{None}).collect::<Vec<_>>().join(" "); }
            assert!(text.contains("Xác nhận xuất lại") && text.contains("Không xuất các dự án sau"));
            for key in ["batch_export_cancel","batch_export_confirm"] {assert!(screen.contains_rect(app.hitboxes[key]),"{key} exceeds {width}");}
            let pos = app.hitboxes["batch_export_cancel"].center();
            for pressed in [true,false] {let _=draw(&mut app,vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton {pos,button:egui::PointerButton::Primary,pressed,modifiers:egui::Modifiers::NONE}]);}
            assert!(app.batch_view.pending_export.is_none());
            assert!(!app.batch_view.active && app.batch_view.child.is_none());
            assert_eq!(fs::read(output).unwrap(),b"keep existing video");
        }
    }
    #[test]
    fn export_buttons_fit_rows_and_toolbar_targets_the_selected_session_with_an_empty_editor() {
        for width in [280.,540.,1080.] {
            let ctx=egui::Context::default();
            let mut app=StudioApp::from_context(&ctx);app.capture_path=None;
            let data=app.projects_root.parent().unwrap().to_owned();
            let mut b=batch::create(&data,"Phiên xuất MP4","https://youtu.be/dBy-8s6ly08\nhttps://youtu.be/r1cvbJJ5G54",&app.project,&app.settings,false,None).unwrap();
            b.items[0].state=State::Ready;b.save(&data).unwrap();
            app.batch_view.selected=b.id.clone();app.batch_view.batches=vec![b.clone()];
            assert!(app.project.segments.is_empty());
            assert!(app.batch_export_enabled(),"Export must use the session, not the empty editor");
            let screen=Rect::from_min_size(Pos2::ZERO,Vec2::new(width,900.));
            for _ in 0..3 {let _=ctx.run(egui::RawInput{screen_rect:Some(screen),..Default::default()},|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|app.batch_page(ui));
            });}
            for key in ["batch_export_0","batch_export_1"] {
                let r=app.hitboxes[key];assert!(screen.contains_rect(r),"{key} outside {width}: {r:?}");assert!(r.height()>=30.);
            }
            app.batch_view.active=true;assert!(!app.batch_export_enabled());
            app.export_batch(None);
            assert!(app.is_error);
            assert_eq!(Batch::load(&batch::root(&data,&b.id).unwrap().join("batch.json")).unwrap().items[0].state,State::Ready);
            app.batch_view.active=false;
            app.batch_view.batches[0].items[0].state=State::Pending;
            assert!(!app.batch_export_enabled());
        }
        for width in [1080.,1440.] {
            let ctx=egui::Context::default();let mut app=StudioApp::from_context(&ctx);app.capture_path=None;app.page=Page::Batch;
            let data=app.projects_root.parent().unwrap().to_owned();
            let mut b=batch::create(&data,"Phiên đang chọn","https://youtu.be/dBy-8s6ly08",&app.project,&app.settings,false,None).unwrap();
            b.items[0].state=State::Ready;
            app.batch_view.selected=b.id.clone();app.batch_view.batches=vec![b];
            let screen=Rect::from_min_size(Pos2::ZERO,Vec2::new(width,900.));
            let mut text=String::new();
            for _ in 0..3 {let out=ctx.run(egui::RawInput{screen_rect:Some(screen),..Default::default()},|ctx|app.draw(ctx));
                text=out.shapes.iter().filter_map(|s|if let egui::Shape::Text(t)=&s.shape{Some(t.galley.text())}else{None}).collect::<Vec<_>>().join(" ");}
            assert!(text.contains("Xuất MP4 cả phiên"));
            assert!(text.contains("Phiên đang chọn"));
            assert!(screen.contains_rect(app.hitboxes["top_export"]));
            assert!(!app.hitboxes["top_export"].intersects(app.hitboxes["top_export_location"]));
        }
    }
    #[test]
    fn batch_header_keeps_filters_left_and_session_right_including_date_ranges() {
        for width in [820.,1080.,1440.] {
            let ctx=egui::Context::default();let mut app=StudioApp::from_context(&ctx);app.capture_path=None;
            let b=Batch{version:1,id:"aaaaaa-header".into(),title:"Phiên hàng loạt có tên rất dài để kiểm tra bố cục bộ chọn phiên".into(),created_at:crate::date_filter::now(),status:"paused".into(),auto_export:false,output_folder:None,items:vec![],run_items:None};
            app.batch_view.selected=b.id.clone();app.batch_view.batches=vec![b];
            for period in [crate::date_filter::Period::All,crate::date_filter::Period::Range] {
                app.batch_view.dates.period=period;
                let screen=Rect::from_min_size(Pos2::ZERO,Vec2::new(width,900.));
                for _ in 0..3 {let _=ctx.run(egui::RawInput{screen_rect:Some(screen),..Default::default()},|ctx|{egui::CentralPanel::default().show(ctx,|ui|app.batch_page(ui));});}
                let left=app.hitboxes["batch_header_left"];let right=app.hitboxes["batch_header_right"];
                assert!(left.right()<=right.left(),"Overlapping header columns at {width}px: {left:?}, {right:?}");
                assert!((left.top()-right.top()).abs()<1.);
                assert!(screen.contains_rect(right));
                assert!(right.contains_rect(app.hitboxes["batch_session_picker"]));
                for (key,rect) in &app.hitboxes {if key.starts_with("batch_date_"){assert!(left.contains_rect(*rect),"{key} outside filter column: {rect:?}");}}
            }
        }
    }
    #[test]
    fn date_filter_keeps_selection_inside_visible_sessions_and_distinguishes_duplicate_titles() {
        let temp=tempfile::tempdir().unwrap();
        let mut view=BatchView::load(temp.path());
        let mut recent=Batch{version:1,id:"aaaaaa-recent".into(),title:"Đợt xử lý mới".into(),created_at:crate::date_filter::now(),status:"paused".into(),auto_export:false,output_folder:None,items:vec![],run_items:None};
        let today=crate::date_filter::local_parts(recent.created_at).unwrap().0;
        let mut older=recent.clone();older.id="bbbbbb-older".into();older.created_at-=10*86400;older.status="running".into();
        view.selected=older.id.clone();view.batches=vec![recent.clone(),older.clone()];
        view.dates.period=crate::date_filter::Period::Today;
        assert_eq!(view.visible_sessions(today),vec![recent.id.clone()]);
        assert_eq!(view.selected,recent.id);
        view.dates.period=crate::date_filter::Period::Yesterday;
        assert!(view.visible_sessions(today).is_empty());assert!(view.selected.is_empty());
        view.dates.period=crate::date_filter::Period::All;
        assert_eq!(view.visible_sessions(today).len(),2);assert_eq!(view.selected,older.id);
        // Even two same-title sessions created in the same minute remain distinguishable.
        recent.created_at=older.created_at;
        assert_ne!(session_label(&recent),session_label(&older));
        assert!(session_label(&recent).contains("#aaaaaa"));
    }
    #[test]
    fn saved_disk_error_shows_readable_recovery_and_fits_queue_cards() {
        for width in [280.,540.,1080.] {
            let ctx=egui::Context::default();let mut app=StudioApp::from_context(&ctx);
            app.capture_path=None;
            let data=app.projects_root.parent().unwrap().to_owned();
            let mut b=batch::create(&data,"Disk error QA","https://youtu.be/dBy-8s6ly08",&app.project,&app.settings,false,None).unwrap();
            // Persist an old-format error, then reopen it with the new UI.
            b.items[0].state=State::Failed;b.items[0].stage="Assets".into();
            b.items[0].message="Quy trình tự động / Giọng & nhân vật: Cảnh 8 / Avatar 2D: [vost#0:0/ffv1] Error submitting a packet to the muxer: No space left on device; error code: -28".into();
            b.save(&data).unwrap();
            app.batch_view.selected=b.id.clone();
            app.batch_view.batches=vec![Batch::load(&batch::root(&data,&b.id).unwrap().join("batch.json")).unwrap()];
            let screen=Rect::from_min_size(Pos2::ZERO,Vec2::new(width,900.));
            let mut visible=String::new();
            for _ in 0..3 {
                let output=ctx.run(egui::RawInput{screen_rect:Some(screen),..Default::default()},|ctx|{
                    egui::CentralPanel::default().show(ctx,|ui|app.batch_page(ui));
                });
                visible=output.shapes.iter().filter_map(|s|if let egui::Shape::Text(t)=&s.shape{Some(t.galley.text())}else{None}).collect::<Vec<_>>().join(" ");
            }
            assert!(screen.contains_rect(app.hitboxes["batch_error_0"]));
            assert!(visible.contains("Không đủ dung lượng ổ đĩa") && visible.contains("Thử lại"));
            assert!(!visible.contains("muxer") && !visible.contains("-28"),"Raw details should be collapsed: {visible}");
            assert!(app.batch_view.batches[0].items[0].message.contains("-28"),"Keep technical details in durable state");
        }
    }
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
