use crate::{
    media,
    model::*,
    pipeline::{self, Event},
    player::{Player, PlayerEvent},
    process, project_store, providers,
};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use anyhow::Context;
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{atomic::Ordering, mpsc},
    thread,
    time::{Duration, Instant},
};

const BG: Color32 = Color32::from_rgb(10, 16, 29);
const PANEL: Color32 = Color32::from_rgb(17, 27, 45);
const CARD: Color32 = Color32::from_rgb(23, 36, 57);
const EDGE: Color32 = Color32::from_rgb(44, 61, 83);
const BLUE: Color32 = Color32::from_rgb(56, 139, 253);
const TEAL: Color32 = Color32::from_rgb(40, 206, 186);
const MUTED: Color32 = Color32::from_rgb(144, 162, 185);
#[path = "timeline_ui.rs"]
mod timeline_ui;
use timeline_ui::TimelineView;
#[path = "editor_ui.rs"]
mod editor_ui;
use editor_ui::EditorLayout;
#[path = "navigation_ui.rs"]
mod navigation_ui;
#[path = "scene_inspector_ui.rs"]
mod scene_inspector_ui;
#[path = "project_setup_ui.rs"]
mod project_setup_ui;
#[path = "layer_ui.rs"]
mod layer_ui;
#[path="evidence_ui.rs"]
mod evidence_ui;
#[path="metadata_ui.rs"]
mod metadata_ui;
#[path="updates_ui.rs"]
mod updates_ui;
#[derive(PartialEq, Clone, Copy, Debug)]
enum Page {
    Editor,
    New,
    Projects,
    Queue,
    Settings,
}
enum Task {
    Automatic { resume: bool },
    Prepare,
    Assets,
    Evidence,
    Speech,
    Render,
    Metadata { input: PathBuf, output: PathBuf },
    InspectMetadata(PathBuf),
    Transcribe,
    Import(PathBuf),
}
pub struct StudioApp {
    project: Project,
    settings: Settings,
    page: Page,
    selected: usize,
    inspector: usize,
    urls: String,
    setup_open: bool,
    auto_running: bool,
    workflow_state: Option<crate::workflow::JobState>,
    notice: String,
    is_error: bool,
    logs: Vec<String>,
    lanes: BTreeMap<String, (String, bool)>,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
    cancel: process::Cancel,
    busy: bool,
    player: Player,
    texture: Option<egui::TextureHandle>,
    blurred_texture: Option<egui::TextureHandle>,
    playhead: u64,
    preview_final: bool,
    preview_scene: Option<(PathBuf, u64)>,
    zoom: f32,
    timeline_view: TimelineView,
    editor_layout: EditorLayout,
    branding_icon: egui::TextureHandle,
    dirty: bool,
    last_save: Instant,
    history: Vec<PathBuf>,
    library_cache: project_store::SummaryCache,
    projects_root: PathBuf,
    pending_delete: Option<project_store::DeleteInfo>,
    diagnostics: String,
    cli_args_text: String,
    heygen_key_input: String,
    avatar_preview: Option<(String, egui::TextureHandle)>,
    layers: layer_ui::LayerView,
    evidence_view: evidence_ui::EvidenceView,
    export_location: crate::export_location::ExportLocation,
    metadata_open: bool,
    metadata_actual: String,
    updates: crate::updates::Updates,
    range_source: String,
    range_start: f64,
    range_end: f64,
    capture_path: Option<PathBuf>,
    capture_requested: bool,
    frames: u32,
    #[cfg(test)]
    hitboxes: BTreeMap<String, Rect>,
}
fn panel() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0, EDGE))
        .corner_radius(12)
        .inner_margin(16)
}
fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(12.0).strong().color(MUTED));
    ui.add_space(8.0);
}
fn primary(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).strong().color(Color32::WHITE))
            .fill(BLUE)
            .min_size(Vec2::new(116.0, 34.0))
            .corner_radius(8),
    )
}
fn seconds(ms: u64) -> String {
    format!(
        "{:02}:{:02}.{:01}",
        ms / 60000,
        ms / 1000 % 60,
        ms / 100 % 10
    )
}
fn chip(ui: &mut egui::Ui, label: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.13))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(9, 4))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(11.0).strong().color(color));
        });
}
fn field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.label(RichText::new(label).color(MUTED).size(12.0));
    ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
    ui.add_space(8.0);
}
impl StudioApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::from_context(&cc.egui_ctx)
    }
    fn from_context(ctx: &egui::Context) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BG;
        visuals.window_fill = PANEL;
        visuals.faint_bg_color = PANEL;
        visuals.extreme_bg_color = BG;
        visuals.widgets.noninteractive.bg_fill = PANEL;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, EDGE);
        visuals.widgets.inactive.bg_fill = CARD;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, EDGE);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(33, 55, 82);
        visuals.widgets.active.bg_fill = BLUE.gamma_multiply(0.4);
        visuals.selection.bg_fill = BLUE;
        ctx.set_visuals(visuals);
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = Vec2::new(9.0, 8.0);
        style.spacing.button_padding = Vec2::new(12.0, 8.0);
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.0));
        ctx.set_style(style);
        let mut fonts = egui::FontDefinitions::default();
        for path in ["C:/Windows/Fonts/segoeui.ttf", "C:/Windows/Fonts/arial.ttf"] {
            if let Ok(bytes) = fs::read(path) {
                fonts.font_data.insert(
                    "vietnamese".into(),
                    egui::FontData::from_owned(bytes).into(),
                );
                fonts
                    .families
                    .get_mut(&egui::FontFamily::Proportional)
                    .unwrap()
                    .insert(0, "vietnamese".into());
                break;
            }
        }
        ctx.set_fonts(fonts);
        let root = data_dir();
        #[cfg(test)]
        let root = { let _ = root; std::env::temp_dir().join(format!("reaction-ui-{}", uuid::Uuid::new_v4())) };
        let mut settings = Settings::load(&root.join("settings.json")).unwrap_or_default();
        // Migrate a key accidentally pasted into the environment-name field into
        // the masked, in-memory editor. Save persists it to User environment only.
        let heygen_key_input = if !providers::is_environment_name(&settings.heygen_key_env) {
            std::mem::replace(&mut settings.heygen_key_env, "HEYGEN_API_KEY".into())
        } else {
            String::new()
        };
        let history = project_store::list(&root.join("projects"));
        let (tx, rx) = mpsc::channel();
        let mut project = Project::default();
        if let Some(defaults) = ProjectDefaults::load(&root.join("project-defaults.json"))
            .or_else(|| history.first().and_then(|path| Project::load(path).ok()).map(|p| ProjectDefaults::from_project(&p))) {
            defaults.apply(&mut project);
        }
        project.root = root.join("projects").join(&project.id);
        if let Some(path) = std::env::args().skip(1).find(|a| a.ends_with(".json")) {
            if let Ok(p) = Project::load(std::path::Path::new(&path)) {
                project = p;
            }
        }
        let cli_args_text = serde_json::to_string(&settings.cli_args).unwrap();
        let capture_path =
            std::env::args().find_map(|a| a.strip_prefix("--capture-ui=").map(PathBuf::from));
        let mut editor_layout = EditorLayout::load(&root.join("editor-layout.json"));
        editor_layout.apply_args(std::env::args());
        let page = if std::env::args().any(|a| a == "--page=settings") {
            Page::Settings
        } else if std::env::args().any(|a| a == "--page=new") {
            Page::New
        } else if std::env::args().any(|a| a == "--page=projects") {
            Page::Projects
        } else {
            Page::Editor
        };
        if project.production.is_none() { project.production = Some(ProductionSettings::from_settings(&settings)); }
        let urls = if project.input_urls.is_empty() { project.sources.iter().filter(|src| !src.url.is_empty()).map(|src| src.url.as_str()).collect::<Vec<_>>().join("\n") } else { project.input_urls.clone() };
        project.input_urls = urls.clone();
        let workflow_state = crate::workflow::JobState::load(&project.root).filter(|job|job.project_id==project.id);
        let setup_open = std::env::args().any(|a|a=="--editor-tab=setup" || a=="--page=new")
            || (project.segments.is_empty() && !std::env::args().any(|a|a=="--editor-tab=edit"))
            || workflow_state.as_ref().is_some_and(|job|job.can_resume());
        #[cfg(test)] let setup_open = { let _ = setup_open; false };
        let selected = std::env::args()
            .find_map(|arg| arg.strip_prefix("--scene=").and_then(|value| value.parse::<usize>().ok()))
            .unwrap_or(1).saturating_sub(1).min(project.segments.len().saturating_sub(1));
        let mut app = Self {
            project,
            settings,
            page,
            selected,
            inspector: if std::env::args().any(|a|a=="--inspector=evidence"){3}else if std::env::args().any(|a| a == "--inspector=output") { 1 } else { 0 },
            urls,
            setup_open,
            auto_running: false,
            workflow_state,
            notice: String::new(),
            is_error: false,
            logs: vec![],
            lanes: BTreeMap::new(),
            tx,
            rx,
            cancel: process::cancel_token(),
            busy: false,
            player: Player::default(),
            texture: None,
            blurred_texture: None,
            playhead: 0,
            preview_final: false,
            preview_scene: None,
            zoom: 1.0,
            timeline_view: Default::default(),
            editor_layout,
            branding_icon: crate::branding::logo(ctx),
            dirty: false,
            last_save: Instant::now(),
            history,
            library_cache: Default::default(),
            projects_root: root.join("projects"),
            pending_delete: None,
            diagnostics: String::new(),
            cli_args_text,
            heygen_key_input,
            avatar_preview: None,
            layers: Default::default(),
            evidence_view:Default::default(),
            export_location: crate::export_location::ExportLocation::load(root.join("export-location.json")),
            metadata_open: std::env::args().any(|a|a=="--metadata=open"),
            metadata_actual: String::new(),
            updates: crate::updates::Updates::new(root.join("updates.json")),
            range_source: "src_1".into(),
            range_start: 0.0,
            range_end: 10.0,
            capture_path,
            capture_requested: false,
            frames: 0,
            #[cfg(test)]
            hitboxes: BTreeMap::new(),
        };
        app.restore_job_notice();
        if std::env::args().any(|a|a=="--updates=open") { app.updates.open=true; }
        app.timeline_view.cursor=crate::timeline::starts(&app.project).get(app.selected).copied().unwrap_or(0);
        if let Some(ms)=std::env::args().find_map(|a|a.strip_prefix("--preview-ms=").and_then(|v|v.parse().ok())) {app.timeline_view.cursor=ms;}
        if std::env::args().any(|a|a=="--layer=avatar"){app.select_timeline_layer(crate::timeline::AVATAR);}
        app.request_frame();
        app
    }
    fn read_history() -> Vec<PathBuf> {
        let mut paths: Vec<_> = fs::read_dir(data_dir().join("projects"))
            .into_iter()
            .flatten()
            .filter_map(|d| d.ok().map(|d| d.path().join("project.json")))
            .filter(|p| p.is_file())
            .collect();
        paths.sort_by_key(|p| std::cmp::Reverse(fs::metadata(p).and_then(|m| m.modified()).ok()));
        paths
    }
    fn new_project(&mut self) {
        if self.busy { self.message("Chờ tác vụ hoàn tất hoặc hủy trước khi tạo dự án khác.",true); return; }
        if let Err(e) = self.remember_project_defaults() {
            self.message(format!("Không lưu được cấu hình mẫu: {e:#}"), true);
            return;
        }
        if !self.project.sources.is_empty() {
            let _ = self.project.save();
        }
        let mut p = Project::default();
        ProjectDefaults::from_project(&self.project).apply(&mut p);
        p.root = self.projects_root.join(&p.id);
        self.project = p;
        self.page = Page::Editor;
        self.setup_open = true;
        self.workflow_state = None;
        self.selected = 0;
        self.texture = None;
        self.player.stop();
        self.preview_final = false;
        self.preview_scene = None;
        self.dirty = false;
        self.playhead = 0;
        self.urls.clear();
        self.notice.clear();
    }
    fn message(&mut self, text: impl Into<String>, error: bool) {
        self.notice = text.into();
        self.is_error = error;
        let _ = crate::diagnostics::record(
            &self.project.root,
            "UI",
            None,
            if error { "error" } else { "notice" },
            serde_json::json!({"message":crate::diagnostics::safe_text(&self.notice, &[])}),
        );
        self.logs.push(self.notice.clone());
        if self.logs.len() > 300 {
            self.logs.remove(0);
        }
    }
    fn save(&mut self) {
        match self.project.save() {
            Ok(()) => {
                self.dirty = false;
                self.last_save = Instant::now();
                self.history = Self::read_history();
                if let Err(e) = self.remember_project_defaults() {
                    self.message(format!("Dự án đã lưu nhưng không lưu được cấu hình mẫu: {e:#}"), true);
                }
            }
            Err(e) => self.message(format!("Không lưu được project: {e:#}"), true),
        }
    }
    fn remember_project_defaults(&self) -> anyhow::Result<()> {
        let root = self.projects_root.parent().context("Thiếu thư mục cấu hình")?;
        write_json(&root.join("project-defaults.json"), &ProjectDefaults::from_project(&self.project))
    }
    fn restore_job_notice(&mut self) {
        self.notice.clear();self.is_error=false;
        if let Some(job)=&self.workflow_state {
            if job.status==crate::workflow::Status::Failed {
                self.notice=job.message.clone();self.is_error=true;
            }
        }
    }
    fn open(&mut self, path: PathBuf) {
        if self.busy { self.message("Chờ tác vụ hoàn tất hoặc hủy trước khi mở dự án khác.",true); return; }
        match Project::load(&path) {
            Ok(p) => {
                self.player.stop();
                self.project = p;
                self.layers=Default::default();
                self.timeline_view.cursor=0;
                self.setup_open = self.project.segments.is_empty();
                self.workflow_state = crate::workflow::JobState::load(&self.project.root).filter(|job|job.project_id==self.project.id);
                self.restore_job_notice();
                self.setup_open |= self.workflow_state.as_ref().is_some_and(|job|job.can_resume());
                self.urls = if self.project.input_urls.is_empty() { self.project.sources.iter().filter(|src|!src.url.is_empty()).map(|src|src.url.as_str()).collect::<Vec<_>>().join("\n") } else {self.project.input_urls.clone()};
                self.selected = 0;
                self.playhead = 0;
                self.preview_final = false;
                self.texture = None;
                self.page = Page::Editor;
                self.request_frame();
            }
            Err(e) => self.message(format!("{e:#}"), true),
        }
    }
    fn apply_plan(&mut self, plan: ScriptPlan) -> anyhow::Result<()> {
        let mut p = self.project.clone();
        pipeline::apply_plan(&mut p,plan)?;
        p.validate(!p.sources.iter().any(|s| s.path.as_os_str().is_empty()))?;
        p.validate_opening_hook()?;
        p.check_duration()?;
        p.rendered = None;
        self.project = p;
        self.selected = 0;
        self.playhead = 0;
        self.preview_final = false;
        self.save();
        self.request_frame();
        Ok(())
    }
    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Workflow(job) => { if job.project_id==self.project.id { self.workflow_state=Some(job); } }
                Event::Stage {
                    lane,
                    message,
                    done,
                } => {
                    self.logs.push(format!("{lane}: {message}"));
                    if self.logs.len() > 300 {
                        self.logs.remove(0);
                    }
                    self.lanes.insert(lane, (message, done));
                }
                Event::Log(text) => self.diagnostics = text,
                Event::Metadata {project_id,text} => {if project_id==self.project.id {self.metadata_actual=text;}}
                Event::Project(p) => {
                    if p.id != self.project.id { continue; }
                    self.timeline_view.history = Default::default();
                    self.project = *p;
                    self.selected = self
                        .selected
                        .min(self.project.segments.len().saturating_sub(1));
                    self.dirty = false;
                }
                Event::Preview { path, duration_ms } => {
                    self.timeline_view.full_preview = false;
                    self.preview_scene = Some((path, duration_ms));
                    self.preview_final = true;
                    self.playhead = 0;
                }
                Event::TimelinePreview { path, duration_ms, start_ms } => {
                    self.preview_scene = Some((path.clone(), duration_ms));
                    self.preview_final = true;
                    self.timeline_view.full_preview = true;
                    self.playhead = start_ms.min(duration_ms.saturating_sub(1));
                    self.timeline_view.cursor = self.playhead;
                    self.player.play(self.settings.clone(), path, self.playhead, duration_ms);
                }
                Event::Done(message) => {
                    self.busy = false;
                    if self.auto_running {
                        self.auto_running=false;
                        self.setup_open=false;
                        self.page=Page::Editor;
                        self.preview_final=false;
                        self.timeline_view.full_preview=false;
                    }
                    self.message(message, false);
                    self.history = Self::read_history();
                    if !self.player.playing { self.request_frame(); }
                }
                Event::Error(message) => {
                    self.busy = false;
                    if self.auto_running { self.auto_running=false; self.setup_open=true; self.page=Page::Editor; }
                    self.project.status = "Cần kiểm tra".into();
                    let _ = self.project.save();
                    self.message(message, true);
                }
            }
        }
        while let Ok(event) = self.player.rx.try_recv() {
            match event {
                PlayerEvent::Layers {generation,ms,frames} if generation==self.player.generation => {
                    self.layers.live=true;
                    self.preview_final=false;self.preview_scene=None;
                    self.playhead=ms;self.timeline_view.cursor=ms;
                    if let Some((i,_))=crate::timeline::locate(&self.project,ms){self.selected=i;}
                    let comp=self.project.composition_snapshot();
                    self.layers.textures.retain(|id,_|frames.iter().any(|(i,_)|i==id)||comp.clips.iter().any(|c|c.id==id.trim_end_matches("#blur")&&c.still()&&c.enabled(&self.project)&&c.active(ms)));
                    self.layers.alpha.retain(|id,_|self.layers.textures.contains_key(id));
                    for (id,frame) in frames {
                        let small=image::imageops::resize(&frame,96,96,image::imageops::FilterType::Nearest);
                        self.layers.alpha.insert(id.clone(),image::GrayImage::from_fn(96,96,|x,y|image::Luma([small.get_pixel(x,y)[3]])));
                        let image=egui::ColorImage::from_rgba_unmultiplied([frame.width() as usize,frame.height() as usize],frame.as_raw());
                        if let Some(texture)=self.layers.textures.get_mut(&id){texture.set(image,egui::TextureOptions::LINEAR);}
                        else {self.layers.textures.insert(id.clone(),ctx.load_texture(id,image,egui::TextureOptions::LINEAR));}
                    }
                }
                PlayerEvent::Frame {
                    generation,
                    ms,
                    image,
                } if generation == self.player.generation => {
                    self.playhead = ms;
                    self.sync_timeline_cursor();
                    let small = image::DynamicImage::ImageRgba8(image.clone()).thumbnail(160, 160).blur(8.0).to_rgba8();
                    let blurred = egui::ColorImage::from_rgba_unmultiplied(
                        [small.width() as usize, small.height() as usize], small.as_raw());
                    if let Some(t) = &mut self.blurred_texture {
                        t.set(blurred, egui::TextureOptions::LINEAR);
                    } else {
                        self.blurred_texture = Some(ctx.load_texture("preview-blur", blurred, egui::TextureOptions::LINEAR));
                    }
                    let size = [image.width() as usize, image.height() as usize];
                    let image = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
                    if let Some(t) = &mut self.texture {
                        t.set(image, egui::TextureOptions::LINEAR);
                    } else {
                        self.texture =
                            Some(ctx.load_texture("preview", image, egui::TextureOptions::LINEAR));
                    }
                }
                PlayerEvent::Stopped { generation } if generation == self.player.generation => {
                    self.player.playing = false
                }
                PlayerEvent::Error {
                    generation,
                    message,
                } if generation == self.player.generation => {
                    self.player.playing = false;
                    self.message(message, true);
                }
                _ => {}
            }
        }
        if self.dirty && !self.busy && self.last_save.elapsed() > Duration::from_secs(2) {
            self.save();
        }
    }
    fn launch(&mut self, task: Task) {
        if self.busy {
            return;
        }
        if matches!(task, Task::Render) {
            if let Err(error) = self
                .project
                .validate(true)
                .and_then(|_| self.project.check_duration())
            {
                self.message(format!("{error:#}"), true);
                return;
            }
        }
        let export_path = if matches!(task, Task::Render) {
            match self.export_location.resolve(&self.project, || {
                rfd::FileDialog::new().set_title("Chọn thư mục lưu video — AIR3view sẽ ghi nhớ cho lần sau").pick_folder()
            }) {
                Ok(Some(path)) => Some(path),
                Ok(None) => return,
                Err(error) => { self.message(format!("{error:#}"), true); return; }
            }
        } else { None };
        if matches!(task, Task::Prepare | Task::Automatic { resume: false }) {
            match self.planned_sources() {
                Ok(sources) if !sources.is_empty() => {
                    let same = sources.len()==self.project.sources.len() && sources.iter().zip(&self.project.sources).all(|(a,b)|a.id==b.id && a.url==b.url && a.path==b.path);
                    self.project.sources=sources;
                    if !same { self.project.segments.clear(); }
                    self.project.input_urls=self.urls.clone();
                }
                Ok(_) => { self.message("Thêm ít nhất một URL hoặc video từ máy.",true); return; }
                Err(e)=> { self.message(format!("{e:#}"),true); return; }
            }
        }
        if self.project.production.is_none() { self.project.production=Some(ProductionSettings::from_settings(&self.settings)); }
        if matches!(task, Task::Automatic { .. }) {
            let effective=crate::workflow::effective_settings(&self.project,&self.settings);
            if let Err(error)=crate::workflow::preflight(&self.project,&effective) { self.message(format!("{error:#}"),true); return; }
        }
        self.dirty=true;
        self.save();
        if self.dirty { return; }
        self.auto_running=matches!(task,Task::Automatic { .. });
        self.player.stop();
        self.busy = true;
        self.notice.clear();
        self.lanes.clear();
        self.cancel = process::cancel_token();
        let p = self.project.clone();
        let s = crate::workflow::effective_settings(&p,&self.settings);
        let tx = self.tx.clone();
        let cancel = self.cancel.clone();
        let completion = match &task {
            Task::Metadata {output,..}=>format!("Đã cập nhật metadata, giữ nguyên hình/tiếng: {}",output.display()),
            Task::InspectMetadata(_)=>"Đã đọc metadata thực tế của MP4".into(),
            _=>export_path.as_ref().map(|path| format!("Đã xuất video: {}", path.display())).unwrap_or_else(|| "Tác vụ hoàn tất".into()),
        };
        thread::spawn(move || {
            let result = (|| -> anyhow::Result<Project> {
                match task {
                    Task::Automatic { resume } => crate::workflow::run(p,s,tx.clone(),cancel,resume),
                    Task::Prepare => pipeline::prepare(p, s, tx.clone(), cancel),
                    Task::Assets => pipeline::generate_assets(p, s, tx.clone(), cancel),
                    Task::Evidence => pipeline::generate_evidence(p,s,tx.clone(),cancel),
                    Task::Speech => pipeline::generate_speech(p, s, tx.clone(), cancel),
                    Task::Transcribe => {
                        pipeline::transcribe(p, s, worker_path("transcribe.py"), tx.clone(), cancel)
                    }
                    Task::Import(path) => {
                        let mut p = p;
                        let mut next=1;
                        while p.sources.iter().any(|s|s.id==format!("src_{next}")){next+=1;}
                        let id = format!("src_{next}");
                        let mut source = media::import_local(&s, &path, id.clone(), &cancel)?;
                        source.proxy =
                            media::proxy(&s, &source, &p.root.join("cache/proxy"), &cancel)?;
                        if p.segments.is_empty() {
                            let dur = source.duration_ms.min(10000);
                            p.segments.push(Segment {
                                source_id: id,
                                out_ms: dur,
                                duration_ms: dur,
                                ..Default::default()
                            });
                        }
                        p.sources.push(source);
                        p.save()?;
                        Ok(p)
                    }
                    Task::Render => {
                        let mut p = p;
                        let path = export_path.context("Chưa chọn thư mục xuất video")?;
                        media::render(&p, &s, &path, &cancel, |n, t| {
                            let _ = tx.send(Event::Stage {
                                lane: "Render".into(),
                                message: format!("Đã hoàn tất {n}/{t} bước xuất video"),
                                done: n == t,
                            });
                        })?;
                        if let Ok(text)=fs::read_to_string(p.root.join("metadata-report.json")) {
                            let _=tx.send(Event::Metadata{project_id:p.id.clone(),text});
                        }
                        p.rendered = Some(path);
                        p.status = "Đã xuất video".into();
                        p.save()?;
                        Ok(p)
                    }
                    Task::Metadata {input,output} => {
                        let report=crate::export_metadata::apply_to_mp4(&p,&s,&input,&output,&cancel)?;
                        let _=tx.send(Event::Metadata{project_id:p.id.clone(),text:serde_json::to_string_pretty(&report)?});
                        let _=tx.send(Event::Stage{lane:"Metadata".into(),message:format!("Đã lưu {} — stream copy hình và tiếng",output.display()),done:true});
                        Ok(p)
                    }
                    Task::InspectMetadata(path) => {
                        let actual=crate::export_metadata::inspect(&s,&path,&cancel)?;
                        let _=tx.send(Event::Metadata{project_id:p.id.clone(),text:serde_json::to_string_pretty(&actual)?});Ok(p)
                    }
                }
            })();
            match result {
                Ok(p) => {
                    let _ = tx.send(Event::Project(Box::new(p)));
                    let _ = tx.send(Event::Done(completion));
                }
                Err(e) => {
                    let _ = tx.send(Event::Error(format!("{e:#}")));
                }
            }
        });
    }
    fn preview_path(&self) -> Option<PathBuf> {
        if self.preview_final {
            if let Some((path, _)) = &self.preview_scene {
                return Some(path.clone());
            }
            return self.project.rendered.clone().filter(|p| p.is_file());
        }
        let seg = self.project.segments.get(self.selected)?;
        if seg.kind == "host" {
            return seg.avatar.clone();
        }
        self.project
            .source(&seg.source_id)
            .ok()
            .map(|s| {
                if s.proxy.is_file() {
                    s.proxy.clone()
                } else {
                    s.path.clone()
                }
            })
            .filter(|p| p.is_file())
    }
    fn request_frame(&mut self) {
        if !self.preview_final && !self.project.segments.is_empty() {
            self.project.ensure_composition();
            self.layers.live=true;
            self.player.composition(self.project.clone(),self.settings.clone(),self.timeline_view.cursor,false);
            return;
        }
        self.layers.live=false;
        self.layers.textures.clear();
        if let Some(path) = self.preview_path() {
            let ms = if self.preview_final {
                self.playhead
            } else {
                self.project
                    .segments
                    .get(self.selected)
                    .map(|s| {
                        let local = crate::timeline::locate(&self.project, self.timeline_view.cursor)
                            .filter(|(i,_)| *i == self.selected).map(|(_,ms)| ms).unwrap_or(0);
                        local+if s.kind == "host" { s.avatar_start_ms } else { s.in_ms }
                    })
                    .unwrap_or(0)
            };
            self.playhead = ms;
            self.sync_timeline_cursor();
            self.player.seek(self.settings.clone(), path, ms);
        }
    }
    fn touch(&mut self) {
        if self.preview_final {
            self.texture=None;self.blurred_texture=None;
            self.layers.textures.clear();
        }
        let was_final=self.preview_final;
        self.player.stop();
        self.dirty = true;
        self.preview_final = false;
        self.preview_scene = None;
        self.project.rendered = None;
        self.timeline_view.full_preview=false;
        self.project.ensure_composition();
        let _=was_final;
        self.request_frame();
    }
    fn topbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("topbar")
            .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::symmetric(22, 14)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_max_width(250.0);
                        ui.label(RichText::new(match self.page {
                            Page::Editor => "Biên tập video",
                            Page::New => "Tạo dự án mới",
                            Page::Projects => "Thư viện dự án",
                            Page::Queue => "Tiến trình xử lý",
                            Page::Settings => "Cài đặt kết nối",
                        }).size(24.0).strong());
                        ui.add(egui::Label::new(RichText::new(&self.project.title).color(MUTED).size(12.0)).truncate());
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().interact_size.y = 34.0;
                        if self.busy {
                            if ui.add(egui::Button::new("Hủy tác vụ").fill(Color32::from_rgb(110,36,49))).clicked() {
                                self.cancel.store(true, Ordering::Relaxed);
                                self.message("Đang hủy. Request cloud đã gửi có thể vẫn tiếp tục trên máy chủ.", false);
                            }
                        } else {
                            let hint = self.export_location.folder().map(|folder| format!("Lưu vào {}\\AIR3view-{}\\final.mp4", folder.display(), self.project.id))
                                .unwrap_or_else(|| "Chọn thư mục lưu lần đầu; tự ghi nhớ cho những lần sau.".into());
                            let export = primary(ui, "Xuất MP4  ↗").on_hover_text(hint);
                            #[cfg(test)] self.hitboxes.insert("top_export".into(), export.rect);
                            if export.clicked() { self.launch(Task::Render); }
                            let mut choose = false;
                            let _location = ui.menu_button("Nơi lưu / metadata", |ui| {
                                ui.set_max_width(340.0);
                                ui.label("Thư mục xuất video đã ghi nhớ");
                                ui.label(self.export_location.folder().map(|p|p.display().to_string()).unwrap_or_else(|| "Chưa chọn — sẽ hỏi khi xuất lần đầu".into()));
                                ui.separator();
                                let change = ui.button("Đổi thư mục…");
                                #[cfg(test)] self.hitboxes.insert("export_change_folder".into(), change.rect);
                                if change.clicked() { choose = true; ui.close_menu(); }
                                ui.separator();
                                let metadata=ui.button("Metadata video…");
                                #[cfg(test)] self.hitboxes.insert("export_metadata_open".into(),metadata.rect);
                                if metadata.clicked() {self.metadata_open=true;ui.close_menu();}
                            });
                            #[cfg(test)] self.hitboxes.insert("top_export_location".into(), _location.response.rect);
                            if choose { self.choose_export_folder(); }
                            let save = ui.button("Lưu dự án");
                            #[cfg(test)] self.hitboxes.insert("top_save".into(), save.rect);
                            if save.clicked() { self.save(); self.message("Đã lưu project.json", false); }
                            let open = ui.button("Mở dự án");
                            #[cfg(test)] self.hitboxes.insert("top_open".into(), open.rect);
                            if open.clicked() {
                                if let Some(path) = rfd::FileDialog::new().add_filter("Reaction project", &["json"]).pick_file() { self.open(path); }
                            }
                        }
                    });
                });
            });
    }

    fn choose_export_folder(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title("Chọn thư mục lưu video — AIR3view sẽ ghi nhớ");
        if let Some(folder) = self.export_location.folder().filter(|folder| folder.is_dir()) {
            dialog = dialog.set_directory(folder);
        }
        if let Some(folder) = dialog.pick_folder() {
            match self.export_location.remember(folder) {
                Ok(()) => self.message("Đã ghi nhớ thư mục xuất video. Bấm Xuất MP4 để lưu vào đó.", false),
                Err(error) => self.message(format!("{error:#}"), true),
            }
        }
    }
    fn bottom(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("statusbar")
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(18, 7)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if self.busy {
                        ui.spinner();
                    }
                    let text = if self.page==Page::Editor && self.setup_open && self.is_error {
                        "Cần xử lý lỗi • xem chi tiết trong thiết lập dự án"
                    } else if !self.notice.is_empty() {
                        &self.notice
                    } else {
                        &self.project.status
                    };
                    let width=(ui.available_width()-125.).max(80.);
                    let _notice=ui.add_sized([width,18.],egui::Label::new(RichText::new(text).size(12.0).color(if self.is_error {
                        Color32::from_rgb(255, 130, 140)
                    } else {
                        MUTED
                    })).truncate()).on_hover_text(text);
                    #[cfg(test)] self.hitboxes.insert("status_notice".into(),_notice.rect);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let _saved=ui.label(
                            RichText::new(if self.dirty {
                                "Chưa lưu"
                            } else {
                                "Đã lưu cục bộ"
                            })
                            .color(MUTED)
                            .size(11.0),
                        );
                        #[cfg(test)] self.hitboxes.insert("status_saved".into(),_saved.rect);
                    });
                });
            });
    }
    fn launch_preview_selected(&mut self) {
        if self.busy {
            return;
        }
        let Some(seg) = self.project.segments.get(self.selected).cloned() else {
            return;
        };
        self.busy = true;
        self.player.stop();
        self.cancel = process::cancel_token();
        let cancel = self.cancel.clone();
        let p = self.project.clone();
        let s = crate::workflow::effective_settings(&p, &self.settings);
        let tx = self.tx.clone();
        thread::spawn(move || {
            match media::render_segment(&p, &seg, &s, &p.root.join("cache/render"), &cancel) {
                Ok(path) => {
                    let _ = tx.send(Event::Preview {
                        path,
                        duration_ms: seg.duration_ms,
                    });
                    let _ = tx.send(Event::Done(
                        "Đang xem bản dựng chính xác của cảnh đã chọn.".into(),
                    ));
                }
                Err(e) => {
                    let _ = tx.send(Event::Error(format!("{e:#}")));
                }
            }
        });
    }
    fn preview(&mut self, ui: &mut egui::Ui, size: Vec2) {
        if !self.preview_final && !self.project.segments.is_empty() {
            self.layer_canvas(ui,size);
            return;
        }
        let production = crate::workflow::effective_settings(&self.project, &self.settings);
        let (outer, response) = ui.allocate_exact_size(size, Sense::drag());
        let painter = ui.painter_at(outer);
        painter.rect_filled(outer, 10.0, Color32::from_rgb(5, 10, 19));
        let (w, h) = self.project.output.dimensions();
        let aspect = w as f32 / h as f32;
        let ch = (outer.height() - 24.0).min((outer.width() - 24.0) / aspect);
        let canvas = Rect::from_center_size(outer.center(), Vec2::new(ch * aspect, ch));
        #[cfg(test)]
        self.hitboxes.insert("preview_canvas".into(), canvas);
        painter.rect_filled(
            canvas,
            0.0,
            if self.project.output.editorial {
                Color32::from_rgb(244, 240, 230)
            } else {
                Color32::from_rgb(11, 18, 32)
            },
        );
        let track_hidden = !self.preview_final && self.project.segments.get(self.selected).is_some_and(|s|
            self.project.timeline.tracks[if s.kind == "host" {crate::timeline::AVATAR} else {crate::timeline::VIDEO}].muted);
        if let Some(texture) = self.texture.as_ref().filter(|_| !track_hidden) {
            if self.project.output.blur_background && !self.preview_final
                && self.project.segments.get(self.selected).is_some_and(|s| s.kind != "host") {
                if let Some(blurred) = &self.blurred_texture {
                    let sz = blurred.size_vec2();
                    let factor = (canvas.width() / sz.x).max(canvas.height() / sz.y);
                    painter.with_clip_rect(canvas).image(blurred.id(),
                        Rect::from_center_size(canvas.center(), sz * factor),
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::from_gray(224));
                }
            }
            let sz = texture.size_vec2();
            let factor = if self.preview_final {
                (canvas.width() / sz.x).min(canvas.height() / sz.y)
            } else if self.project.output.fit == "cover" {
                (canvas.width() / sz.x).max(canvas.height() / sz.y)
            } else {
                (canvas.width() / sz.x).min(canvas.height() / sz.y)
            };
            let target = sz
                * factor
                * if self.preview_final {
                    1.0
                } else {
                    self.project.output.scale
                };
            let pos = canvas.min
                + Vec2::new(
                    (canvas.width() - target.x) * self.project.output.x,
                    (canvas.height() - target.y) * self.project.output.y,
                );
            painter.with_clip_rect(canvas).image(
                texture.id(),
                Rect::from_min_size(pos, target),
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            painter.text(
                canvas.center(),
                Align2::CENTER_CENTER,
                "Chọn video để preview",
                FontId::proportional(16.0),
                MUTED,
            );
        }
        if !self.preview_final {
            if self.project.subtitles.enabled && !self.project.timeline.tracks[crate::timeline::CAPTIONS].muted {
                let st = &self.project.subtitles;
                let text = self
                    .project
                    .segments
                    .get(self.selected)
                    .map(|s| {
                        if !s.caption.is_empty() {
                            s.caption.clone()
                        } else {
                            s.narration.clone()
                        }
                    })
                    .unwrap_or_default();
                if !text.is_empty() {
                    let sample = text
                        .split_whitespace()
                        .take(st.words_per_line * 2)
                        .collect::<Vec<_>>()
                        .join(" ");
                    let pos = Pos2::new(
                        canvas.left()+canvas.width()*crate::subtitle::anchor(st,w,h)[0]/w as f32,
                        canvas.top()+canvas.height()*crate::subtitle::anchor(st,w,h)[1]/h as f32,
                    );
                    let font = FontId::proportional(st.size * canvas.height() / h as f32);
                    let color = Color32::from_rgb(st.color[0], st.color[1], st.color[2]);
                    let wrap = (canvas.width() - canvas.width() * 80.0 / w as f32).max(10.0);
                    let text = painter.layout(sample.clone(), font.clone(), color, wrap);
                    let shadow = painter.layout(sample, font, Color32::BLACK, wrap);
                    let origin = pos - Vec2::new(text.size().x / 2.0, text.size().y);
                    painter.with_clip_rect(canvas).galley(
                        origin + Vec2::splat(1.0),
                        shadow,
                        Color32::BLACK,
                    );
                    painter.with_clip_rect(canvas).galley(origin, text, color);
                }
            }
            if let Some(seg) = self.project.segments.get(self.selected) {
                if seg.kind == "review" && production.avatar_enabled && !self.project.timeline.tracks[crate::timeline::AVATAR].muted {
                    let factor = if seg.avatar_size == "large" { 1.6 } else { 1.0 };
                    let scale = (self.project.output.avatar_scale * factor).min(0.8);
                    let av_size = Vec2::new(
                        canvas.width() * scale,
                        canvas.width()
                            * scale
                            * if production.avatar_provider == "cartoon2d" {
                                1.0
                            } else {
                                0.7
                            },
                    );
                    let av = Rect::from_min_size(
                        canvas.min
                            + Vec2::new(
                                (canvas.width() - av_size.x) * self.project.output.avatar_x,
                                (canvas.height() - av_size.y) * self.project.output.avatar_y,
                            ),
                        av_size,
                    );
                    if production.avatar_provider == "cartoon2d" && self.avatar_preview.is_some()
                    {
                        painter.with_clip_rect(canvas).image(
                            self.avatar_preview.as_ref().unwrap().1.id(),
                            av,
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    } else {
                        painter.rect_filled(
                            av,
                            6.0,
                            Color32::from_rgba_unmultiplied(26, 50, 70, 220),
                        );
                        painter.text(
                            av.center(),
                            Align2::CENTER_CENTER,
                            "Avatar",
                            FontId::proportional(12.0),
                            TEAL,
                        );
                    }
                }
            }
        }
        painter.rect_stroke(
            canvas,
            0.0,
            Stroke::new(1.0, EDGE),
            egui::StrokeKind::Inside,
        );
        if response.dragged() && !self.preview_final && !self.busy {
            let d = ui.input(|input| input.pointer.delta());
            self.project.output.x = (self.project.output.x + d.x / canvas.width()).clamp(0.0, 1.0);
            self.project.output.y = (self.project.output.y + d.y / canvas.height()).clamp(0.0, 1.0);
            self.touch();
        }
    }
    fn output_settings(&mut self, ui: &mut egui::Ui) {
        self.original_audio_settings(ui);
        ui.separator();
        let old_output=self.project.output.clone();
        let before = serde_json::to_string(&self.project.output).unwrap();
        let duration_before = (
            self.project.rules.min_seconds,
            self.project.rules.max_seconds,
            self.project.rules.hook_enabled,
        );
        let o = &mut self.project.output;
        heading(ui, "KHUNG VIDEO ĐẦU RA");
        ui.checkbox(&mut o.editorial, "Nền sáng / thẻ thông tin theo mẫu");
        ui.checkbox(&mut o.blur_background, "Nền video mờ (theo clip gốc)");
        if ui.button("Áp dụng bố cục review mẫu").clicked() {
            o.editorial = true;
            o.fit = "contain".into();
            o.scale = 0.86;
            o.x = 0.5;
            o.y = 0.52;
            o.avatar_scale = 0.34;
            o.avatar_x = 0.98;
            o.avatar_y = 0.76;
        }
        ui.horizontal(|ui| {
            ui.selectable_value(&mut o.portrait, true, "9:16  Dọc");
            ui.selectable_value(&mut o.portrait, false, "16:9  Ngang");
        });
        egui::ComboBox::from_id_salt("resolution")
            .selected_text(format!("{}p", o.resolution))
            .show_ui(ui, |ui| {
                for n in [360, 720, 1080] {
                    ui.selectable_value(&mut o.resolution, n, format!("{n}p"));
                }
            });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut o.fit, "contain".into(), "Vừa khung");
            ui.selectable_value(&mut o.fit, "cover".into(), "Lấp đầy / crop");
        });
        ui.add(egui::Slider::new(&mut o.scale, 0.2..=3.0).text("Scale"));
        ui.add(egui::Slider::new(&mut o.x, 0.0..=1.0).text("Vị trí X"));
        ui.add(egui::Slider::new(&mut o.y, 0.0..=1.0).text("Vị trí Y"));
        ui.add_space(12.0);
        heading(ui, "NHÂN VẬT Ở GÓC MÀN HÌNH");
        let size = ui.add(egui::Slider::new(&mut o.avatar_scale, 0.1..=0.8).text("Kích thước")
            .custom_formatter(|n, _| format!("{:.0}%", n * 100.0))
            .custom_parser(|s| s.trim_end_matches('%').trim().parse::<f64>().ok().map(|n| n / 100.0)));
        #[cfg(test)] self.hitboxes.insert("avatar_scale".into(), size.rect);
        #[cfg(not(test))] let _ = size;
        ui.label(RichText::new("Tỷ lệ chiều rộng khung; cảnh Lớn = 1,6×, tối đa 80%.").size(11.0).color(MUTED));
        let (w,h)=o.dimensions();
        let aw=o.avatar_scale*w as f32;let ah=aw;
        let mut x=(w as f32-aw)*(o.avatar_x-0.5);
        let mut y=(h as f32-ah)*(o.avatar_y-0.5);
        ui.horizontal(|ui|{
            ui.label("X");ui.add(egui::DragValue::new(&mut x).range(-10000.0..=10000.0).suffix(" px"));
            ui.label("Y");ui.add(egui::DragValue::new(&mut y).range(-10000.0..=10000.0).suffix(" px"));
        });
        o.avatar_x=0.5+x/(w as f32-aw).max(1.);
        o.avatar_y=0.5+y/(h as f32-ah).max(1.);
        if !self.setup_open {
        ui.add_space(12.0);
        heading(ui, "THỜI LƯỢNG MỤC TIÊU");
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut self.project.rules.min_seconds)
                    .range(1..=7200)
                    .suffix(" s"),
            );
            ui.label("đến");
            ui.add(
                egui::DragValue::new(&mut self.project.rules.max_seconds)
                    .range(1..=7200)
                    .suffix(" s"),
            );
        });
        ui.label(
            RichText::new("Đo thời lượng thật sau TTS; không tăng tốc giọng để ép đủ phút.")
                .color(MUTED)
                .size(12.0),
        );
        hook_settings(ui, &mut self.project.rules);
        }
        if before != serde_json::to_string(&self.project.output).unwrap() {
            let fresh=crate::composition::Composition::from_project(&self.project);
            if let Some(comp)=&mut self.project.composition {
                for clip in &mut comp.clips {
                    let replace=match clip.track {
                        crate::timeline::AVATAR => old_output.avatar_scale!=self.project.output.avatar_scale || old_output.avatar_x!=self.project.output.avatar_x || old_output.avatar_y!=self.project.output.avatar_y,
                        crate::timeline::VIDEO => old_output.scale!=self.project.output.scale || old_output.x!=self.project.output.x || old_output.y!=self.project.output.y || old_output.fit!=self.project.output.fit || old_output.portrait!=self.project.output.portrait,
                        _ => false,
                    };
                    if replace {if let Some(new)=fresh.clips.iter().find(|c|c.id==clip.id){clip.transform=new.transform.clone();}}
                }
                if old_output.blur_background!=self.project.output.blur_background {
                    comp.clips.retain(|c|c.track!=crate::composition::BACKGROUND);
                    comp.clips.extend(fresh.clips.iter().filter(|c|c.track==crate::composition::BACKGROUND).cloned());
                }
            }
            self.request_frame();
        }
        if before != serde_json::to_string(&self.project.output).unwrap()
            || duration_before
                != (
                    self.project.rules.min_seconds,
                    self.project.rules.max_seconds,
                    self.project.rules.hook_enabled,
                )
        {
            self.touch();
        }
    }
    fn subtitle_settings(&mut self, ui: &mut egui::Ui) {
        if self.page == Page::Editor && !self.setup_open {
            let enabled = crate::timeline::editable(&self.project,self.selected).is_ok();
            if let Some(seg) = self.project.segments.get_mut(self.selected) {
                heading(ui,"CHỮ CỦA CẢNH CHỌN");
                if ui.add_enabled(enabled,egui::TextEdit::multiline(&mut seg.caption).desired_width(f32::INFINITY).desired_rows(3).hint_text("Để trống để dùng lời thoại")).changed() {
                    seg.words.clear();seg.timing_estimated=true;seg.suppress_captions=false;
                    self.touch();
                }
                ui.separator();
            }
        }
        let before = serde_json::to_string(&self.project.subtitles).unwrap();
        let s = &mut self.project.subtitles;
        heading(ui, "PHỤ ĐỀ ĐẦU RA");
        ui.checkbox(&mut s.enabled, "Burn-in phụ đề");
        ui.label("Vị trí sub · dịch chuyển theo pixel đầu ra");
        ui.scope(|ui| {
            ui.spacing_mut().slider_width=(ui.available_width()-95.).clamp(60.,180.);
            ui.label("Trục X · trái / phải");
            let _x=ui.add(egui::Slider::new(&mut s.offset_x,-2000..=2000).suffix(" px"));
            #[cfg(test)] self.hitboxes.insert("subtitle_x".into(),_x.rect);
            ui.label("Trục Y · lên / xuống");
            let _y=ui.add(egui::Slider::new(&mut s.offset_y,-2000..=2000).suffix(" px"));
            #[cfg(test)] self.hitboxes.insert("subtitle_y".into(),_y.rect);
            let reset=ui.button("Đặt lại X/Y");
            #[cfg(test)] self.hitboxes.insert("subtitle_position_reset".into(),reset.rect);
            if reset.clicked(){s.offset_x=0;s.offset_y=0;}
        });
        ui.label(RichText::new("X dương: sang phải · Y dương: xuống dưới · 0: vị trí gốc theo Cách đáy.").size(12.).color(MUTED));
        ui.separator();
        field(ui, "Font", &mut s.font);
        ui.add(egui::Slider::new(&mut s.size, 16.0..=100.0).text("Cỡ chữ"));
        ui.horizontal(|ui| {
            ui.label("Màu chữ");
            ui.color_edit_button_srgb(&mut s.color);
        });
        ui.horizontal(|ui| {
            ui.label("Highlight");
            ui.color_edit_button_srgb(&mut s.highlight);
        });
        ui.horizontal(|ui| {
            ui.label("Viền");
            ui.color_edit_button_srgb(&mut s.outline);
        });
        ui.add(egui::Slider::new(&mut s.outline_width, 0.0..=8.0).text("Độ dày viền"));
        ui.add(egui::Slider::new(&mut s.bottom_margin, 20..=400).text("Cách đáy"));
        ui.add(egui::Slider::new(&mut s.words_per_line, 2..=10).text("Từ / dòng"));
        ui.checkbox(&mut s.karaoke, "Highlight theo từ");
        ui.label(RichText::new("Preview nhanh hiển thị bố cục. Dựng preview cảnh để kiểm tra font, viền và highlight đúng như export.").size(12.0).color(MUTED));
        if before != serde_json::to_string(&self.project.subtitles).unwrap() {
            self.touch();
        }
        ui.separator();
        if !self.setup_open && ui.button("Căn từ bằng Whisper").clicked() {
            self.launch(Task::Transcribe);
        }
    }
    fn range_rules(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if !self.setup_open {
            ui.label("Source ID");
            ui.add(
                egui::TextEdit::singleline(&mut self.range_source)
                    .desired_width(100.0)
                    .hint_text("src_1"),
            );
            }
            ui.add(
                egui::DragValue::new(&mut self.range_start)
                    .range(0.0..=86400.0)
                    .suffix(" s"),
            );
            ui.label("→");
            ui.add(
                egui::DragValue::new(&mut self.range_end)
                    .range(0.1..=86400.0)
                    .suffix(" s"),
            );
            let valid = self.range_end > self.range_start && !self.range_source.trim().is_empty();
            if ui
                .add_enabled(valid, egui::Button::new("+ Bắt buộc giữ"))
                .clicked()
            {
                self.project.rules.required_ranges.push(TimeRange {
                    source_id: self.range_source.clone(),
                    start_ms: (self.range_start * 1000.0) as u64,
                    end_ms: (self.range_end * 1000.0) as u64,
                });
                self.dirty = true;
            }
            if ui
                .add_enabled(valid, egui::Button::new("+ Loại hoàn toàn"))
                .clicked()
            {
                self.project.rules.excluded_ranges.push(TimeRange {
                    source_id: self.range_source.clone(),
                    start_ms: (self.range_start * 1000.0) as u64,
                    end_ms: (self.range_end * 1000.0) as u64,
                });
                self.dirty = true;
            }
        });
        let mut remove = None;
        for (kind, ranges) in [
            (true, &self.project.rules.required_ranges),
            (false, &self.project.rules.excluded_ranges),
        ] {
            for (i, r) in ranges.iter().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    chip(
                        ui,
                        if kind { "GIỮ" } else { "LOẠI" },
                        if kind {
                            TEAL
                        } else {
                            Color32::from_rgb(250, 120, 120)
                        },
                    );
                    ui.label(format!(
                        "{}   {} → {}",
                        r.source_id,
                        seconds(r.start_ms),
                        seconds(r.end_ms)
                    ));
                    if ui.small_button("×").clicked() {
                        remove = Some((kind, i));
                    }
                });
            }
        }
        if let Some((kind, i)) = remove {
            if kind {
                self.project.rules.required_ranges.remove(i);
            } else {
                self.project.rules.excluded_ranges.remove(i);
            }
            self.dirty = true;
        }
    }
    fn import_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Video", &["mp4", "mov", "mkv", "webm"])
            .pick_file()
        {
            self.launch(Task::Import(path));
        }
    }
    fn editor_actions(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Prompt / phân tích nội dung").show(ui, |ui| {
            if ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new("Dùng prompt review / reaction theo Độ dài"),
                )
                .clicked()
            {
                self.project.rules.prompt = DEFAULT_REACTION_PROMPT.into();
                self.touch();
            }
            let changed = ui
                .add_enabled(
                    !self.busy,
                    egui::TextEdit::multiline(&mut self.project.rules.prompt)
                        .desired_width(f32::INFINITY)
                        .desired_rows(3),
                )
                .changed();
            if changed {
                self.touch();
            }
            prompt_duration_hint(ui, &self.project.rules);
            if !self.project.analysis_summary.is_empty() {
                ui.label(&self.project.analysis_summary);
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(!self.busy, |ui| {
                if self.settings.provider == "antigravity"
                    && !self.project.sources.is_empty()
                    && ui.button("Viết lại script bằng CLI").clicked()
                {
                    self.launch(Task::Prepare);
                }
                if ui.button("+ Nguồn local").clicked() {
                    self.import_dialog();
                }
                if ui.button("Nhập script JSON").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Script", &["json"])
                        .pick_file()
                    {
                        match fs::read_to_string(path)
                            .map_err(anyhow::Error::from)
                            .and_then(|text| providers::parse_plan(&text))
                            .and_then(|p| self.apply_plan(p))
                        {
                            Ok(()) => self.message("Đã nhập script", false),
                            Err(e) => self.message(format!("{e:#}"), true),
                        }
                    }
                }
                if ui.button("Xuất script / prompt").clicked() {
                    let folder = self.project.root.join("analysis");
                    let result = (|| -> anyhow::Result<()> {
                        fs::create_dir_all(&folder)?;
                        fs::write(
                            folder.join("prompt.txt"),
                            providers::build_prompt(&self.project),
                        )?;
                        write_json(
                            &folder.join("script.json"),
                            &ScriptPlan {
                                title: self.project.title.clone(),
                                analysis_summary: self.project.analysis_summary.clone(),
                                segments: self.project.segments.clone(),
                            },
                        )?;
                        write_json(&folder.join("schema.json"), &providers::plan_schema())?;
                        Ok(())
                    })();
                    match result {
                        Ok(()) => self.message(format!("Đã lưu: {}", folder.display()), false),
                        Err(e) => self.message(format!("{e:#}"), true),
                    }
                }
                if ui.button("Chỉ tạo giọng (lưu TTS)").clicked() {
                    self.launch(Task::Speech);
                }
                if ui.button("Tạo giọng + avatar").clicked() {
                    self.launch(Task::Assets);
                }
                if ui.button("Kiểm tra timeline").clicked() {
                    match self
                        .project
                        .validate(true)
                        .and_then(|_| self.project.check_duration())
                    {
                        Ok(()) => self.message("Timeline hợp lệ", false),
                        Err(e) => self.message(format!("{e:#}"), true),
                    }
                }
            });
        });
    }
    fn queue_page(&mut self, ui: &mut egui::Ui) {
        ui.columns(2, |cols| {
            panel().show(&mut cols[0], |ui| {
                heading(ui, "TÁC VỤ HIỆN TẠI");
                if self.lanes.is_empty() {
                    ui.label("Chưa có tác vụ đang chạy.");
                }
                for (lane, (message, done)) in &self.lanes {
                    ui.horizontal(|ui| {
                        if !done && self.busy {
                            ui.spinner();
                        }
                        chip(ui, lane, if *done { TEAL } else { BLUE });
                    });
                    ui.label(message);
                    ui.separator();
                }
                ui.add_space(10.0);
                if ui.button("Mở editor").clicked() {
                    self.page = Page::Editor;
                }
                if !self.busy
                    && !self.project.sources.is_empty()
                    && ui.button("Phân tích lại URL (dùng cache tải)").clicked()
                {
                    self.urls = self
                        .project
                        .sources
                        .iter()
                        .map(|s| s.url.clone())
                        .filter(|u| !u.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.launch(Task::Prepare);
                }
            });
            panel().show(&mut cols[1], |ui| {
                heading(ui, "NHẬT KÝ");
                ui.label(
                    RichText::new(format!(
                        "Log: {}",
                        self.project.root.join("logs/pipeline.jsonl").display()
                    ))
                    .size(11.0)
                    .color(MUTED),
                );
                egui::ScrollArea::vertical()
                    .max_height(550.0)
                    .show(ui, |ui| {
                        for text in self.logs.iter().rev() {
                            ui.label(RichText::new(text).size(12.0).color(MUTED));
                            ui.separator();
                        }
                    });
            });
        });
    }
    fn projects_page(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.busy, egui::Button::new("+ Dự án mới"))
                .clicked()
            {
                self.new_project();
            }
            if ui.button("Làm mới").clicked() {
                self.history = Self::read_history();
                self.library_cache.clear();
            }
        });
        ui.add_space(15.0);
        let mut open = None;
        let mut delete = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for path in &self.history {
                if let Some(p) = self.library_cache.get(path) {
                    panel().show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.set_max_width((ui.available_width() - 430.0).max(120.0));
                                let _title = ui.add(egui::Label::new(RichText::new(&p.title).size(18.0).strong()).truncate()).on_hover_text(&p.title);
                                #[cfg(test)] self.hitboxes.insert(format!("library_title_{}", p.id), _title.rect);
                                ui.label(
                                    RichText::new(format!(
                                        "{} nguồn · {} cảnh · {}",
                                        p.source_count,
                                        p.segment_count,
                                        seconds(p.duration_ms)
                                    ))
                                    .color(MUTED),
                                );
                            });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let open_button = ui.add_enabled(
                                        !self.busy,
                                        egui::Button::new("Mở editor →")
                                            .min_size(Vec2::new(0.0, 34.0)),
                                    );
                                    #[cfg(test)]
                                    self.hitboxes.insert(format!("open_{}", p.id), open_button.rect);
                                    if open_button.clicked() {
                                        open = Some(path.clone());
                                    }
                                    let button = ui.add_enabled(
                                        !self.busy,
                                        egui::Button::new(
                                            RichText::new("Xóa dự án")
                                                .color(Color32::from_rgb(255, 110, 120)),
                                        ).min_size(Vec2::new(0.0, 34.0)),
                                    );
                                    #[cfg(test)]
                                    self.hitboxes
                                        .insert(format!("delete_{}", p.id), button.rect);
                                    if button.clicked() {
                                        delete = Some(path.clone());
                                    }
                                    let status = ui.add(
                                        egui::Button::new(
                                            RichText::new(&p.status).size(11.0).strong().color(TEAL),
                                        )
                                        .fill(TEAL.gamma_multiply(0.13))
                                        .stroke(Stroke::NONE)
                                        .corner_radius(6)
                                        .min_size(Vec2::new(0.0, 34.0))
                                        .sense(Sense::hover()),
                                    );
                                    #[cfg(test)]
                                    self.hitboxes.insert(format!("status_{}", p.id), status.rect);
                                    #[cfg(not(test))]
                                    let _ = status;
                                },
                            );
                        });
                    });
                    ui.add_space(10.0);
                }
            }
        });
        if let Some(path) = open {
            self.open(path);
        }
        if let Some(path) = delete {
            match project_store::inspect(&self.projects_root, &path) {
                Ok(info) => self.pending_delete = Some(info),
                Err(error) => self.message(format!("Không thể xóa: {error:#}"), true),
            }
        }
    }

    fn delete_project_dialog(&mut self, ctx: &egui::Context) {
        let Some(info) = self.pending_delete.clone() else {
            return;
        };
        let mut confirm = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::new("delete_project_confirmation")).show(ctx, |ui| {
            ui.set_width(500.0);
            ui.heading("Xóa dự án khỏi máy?");
            ui.add_space(10.0);
            ui.label(RichText::new(&info.title).strong().size(18.0));
            ui.label(format!("Dữ liệu trong thư mục: {:.1} MB", info.bytes as f64 / 1_048_576.0));
            ui.label(info.root.to_string_lossy());
            ui.add_space(8.0);
            ui.label("Xóa vĩnh viễn video nguồn, voice, nhân vật, cache và video xuất nằm trong thư mục dự án. File đã nhập từ vị trí khác vẫn được giữ. Thao tác không thể hoàn tác.");
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let response = ui.button("Hủy");
                #[cfg(test)] self.hitboxes.insert("delete_cancel".into(), response.rect);
                cancel = response.clicked();
                let response = ui.add_enabled(!self.busy, egui::Button::new(RichText::new("Xóa vĩnh viễn").color(Color32::WHITE)).fill(Color32::from_rgb(165,45,60)));
                #[cfg(test)] self.hitboxes.insert("delete_confirm".into(), response.rect);
                confirm = response.clicked();
            });
        });
        if cancel || modal.should_close() {
            self.pending_delete = None;
        }
        if confirm && !self.busy {
            self.player.stop();
            let deleting_active =
                fs::canonicalize(&self.project.root).ok().as_ref() == Some(&info.root);
            match project_store::delete(&self.projects_root, &info.path, Some(&self.project)) {
                Ok(bytes) => {
                    if deleting_active {
                        // Clear the old draft so autosave cannot recreate it.
                        self.reset_after_project_delete();
                    }
                    self.history = project_store::list(&self.projects_root);
                    self.pending_delete = None;
                    self.notice = format!(
                        "Đã xóa “{}” ({:.1} MB dữ liệu)",
                        info.title,
                        bytes as f64 / 1_048_576.0
                    );
                    self.is_error = false;
                }
                Err(error) => {
                    self.pending_delete = None;
                    self.message(format!("Không xóa được: {error:#}"), true);
                }
            }
        }
    }

    fn reset_after_project_delete(&mut self) {
        let mut project = Project::default();
        ProjectDefaults::from_project(&self.project).apply(&mut project);
        project.root = self.projects_root.join(&project.id);
        self.project = project;
        self.dirty = false;
        self.selected = 0;
        self.playhead = 0;
        self.texture = None;
        self.preview_scene = None;
        self.preview_final = false;
        self.urls.clear();
        self.setup_open=true;
        self.workflow_state=None;
        self.logs.clear();
        self.lanes.clear();
    }
    fn voice_settings(&mut self, ui: &mut egui::Ui) {
        heading(ui, "VIENEU v3 TURBO");
        field(ui, "Speech endpoint", &mut self.settings.vieneu_url);
        let toggle = ui.checkbox(
            &mut self.settings.voice_reference_enabled,
            "Dùng giọng tham chiếu từ file audio",
        );
        #[cfg(test)]
        self.hitboxes
            .insert("voice_reference_toggle".into(), toggle.rect);
        let _ = toggle;
        ui.add_enabled_ui(!self.settings.voice_reference_enabled, |ui| {
            field(ui, "Giọng preset", &mut self.settings.voice);
        });
        ui.horizontal(|ui| {
            if ui.button("Chọn audio tham chiếu…").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Audio", &["wav", "mp3", "m4a", "flac", "ogg"])
                    .pick_file()
                {
                    match crate::input_assets::reference_audio(
                        &self.settings,
                        &path,
                        &data_dir().join("assets/voices"),
                    ) {
                        Ok(saved) => {
                            self.settings.voice_reference_path =
                                saved.to_string_lossy().into_owned();
                            self.settings.voice_reference_enabled = true;
                            self.message(
                                "Đã chọn giọng mẫu; dùng tối đa 8 giây đầu. Bấm Lưu cấu hình.",
                                false,
                            );
                        }
                        Err(e) => self.message(format!("Không nhập được giọng mẫu: {e:#}"), true),
                    }
                }
            }
            let clear = ui.add_enabled(
                !self.settings.voice_reference_path.is_empty(),
                egui::Button::new("Bỏ audio"),
            );
            #[cfg(test)]
            self.hitboxes.insert("clear_reference".into(), clear.rect);
            if clear.clicked() {
                self.settings.voice_reference_path.clear();
                self.settings.voice_reference_enabled = false;
            }
        });
        if !self.settings.voice_reference_path.is_empty() {
            ui.label(RichText::new("Đã chọn audio tham chiếu (bản sao lưu trong app)").color(TEAL))
                .on_hover_text(&self.settings.voice_reference_path);
        }
        ui.add_enabled_ui(self.settings.voice_reference_enabled, |ui| {
            ui.checkbox(
                &mut self.settings.voice_reference_denoise,
                "Lọc nhiễu giọng mẫu",
            );
        });
        ui.label(RichText::new("Chọn đoạn nói rõ, một người, ít nhạc nền, khoảng 3–8 giây. App chuẩn hóa WAV và dùng tối đa 8 giây đầu; VieNeu v3 không cần lời mẫu.").size(12.0).color(MUTED));
        ui.add_space(8.0);
        field(
            ui,
            "Tên biến key (local không cần)",
            &mut self.settings.vieneu_key_env,
        );
    }
    fn avatar_settings(&mut self, ui: &mut egui::Ui) {
        heading(ui, "NHÂN VẬT REVIEW / REACTION");
        ui.checkbox(
            &mut self.settings.avatar_enabled,
            "Tạo avatar cho cảnh review / host",
        );
        egui::ComboBox::from_id_salt("avatar_provider")
            .selected_text(if self.settings.avatar_provider == "cartoon2d" {
                "2D local • miễn phí API"
            } else {
                "HeyGen Avatar IV • API tính phí"
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut self.settings.avatar_provider,
                    "cartoon2d".into(),
                    "2D local • Rhubarb (khuyên dùng)",
                );
                ui.selectable_value(
                    &mut self.settings.avatar_provider,
                    "heygen".into(),
                    "HeyGen Avatar IV • API tính phí",
                );
            });
        if self.settings.avatar_provider == "cartoon2d" {
            egui::ComboBox::from_id_salt("cartoon_character")
                .selected_text(match self.settings.cartoon_character.as_str() {
                    "portrait" => "Bán thân • hoodie xám",
                    "analyst" => "Analyst • kính / áo teal",
                    _ => "Studio • tai nghe / áo xanh",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.settings.cartoon_character,
                        "portrait".into(),
                        "Bán thân • tóc nâu / hoodie xám (ảnh của bạn)",
                    );
                    ui.selectable_value(
                        &mut self.settings.cartoon_character,
                        "studio".into(),
                        "Studio • tai nghe / áo xanh",
                    );
                    ui.selectable_value(
                        &mut self.settings.cartoon_character,
                        "analyst".into(),
                        "Analyst • kính / áo teal",
                    );
                });
            let preview = worker_path(&format!("cartoon-{}.png", self.settings.cartoon_character));
            let path = preview.to_string_lossy().into_owned();
            if self.avatar_preview.as_ref().map(|(p, _)| p.as_str()) != Some(path.as_str()) {
                if let Ok(image) = image::open(&preview) {
                    let image = image.thumbnail(220, 220).to_rgba8();
                    let color = egui::ColorImage::from_rgba_unmultiplied(
                        [image.width() as usize, image.height() as usize],
                        image.as_raw(),
                    );
                    self.avatar_preview = Some((
                        path,
                        ui.ctx()
                            .load_texture("cartoon-preview", color, Default::default()),
                    ));
                }
            }
            if let Some((_, texture)) = &self.avatar_preview {
                ui.image(texture);
            }
            field(ui, "Rhubarb executable", &mut self.settings.rhubarb);
            ui.label(RichText::new("Nhân vật bán thân, tay nối từ vai; cử chỉ theo lời thoại. Nhép miệng theo audio VieNeu bằng Rhubarb phonetic; chạy CPU, không cần key HeyGen. Mẫu tóc nâu giữ nét mặt ảnh gốc; Studio/Analyst có thêm chớp mắt. Đổi nhân vật dùng lại WAV. Bấm Lưu cấu hình.").color(TEAL).size(12.0));
            return;
        }
        ui.horizontal(|ui| {
            if ui.button("Chọn ảnh nhân vật…").clicked() {
                if let Some(path) = rfd::FileDialog::new().add_filter("Ảnh PNG / JPEG", &["png", "jpg", "jpeg"]).pick_file() {
                    match crate::input_assets::avatar_image(&path, &data_dir().join("assets/characters")) {
                        Ok(saved) => {
                            self.settings.avatar_image_path = saved.to_string_lossy().into_owned();
                            self.settings.avatar_enabled = true;
                            self.message("Đã chọn ảnh; app sẽ upload lên HeyGen khi tạo avatar. Bấm Lưu cấu hình.", false);
                        },
                        Err(e) => self.message(format!("Không nhập được ảnh: {e:#}"), true),
                    }
                }
            }
            let clear = ui.add_enabled(!self.settings.avatar_image_path.is_empty(), egui::Button::new("Bỏ ảnh"));
            #[cfg(test)]
            self.hitboxes.insert("clear_avatar_image".into(), clear.rect);
            if clear.clicked() {
                self.settings.avatar_image_path.clear();
                self.avatar_preview = None;
            }
        });
        if !self.settings.avatar_image_path.is_empty() {
            if self.avatar_preview.as_ref().map(|(path, _)| path.as_str())
                != Some(&self.settings.avatar_image_path)
            {
                if let Ok(image) = image::open(&self.settings.avatar_image_path) {
                    let image = image.thumbnail(240, 180).to_rgba8();
                    let color = egui::ColorImage::from_rgba_unmultiplied(
                        [image.width() as usize, image.height() as usize],
                        image.as_raw(),
                    );
                    self.avatar_preview = Some((
                        self.settings.avatar_image_path.clone(),
                        ui.ctx()
                            .load_texture("character-photo", color, Default::default()),
                    ));
                }
            }
            if let Some((_, texture)) = &self.avatar_preview {
                ui.image(texture);
            }
            ui.label(
                RichText::new("Ảnh từ máy được ưu tiên; tự upload và dùng lại cho các cảnh.")
                    .color(TEAL)
                    .size(12.0),
            )
            .on_hover_text(&self.settings.avatar_image_path);
        }
        ui.add_enabled_ui(self.settings.avatar_image_path.is_empty(), |ui| {
            field(ui, "Hoặc avatar look ID", &mut self.settings.avatar_id);
            field(
                ui,
                "Hoặc URL ảnh nhân vật",
                &mut self.settings.avatar_image_url,
            );
        });
        field(ui, "API base", &mut self.settings.heygen_base);
        field(
            ui,
            "Tên biến môi trường key",
            &mut self.settings.heygen_key_env,
        );
        ui.label(
            RichText::new("API key (dán trực tiếp để lưu vào biến môi trường User)")
                .size(12.0)
                .color(MUTED),
        );
        ui.add(
            egui::TextEdit::singleline(&mut self.heygen_key_input)
                .password(true)
                .desired_width(f32::INFINITY)
                .hint_text("Để trống nếu đã cấu hình key"),
        );
        if !providers::is_environment_name(&self.settings.heygen_key_env) {
            ui.colored_label(
                Color32::LIGHT_RED,
                "Ô tên biến cần HEYGEN_API_KEY; dán key vào ô được che bên dưới.",
            );
        }
        let has_key = providers::environment_key(&self.settings.heygen_key_env).is_some();
        ui.label(
            RichText::new(if has_key {
                "Đã cấu hình key trong môi trường."
            } else {
                "Chưa có key trong môi trường; dán key rồi Lưu cấu hình."
            })
            .size(12.0)
            .color(MUTED),
        );
    }
    fn save_settings(&mut self) -> anyhow::Result<()> {
        self.settings.cli_args = serde_json::from_str::<Vec<String>>(&self.cli_args_text)
            .map_err(|_| anyhow::anyhow!("CLI argv cần JSON array hợp lệ"))?;
        anyhow::ensure!(
            providers::is_environment_name(&self.settings.heygen_key_env),
            "Nhập tên biến HEYGEN_API_KEY, không nhập key vào ô tên biến"
        );
        if !self.heygen_key_input.trim().is_empty() {
            providers::store_user_key(&self.settings.heygen_key_env, &self.heygen_key_input)?;
            self.heygen_key_input.clear();
        }
        let path = data_dir().join("settings.json");
        write_json(&path, &self.settings)?;
        // Replace legacy backups too: they may contain a key in the old name field.
        let backup = path.with_extension("json.bak");
        if backup.is_file() {
            fs::write(backup, serde_json::to_vec_pretty(&self.settings)?)?;
        }
        Ok(())
    }
    fn youtube_cookie_settings(&mut self, ui: &mut egui::Ui) {
        panel().show(ui, |ui| {
            heading(ui, "COOKIE YOUTUBE / VIDEO XÁC NHẬN TUỔI");
            ui.label("Chọn cookies.txt định dạng Netscape từ tài khoản YouTube xem được video.");
            ui.horizontal_wrapped(|ui| {
                let choose = ui.add_sized([160.0, 36.0], egui::Button::new("Chọn file cookie…"));
                #[cfg(test)] self.hitboxes.insert("cookie_choose".into(), choose.rect);
                if choose.clicked() {
                    if let Some(path) = rfd::FileDialog::new().set_title("Chọn cookie YouTube — Netscape cookies.txt")
                        .add_filter("Netscape cookies.txt", &["txt"]).pick_file() {
                        match crate::youtube_cookies::validate(&path).and_then(|_| fs::canonicalize(path).context("Không tìm thấy file cookie")) {
                            Ok(path) => {
                                self.settings.youtube_cookie_path = path.to_string_lossy().into_owned();
                                self.message("Định dạng cookie hợp lệ. Bấm Lưu cấu hình để áp dụng.", false);
                            }
                            Err(e) => self.message(format!("Không chọn được cookie: {e:#}"), true),
                        }
                    }
                }
                let clear = ui.add_enabled(!self.settings.youtube_cookie_path.is_empty(), egui::Button::new("Bỏ cookie").min_size(Vec2::new(110.0, 36.0)));
                #[cfg(test)] self.hitboxes.insert("cookie_clear".into(), clear.rect);
                if clear.clicked() {
                    self.settings.youtube_cookie_path.clear();
                    self.message("Đã bỏ chọn cookie. Bấm Lưu cấu hình để áp dụng.", false);
                }
                let save = ui.add_sized([145.0, 36.0], egui::Button::new("Lưu cấu hình"));
                #[cfg(test)] self.hitboxes.insert("cookie_save".into(), save.rect);
                if save.clicked() {
                    match self.save_settings() {
                        Ok(()) => self.message("Đã lưu cấu hình cookie. Quyền truy cập YouTube được kiểm tra khi tải video.", false),
                        Err(e) => self.message(format!("{e:#}"), true),
                    }
                }
            });
            let path = &self.settings.youtube_cookie_path;
            let label = if path.is_empty() { "Chưa chọn cookie — tải video công khai như bình thường".to_owned() }
                else if !std::path::Path::new(path).is_file() { format!("Không tìm thấy file đã chọn: {path}") }
                else { format!("File đã chọn: {path}") };
            let status = ui.add(egui::Label::new(RichText::new(label).color(MUTED).size(12.0)).truncate()).on_hover_text(path);
            #[cfg(test)] self.hitboxes.insert("cookie_path".into(), status.rect);
            let _ = status;
            ui.label(RichText::new("App ghi nhớ đường dẫn: giữ file trên máy. Cookie chỉ dùng khi tải, không gửi cho AI. Phiên hết hạn cần xuất lại; cookie không tự cấp quyền xem video.").size(12.0).color(MUTED));
            ui.collapsing("Nâng cao: JavaScript YouTube", |ui| {
                field(ui, "Node.js executable (từ phiên bản 22)", &mut self.settings.youtube_node);
                ui.label(RichText::new("Tự nhận Node trong tools, Program Files hoặc PATH và bật cho yt-dlp. Bản yt-dlp.exe chính thức đã kèm EJS; không cần tải script ngoài.").size(12.0).color(MUTED));
            });
        });
    }
    fn settings_page(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui,|ui|{ui.add_enabled_ui(!self.busy,|ui|{
        self.youtube_cookie_settings(ui);
        ui.add_space(12.0);
        field(ui,"Ngôn ngữ nguồn ASR (trống = tự nhận diện)",&mut self.settings.asr_language);
        ui.horizontal_wrapped(|ui|{
            ui.label("Model ASR / Whisper");
            egui::ComboBox::from_id_salt("asr_model_setting").selected_text(&self.settings.asr_model).show_ui(ui,|ui|{
                for (value,label) in [("base","base • tiết kiệm RAM"),("small","small • cân bằng"),("large-v3-turbo","large-v3-turbo • chất lượng cao")]{
                    ui.selectable_value(&mut self.settings.asr_model,value.into(),label);
                }
            });
        });
        ui.columns(2,|cols|{panel().show(&mut cols[0],|ui|{heading(ui,"GEMINI / ANTIGRAVITY");ui.horizontal(|ui|{ui.selectable_value(&mut self.settings.provider,"antigravity".into(),"Antigravity CLI");ui.selectable_value(&mut self.settings.provider,"gemini".into(),"Gemini API");});if self.settings.provider=="antigravity"{egui::ComboBox::from_id_salt("analysis_input").selected_text(match self.settings.analysis_input.as_str(){"local"=>"Transcript + ảnh","url"=>"Chỉ URL trực tiếp",_=>"Tự động: URL → fallback local"}).show_ui(ui,|ui|{ui.selectable_value(&mut self.settings.analysis_input,"auto".into(),"Tự động: thử URL cùng lúc tải");ui.selectable_value(&mut self.settings.analysis_input,"url".into(),"Chỉ URL trực tiếp (dừng nếu không đọc được)");ui.selectable_value(&mut self.settings.analysis_input,"local".into(),"Transcript + ảnh sau tải");});field(ui,"Model CLI",&mut self.settings.cli_model);field(ui,"CLI executable / node.exe",&mut self.settings.cli_path);field(ui,"CLI argv (JSON array)",&mut self.cli_args_text);ui.label(RichText::new("URL được gửi ngay cùng lúc tải video; chế độ Tự động chuyển sang bằng chứng local nếu CLI không đọc được URL.").size(12.0).color(MUTED));ui.add(egui::Slider::new(&mut self.settings.visual_sample_seconds,1..=10).text("Giây / ảnh"));field(ui,"Model Whisper",&mut self.settings.asr_model);egui::ComboBox::from_id_salt("asr_device").selected_text(&self.settings.asr_device).show_ui(ui,|ui|{ui.selectable_value(&mut self.settings.asr_device,"cpu".into(),"CPU / int8");ui.selectable_value(&mut self.settings.asr_device,"cuda".into(),"CUDA / float16");});}else{field(ui,"Model Gemini API",&mut self.settings.gemini_model);field(ui,"Gemini API base",&mut self.settings.gemini_base);field(ui,"Tên biến môi trường API key",&mut self.settings.gemini_key_env);}
            ui.add_space(16.0);self.voice_settings(ui);});
            panel().show(&mut cols[1],|ui|{self.avatar_settings(ui);ui.add_space(12.0);heading(ui,"MEDIA TOOLS");field(ui,"FFmpeg",&mut self.settings.ffmpeg);field(ui,"ffprobe",&mut self.settings.ffprobe);field(ui,"yt-dlp",&mut self.settings.ytdlp);field(ui,"Python cho ASR",&mut self.settings.python);egui::ComboBox::from_id_salt("encoder").selected_text(&self.settings.encoder).show_ui(ui,|ui|{for e in["libx264","h264_nvenc","h264_qsv"]{ui.selectable_value(&mut self.settings.encoder,e.to_string(),e);}});});});
        ui.add_space(14.0);ui.horizontal(|ui|{if primary(ui,"Lưu cấu hình").clicked(){match self.save_settings(){Ok(())=>self.message("Đã lưu cấu hình",false),Err(e)=>self.message(format!("{e:#}"),true)}}
if ui.button("Kiểm tra công cụ").clicked(){self.busy=true;let s=self.settings.clone();let tx=self.tx.clone();thread::spawn(move||{let _=tx.send(Event::Log(pipeline::diagnostics(&s)));let _=tx.send(Event::Done("Đã kiểm tra công cụ local".into()));});}});if !self.diagnostics.is_empty(){ui.add_space(12.0);panel().show(ui,|ui|{ui.monospace(&self.diagnostics);});}ui.label(RichText::new(format!("Dữ liệu và project: {}",data_dir().display())).size(12.0).color(MUTED));
    });});
    }
}
fn prompt_duration_hint(ui: &mut egui::Ui, rules: &ScriptRules) {
    ui.label(
        RichText::new(format!(
            "{{duration}} = {} • lấy từ mục Độ dài",
            rules.duration_text()
        ))
        .color(MUTED)
        .size(11.0),
    );
    if rules.prompt.contains("{duration}") {
        ui.collapsing("Xem prompt sau khi điền thời lượng", |ui| {
            ui.label(rules.resolved_prompt());
        });
    }
}
fn hook_settings(ui: &mut egui::Ui, rules: &mut ScriptRules) {
    ui.checkbox(&mut rules.hook_enabled, "Mở đầu bằng hook gốc 3–7 giây");
    ui.label(RichText::new("AI chọn sau khi phân tích toàn bộ nguồn. Hook giữ tiếng gốc, chưa có avatar; áp dụng khi phân tích lại.").size(11.0).color(MUTED));
    ui.separator();
    ui.checkbox(&mut rules.intercuts_enabled,"Xen cảnh gốc + tiếng gốc giữa lời review");
    ui.add_enabled_ui(rules.intercuts_enabled,|ui|{
        ui.horizontal_wrapped(|ui|{
            ui.label("Số đoạn");
            ui.add(egui::DragValue::new(&mut rules.intercut_count).range(1..=8));
            ui.label("Mỗi đoạn");
            ui.add(egui::DragValue::new(&mut rules.intercut_min_seconds).range(1..=10).suffix(" s"));
            ui.label("đến");
            ui.add(egui::DragValue::new(&mut rules.intercut_max_seconds).range(rules.intercut_min_seconds..=10).suffix(" s"));
            rules.intercut_max_seconds=rules.intercut_max_seconds.max(rules.intercut_min_seconds);
        });
    });
    ui.label(RichText::new("Áp dụng khi phân tích lại. AI chọn mốc từ nguồn, giữ câu review trọn vẹn; các đoạn gốc nằm trong tổng Độ dài.").size(11.).color(MUTED));
}

