//! Native preset dialogs. File hashing/copying never runs on the egui thread.
use super::*;
use crate::settings_package::{self as package, Loaded};
use std::sync::Arc;

enum Reply {
    Progress(String),
    Loaded(anyhow::Result<Box<Loaded>>),
    Exported(anyhow::Result<PathBuf>),
    Imported(anyhow::Result<Box<Imported>>),
}
struct Imported {
    project: Option<Project>,
    settings: ProjectDefaults,
    remember: bool,
}
pub(super) struct PresetView {
    pub open: bool,
    pub running: bool,
    pub export: bool,
    include_assets: bool,
    apply_current: bool,
    remember: bool,
    loaded: Option<Arc<Loaded>>,
    replacements: BTreeMap<String, PathBuf>,
    message: String,
    error: String,
    tx: mpsc::Sender<Reply>,
    rx: mpsc::Receiver<Reply>,
    // An explicit destination choice takes precedence over automatic remembering
    // until the next project is created, then normal remembering resumes.
    pub next_defaults: Option<ProjectDefaults>,
}
impl Default for PresetView {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            open: false,
            running: false,
            export: false,
            include_assets: true,
            apply_current: true,
            remember: true,
            loaded: None,
            replacements: BTreeMap::new(),
            message: String::new(),
            error: String::new(),
            tx,
            rx,
            next_defaults: None,
        }
    }
}
impl StudioApp {
    fn preset_allowed(&self) -> bool {
        !self.busy
            && !self.presets.running
            && !self.batch_view.active
            && !self.batch_view.creating
            && !crate::batch::reserved_project(
                self.projects_root.parent().unwrap(),
                &self.project.id,
            )
    }
    pub(super) fn preset_menu(&mut self, ui: &mut egui::Ui) {
        let enabled = self.preset_allowed();
        let menu = ui
            .add_enabled_ui(enabled, |ui| {
                ui.menu_button("Thiết lập", |ui| {
                    let export = ui.button("Xuất thiết lập…");
                    let import = ui.button("Nhập thiết lập…");
                    #[cfg(test)]
                    {
                        self.hitboxes.insert("preset_export".into(), export.rect);
                        self.hitboxes.insert("preset_import".into(), import.rect);
                    }
                    if export.clicked() {
                        self.layers.canvas_focused=false;
                        self.presets.open = true;
                        self.presets.export = true;
                        self.presets.error.clear();
                        ui.close_menu();
                    }
                    if import.clicked() {
                        ui.close_menu();
                        if let Some(path) = rfd::FileDialog::new()
                            .set_title("Nhập thiết lập AIR3view")
                            .add_filter("Thiết lập AIR3view", &["air3preset"])
                            .pick_file()
                        {
                            self.read_preset(path);
                        }
                    }
                })
            })
            .inner;
        #[cfg(test)]
        self.hitboxes
            .insert("preset_menu".into(), menu.response.rect);
        menu.response.on_disabled_hover_text(
            "Chờ tác vụ hoặc tạm dừng hàng loạt trước khi chuyển thiết lập.",
        );
    }
    pub(super) fn read_preset(&mut self, path: PathBuf) {
        self.layers.canvas_focused=false;
        self.presets.open = true;
        self.presets.export = false;
        self.presets.running = true;
        self.presets.loaded = None;
        self.presets.replacements.clear();
        self.presets.error.clear();
        self.presets.apply_current = true;
        self.presets.remember = true;
        self.presets.message = "Đang đọc file thiết lập…".into();
        let tx = self.presets.tx.clone();
        thread::spawn(move || {
            let result = package::load(&path, &|text| {
                let _ = tx.send(Reply::Progress(text.into()));
            });
            let _ = tx.send(Reply::Loaded(result.map(Box::new)));
        });
    }
    fn write_preset(&mut self, path: PathBuf) {
        let project = self.project.clone();
        let settings = self.settings.clone();
        let include = self.presets.include_assets;
        self.presets.running = true;
        self.presets.error.clear();
        self.presets.message = "Chuẩn bị xuất thiết lập…".into();
        let tx = self.presets.tx.clone();
        thread::spawn(move || {
            let result = package::export(&project, &settings, &path, include, &|text| {
                let _ = tx.send(Reply::Progress(text.into()));
            })
            .map(|_| path);
            let _ = tx.send(Reply::Exported(result));
        });
    }
    fn import_preset(&mut self) {
        if !self.preset_allowed() {
            self.presets.error =
                "Chờ tác vụ hoàn tất hoặc tạm dừng hàng loạt trước khi nhập.".into();
            return;
        }
        let Some(loaded) = self.presets.loaded.clone() else {
            return;
        };
        if !self.presets.apply_current && !self.presets.remember {
            return;
        }
        let original = self.project.clone();
        let current = self.presets.apply_current;
        let remember = self.presets.remember;
        let replacements = self.presets.replacements.clone();
        let data = self.projects_root.parent().unwrap().to_owned();
        self.presets.running = true;
        self.presets.error.clear();
        self.presets.message = "Đang nhập thiết lập…".into();
        let tx = self.presets.tx.clone();
        thread::spawn(move || {
            let result = (|| -> anyhow::Result<Imported> {
                let settings = loaded.resolve(&data, &replacements, &|text| {
                    let _ = tx.send(Reply::Progress(text.into()));
                })?;
                let project = if current {
                    Some(package::apply(&settings, &original)?)
                } else {
                    None
                };
                package::commit(&data, &settings, project.as_ref(), remember)?;
                Ok(Imported {
                    project,
                    settings,
                    remember,
                })
            })();
            let _ = tx.send(Reply::Imported(result.map(Box::new)));
        });
    }
    pub(super) fn poll_presets(&mut self) {
        while let Ok(reply) = self.presets.rx.try_recv() {
            match reply {
                Reply::Progress(text) => self.presets.message = text,
                Reply::Loaded(result) => {
                    self.presets.running = false;
                    match result {
                        Ok(loaded) => self.presets.loaded = Some(Arc::from(loaded)),
                        Err(e) => self.presets.error = format!("{e:#}"),
                    }
                }
                Reply::Exported(result) => {
                    self.presets.running = false;
                    match result {
                        Ok(path) => {
                            self.presets.open = false;
                            self.message(format!("Đã xuất thiết lập: {}", path.display()), false);
                        }
                        Err(e) => self.presets.error = format!("{e:#}"),
                    }
                }
                Reply::Imported(result) => {
                    self.presets.running = false;
                    match result {
                        Err(e) => self.presets.error = format!("{e:#}"),
                        Ok(imported) => {
                            let imported=*imported;
                            let previous = ProjectDefaults::load(
                                &self
                                    .projects_root
                                    .parent()
                                    .unwrap()
                                    .join("project-defaults.json"),
                            )
                            .unwrap_or_else(|| ProjectDefaults::from_project(&self.project));
                            self.presets.next_defaults = Some(if imported.remember {
                                imported.settings
                            } else {
                                previous
                            });
                            if let Some(project) = imported.project {
                                let before = self.project.clone();
                                self.player.stop();
                                self.project = project;
                                self.layers = Default::default();
                                self.evidence_view = Default::default();
                                self.avatar_preview = None;
                                self.preview_final = false;
                                self.preview_scene = None;
                                self.texture = None;
                                self.blurred_texture = None;
                                self.timeline_view.history.record(before);
                                self.last_save = Instant::now();
                                self.dirty = false;
                                self.history = project_store::list(&self.projects_root);
                                self.library_cache.clear();
                                self.request_frame();
                            }
                            self.presets.open = false;
                            self.presets.loaded = None;
                            self.message("Đã nhập thiết lập. Voice, nhân vật và ảnh dẫn chứng đã tạo cần chạy lại bước tương ứng nếu đổi cấu hình tạo nội dung.",false);
                        }
                    }
                }
            }
        }
    }
    pub(super) fn preset_dialog(&mut self, ctx: &egui::Context) {
        if !self.presets.open {
            return;
        }
        let mut close = false;
        let mut export = false;
        let mut import = false;
        let mut replace = None;
        let modal=egui::Modal::new(egui::Id::new("preset_dialog")).show(ctx,|ui| {
            ui.set_width((ctx.screen_rect().width()-100.).clamp(280.,580.));
            ui.spacing_mut().interact_size.y=34.;
            ui.heading(if self.presets.export{"Xuất thiết lập"}else{"Nhập thiết lập"});
            ui.add_space(6.);
            if self.presets.running {
                ui.horizontal(|ui|{ui.spinner();ui.label("Đang xử lý ở nền");});
                ui.add(egui::Label::new(&self.presets.message).wrap());
                ui.label(RichText::new("Chờ hoàn tất trước khi đóng app.").small().color(MUTED));return;
            }
            egui::ScrollArea::vertical().max_height((ctx.screen_rect().height()-260.).max(140.)).show(ui,|ui| {
                if self.presets.export {
                    ui.add(egui::Label::new("Xuất các thông số hiện tại để dùng trên máy khác, kể cả thay đổi chưa lưu.").wrap());
                    let mut summary=ProjectDefaults::from_project(&self.project);
                    if summary.production.is_none(){summary.production=Some(ProductionSettings::from_settings(&self.settings));}
                    settings_summary(ui,&summary);
                    ui.separator();
                    ui.checkbox(&mut self.presets.include_assets,"Kèm ảnh nhân vật, giọng mẫu, nhạc và LUT đang chọn");
                    if !self.presets.include_assets {ui.colored_label(egui::Color32::YELLOW,"Máy nhận cần chọn lại đúng các file tài nguyên trước khi nhập.");}
                    ui.add(egui::Label::new(RichText::new("Giữ cấu hình kết nối, cookie, đăng nhập và nơi lưu theo máy nhận. Video nguồn và nội dung timeline giữ riêng trong dự án.").small().color(MUTED)).wrap());
                }else if let Some(loaded)=&self.presets.loaded {
                    ui.add(egui::Label::new(format!("Bộ thiết lập: {}",loaded.manifest.name)).wrap());
                    ui.label(RichText::new(format!("AIR3view {} • {} tài nguyên • {:.1} MB",loaded.manifest.app_version,loaded.manifest.assets.len(),loaded.manifest.assets.iter().map(|a|a.bytes).sum::<u64>() as f64/1048576.)).small().color(MUTED));
                    settings_summary(ui,&loaded.manifest.settings);
                    ui.separator();
                    let a=ui.checkbox(&mut self.presets.apply_current,"Áp dụng cho dự án hiện tại");
                    let b=ui.checkbox(&mut self.presets.remember,"Ghi nhớ cho các dự án tiếp theo");
                    #[cfg(test)] {self.hitboxes.insert("preset_current".into(),a.rect);self.hitboxes.insert("preset_remember".into(),b.rect);}
                    #[cfg(not(test))] let _=(a,b);
                    if self.presets.apply_current&&!self.project.segments.is_empty() {
                        ui.add(egui::Label::new(RichText::new("Sẽ thay các thiết lập/layer cùng loại. Giữ URL và nội dung cảnh; tốc độ footage mới có thể điều chỉnh thời lượng cảnh gốc. Đổi giọng, nhân vật, ngôn ngữ hoặc kiểu dẫn chứng cần tạo lại nội dung tương ứng.").color(egui::Color32::YELLOW)).wrap());
                    }
                    for asset in &loaded.manifest.assets {
                        ui.horizontal_wrapped(|ui| {
                            let ready=asset.included||self.presets.replacements.contains_key(&asset.id);
                            ui.colored_label(if ready{TEAL}else{egui::Color32::YELLOW},if ready{"Có sẵn"}else{"Cần chọn"});ui.add(egui::Label::new(format!("{}: {}",asset.kind.label(),asset.name)).wrap());
                            if !asset.included&&ui.button("Chọn file…").clicked(){replace=Some(asset.clone());}
                        });
                    }
                    ui.add(egui::Label::new(RichText::new(format!("Phụ đề dùng font ‘{}’. Nếu máy này chưa có font, hãy cài font hoặc chọn font khác trước khi xuất video.",loaded.manifest.settings.subtitles.font)).small().color(MUTED)).wrap());
                }
                if !self.presets.error.is_empty(){ui.add_space(6.);ui.add(egui::Label::new(RichText::new(&self.presets.error).color(egui::Color32::LIGHT_RED)).wrap());}
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                let cancel=ui.add(egui::Button::new("Hủy").min_size(Vec2::new(100.,34.)));close=cancel.clicked();
                let enabled=self.presets.export||self.presets.loaded.as_ref().is_some_and(|p|p.missing().all(|a|self.presets.replacements.contains_key(&a.id)))&&(self.presets.apply_current||self.presets.remember);
                let apply=ui.add_enabled(enabled,egui::Button::new(if self.presets.export{"Chọn nơi lưu & xuất…"}else{"Nhập thiết lập"}).fill(BLUE).min_size(Vec2::new(180.,34.)));
                if apply.clicked(){if self.presets.export{export=true;}else{import=true;}}
                #[cfg(test)] {self.hitboxes.insert("preset_cancel".into(),cancel.rect);self.hitboxes.insert("preset_confirm".into(),apply.rect);}
            });
        });
        #[cfg(test)]
        self.hitboxes
            .insert("preset_dialog".into(), modal.response.rect);
        if (close || modal.should_close()) && !self.presets.running {
            self.presets.open = false;
            self.presets.loaded = None;
        }
        if let Some(asset) = replace {
            if let Some(path) = rfd::FileDialog::new()
                .set_title(format!("Chọn {}: {}", asset.kind.label(), asset.name))
                .add_filter(asset.kind.label(), asset.kind.extensions())
                .pick_file()
            {
                self.presets.replacements.insert(asset.id, path);
            }
        }
        if export {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Xuất thiết lập AIR3view")
                .set_file_name("AIR3view-settings.air3preset")
                .add_filter("Thiết lập AIR3view", &["air3preset"])
                .save_file()
            {
                if path.extension().and_then(|s|s.to_str()).is_some_and(|s|s.eq_ignore_ascii_case("air3preset")) {
                    self.write_preset(path);
                }else{
                    self.presets.error="Tên file cần kết thúc bằng .air3preset; hãy chọn lại nơi lưu.".into();
                }
            }
        }
        if import {
            self.import_preset();
        }
    }
}
fn settings_summary(ui: &mut egui::Ui, s: &ProjectDefaults) {
    ui.add_space(6.);
    let (w, h) = s.output.dimensions();
    for line in [
        format!(
            "Kịch bản: {} • {}–{} giây • hook {}",
            crate::content_language::label(&s.rules.language),
            s.rules.min_seconds,
            s.rules.max_seconds,
            if s.rules.hook_enabled {
                "bật"
            } else {
                "tắt"
            }
        ),
        format!(
            "Hình ảnh: {w} × {h} • {} FPS • footage {}",
            s.output.fps, match s.output.footage_speed.mode.as_str(){"off"=>"1×","auto"=>"Tự động",_=>"Cố định"}
        ),
        format!(
            "Giọng/nhân vật: {} • {}",
            s.production
                .as_ref()
                .map(|p| p.voice.as_str())
                .unwrap_or("Theo máy"),
            s.production
                .as_ref()
                .map(|p| if p.avatar_enabled {
                    if p.avatar_provider=="cartoon2d" {"Nhân vật 2D"}else{"HeyGen"}
                } else {
                    "Tắt nhân vật"
                })
                .unwrap_or("Theo máy")
        ),
        format!(
            "Phụ đề: {} • cỡ {} • X/Y {}/{}",
            if s.subtitles.enabled {
                "bật"
            } else {
                "tắt"
            },
            s.subtitles.size,
            s.subtitles.offset_x,
            s.subtitles.offset_y
        ),
        format!(
            "Dẫn chứng: {} • crop {}% • SFX {}",
            if s.evidence_settings.presentation=="stack" {"Xếp chồng"}else{"Bố cục thẻ"},
            s.evidence_settings.crop_percent,
            s.evidence_settings.sound_style
        ),
        format!(
            "Layer: {} mẫu thông số • {} mẫu màu/lật",
            s.layer_defaults.presets.len(),
            s.layer_defaults.effects.len()
        ),
        "Kèm cấu hình nhạc/tiếng gốc, thẻ thông tin, track và metadata.".into(),
    ] {
        ui.add(egui::Label::new(line).wrap());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wait(app: &mut StudioApp) {
        let start = Instant::now();
        while app.presets.running && start.elapsed() < Duration::from_secs(8) {
            thread::sleep(Duration::from_millis(10));
            app.poll_presets();
        }
        assert!(!app.presets.running, "Preset worker did not complete");
        assert!(app.presets.error.is_empty(), "{}", app.presets.error);
    }
    fn input(ctx: &egui::Context, app: &mut StudioApp, screen: Rect, events: Vec<egui::Event>) {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |ctx| app.draw(ctx),
        );
    }
    fn click(ctx: &egui::Context, app: &mut StudioApp, screen: Rect, key: &str) {
        let pos = app.hitboxes[key].center();
        for pressed in [true, false] {
            input(
                ctx,
                app,
                screen,
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
        input(ctx, app, screen, vec![]);
    }
    #[test]
    fn native_preset_menu_and_dialog_fit_small_and_large_windows_and_cancel_keeps_settings() {
        for (w, h) in [(1080., 720.), (1440., 900.), (1920., 1080.)] {
            let ctx = egui::Context::default();
            let mut app = StudioApp::from_context(&ctx);
            app.capture_path = None;
            ctx.style_mut(|s| s.animation_time = 0.);
            app.page = Page::Editor;
            app.setup_open = true;
            let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(w, h));
            let before = serde_json::to_vec(&app.project).unwrap();
            for _ in 0..3 {
                input(&ctx, &mut app, screen, vec![]);
            }
            for key in ["preset_menu", "top_open", "top_save", "top_export"] {
                assert!(
                    screen.contains_rect(app.hitboxes[key]),
                    "{key} clipped at {w}: {:?}",
                    app.hitboxes[key]
                );
            }
            click(&ctx, &mut app, screen, "preset_menu");
            click(&ctx, &mut app, screen, "preset_export");
            for _ in 0..3 {
                input(&ctx, &mut app, screen, vec![]);
            }
            for key in ["preset_dialog", "preset_cancel", "preset_confirm"] {
                assert!(
                    screen.contains_rect(app.hitboxes[key]),
                    "{key} clipped at {w}: {:?}",
                    app.hitboxes[key]
                );
            }
            assert_eq!(
                app.hitboxes["preset_cancel"].height(),
                app.hitboxes["preset_confirm"].height()
            );
            click(&ctx, &mut app, screen, "preset_cancel");
            assert!(!app.presets.open);
            assert_eq!(before, serde_json::to_vec(&app.project).unwrap());
        }
    }
    #[test]
    fn native_import_confirms_applies_restarts_and_reuses_for_new_and_batch_projects() {
        let temp = tempfile::tempdir().unwrap();
        let mut source = Project::default();
        source.output.portrait = false;
        source.rules.language = "en".into();
        source.subtitles.offset_y = -210;
        let path = temp.path().join("settings.air3preset");
        package::export(&source, &Settings::default(), &path, true, &|_| {}).unwrap();
        let ctx = egui::Context::default();
        let mut app = StudioApp::from_context(&ctx);
        app.capture_path = None;
        app.page = Page::Editor;
        ctx.style_mut(|s| s.animation_time = 0.);
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1080., 720.));
        let root = app.projects_root.parent().unwrap().to_owned();
        let machine = serde_json::to_value(&app.settings).unwrap();
        app.read_preset(path);
        wait(&mut app);
        for _ in 0..3 {
            input(&ctx, &mut app, screen, vec![]);
        }
        for key in [
            "preset_dialog",
            "preset_current",
            "preset_remember",
            "preset_confirm",
        ] {
            assert!(screen.contains_rect(app.hitboxes[key]));
        }
        click(&ctx, &mut app, screen, "preset_confirm");
        wait(&mut app);
        assert!(!app.presets.open && !app.project.output.portrait);
        assert_eq!(app.project.rules.language, "en");
        assert_eq!(app.project.subtitles.offset_y, -210);
        assert_eq!(machine, serde_json::to_value(&app.settings).unwrap());
        let saved = Project::load(&app.project.root.join("project.json")).unwrap();
        assert_eq!(saved.subtitles.offset_y, -210);
        app.new_project();
        assert!(!app.project.output.portrait);
        assert_eq!(app.project.rules.language, "en");
        assert!(app.presets.next_defaults.is_none());
        let b = crate::batch::create(
            &root,
            "Imported batch",
            "https://youtu.be/dBy-8s6ly08\nhttps://youtu.be/r1cvbJJ5G54",
            &app.project,
            &app.settings,
            false,
            None,
        )
        .unwrap();
        for item in &b.items {
            let p = Project::load(&crate::batch::project_path(&root, item).unwrap()).unwrap();
            assert_eq!(p.subtitles.offset_y, -210);
            assert_eq!(p.rules.language, "en");
        }
        let mut restarted = Project::default();
        ProjectDefaults::load(&root.join("project-defaults.json"))
            .unwrap()
            .apply(&mut restarted);
        assert!(!restarted.output.portrait);
    }
    #[test]
    fn import_destinations_are_independent_and_busy_projects_are_protected() {
        let temp = tempfile::tempdir().unwrap();
        let mut source = Project::default();
        source.subtitles.offset_x = 310;
        let path = temp.path().join("settings.air3preset");
        package::export(&source, &Settings::default(), &path, true, &|_| {}).unwrap();
        for current in [true, false] {
            let ctx = egui::Context::default();
            let mut app = StudioApp::from_context(&ctx);
            app.capture_path = None;
            app.project.subtitles.offset_x = 20;
            app.remember_project_defaults().unwrap();
            app.read_preset(path.clone());
            wait(&mut app);
            app.presets.apply_current = current;
            app.presets.remember = !current;
            app.busy = true;
            app.import_preset();
            assert!(!app.presets.running);
            assert!(!app.presets.error.is_empty());
            app.busy = false;
            app.presets.error.clear();
            app.import_preset();
            wait(&mut app);
            assert_eq!(
                app.project.subtitles.offset_x,
                if current { 310 } else { 20 }
            );
            app.remember_project_defaults().unwrap();
            app.new_project();
            assert_eq!(
                app.project.subtitles.offset_x,
                if current { 20 } else { 310 }
            );
        }
    }
}