fn worker_path(name: &str) -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let p = parent.join("workers").join(name);
            if p.exists() {
                return p;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("workers")
        .join(name)
}
impl StudioApp {
    fn draw(&mut self, ctx: &egui::Context) {
        self.updates.poll();
        self.update_banner(ctx);
        let production = crate::workflow::effective_settings(&self.project, &self.settings);
        if self.page == Page::Editor && production.avatar_provider == "cartoon2d" {
            let path = worker_path(&format!("cartoon-{}.png", production.cartoon_character))
                .to_string_lossy()
                .into_owned();
            if self.avatar_preview.as_ref().map(|(p, _)| p.as_str()) != Some(path.as_str()) {
                if let Ok(image) = image::open(&path) {
                    let image = image.thumbnail(220, 220).to_rgba8();
                    let color = egui::ColorImage::from_rgba_unmultiplied(
                        [image.width() as usize, image.height() as usize],
                        image.as_raw(),
                    );
                    self.avatar_preview = Some((
                        path,
                        ctx.load_texture("cartoon-preview", color, Default::default()),
                    ));
                }
            }
        }
        if let Some(path) = &self.capture_path {
            let path = path.clone();
            let captured = ctx.input(|input| {
                input.events.iter().find_map(|event| {
                    if let egui::Event::Screenshot { image, .. } = event {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
            });
            if let Some(image) = captured {
                let data: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                if let Some(img) =
                    image::RgbaImage::from_raw(image.width() as u32, image.height() as u32, data)
                {
                    let _ = img.save(&path);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                self.capture_path = None;
            }
            self.frames += 1;
            if self.frames >= 12
                && !self.capture_requested
                && self.last_save.elapsed() > Duration::from_secs(2)
            {
                self.capture_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
        }
        self.poll(ctx);
        self.layers.managed_edit=false;
        let before_edit = self.project.clone();
        self.timeline_view.navigating_history = false;
        if self.timeline_view.project_id != self.project.id {
            self.timeline_view = TimelineView::default();
            self.timeline_view.project_id = self.project.id.clone();
        }
        if self.page == Page::New { self.page=Page::Editor; self.setup_open=true; }
        if self.page == Page::Editor && !self.setup_open { self.timeline_shortcuts(ctx); }
        self.sidebar(ctx);
        self.topbar(ctx);
        self.bottom(ctx);
        if self.page == Page::Editor {
            self.editor_layout_toolbar(ctx);
            self.editor(ctx);
        } else {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(BG).inner_margin(24))
                .show(ctx, |ui| match self.page {
                    Page::New => {},
                    Page::Projects => self.projects_page(ui),
                    Page::Queue => self.queue_page(ui),
                    Page::Settings => self.settings_page(ui),
                    _ => {}
                });
        }
        self.delete_project_dialog(ctx);
        self.evidence_source_window(ctx);
        self.metadata_window(ctx);
        self.updates_window(ctx);
        if !self.busy && before_edit.id == self.project.id && !self.timeline_view.navigating_history
            && !self.layers.managed_edit
            && serde_json::to_vec(&before_edit).ok() != serde_json::to_vec(&self.project).ok() {
            self.timeline_view.history.record(before_edit);
        }
        ctx.request_repaint_after(Duration::from_millis(if self.busy || self.player.playing {
            33
        } else {
            150
        }));
    }
}
impl eframe::App for StudioApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.draw(ctx);
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.cancel.store(true, Ordering::Relaxed);
        self.player.stop();
        let _ = self.remember_project_defaults();
        if self.dirty {
            let _ = self.project.save();
        }
    }
}

#[cfg(test)]
mod ui_tests {
    use super::*;
    #[test]
    fn cookie_controls_fit_settings_and_clear_without_deleting_selected_file() {
        for (width,height) in [(1080.,720.),(1440.,900.)] {
            let temp=tempfile::tempdir().unwrap();
            let file=temp.path().join("cookie.txt");fs::write(&file,"fixture, never delete").unwrap();
            let ctx=egui::Context::default();let mut app=StudioApp::from_context(&ctx);
            app.capture_path=None;app.page=Page::Settings;
            app.settings.youtube_cookie_path=file.to_string_lossy().into_owned();
            let screen=Rect::from_min_size(Pos2::ZERO,Vec2::new(width,height));
            for _ in 0..3 {let _=ctx.run(egui::RawInput{screen_rect:Some(screen),..Default::default()},|ctx|app.draw(ctx));}
            for key in ["cookie_choose","cookie_clear","cookie_save","cookie_path"] {
                assert!(screen.contains_rect(app.hitboxes[key]),"{key} at {width}×{height}");
            }
            let rect=app.hitboxes["cookie_choose"];
            for key in ["cookie_clear","cookie_save"] {
                assert!((rect.height()-app.hitboxes[key].height()).abs()<1.);
                assert!(!rect.intersects(app.hitboxes[key]));
            }
            click(&mut app,&ctx,"cookie_clear");
            assert!(app.settings.youtube_cookie_path.is_empty());assert!(file.is_file());
        }
    }
    #[test]
    fn canvas_avatar_drag_is_one_undo_preserves_other_layers_and_live_playback() {
        let temp=tempfile::tempdir().unwrap();let ctx=egui::Context::default();let mut app=StudioApp::from_context(&ctx);
        app.capture_path=None;app.page=Page::Editor;app.setup_open=false;
        app.editor_layout.mode=editor_ui::PreviewLayout::Standard;
        app.project=Project{root:temp.path().into(),sources:vec![Source{id:"s".into(),width:640,height:360,duration_ms:4000,..Default::default()}],segments:vec![Segment{id:"scene".into(),source_id:"s".into(),kind:"review".into(),duration_ms:4000,out_ms:4000,narration:"Lời kiểm thử".into(),..Default::default()}],..Default::default()};
        let mut second=app.project.segments[0].clone();second.id="second".into();app.project.segments.push(second);
        app.project.timeline.tracks[crate::timeline::AVATAR].sync_parameters=true;
        app.project.ensure_composition();
        let id="scene-1".to_owned();
        app.layers.textures.insert(id.clone(),ctx.load_texture("qa-avatar",egui::ColorImage::new([64,64],Color32::RED),Default::default()));
        app.layers.alpha.insert(id.clone(),image::GrayImage::from_pixel(64,64,image::Luma([255])));
        for _ in 0..3{frame(&mut app,&ctx,vec![]);}
        app.timeline_view.history=Default::default();
        let c=app.project.composition.as_ref().unwrap().clips.iter().find(|c|c.id==id).unwrap().clone();
        let canvas=app.hitboxes["preview_canvas"];
        let start=canvas.min+Vec2::new(c.transform.x*canvas.width(),c.transform.y*canvas.height());
        let video=app.project.composition.as_ref().unwrap().clips.iter().find(|c|c.track==0).unwrap().transform.clone();
        let end=start-Vec2::new(35.,26.);app.player.playing=true;
        frame(&mut app,&ctx,vec![egui::Event::PointerMoved(start),egui::Event::PointerButton{pos:start,button:egui::PointerButton::Primary,pressed:true,modifiers:egui::Modifiers::NONE}]);
        let mut latencies=vec![];
        for step in 1..=120 {
            let clock=Instant::now();frame(&mut app,&ctx,vec![egui::Event::PointerMoved(start+(end-start)*step as f32/120.)]);
            latencies.push(clock.elapsed().as_secs_f64()*1000.);
        }
        frame(&mut app,&ctx,vec![egui::Event::PointerButton{pos:end,button:egui::PointerButton::Primary,pressed:false,modifiers:egui::Modifiers::NONE}]);
        assert_eq!(app.layers.selected.as_deref(),Some(id.as_str()));
        let moved=app.project.composition.as_ref().unwrap().clips.iter().find(|c|c.id==id).unwrap().transform.clone();
        assert!((moved.x-c.transform.x+35./canvas.width()).abs()<0.01);
        assert_eq!(app.project.composition.as_ref().unwrap().clips.iter().find(|c|c.id=="second-1").unwrap().transform,moved);
        assert_eq!(app.project.composition.as_ref().unwrap().clips.iter().find(|c|c.track==0).unwrap().transform,video);
        assert_eq!(app.timeline_view.history.past.len(),1,"One undo per drag");
        assert!(app.player.playing,"Layout changes must not restart decoders");
        latencies.sort_by(f64::total_cmp);println!("Native egui drag, 120 updates: p95 {:.3} ms",latencies[114]);
        assert!(latencies[114]<50.,"Transform UI exceeds 50 ms p95 on this QA fixture");
        assert!(!temp.path().join("cache/timeline-preview").exists());
        app.player.stop();
        assert!(app.timeline_view.history.undo(&mut app.project));
        assert_eq!(app.project.composition.as_ref().unwrap().clips.iter().find(|c|c.id==id).unwrap().transform,c.transform);
        assert_eq!(app.project.composition.as_ref().unwrap().clips.iter().find(|c|c.id=="second-1").unwrap().transform,c.transform);
        app.project.composition.as_mut().unwrap().clips.iter_mut().find(|c|c.id==id).unwrap().linked=false;
        app.timeline_view.cursor=2000;app.independent_layer_edit("split");
        let comp=app.project.composition.as_ref().unwrap();
        assert_eq!(comp.clips.iter().filter(|c|c.track==1).count(),3);
        assert_eq!(comp.clips.iter().find(|c|c.track==0).unwrap().duration_ms,4000);
        assert_eq!(comp.clips.iter().find(|c|c.track==1&&c.start_ms==2000).unwrap().media_in_ms,2000);
        for layout in [editor_ui::PreviewLayout::Standard,editor_ui::PreviewLayout::PortraitRight] {
            app.editor_layout.mode=layout;
            for dpi in [1.0,1.25,1.5] {
                ctx.set_pixels_per_point(dpi);
                for _ in 0..3 {let _=ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(1080.,720.))),..Default::default()},|ctx|app.draw(ctx));}
                for key in ["layer_x","layer_y"] {assert!(app.hitboxes["layer_inspector_clip"].contains_rect(app.hitboxes[key]),"{layout:?}, DPI {dpi}: {key} hidden");}
            }
        }
    }
    fn frame(app: &mut StudioApp, ctx: &egui::Context, events: Vec<egui::Event>) {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 1100.0))),
                events,
                ..Default::default()
            },
            |ctx| app.draw(ctx),
        );
    }
    fn click(app: &mut StudioApp, ctx: &egui::Context, key: &str) {
        let pos = app
            .hitboxes
            .get(key)
            .unwrap_or_else(|| panic!("Missing UI control: {key}"))
            .center();
        frame(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame(
            app,
            ctx,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        frame(app, ctx, vec![]);
    }
    #[test]
    fn unified_setup_layout_keeps_sources_and_run_visible_and_sections_expand() {
        for (width,height) in [(1080.,720.),(1440.,900.)] {
            for collapsed in [false,true] {
                for portrait in [false,true] {
                    let ctx=egui::Context::default();
                    let mut app=StudioApp::from_context(&ctx);
                    ctx.style_mut(|style|style.animation_time=0.);
                    app.setup_open=true;
                    app.editor_layout.sidebar_collapsed=collapsed;
                    app.project.output.portrait=portrait;
                    let screen=Rect::from_min_size(Pos2::ZERO,Vec2::new(width,height));
                    for _ in 0..3 {let _=ctx.run(egui::RawInput{screen_rect:Some(screen),..Default::default()},|ctx|app.draw(ctx));}
                    let sidebar=app.hitboxes["navigation_panel"];
                    let preview=app.hitboxes["preview_panel"];
                    for key in ["setup_urls","setup_run","setup_tab","edit_tab"] {
                        let r=app.hitboxes[key];
                        assert!(screen.contains_rect(r),"{width}x{height} collapsed={collapsed} {key}: {r:?}");
                        assert!(r.left()>=sidebar.right() && r.right()<=preview.left(),"{key} overlaps dock: {r:?}");
                    }
                    assert!(preview.contains_rect(app.hitboxes["preview_seek"]));
                    assert!(app.hitboxes["setup_form_visible"].contains_rect(app.hitboxes["setup_urls"]),"URL input clipped at {width}x{height}: {:?} / {:?}",app.hitboxes["setup_urls"],app.hitboxes["setup_form_visible"]);
                    for key in ["setup_script","setup_image","setup_production","setup_subtitles"] {
                        let card:Rect=ctx.data(|d|d.get_temp(egui::Id::new(("setup_card",key))).unwrap());
                        assert!(card.left()>=sidebar.right() && card.right()<=preview.left(),"{key} card outside form: {card:?}");
                    }
                    let header:Rect=ctx.data(|d|d.get_temp(egui::Id::new(("setup_header","setup_script"))).unwrap());
                    for events in [vec![egui::Event::PointerMoved(header.center()),egui::Event::PointerButton{pos:header.center(),button:egui::PointerButton::Primary,pressed:true,modifiers:egui::Modifiers::NONE}],vec![egui::Event::PointerButton{pos:header.center(),button:egui::PointerButton::Primary,pressed:false,modifiers:egui::Modifiers::NONE}],vec![]] {
                        let _=ctx.run(egui::RawInput{screen_rect:Some(screen),events,..Default::default()},|ctx|app.draw(ctx));
                    }
                    assert!(app.hitboxes.contains_key("setup_min_duration"));
                    assert!(app.hitboxes["setup_run"].bottom()<=height);
                }
            }
        }
    }
    #[test]
    fn subtitle_xy_sliders_fit_narrow_columns_and_reset_without_changing_bottom_margin() {
        for width in [240.,320.,560.] {
            let ctx=egui::Context::default();let mut app=StudioApp::from_context(&ctx);
            app.capture_path=None;app.project=Project::default();app.setup_open=true;
            let screen=Rect::from_min_size(Pos2::ZERO,Vec2::new(width+24.,1200.));
            let draw=|app:&mut StudioApp,events| {let _=ctx.run(egui::RawInput{screen_rect:Some(screen),events,..Default::default()},|ctx|{egui::CentralPanel::default().show(ctx,|ui|app.subtitle_settings(ui));});};
            draw(&mut app,vec![]);draw(&mut app,vec![]);
            let x=app.hitboxes["subtitle_x"];let y=app.hitboxes["subtitle_y"];let reset=app.hitboxes["subtitle_position_reset"];
            assert!(screen.contains_rect(x)&&screen.contains_rect(y)&&screen.contains_rect(reset),"Subtitle position clipped at {width}");assert!(!x.intersects(y)&&!y.intersects(reset));
            for rect in [x,y] {
                let pos=egui::pos2(rect.left()+20.,rect.center().y);
                draw(&mut app,vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:true,modifiers:egui::Modifiers::NONE}]);
                draw(&mut app,vec![egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:false,modifiers:egui::Modifiers::NONE}]);
            }
            assert_ne!(app.project.subtitles.offset_x,0);assert_ne!(app.project.subtitles.offset_y,0);assert!(app.dirty);
            let pos=app.hitboxes["subtitle_position_reset"].center();
            draw(&mut app,vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:true,modifiers:egui::Modifiers::NONE},egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:false,modifiers:egui::Modifiers::NONE}]);
            assert_eq!((app.project.subtitles.offset_x,app.project.subtitles.offset_y),(0,0));assert_eq!(app.project.subtitles.bottom_margin,130);
        }
    }
    #[test]
    fn original_audio_controls_fit_and_apply_without_invalidating_speech() {
        for width in [240.,320.,560.] {
            let ctx=egui::Context::default();let mut app=StudioApp::from_context(&ctx);
            app.capture_path=None;app.project=Project::default();
            app.project.segments=vec![Segment{id:"s".into(),kind:"review".into(),audio:Some("keep.wav".into()),asset_key:"keep-voice".into(),..Default::default()}];
            let screen=Rect::from_min_size(Pos2::ZERO,Vec2::new(width+24.,720.));
            let draw=|app:&mut StudioApp,events| {let _=ctx.run(egui::RawInput{screen_rect:Some(screen),events,..Default::default()},|ctx|{egui::CentralPanel::default().show(ctx,|ui|{app.original_audio_settings(ui);});});};
            draw(&mut app,vec![]);draw(&mut app,vec![]);
            let toggle=app.hitboxes["original_audio_enabled"];let slider=app.hitboxes["original_audio_gain"];
            assert!(screen.contains_rect(toggle)&&screen.contains_rect(slider),"Audio controls clipped at {width}");assert!(!toggle.intersects(slider));
            let click=|pos|vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:true,modifiers:egui::Modifiers::NONE},egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:false,modifiers:egui::Modifiers::NONE}];
            draw(&mut app,click(toggle.center()));assert!(app.project.output.original_audio_enabled&&app.dirty);
            draw(&mut app,vec![]);let slider=app.hitboxes["original_audio_gain"];
            let pos=egui::pos2(slider.left()+45.,slider.center().y);
            draw(&mut app,vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:true,modifiers:egui::Modifiers::NONE}]);
            draw(&mut app,vec![egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:false,modifiers:egui::Modifiers::NONE}]);assert_ne!(app.project.output.original_audio_gain,0.15);
            assert_eq!(app.project.segments[0].asset_key,"keep-voice");assert_eq!(app.project.segments[0].audio.as_deref(),Some(std::path::Path::new("keep.wav")));
        }
    }
    #[test]
    fn crop_and_intercut_controls_wrap_inside_narrow_setup_columns() {
        for width in [240.,320.,560.] {
            let ctx=egui::Context::default();let mut app=StudioApp::from_context(&ctx);
            app.capture_path=None;app.project=Project::default();
            let screen=Rect::from_min_size(Pos2::ZERO,Vec2::new(width+24.,1200.));
            for _ in 0..3 {let _=ctx.run(egui::RawInput{screen_rect:Some(screen),..Default::default()},|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    hook_settings(ui,&mut app.project.rules);
                    ui.separator();app.evidence_settings_ui(ui);
                });
            });}
            let crop=app.hitboxes["evidence_crop_percent"];
            assert!(screen.contains_rect(crop),"Crop outside {width}px column: {crop:?}");
        }
    }
    #[test]
    fn setup_failure_wraps_without_duplicate_or_overlapping_action_buttons() {
        for (width,height) in [(1080.,720.),(1440.,900.)] {
            for portrait in [false,true] {
                let ctx=egui::Context::default();let mut app=StudioApp::from_context(&ctx);
                app.capture_path=None;app.page=Page::Editor;app.setup_open=true;
                app.project.output.portrait=portrait;
                app.notice="Cảnh 8 / thẻ 2 (Thời gian nghẹt thở): formula sai định dạng, thiếu timestamp kết thúc. ".repeat(8);
                app.is_error=true;
                app.workflow_state=Some(crate::workflow::JobState{id:"qa".into(),project_id:app.project.id.clone(),fingerprint:"qa".into(),
                    stage_keys:Default::default(),phase:crate::workflow::Phase::Prepare,status:crate::workflow::Status::Failed,message:app.notice.clone()});
                app.notice.clear();app.is_error=false;app.restore_job_notice();
                assert!(app.is_error && app.notice.contains("Cảnh 8"));
                let screen=Rect::from_min_size(Pos2::ZERO,Vec2::new(width,height));
                for _ in 0..3 {let _=ctx.run(egui::RawInput{screen_rect:Some(screen),..Default::default()},|ctx|app.draw(ctx));}
                for key in ["setup_run","setup_resume","setup_logs","setup_error"] {
                    assert!(screen.contains_rect(app.hitboxes[key]),"{key} clipped at {width}x{height}");
                }
                let a=app.hitboxes["setup_run"];let b=app.hitboxes["setup_resume"];let c=app.hitboxes["setup_logs"];
                assert!((a.height()-b.height()).abs()<1. && (b.height()-c.height()).abs()<1.);
                assert!(!a.intersects(b)&&!b.intersects(c)&&!a.intersects(c));
                assert!(screen.contains_rect(app.hitboxes["status_notice"]));
                assert!(!app.hitboxes["status_notice"].intersects(app.hitboxes["status_saved"]));
                assert!(app.hitboxes["setup_form_visible"].height()>100.,"Error consumed the whole editor");
            }
        }
    }
    #[test]
    fn unified_setup_sources_draft_preflight_and_worker_events() {
        let temp=tempfile::tempdir().unwrap();
        let ctx=egui::Context::default();
        let mut app=StudioApp::from_context(&ctx);
        app.projects_root=temp.path().join("projects");
        app.new_project();
        assert!(app.setup_open && app.page==Page::Editor);
        app.settings.provider="gemini".into();
        app.settings.gemini_key_env="AIR3VIEW_UNSET_SETUP_TEST_KEY".into();
        app.project.production.as_mut().unwrap().avatar_provider="cartoon2d".into();
        app.urls="https://www.youtube.com/watch?v=dQw4w9WgXcQ\nhttps://www.youtube.com/watch?v=dQw4w9WgXcQ".into();
        let sources=app.planned_sources().unwrap();
        assert_eq!(sources.len(),1);
        app.project.sources=sources;
        let id=app.project.sources[0].id.clone();
        app.urls.push_str("\nhttps://www.youtube.com/watch?v=aqz-KE-bpKQ");
        assert_eq!(app.planned_sources().unwrap()[0].id,id);
        frame(&mut app,&ctx,vec![]);frame(&mut app,&ctx,vec![]);
        click(&mut app,&ctx,"setup_run");
        assert!(!app.busy && app.is_error);
        assert!(app.notice.contains("Gemini API key"),"{}",app.notice);
        assert!(!app.notice.contains("HeyGen"));
        app.save();
        let path=app.project.root.join("project.json");
        assert_eq!(Project::load(&path).unwrap().input_urls,app.urls);
        app.busy=true;app.auto_running=true;
        let project_id=app.project.id.clone();
        app.new_project();app.open(path);
        assert_eq!(app.project.id,project_id);
        app.tx.send(Event::Project(Box::default())).unwrap();
        app.poll(&ctx);
        assert_eq!(app.project.id,project_id);
        let mut completed=app.project.clone();completed.analysis_summary="Worker checkpoint".into();
        app.tx.send(Event::Project(Box::new(completed))).unwrap();
        app.tx.send(Event::Done("Sẵn sàng".into())).unwrap();
        app.poll(&ctx);
        assert!(!app.setup_open && !app.busy && !app.auto_running);
        assert_eq!(app.project.analysis_summary,"Worker checkpoint");
    }
    #[test]
    fn new_project_keeps_preferences_and_corner_avatar_slider_changes_size() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = StudioApp::from_context(&ctx);
        app.projects_root = temp.path().join("projects");
        app.project.output.blur_background = true;
        app.project.output.avatar_scale = 0.42;
        app.project.rules.prompt = "Prompt lần trước {duration}".into();
        app.project.rules.required_ranges.push(TimeRange { source_id: "old".into(), start_ms: 0, end_ms: 1000 });
        app.project.subtitles.size = 62.0;
        app.new_project();
        assert!(app.project.output.blur_background);
        assert_eq!(app.project.output.avatar_scale, 0.42);
        assert_eq!(app.project.subtitles.size, 62.0);
        assert_eq!(app.project.rules.prompt, "Prompt lần trước {duration}");
        assert!(app.project.sources.is_empty() && app.project.rules.required_ranges.is_empty());
        assert!(ProjectDefaults::load(&temp.path().join("project-defaults.json")).is_some());
        // Exercise the real output controls without depending on scroll position.
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 950.0))), ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.output_settings(ui));
            });
        }
        let slider = app.hitboxes["avatar_scale"];
        assert!(slider.right() < 500.0 && slider.bottom() < 950.0);
        assert!(slider.height() >= 14.0);
        let pos = Pos2::new(slider.left() + 10.0, slider.center().y);
        for pressed in [true, false] {
            let _ = ctx.run(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 950.0))),
                events: vec![egui::Event::PointerMoved(pos), egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }], ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.output_settings(ui));
            });
        }
        assert!(app.project.output.avatar_scale < 0.42 && app.dirty);
    }
    #[test]
    fn library_actions_have_equal_height_and_alignment_at_supported_widths() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = StudioApp::from_context(&ctx);
        app.projects_root = temp.path().join("projects");
        app.project = Project::default();
        app.project.root = app.projects_root.join(&app.project.id);
        app.project.title = "Giải Mã Những Pha Đụng Độ Nghẹt Thở Từ Bodycam Cảnh Sát".into();
        app.project.status = "Cần kiểm tra".into();
        app.project.save().unwrap();
        app.history = project_store::list(&app.projects_root);
        app.capture_path = None;
        app.page = Page::Projects;
        for width in [1080.0, 1440.0] {
            for busy in [false, true] {
                app.busy = busy;
                for _ in 0..3 {
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 900.0))),
                            ..Default::default()
                        },
                        |ctx| app.draw(ctx),
                    );
                }
                let rects: Vec<_> = ["status", "delete", "open"].iter()
                    .map(|key| app.hitboxes[&format!("{key}_{}", app.project.id)])
                    .collect();
                for rect in &rects {
                    assert!((rect.height() - 34.0).abs() < 0.1, "{width}: {rect:?}");
                    assert!((rect.top() - rects[0].top()).abs() < 0.1);
                    assert!((rect.bottom() - rects[0].bottom()).abs() < 0.1);
                    assert!(rect.left() >= 0.0 && rect.right() <= width);
                }
                assert!(rects[0].right() < rects[1].left());
                assert!(rects[1].right() < rects[2].left());
                assert!(app.hitboxes[&format!("library_title_{}", app.project.id)].right() < rects[0].left(), "Project title must not overlap library actions");
            }
        }
        assert_eq!(app.library_cache.reads, 1, "Idle library frames must reuse the manifest summary");
    }
    #[test]
    fn library_delete_requires_confirmation_and_does_not_autosave_deleted_project() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = StudioApp::from_context(&ctx);
        app.projects_root = temp.path().join("projects");
        app.project = Project::default();
        app.project.root = app.projects_root.join(&app.project.id);
        app.project.save().unwrap();
        let old_root = app.project.root.clone();
        let key = format!("delete_{}", app.project.id);
        app.history = project_store::list(&app.projects_root);
        app.capture_path = None;
        app.page = Page::Projects;
        frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, &key);
        assert!(app.pending_delete.is_some() && old_root.exists());
        click(&mut app, &ctx, "delete_cancel");
        assert!(app.pending_delete.is_none() && old_root.exists());
        app.busy = true;
        frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, &key);
        assert!(app.pending_delete.is_none());
        app.busy = false;
        frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, &key);
        app.dirty = true;
        click(&mut app, &ctx, "delete_confirm");
        assert!(!old_root.exists());
        assert!(app.pending_delete.is_none() && app.history.is_empty());
        assert!(!app.dirty && !app.is_error);
        assert_ne!(app.project.root, old_root);
        app.last_save = Instant::now() - Duration::from_secs(3);
        frame(&mut app, &ctx, vec![]);
        assert!(
            !old_root.exists(),
            "Autosave must not recreate deleted project"
        );
    }
    #[test]
    fn collapsible_navigation_keeps_routes_icons_and_saved_state() {
        let temp=tempfile::tempdir().unwrap();
        let ctx=egui::Context::default();
        let mut app=StudioApp::from_context(&ctx);
        app.capture_path=None;
        app.projects_root=temp.path().join("projects");
        app.project.root=temp.path().join("draft");
        app.editor_layout.mode=editor_ui::PreviewLayout::Standard;
        app.project.output.portrait=false;
        for _ in 0..3 {frame(&mut app,&ctx,vec![]);}
        let original=serde_json::to_vec(&app.project).unwrap();
        let expanded=app.hitboxes["preview_panel"].width();
        click(&mut app,&ctx,"nav_toggle");
        assert!(app.editor_layout.sidebar_collapsed);
        assert!(EditorLayout::load(&temp.path().join("editor-layout.json")).sidebar_collapsed);
        assert!((app.hitboxes["navigation_panel"].width()-64.).abs()<1.);
        assert!(app.hitboxes["preview_panel"].width()>expanded+120.);
        for page in [Page::Projects,Page::Queue,Page::Settings,Page::Editor] {
            click(&mut app,&ctx,&format!("nav_{page:?}"));
            assert_eq!(app.page,page);
            assert!(app.editor_layout.sidebar_collapsed);
        }
        for mode in [editor_ui::PreviewLayout::Standard,editor_ui::PreviewLayout::PortraitRight] {
            app.editor_layout.mode=mode;
            for (width,height) in [(1080.,720.),(1440.,900.)] {
                for _ in 0..4 {
                    let _=ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(width,height))),..Default::default()},|ctx|app.draw(ctx));
                }
                let sidebar=app.hitboxes["navigation_panel"];
                for key in ["nav_toggle","nav_New","nav_Projects","nav_Editor","nav_Queue","nav_Settings"] {
                    let r=app.hitboxes[key];
                    assert!(sidebar.contains_rect(r),"{mode:?} {width}x{height}: {key} outside sidebar: {r:?}");
                    assert!(r.width()>=40. && r.height()>=28.);
                }
                assert!(app.hitboxes["preview_panel"].contains_rect(app.hitboxes["preview_seek"]));
            }
        }
        click(&mut app,&ctx,"nav_toggle");
        assert!(!app.editor_layout.sidebar_collapsed);
        assert!(!EditorLayout::load(&temp.path().join("editor-layout.json")).sidebar_collapsed);
        assert!((app.hitboxes["navigation_panel"].width()-202.).abs()<1.);
        assert_eq!(serde_json::to_vec(&app.project).unwrap(),original);
        assert!(!app.dirty);
        let old=temp.path().join("old-layout.json");
        fs::write(&old,r#"{"mode":"standard","inspector_open":false}"#).unwrap();
        let loaded=EditorLayout::load(&old);
        assert!(!loaded.sidebar_collapsed && !loaded.inspector_open);
        assert_eq!(loaded.mode,editor_ui::PreviewLayout::Standard);
    }
    #[test]
    fn editor_layout_docks_portrait_collapses_inspector_and_keeps_transport_visible() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = StudioApp::from_context(&ctx);
        app.projects_root = temp.path().join("projects");
        app.capture_path = None;
        app.page = Page::Editor;
        app.inspector = 1;
        app.project = Project {
            root: temp.path().join("draft"),
            sources: vec![Source { id:"s".into(), duration_ms:6000,..Default::default() }],
            segments:vec![Segment {source_id:"s".into(),duration_ms:5000,..Default::default()}],
            ..Default::default()
        };
        app.project.ensure_composition();
        let original = serde_json::to_vec(&app.project).unwrap();
        for _ in 0..3 { frame(&mut app, &ctx, vec![]); }
        assert!(app.editor_layout.portrait_right(true));
        let portrait_canvas = app.hitboxes["preview_canvas"];
        let portrait_preview = app.hitboxes["preview_panel"];
        assert!(portrait_preview.left() >= app.hitboxes["tl_ruler"].right());
        assert!(portrait_canvas.height() > 500.);
        assert!((portrait_canvas.width()/portrait_canvas.height()-9./16.).abs() < 0.001);
        click(&mut app, &ctx, "inspector_collapse");
        assert!(!app.editor_layout.inspector_open);
        assert!(!EditorLayout::load(&temp.path().join("editor-layout.json")).inspector_open);
        click(&mut app, &ctx, "layout_inspector_toggle");
        assert!(app.editor_layout.inspector_open);
        click(&mut app, &ctx, "layout_standard");
        assert_eq!(EditorLayout::load(&temp.path().join("editor-layout.json")).mode, editor_ui::PreviewLayout::Standard);
        let expanded_width = app.hitboxes["preview_panel"].width();
        click(&mut app, &ctx, "inspector_collapse");
        assert!(app.hitboxes["preview_panel"].width() > expanded_width+200.);
        for i in 0..3 {
            assert!(app.hitboxes[&format!("inspector_rail_{i}")].height() <= 30., "Collapsed tab labels must stay on one line");
        }
        click(&mut app, &ctx, "inspector_rail_2");
        assert!(app.editor_layout.inspector_open && app.inspector==2);
        for mode in [editor_ui::PreviewLayout::Standard, editor_ui::PreviewLayout::PortraitRight] {
            app.editor_layout.mode=mode;
            for (width,height) in [(1080.,720.), (1440.,900.)] {
                for _ in 0..4 {
                    let _=ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(width,height))),..Default::default()},|ctx|app.draw(ctx));
                }
                let preview=app.hitboxes["preview_panel"];
                for key in ["preview_canvas","preview_source_play","preview_seek"] {
                    let r=app.hitboxes[key];
                    assert!(preview.contains_rect(r),"{mode:?} {width}x{height}: {key} outside preview: {r:?} vs {preview:?}");
                }
                if mode==editor_ui::PreviewLayout::PortraitRight {
                    assert!(preview.left()>=app.hitboxes["tl_ruler"].right());
                    assert!(app.hitboxes["preview_canvas"].height()>280.);
                } else {
                    assert!(app.hitboxes["preview_canvas"].height()>100.,"Standard preview too small at {width}x{height}");
                }
            }
        }
        app.editor_layout.mode=editor_ui::PreviewLayout::Standard;
        for _ in 0..3 {frame(&mut app, &ctx, vec![]);}
        click(&mut app, &ctx, "workflow_header");
        for _ in 0..16 {
            let _=ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(1080.,720.))),..Default::default()},|ctx|app.draw(ctx));
        }
        assert!(app.hitboxes["preview_panel"].contains_rect(app.hitboxes["preview_seek"]),"Expanded workflow must not hide preview transport");
        // Auto follows the output aspect, while manual docking never rewrites it.
        app.editor_layout.mode=editor_ui::PreviewLayout::Auto;
        assert!(!app.editor_layout.portrait_right(false));
        assert!(app.editor_layout.portrait_right(true));
        assert_eq!(serde_json::to_vec(&app.project).unwrap(),original);
        assert!(!app.dirty);
    }
    #[test]
    fn timeline_plays_layers_without_encoding_or_changing_script_duration() {
        let temp=tempfile::tempdir().unwrap();let ctx=egui::Context::default();let mut app=StudioApp::from_context(&ctx);
        app.projects_root=temp.path().join("projects");app.settings=Settings::default();app.capture_path=None;app.page=Page::Editor;
        let source=temp.path().join("source.mp4");
        process::run(&app.settings.ffmpeg,&process::args(&["-y","-v","error","-f","lavfi","-i","testsrc2=s=160x90:r=25:d=3","-c:v","libx264","-preset","ultrafast",&source.to_string_lossy()]),None,&process::cancel_token(),Duration::from_secs(20),None).unwrap();
        let source=media::import_local(&app.settings,&source,"s".into(),&process::cancel_token()).unwrap();
        let final_reference=temp.path().join("previous-final.mp4");
        app.project=Project{root:temp.path().into(),sources:vec![source],segments:vec![Segment{source_id:"s".into(),out_ms:2000,duration_ms:2000,..Default::default()}],rendered:Some(final_reference.clone()),..Default::default()};
        app.project.output.resolution=360;app.project.output.fps=25;
        for _ in 0..3 {frame(&mut app,&ctx,vec![]);}
        click(&mut app,&ctx,"tl_play");
        assert!(!app.busy && app.player.playing);
        let deadline=Instant::now()+Duration::from_secs(10);
        while app.layers.textures.is_empty() && Instant::now()<deadline {std::thread::sleep(Duration::from_millis(30));frame(&mut app,&ctx,vec![]);}
        assert!(!app.is_error,"{}",app.notice);
        assert!(app.layers.live && !app.preview_final && !app.layers.textures.is_empty());
        assert_eq!(app.project.rendered,Some(final_reference));
        assert_eq!(app.project.rules.min_seconds,180);
        assert!(!temp.path().join("cache/timeline-preview").exists());
        app.player.stop();
    }
    #[test]
    fn timeline_pointer_scrub_split_trim_reorder_undo_and_track_controls() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = StudioApp::from_context(&ctx);
        app.editor_layout.mode = editor_ui::PreviewLayout::Standard;
        app.projects_root = temp.path().join("projects");
        app.project = Project { root: temp.path().into(), sources: vec![Source {id:"s".into(),duration_ms:60000,..Default::default()}],
            segments: [4000,6000,4000].into_iter().enumerate().map(|(i,d)| Segment {source_id:"s".into(),in_ms:i as u64*10000,out_ms:i as u64*10000+d,duration_ms:d,..Default::default()}).collect(),..Default::default() };
        app.page=Page::Editor;app.capture_path=None;app.project.timeline.snapping=false;
        for _ in 0..3 {frame(&mut app,&ctx,vec![]);}
        let ruler=app.hitboxes["tl_ruler"];
        let clip=app.hitboxes["tl_clip_0_0"];
        let pos=Pos2::new(clip.center().x,ruler.center().y);
        frame(&mut app,&ctx,vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:true,modifiers:egui::Modifiers::NONE}]);
        frame(&mut app,&ctx,vec![egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:false,modifiers:egui::Modifiers::NONE}]);
        frame(&mut app,&ctx,vec![]);
        assert!(app.timeline_view.cursor.abs_diff(2000)<100,"Ruler should scrub output time, not source time: {}",app.timeline_view.cursor);
        click(&mut app,&ctx,"tl_split");
        assert_eq!(app.project.segments.len(),4);assert_eq!(app.project.duration_ms(),14000);
        click(&mut app,&ctx,"tl_undo");assert_eq!(app.project.segments.len(),3);
        click(&mut app,&ctx,"tl_redo");assert_eq!(app.project.segments.len(),4);
        click(&mut app,&ctx,"tl_undo");
        // Drag a trim handle inward. One drag is one undo step.
        let handle=app.hitboxes["tl_trim_0_0_true"].center();
        let target=handle-Vec2::new(50.,0.);
        for events in [vec![egui::Event::PointerMoved(handle),egui::Event::PointerButton{pos:handle,button:egui::PointerButton::Primary,pressed:true,modifiers:egui::Modifiers::NONE}],
            vec![egui::Event::PointerMoved(target)],vec![egui::Event::PointerButton{pos:target,button:egui::PointerButton::Primary,pressed:false,modifiers:egui::Modifiers::NONE}]] {frame(&mut app,&ctx,events);}
        assert!(app.project.segments[0].duration_ms<3900,"Trim must change actual clip duration");
        frame(&mut app,&ctx,vec![]);
        click(&mut app,&ctx,"tl_undo");assert_eq!(app.project.duration_ms(),14000);
        // Move the first scene across both following scenes.
        let id=app.project.segments[0].id.clone();
        let from=app.hitboxes["tl_clip_0_0"].center();let to=app.hitboxes["tl_clip_0_2"].center();
        for events in [vec![egui::Event::PointerMoved(from),egui::Event::PointerButton{pos:from,button:egui::PointerButton::Primary,pressed:true,modifiers:egui::Modifiers::NONE}],
            vec![egui::Event::PointerMoved(to)],vec![egui::Event::PointerButton{pos:to,button:egui::PointerButton::Primary,pressed:false,modifiers:egui::Modifiers::NONE}]] {frame(&mut app,&ctx,events);}
        assert_eq!(app.project.segments[2].id,id);
        click(&mut app,&ctx,"tl_track_0_true");assert!(app.project.timeline.tracks[0].locked);
        click(&mut app,&ctx,"tl_delete");assert_eq!(app.project.segments.len(),3);
        click(&mut app,&ctx,"tl_track_0_false");assert!(app.project.timeline.tracks[0].muted);
        app.project.save().unwrap();let loaded=Project::load(&temp.path().join("project.json")).unwrap();
        assert!(loaded.timeline.tracks[0].muted && loaded.timeline.tracks[0].locked);
        // Toolbar and ruler must remain inside the native window at both sizes.
        for (width,height) in [(1080.,720.),(1440.,900.)] {
            for _ in 0..3 {let _=ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(width,height))),..Default::default()},|ctx|app.draw(ctx));}
            for key in ["tl_undo","tl_delete","tl_play","tl_ruler"] {let r=app.hitboxes[key];assert!(r.left()>=202. && r.right()<=width && r.top()>=0. && r.bottom()<=height,"{width}x{height}: {key}: {r:?}");}
            let play=app.hitboxes["preview_source_play"];
            assert!(play.bottom()<app.hitboxes["tl_undo"].top(),"Preview transport must stay above the timeline at {width}x{height}: {play:?}");
        }
    }
    #[test]
    fn export_location_native_toolbar_and_real_mp4_use_saved_folder() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = StudioApp::from_context(&ctx);
        ctx.style_mut(|s| s.animation_time = 0.);
        app.capture_path = None;
        app.project.title = "Dự án với tiêu đề rất dài để kiểm tra bố cục thanh công cụ không tràn ra khỏi cửa sổ".repeat(3);
        for (width, height) in [(1080., 720.), (1440., 900.)] {
            for collapsed in [false, true] {
                app.editor_layout.sidebar_collapsed = collapsed;
                for _ in 0..3 {
                    let _ = ctx.run(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height))), ..Default::default() }, |ctx| app.draw(ctx));
                }
                let mut previous = None;
                for key in ["top_open", "top_save", "top_export_location", "top_export"] {
                    let rect = app.hitboxes[key];
                    assert!(rect.left() >= 202. && rect.right() <= width && rect.top() >= 0. && rect.bottom() < 105., "{width} {collapsed} {key}: {rect:?}");
                    assert!((rect.height() - 34.).abs() < 0.1, "{key}: {rect:?}");
                    if let Some(right) = previous { assert!(rect.left() > right); }
                    previous = Some(rect.right());
                }
            }
        }
        frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, "top_export_location");
        assert!(app.hitboxes.contains_key("export_change_folder"));
        frame(&mut app, &ctx, vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }]);

        let settings = Settings::default();
        let cancel = process::cancel_token();
        let input = temp.path().join("input.mp4");
        process::run(&settings.ffmpeg, &[
            "-y".into(), "-v".into(), "error".into(), "-f".into(), "lavfi".into(), "-i".into(),
            "testsrc2=size=320x180:rate=24:duration=2".into(), "-c:v".into(), "libx264".into(),
            "-preset".into(), "ultrafast".into(), input.to_string_lossy().into_owned(),
        ], None, &cancel, Duration::from_secs(30), None).unwrap();
        let source = media::import_local(&settings, &input, "src".into(), &cancel).unwrap();
        app.project = Project { root: temp.path().join("project"), sources: vec![source], segments: vec![Segment {
            source_id: "src".into(), in_ms: 0, out_ms: 2000, duration_ms: 2000, ..Default::default()
        }], ..Default::default() };
        app.project.rules.min_seconds = 1;
        app.project.rules.max_seconds = 3;
        app.project.output.resolution = 360;
        app.settings = settings.clone();
        let folder = temp.path().join("Ổ lưu video có dấu");
        fs::create_dir(&folder).unwrap();
        let prefs = temp.path().join("export-location.json");
        app.export_location = crate::export_location::ExportLocation::load(prefs.clone());
        app.export_location.remember(folder.clone()).unwrap();
        let expected = folder.join(format!("AIR3view-{}", app.project.id)).join("final.mp4");
        for _ in 0..2 {
            app.export_location = crate::export_location::ExportLocation::load(prefs.clone());
            app.launch(Task::Render);
            assert!(app.busy, "{}", app.notice);
            let deadline = Instant::now() + Duration::from_secs(60);
            while app.busy && Instant::now() < deadline { app.poll(&ctx); thread::sleep(Duration::from_millis(20)); }
            assert!(!app.busy && !app.is_error, "{}", app.notice);
            assert_eq!(app.project.rendered.as_ref(), Some(&expected));
            assert!(app.notice.contains(&expected.display().to_string()));
        }
        assert!(expected.is_file());
        assert!(!app.project.root.join("export/final.mp4").exists());
        assert!(expected.parent().unwrap().join("sources.json").is_file());
        let probe = media::probe(&settings, &expected, &cancel).unwrap();
        assert_eq!((probe.width, probe.height), (360, 640));
        assert!(probe.duration_ms.abs_diff(2000) < 150);
        assert_eq!(Project::load(&app.project.root.join("project.json")).unwrap().rendered, Some(expected));
    }

    #[test]
    fn native_navigation_lock_split_and_export_validation() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = StudioApp::from_context(&ctx);
        app.project.root = temp.path().to_owned();
        app.capture_path = None;
        // Empty new page stays clean while idle.
        app.page = Page::New;
        frame(&mut app, &ctx, vec![]);
        assert!(!app.dirty);
        click(&mut app, &ctx, "nav_Settings");
        assert_eq!(app.page, Page::Settings);
        click(&mut app, &ctx, "nav_Editor");
        assert_eq!(app.page, Page::Editor);
        app.project.sources = vec![Source {
            id: "src_1".into(),
            duration_ms: 10000,
            ..Default::default()
        }];
        app.project.segments = vec![Segment {
            source_id: "src_1".into(),
            in_ms: 0,
            out_ms: 6000,
            duration_ms: 6000,
            ..Default::default()
        }];
        frame(&mut app, &ctx, vec![]);
        click(&mut app, &ctx, "edit_tab");
        click(&mut app, &ctx, "lock");
        assert!(app.project.segments[0].locked);
        click(&mut app, &ctx, "tl_delete");
        assert_eq!(app.project.segments.len(), 1);
        click(&mut app, &ctx, "lock");
        assert!(!app.project.segments[0].locked);
        click(&mut app, &ctx, "action_3");
        assert_eq!(app.project.segments.len(), 2);
        assert_eq!(app.project.duration_ms(), 6000);
        assert_ne!(app.project.segments[0].id, app.project.segments[1].id);
        click(&mut app, &ctx, "tl_delete");
        assert_eq!(app.project.segments.len(), 1);
        app.launch(Task::Render);
        assert!(!app.busy);
        assert!(app.is_error);
        assert!(app.notice.contains("Thiếu video nguồn"));
    }
    #[test]
    fn settings_reference_toggle_and_character_preview() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = StudioApp::from_context(&ctx);
        // This test exercises the paid provider's image controls independently
        // of whichever provider the user has selected in saved settings.
        app.settings.avatar_provider = "heygen".into();
        app.project.root = temp.path().to_owned();
        app.capture_path = None;
        app.page = Page::Settings;
        let photo = temp.path().join("character.png");
        image::RgbaImage::from_pixel(96, 96, image::Rgba([30, 40, 50, 255]))
            .save(&photo)
            .unwrap();
        app.settings.avatar_image_path = photo.to_string_lossy().into_owned();
        app.settings.voice_reference_enabled = false;
        app.settings.voice_reference_path = temp
            .path()
            .join("reference.wav")
            .to_string_lossy()
            .into_owned();
        // Scroll the real settings surface before clicking controls that may
        // fall below the viewport after additions to the settings form.
        ctx.style_mut(|s|s.animation_time=0.);
        frame(&mut app, &ctx, vec![]);
        frame(&mut app, &ctx, vec![]);
        assert!(app.avatar_preview.is_some());
        fn settings_click(app:&mut StudioApp,ctx:&egui::Context,key:&str){
            for _ in 0..8 {
                let y=app.hitboxes[key].center().y;
                if (180.0..900.0).contains(&y){break;}
                frame(app,ctx,vec![egui::Event::PointerMoved(Pos2::new(500.,600.)),egui::Event::MouseWheel{unit:egui::MouseWheelUnit::Point,delta:Vec2::new(0.,if y>900.{-300.}else{300.}),modifiers:egui::Modifiers::NONE}]);
                for _ in 0..12 {frame(app,ctx,vec![]);}
            }
            click(app,ctx,key);
        }
        settings_click(&mut app, &ctx, "voice_reference_toggle");
        assert!(app.settings.voice_reference_enabled);
        settings_click(&mut app, &ctx, "clear_reference");
        assert!(!app.settings.voice_reference_enabled);
        assert!(app.settings.voice_reference_path.is_empty());
        settings_click(&mut app, &ctx, "clear_avatar_image");
        assert!(app.settings.avatar_image_path.is_empty());
        assert!(app.avatar_preview.is_none());
    }
    #[test]
    fn scene_preview_preserves_final_export() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = StudioApp::from_context(&ctx);
        app.project.root = temp.path().to_owned();
        let final_path = temp.path().join("final.mp4");
        let scene = temp.path().join("scene.mp4");
        app.project.rendered = Some(final_path.clone());
        app.tx
            .send(Event::Preview {
                path: scene.clone(),
                duration_ms: 2500,
            })
            .unwrap();
        app.poll(&ctx);
        assert_eq!(app.project.rendered, Some(final_path));
        assert_eq!(app.preview_path(), Some(scene));
        assert_eq!(app.preview_scene.as_ref().unwrap().1, 2500);
    }
    #[test]
    fn cartoon_settings_preview_switches_presets_without_cloud_credentials() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = StudioApp::from_context(&ctx);
        app.project.root = temp.path().to_owned();
        app.capture_path = None;
        app.page = Page::Settings;
        app.settings.avatar_provider = "cartoon2d".into();
        app.settings.avatar_enabled = true;
        app.settings.cartoon_character = "studio".into();
        frame(&mut app, &ctx, vec![]);
        assert!(app
            .avatar_preview
            .as_ref()
            .unwrap()
            .0
            .ends_with("cartoon-studio.png"));
        app.settings.cartoon_character = "analyst".into();
        frame(&mut app, &ctx, vec![]);
        assert!(app
            .avatar_preview
            .as_ref()
            .unwrap()
            .0
            .ends_with("cartoon-analyst.png"));
        let roundtrip: Settings =
            serde_json::from_str(&serde_json::to_string(&app.settings).unwrap()).unwrap();
        assert_eq!(roundtrip.avatar_provider, "cartoon2d");
    }
}
