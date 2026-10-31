use super::*;
use crate::{
    timeline::*,
    timeline_media::{self, Data, MediaCache, Request},
};

const HEIGHTS: [f32; TRACK_COUNT] = [48., 34., 38., 38., 31., 32., 28., 40., 32., 32., 38.];
const HEADER: f32 = 152.;
const RULER: f32 = 27.;
const GOLD: Color32 = Color32::from_rgb(240, 192, 91);
const PURPLE: Color32 = Color32::from_rgb(176, 139, 255);
fn color(track: usize) -> Color32 {
    [
        BLUE,
        PURPLE,
        TEAL,
        Color32::from_rgb(93, 175, 122),
        GOLD,
        Color32::from_rgb(235, 149, 90),
        Color32::from_rgb(110,130,150),
        Color32::from_rgb(105,190,230),
        Color32::from_rgb(230,105,90),
        Color32::from_rgb(184,151,235),
        Color32::from_rgb(100,195,155),
    ][track]
}

struct Drag {
    index: usize,
    clip_id:String,
    edge: Option<bool>,
    delta: f32,
    origin_x: f32,
}
pub(super) struct TimelineView {
    pub cursor: u64,
    pub full_preview: bool,
    pub history: EditHistory,
    pub navigating_history: bool,
    pub project_id: String,
    clipboard: Option<Segment>,
    pub(super) track: usize,
    drag: Option<Drag>,
    media: MediaCache,
    textures: BTreeMap<String, egui::TextureHandle>,
    last_seek: Instant,
}
impl Default for TimelineView {
    fn default() -> Self {
        Self {
            cursor: 0,
            full_preview: false,
            history: Default::default(),
            navigating_history: false,
            project_id: String::new(),
            clipboard: None,
            track: VIDEO,
            drag: None,
            media: Default::default(),
            textures: Default::default(),
            last_seek: Instant::now() - Duration::from_secs(1),
        }
    }
}

impl StudioApp {
    pub(super) fn sync_timeline_cursor(&mut self) {
        let start = starts(&self.project)
            .get(self.selected)
            .copied()
            .unwrap_or(0);
        self.timeline_view.cursor = if self.preview_final
            && (self.timeline_view.full_preview || self.preview_scene.is_none())
        {
            self.playhead
        } else if self.preview_final {
            start + self.playhead
        } else if let Some(seg) = self.project.segments.get(self.selected) {
            start
                + self
                    .playhead
                    .saturating_sub(if seg.kind == "host" {
                        seg.avatar_start_ms
                    } else {
                        seg.in_ms
                    })
                    .min(seg.duration_ms)
        } else {
            0
        };
    }
    pub(super) fn seek_timeline(&mut self,ms:u64) {
        self.project.ensure_composition();
        self.timeline_view.cursor=ms.min(self.project.composition_duration().saturating_sub(1));
        if let Some((i,_))=locate(&self.project,self.timeline_view.cursor){self.selected=i;}
        self.preview_final=false;self.preview_scene=None;self.playhead=self.timeline_view.cursor;
        self.layers.live=true;self.texture=None;self.blurred_texture=None;self.layers.textures.clear();
        self.request_frame();
    }
    pub(super) fn apply_timeline_edit(&mut self, command: &str) {
        if self.busy {
            return;
        }
        if self.independent_layer_edit(command){return;}
        let before = self.project.clone();
        let index = self.selected;
        let local = self
            .timeline_view
            .cursor
            .saturating_sub(starts(&self.project).get(index).copied().unwrap_or(0));
        let result = match command {
            "split" => split(&mut self.project, index, local),
            "delete" => delete(&mut self.project, index),
            "cut" => {
                self.timeline_view.clipboard = self.project.segments.get(index).cloned();
                delete(&mut self.project, index)
            }
            "copy" => {
                self.timeline_view.clipboard = self.project.segments.get(index).cloned();
                return;
            }
            "paste" | "duplicate" => {
                let clip = if command == "paste" {
                    self.timeline_view.clipboard.clone()
                } else {
                    self.project.segments.get(index).cloned()
                };
                clip.context("Chưa sao chép cảnh")
                    .and_then(|s| insert(&mut self.project, index, s).map(|i| self.selected = i))
            }
            "trim_start" => {
                let end = self
                    .project
                    .segments
                    .get(index)
                    .map(|s| s.duration_ms)
                    .unwrap_or(0);
                trim(&mut self.project, index, local, end)
            }
            "trim_end" => trim(&mut self.project, index, 0, local),
            "up" if index > 0 => {
                move_scene(&mut self.project, index, index - 1).map(|_| self.selected -= 1)
            }
            "down" if index + 1 < self.project.segments.len() => {
                move_scene(&mut self.project, index, index + 1).map(|_| self.selected += 1)
            }
            _ => return,
        };
        match result {
            Ok(()) => {
                self.timeline_view.history.record(before);
                self.selected = self
                    .selected
                    .min(self.project.segments.len().saturating_sub(1));
                self.touch();
                self.seek_timeline(self.timeline_view.cursor);
            }
            Err(e) => self.message(format!("{e:#}"), true),
        }
    }
    fn timeline_undo(&mut self, redo: bool) {
        if self.busy {
            return;
        }
        let changed = if redo {
            self.timeline_view.history.redo(&mut self.project)
        } else {
            self.timeline_view.history.undo(&mut self.project)
        };
        if changed {
            self.timeline_view.navigating_history = true;
            self.selected = self
                .selected
                .min(self.project.segments.len().saturating_sub(1));
            self.touch();
            self.seek_timeline(self.timeline_view.cursor);
        }
    }
    pub(super) fn play_timeline(&mut self) {
        if self.player.playing {self.player.stop();return;}
        if self.busy || self.project.segments.is_empty(){return;}
        self.project.ensure_composition();
        let end=self.project.composition_duration();
        let ms=if self.timeline_view.cursor+50>=end {0}else{self.timeline_view.cursor};
        self.preview_final=false;self.preview_scene=None;
        self.timeline_view.full_preview=false;self.layers.live=true;
        self.texture=None;self.blurred_texture=None;
        self.player.composition(self.mask_preview(),self.settings.clone(),ms,true);
    }
    fn step_timeline(&mut self, forward: bool) {
        let fps = self.project.output.fps.max(1) as f64;
        let frame = (self.timeline_view.cursor as f64 * fps / 1000.).round() as u64;
        let frame = if forward {
            frame + 1
        } else {
            frame.saturating_sub(1)
        };
        self.seek_timeline((frame as f64 * 1000. / fps).round() as u64);
    }
    pub(super) fn timeline_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let (ctrl, shift) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));
        let pressed = |key| ctx.input(|i| i.key_pressed(key));
        if pressed(egui::Key::Space) {
            self.play_timeline();
        }
        if pressed(egui::Key::ArrowLeft) && !self.layers.canvas_focused {
            self.step_timeline(false);
        }
        if pressed(egui::Key::ArrowRight) && !self.layers.canvas_focused {
            self.step_timeline(true);
        }
        if pressed(egui::Key::Home) {
            self.seek_timeline(0);
        }
        if pressed(egui::Key::End) {
            self.seek_timeline(self.project.composition_duration().saturating_sub(1));
        }
        if self.busy {
            return;
        }
        if ctrl && pressed(egui::Key::Z) {
            self.timeline_undo(shift);
        }
        if ctrl && pressed(egui::Key::Y) {
            self.timeline_undo(true);
        }
        for (key, command, need_ctrl) in [
            (egui::Key::B, "split", true),
            (egui::Key::C, "copy", true),
            (egui::Key::X, "cut", true),
            (egui::Key::V, "paste", true),
            (egui::Key::D, "duplicate", true),
            (egui::Key::Delete, "delete", false),
            (egui::Key::Q, "trim_start", false),
            (egui::Key::W, "trim_end", false),
        ] {
            if pressed(key) && ctrl == need_ctrl {
                self.apply_timeline_edit(command);
            }
        }
        if !ctrl && pressed(egui::Key::N) {
            self.project.timeline.snapping = !self.project.timeline.snapping;
            self.dirty = true;
        }
        if !ctrl && pressed(egui::Key::F) {
            self.zoom = 1.;
        }
        if pressed(egui::Key::Plus) || pressed(egui::Key::Equals) {
            self.zoom = (self.zoom * 1.3).min(32.);
        }
        if pressed(egui::Key::Minus) {
            self.zoom = (self.zoom / 1.3).max(0.25);
        }
    }
    fn timeline_button(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        command: &str,
        enabled: bool,
        hint: &str,
    ) {
        let response = ui
            .add_enabled(
                enabled && !self.busy,
                egui::Button::new(label).min_size(Vec2::new(0., 27.)),
            )
            .on_hover_text(hint);
        #[cfg(test)]
        self.hitboxes.insert(format!("tl_{command}"), response.rect);
        if response.clicked() {
            self.apply_timeline_edit(command);
        }
    }
    pub(super) fn timeline(&mut self, ui: &mut egui::Ui) {
        self.timeline_view.media.poll();
        let has = !self.project.segments.is_empty();
        let can_edit = has && self.layers.selected.as_ref().and_then(|id|self.project.composition_snapshot().clips.into_iter().find(|c|&c.id==id&&!c.linked)).map(|c|c.editable(&self.project)).unwrap_or_else(||editable(&self.project,self.selected).is_ok());
        let compact=ui.available_width()<1250.;
        egui::ScrollArea::horizontal().id_salt("timeline_toolbar_scroll")
            .auto_shrink([false,true]).show(ui,|ui| {ui.horizontal(|ui| {
            ui.spacing_mut().interact_size.y=28.;
            ui.spacing_mut().item_spacing.x=5.;
            ui.spacing_mut().slider_width=if compact{60.}else{90.};
            ui.style_mut().wrap_mode=Some(egui::TextWrapMode::Extend);
            let _sfx=ui.menu_button("SFX",|ui| {
                ui.style_mut().wrap_mode=Some(egui::TextWrapMode::Wrap);
                ui.set_width(270.);
                self.card_sound_settings_ui(ui);
                ui.separator();
                self.evidence_sound_settings_ui(ui);
            });
            if self.capture_path.is_some() && self.frames==3 && std::env::args().any(|a|a=="--capture-menu=sfx") {
                let mut state=egui::menu::BarState::load(ui.ctx(),ui.id());
                **state=Some(egui::menu::MenuRoot::new(_sfx.response.rect.left_bottom(),_sfx.response.id));
                state.store(ui.ctx(),ui.id());
            }
            #[cfg(test)] self.hitboxes.insert("tl_sfx".into(),_sfx.response.rect);
            let undo = ui
                .add_enabled(
                    !self.busy && !self.timeline_view.history.past.is_empty(),
                    egui::Button::new("Undo").min_size(Vec2::new(28., 28.)),
                )
                .on_hover_text("Hoàn tác • Ctrl+Z");
            #[cfg(test)]
            self.hitboxes.insert("tl_undo".into(), undo.rect);
            if undo.clicked() {
                self.timeline_undo(false);
            }
            let redo = ui
                .add_enabled(
                    !self.busy && !self.timeline_view.history.future.is_empty(),
                    egui::Button::new("Redo").min_size(Vec2::new(28., 28.)),
                )
                .on_hover_text("Làm lại • Ctrl+Shift+Z / Ctrl+Y");
            #[cfg(test)]
            self.hitboxes.insert("tl_redo".into(), redo.rect);
            if redo.clicked() {
                self.timeline_undo(true);
            }
            ui.separator();
            self.timeline_button(
                ui,
                "Tách",
                "split",
                can_edit,
                "Tách tại playhead • Ctrl+B • Giữ voice/avatar đồng bộ",
            );
            ui.add_enabled_ui(can_edit && !self.busy, |ui| {
                ui.menu_button("Trim", |ui| {
                    if ui.button("Bỏ đầu đến playhead • Q").clicked() {
                        self.apply_timeline_edit("trim_start");
                        ui.close_menu();
                    }
                    if ui.button("Bỏ cuối từ playhead • W").clicked() {
                        self.apply_timeline_edit("trim_end");
                        ui.close_menu();
                    }
                });
            });
            self.timeline_button(
                ui,
                if compact{"Copy"}else{"Sao chép"},
                "copy",
                has,
                "Sao chép cảnh liên kết • Ctrl+C",
            );
            self.timeline_button(
                ui,
                "Dán",
                "paste",
                self.timeline_view.clipboard.is_some() || self.layers.clipboard.is_some(),
                "Dán sau cảnh chọn • Ctrl+V",
            );
            self.timeline_button(
                ui,
                if compact{"×2"}else{"Nhân bản"},
                "duplicate",
                can_edit,
                "Nhân bản cả nhóm cảnh • Ctrl+D",
            );
            self.timeline_button(
                ui,
                "Xóa",
                "delete",
                can_edit,
                "Ripple delete: xóa cảnh và nối phần sau • Delete",
            );
            ui.add_enabled_ui(!self.busy, |ui| {
                if ui
                    .button(if compact{"+"}else{"+ Nguồn"})
                    .on_hover_text("Nhập video local")
                    .clicked()
                {
                    self.import_dialog();
                }
            });
            ui.menu_button("?", |ui| {
                ui.label("Space: phát/dừng timeline • ←/→: 1 frame • Home/End");
                ui.label("Ctrl+B: tách • Q/W: trim đầu/cuối • Delete: xóa");
                ui.label("Ctrl+C/X/V/D: copy/cut/paste/nhân bản");
                ui.label("Ctrl+Z/Y: undo/redo • N: snap • F: vừa toàn bộ • +/-: zoom");
                ui.label("Kéo thân clip để đổi thứ tự; kéo mép để rút ngắn.");
                ui.label("Các track cùng cảnh liên kết; khóa một track sẽ bảo vệ nhóm cảnh.");
            });
            ui.separator();
            let previous = ui.button("|◀").on_hover_text("Về đầu • Home");
            if previous.clicked() {
                self.seek_timeline(0);
            }
            if ui.button("◀").on_hover_text("Lùi 1 frame").clicked() {
                self.step_timeline(false);
            }
            let play = ui
                .add_enabled(
                    has && !self.busy,
                    egui::Button::new(if self.player.playing {
                        "Ⅱ"
                    } else {
                        "▶"
                    }),
                )
                .on_hover_text("Space • Phát layer trực tiếp, không dựng MP4");
            #[cfg(test)]
            self.hitboxes.insert("tl_play".into(), play.rect);
            if play.clicked() {
                self.play_timeline();
            }
            if ui.button("▶|").on_hover_text("Tiến 1 frame").clicked() {
                self.step_timeline(true);
            }
            ui.label(
                RichText::new(format!(
                    "{} / {}",
                    seconds(self.timeline_view.cursor),
                    seconds(self.project.composition_duration())
                ))
                .monospace()
                .color(TEAL)
                .size(11.),
            );
            let snap = ui.add_enabled(
                !self.busy,
                egui::Checkbox::new(&mut self.project.timeline.snapping, "Snap"),
            );
            #[cfg(test)]
            self.hitboxes.insert("tl_snap".into(), snap.rect);
            if snap.changed() {
                self.dirty = true;
            }
            if ui.button("−").clicked() {
                self.zoom = (self.zoom / 1.4).max(0.25);
            }
            ui.add(
                egui::Slider::new(&mut self.zoom, 0.25..=32.)
                    .logarithmic(true)
                    .show_value(false)
                    .text(if compact{""}else{"Zoom"}),
            );
            if ui.button("+").clicked() {
                self.zoom = (self.zoom * 1.4).min(32.);
            }
            if ui
                .button("Vừa")
                .on_hover_text("Vừa toàn bộ timeline • F")
                .clicked()
            {
                self.zoom = 1.;
            }
        });});
        if !has {
            ui.label("Thêm nguồn hoặc tạo kịch bản để bắt đầu.");
            return;
        }
        ui.separator();
        self.project.ensure_composition();
        let composition=self.project.composition_snapshot();
        let duration = self.project.composition_duration();
        let viewport_width = (ui.available_width() - HEADER - 12.).max(100.);
        let pps = (viewport_width / (duration as f32 / 1000.).max(1.)) * self.zoom;
        let content_width = (duration as f32 / 1000. * pps + 14.).max(viewport_width);
        let total_height = RULER + HEIGHTS.iter().sum::<f32>();
        let mut action = None;
        let mut seek = None;
        let mut drag_start = None;
        let mut finished = false;
        let mut track_toggle = None;
        let mut context_command = None;
        egui::ScrollArea::vertical()
            .id_salt("timeline_tracks_vertical")
            .max_height(ui.available_height().max(30.))
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    let (header, _) =
                        ui.allocate_exact_size(Vec2::new(HEADER, total_height), Sense::hover());
                    ui.painter().text(
                        header.min + Vec2::new(6., 6.),
                        Align2::LEFT_TOP,
                        format!("{} cảnh • liên kết", self.project.segments.len()),
                        FontId::proportional(11.),
                        MUTED,
                    );
                    let mut y = header.top() + RULER;
                    for (track, height) in HEIGHTS.iter().enumerate() {
                        let row = Rect::from_min_size(
                            Pos2::new(header.left(), y),
                            Vec2::new(HEADER - 4., *height),
                        );
                        ui.painter().rect_filled(
                            row,
                            3.,
                            if self.timeline_view.track == track {
                                CARD
                            } else {
                                PANEL
                            },
                        );
                        ui.painter().rect_filled(
                            Rect::from_min_size(row.min, Vec2::new(3., *height - 2.)),
                            0.,
                            color(track),
                        );
                        ui.painter().text(
                            row.min + Vec2::new(9., height / 2.),
                            Align2::LEFT_CENTER,
                            TRACK_NAMES[track],
                            FontId::proportional(11.),
                            color(track),
                        );
                        for (lock, offset) in [(false, 108.), (true, 129.)] {
                            let state = &self.project.timeline.tracks[track];
                            let active = if lock {
                                state.locked
                            } else {
                                state.muted
                                    || (track == CAPTIONS && !self.project.subtitles.enabled)
                            };
                            let rect = Rect::from_min_size(
                                Pos2::new(row.left() + offset, row.center().y - 10.),
                                Vec2::splat(19.),
                            );
                            let response = ui.interact(
                                rect,
                                ui.id().with(("track_button", track, lock)),
                                Sense::click(),
                            );
                            ui.painter().rect_filled(
                                rect,
                                3.,
                                if active {
                                    color(track).gamma_multiply(0.28)
                                } else {
                                    BG
                                },
                            );
                            ui.painter().text(
                                rect.center(),
                                Align2::CENTER_CENTER,
                                if lock {
                                    "K"
                                } else if matches!(track,VOICE|ORIGINAL|SFX|MUSIC) {
                                    "M"
                                } else {
                                    "Ẩ"
                                },
                                FontId::proportional(10.),
                                if active { color(track) } else { MUTED },
                            );
                            #[cfg(test)]
                            self.hitboxes
                                .insert(format!("tl_track_{track}_{lock}"), rect);
                            if response.clicked() && !self.busy {
                                track_toggle = Some((track, lock));
                            }
                            response.on_hover_text(if lock {
                                "Khóa/mở track • bảo vệ các cảnh liên kết"
                            } else if matches!(track,VOICE|ORIGINAL|SFX|MUSIC) {
                                "Tắt/bật tiếng track • có hiệu lực khi preview/render"
                            } else {
                                "Ẩn/hiện track • có hiệu lực khi preview/render"
                            });
                        }
                        y += height;
                    }
                    egui::ScrollArea::horizontal()
                        .id_salt("timeline_time_horizontal")
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            let (rect, _) = ui.allocate_exact_size(
                                Vec2::new(content_width, total_height),
                                Sense::hover(),
                            );
                            let clip = ui.clip_rect().intersect(rect);
                            let painter = ui.painter_at(clip);
                            let ruler =
                                Rect::from_min_size(rect.min, Vec2::new(content_width, RULER));
                            painter.rect_filled(ruler, 0., BG);
                            let interval =
                                [0.2, 0.5, 1., 2., 5., 10., 15., 30., 60., 120., 300., 600.]
                                    .into_iter()
                                    .find(|s| s * pps >= 65.)
                                    .unwrap_or(600.);
                            let visible_start = ((clip.left() - rect.left()) / pps).max(0.);
                            let visible_end = ((clip.right() - rect.left()) / pps).max(0.);
                            let first = (visible_start / interval).floor() as u64;
                            let last = (visible_end / interval).ceil() as u64;
                            for i in first..=last {
                                let x = rect.left() + i as f32 * interval * pps;
                                painter.line_segment(
                                    [Pos2::new(x, rect.top() + 21.), Pos2::new(x, rect.bottom())],
                                    Stroke::new(1., EDGE.gamma_multiply(0.6)),
                                );
                                painter.text(
                                    Pos2::new(x + 4., rect.top() + 4.),
                                    Align2::LEFT_TOP,
                                    seconds((i as f32 * interval * 1000.) as u64),
                                    FontId::monospace(10.),
                                    MUTED,
                                );
                            }
                            let rr = ui.interact(
                                ruler,
                                ui.id().with("time_ruler"),
                                Sense::click_and_drag(),
                            );
                            #[cfg(test)]
                            self.hitboxes
                                .insert("tl_ruler".into(), ruler.intersect(clip));
                            if rr.clicked() || rr.dragged() {
                                if let Some(pos) = rr.interact_pointer_pos() {
                                    let mut ms =
                                        ((pos.x - rect.left()).max(0.) / pps * 1000.) as u64;
                                    if self.project.timeline.snapping {
                                        ms = snap(ms, &self.project, (7000. / pps) as u64);
                                    }
                                    self.timeline_view.cursor = ms.min(duration.saturating_sub(1));
                                    if rr.clicked()
                                        || self.timeline_view.last_seek.elapsed()
                                            > Duration::from_millis(100)
                                    {
                                        seek = Some(self.timeline_view.cursor);
                                        self.timeline_view.last_seek = Instant::now();
                                    }
                                }
                            }
                            if rr.drag_stopped() {
                                seek = Some(self.timeline_view.cursor);
                            }
                            let mut y = rect.top() + RULER;
                            for (track, height) in HEIGHTS.iter().enumerate() {
                                painter.line_segment(
                                    [
                                        Pos2::new(rect.left(), y + height - 1.),
                                        Pos2::new(rect.right(), y + height - 1.),
                                    ],
                                    Stroke::new(1., EDGE.gamma_multiply(0.5)),
                                );
                                let muted = self.project.timeline.tracks[track].muted
                                    || (track == CAPTIONS && !self.project.subtitles.enabled);
                                for layer in composition.clips.iter().filter(|c|c.track==track) {
                                    let Some((index,seg))=self.project.segments.iter().enumerate().find(|(_,s)|s.id==layer.scene_id) else {continue;};
                                    let muted=muted||!layer.visible;
                                    let start_x = rect.left() + layer.start_ms as f32 / 1000. * pps;
                                    let width = (layer.duration_ms as f32 / 1000. * pps).max(2.);
                                    let clip_rect = Rect::from_min_size(
                                        Pos2::new(start_x + 1., y + 2.),
                                        Vec2::new((width - 2.).max(1.), height - 4.),
                                    );
                                    if !clip_rect.intersects(clip) {
                                        continue;
                                    }
                                    let selected = self.selected == index;
                                    painter.rect_filled(
                                        clip_rect,
                                        4.,
                                        color(track).gamma_multiply(if muted {
                                            0.07
                                        } else if selected {
                                            0.30
                                        } else {
                                            0.15
                                        }),
                                    );
                                    painter.rect_stroke(
                                        clip_rect,
                                        4.,
                                        Stroke::new(
                                            if selected { 1.5 } else { 1. },
                                            if selected { color(track) } else { EDGE },
                                        ),
                                        egui::StrokeKind::Inside,
                                    );
                                    let cp = painter
                                        .with_clip_rect(clip.intersect(clip_rect).shrink(2.));
                                    let ready = match track {
                                        VOICE => seg.audio.is_some(),
                                        AVATAR => seg.avatar.is_some(),
                                        _ => true,
                                    };
                                    let label = match track {
                                        VIDEO => format!(
                                            "{:02} {}{}",
                                            index + 1,
                                            seg.kind.to_uppercase(),
                                            if seg.locked { " • khóa" } else { "" }
                                        ),
                                        AVATAR => {
                                            if ready {
                                                "2D / Avatar".into()
                                            } else {
                                                "Chưa tạo avatar".into()
                                            }
                                        }
                                        VOICE => {
                                            if ready {
                                                "VieNeu".into()
                                            } else {
                                                "Chưa tạo voice".into()
                                            }
                                        }
                                        ORIGINAL => if seg.kind=="review" {"Tiếng gốc nền".into()}else{"Âm thanh gốc".into()},
                                        MUSIC => "Nhạc nền".into(),
                                        CAPTIONS => {
                                            if seg.caption.is_empty() {
                                                seg.narration.clone()
                                            } else {
                                                seg.caption.clone()
                                            }
                                        }
                                        BACKGROUND => "Nền".into(),
                                        EVIDENCE | ANNOTATION => seg.evidence_items.iter().find(|e|Some(&e.id)==layer.evidence_id.as_ref()).map(|e|e.label.clone()).unwrap_or_else(||"Dẫn chứng".into()),
                                        SFX => format!("{} · {}",if layer.evidence_id.is_some(){"Ảnh"}else{"Thẻ"},layer.media.as_deref().and_then(crate::evidence_sfx::managed_style).map(crate::evidence_sfx::style_label).unwrap_or("Âm riêng")),
                                        _ => seg
                                            .graphics
                                            .iter()
                                            .map(|g| g.label.as_str())
                                            .collect::<Vec<_>>()
                                            .join(" • "),
                                    };
                                    if track == VIDEO || track == AVATAR || track == EVIDENCE {
                                        let path = if track == EVIDENCE {layer.media.clone()} else if track == AVATAR {
                                            seg.avatar.clone()
                                        } else {
                                            self.project.source(&seg.source_id).ok().map(|s| {
                                                if s.proxy.is_file() {
                                                    s.proxy.clone()
                                                } else {
                                                    s.path.clone()
                                                }
                                            })
                                        };
                                        if let Some(path) = path.filter(|p| p.is_file()) {
                                            let begin = layer.media_in_ms;
                                            let key = timeline_media::identity(
                                                &path,
                                                begin,
                                                layer.source_duration(layer.duration_ms),
                                                false,
                                            );
                                            self.timeline_view.media.request(Request {
                                                key: key.clone(),
                                                path,
                                                start: begin,
                                                duration: layer.source_duration(layer.duration_ms),
                                                waveform: false,
                                                folder: self.project.root.join("cache/timeline"),
                                                settings: self.settings.clone(),
                                            });
                                            if !self.timeline_view.textures.contains_key(&key) {
                                                if let Some(Data::Thumbnail(im)) =
                                                    self.timeline_view.media.ready.get(&key)
                                                {
                                                    let image =
                                                        egui::ColorImage::from_rgba_unmultiplied(
                                                            [
                                                                im.width() as usize,
                                                                im.height() as usize,
                                                            ],
                                                            im.as_raw(),
                                                        );
                                                    self.timeline_view.textures.insert(
                                                        key.clone(),
                                                        ui.ctx().load_texture(
                                                            key.clone(),
                                                            image,
                                                            Default::default(),
                                                        ),
                                                    );
                                                }
                                            }
                                            if let Some(texture) =
                                                self.timeline_view.textures.get(&key)
                                            {
                                                let available = Vec2::new(42., height - 21.);
                                                let original = texture.size_vec2();
                                                let scale = (available.x / original.x)
                                                    .min(available.y / original.y);
                                                let image_rect = Rect::from_min_size(
                                                    clip_rect.min + Vec2::new(3., 16.),
                                                    original * scale,
                                                );
                                                cp.image(
                                                    texture.id(),
                                                    image_rect,
                                                    Rect::from_min_max(
                                                        Pos2::ZERO,
                                                        Pos2::new(1., 1.),
                                                    ),
                                                    Color32::WHITE,
                                                );
                                            }
                                        }
                                    }
                                    if matches!(track,VOICE|ORIGINAL|SFX|MUSIC) {
                                        let path = if matches!(track,SFX|MUSIC) {layer.media.clone()}else if track == VOICE {
                                            seg.audio.clone()
                                        } else {
                                            self.project
                                                .source(&seg.source_id)
                                                .ok()
                                                .filter(|s| s.has_audio)
                                                .map(|s| s.path.clone())
                                        };
                                        if let Some(path) = path.filter(|p| p.is_file()) {
                                            let begin = layer.media_in_ms;
                                            let key = timeline_media::identity(
                                                &path,
                                                begin,
                                                layer.source_duration(layer.duration_ms),
                                                true,
                                            );
                                            self.timeline_view.media.request(Request {
                                                key: key.clone(),
                                                path,
                                                start: begin,
                                                duration: layer.source_duration(layer.duration_ms),
                                                waveform: true,
                                                folder: self.project.root.join("cache/timeline"),
                                                settings: self.settings.clone(),
                                            });
                                            if let Some(Data::Waveform(peaks)) =
                                                self.timeline_view.media.ready.get(&key)
                                            {
                                                for (n, value) in peaks.iter().enumerate() {
                                                    let x = clip_rect.left()
                                                        + clip_rect.width() * n as f32
                                                            / peaks.len() as f32;
                                                    let amplitude = (value * layer.effective_gain(&self.project))
                                                        .min(1.)
                                                        * (height - 16.)
                                                        / 2.;
                                                    cp.line_segment(
                                                        [
                                                            Pos2::new(
                                                                x,
                                                                clip_rect.center().y + 4.
                                                                    - amplitude,
                                                            ),
                                                            Pos2::new(
                                                                x,
                                                                clip_rect.center().y
                                                                    + 4.
                                                                    + amplitude,
                                                            ),
                                                        ],
                                                        Stroke::new(
                                                            1.,
                                                            color(track).gamma_multiply(if muted {
                                                                0.15
                                                            } else {
                                                                0.7
                                                            }),
                                                        ),
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    if track == GRAPHICS {
                                        for g in &seg.graphics {
                                            let gr = Rect::from_min_max(
                                                Pos2::new(
                                                    start_x + g.start_ms as f32 / 1000. * pps,
                                                    y + 4.,
                                                ),
                                                Pos2::new(
                                                    start_x + g.end_ms as f32 / 1000. * pps,
                                                    y + height - 4.,
                                                ),
                                            );
                                            cp.rect_filled(
                                                gr,
                                                2.,
                                                color(track).gamma_multiply(0.25),
                                            );
                                        }
                                    }
                                    cp.text(
                                        clip_rect.min + Vec2::new(6., 4.),
                                        Align2::LEFT_TOP,
                                        label,
                                        FontId::proportional(10.),
                                        if muted { MUTED } else { color(track) },
                                    );
                                    let handle = if selected && width > 25. { 6. } else { 0. };
                                    let body = clip_rect.shrink2(Vec2::new(handle, 0.));
                                    let response = ui.interact(
                                        body,
                                        ui.id().with(("timeline_clip", &layer.id)),
                                        Sense::click_and_drag(),
                                    );
                                    #[cfg(test)]
                                    self.hitboxes
                                        .insert(format!("tl_clip_{track}_{index}"), body);
                                    if response.clicked() {
                                        action = Some((index, track,layer.id.clone()));
                                    }
                                    if response.double_clicked() {
                                        action = Some((index, track,layer.id.clone()));
                                        self.inspector = if track == CAPTIONS { 2 } else if track==EVIDENCE||track==ANNOTATION {3} else { 0 };
                                    }
                                    if response.drag_started() && !self.busy {
                                        drag_start = Some(Drag {
                                            index,
                                            clip_id:layer.id.clone(),
                                            edge: None,
                                            delta: 0.,
                                            origin_x: ui.input(|i| {
                                                i.pointer.press_origin().map(|p| p.x).unwrap_or(0.)
                                            }),
                                        });
                                    }
                                    if response.drag_stopped() {
                                        finished = true;
                                    }
                                    response.context_menu(|ui| {
                                        for (label, cmd) in [
                                            ("Tách tại playhead", "split"),
                                            ("Sao chép", "copy"),
                                            ("Cắt vào clipboard", "cut"),
                                            ("Dán sau cảnh", "paste"),
                                            ("Nhân bản", "duplicate"),
                                            ("Xóa cảnh liên kết", "delete"),
                                        ] {
                                            if ui
                                                .add_enabled(!self.busy, egui::Button::new(label))
                                                .clicked()
                                            {
                                                context_command = Some((index, track, cmd,layer.id.clone()));
                                                ui.close_menu();
                                            }
                                        }
                                    });
                                    for end in [false, true] {
                                        if handle == 0. {
                                            continue;
                                        }
                                        let edge = Rect::from_min_size(
                                            Pos2::new(
                                                if end {
                                                    clip_rect.right() - handle
                                                } else {
                                                    clip_rect.left()
                                                },
                                                clip_rect.top(),
                                            ),
                                            Vec2::new(handle, clip_rect.height()),
                                        );
                                        let r = ui
                                            .interact(
                                                edge,
                                                ui.id().with(("trim_handle", &layer.id, end)),
                                                Sense::click_and_drag(),
                                            )
                                            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
                                        #[cfg(test)]
                                        self.hitboxes
                                            .insert(format!("tl_trim_{track}_{index}_{end}"), edge);
                                        cp.line_segment(
                                            [
                                                Pos2::new(edge.center().x, edge.top() + 5.),
                                                Pos2::new(edge.center().x, edge.bottom() - 5.),
                                            ],
                                            Stroke::new(2., color(track)),
                                        );
                                        if r.drag_started() && !self.busy {
                                            drag_start = Some(Drag {
                                                index,
                                                clip_id:layer.id.clone(),
                                                edge: Some(end),
                                                delta: 0.,
                                                origin_x: ui.input(|i| {
                                                    i.pointer
                                                        .press_origin()
                                                        .map(|p| p.x)
                                                        .unwrap_or(0.)
                                                }),
                                            });
                                        }
                                        if r.drag_stopped() {
                                            finished = true;
                                        }
                                    }
                                }
                                y += height;
                            }
                            if let Some(d) = &self.timeline_view.drag {
                                let base = starts(&self.project)[d.index]
                                    + if d.edge == Some(true) {
                                        self.project.segments[d.index].duration_ms
                                    } else {
                                        0
                                    };
                                let x = rect.left() + base as f32 / 1000. * pps + d.delta;
                                painter.line_segment(
                                    [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                                    Stroke::new(2., GOLD),
                                );
                                painter.text(
                                    Pos2::new(x + 5., rect.top() + 9.),
                                    Align2::LEFT_TOP,
                                    if d.edge.is_some() {
                                        "Trim liên kết • kéo vào"
                                    } else {
                                        "Di chuyển nhóm cảnh"
                                    },
                                    FontId::proportional(11.),
                                    GOLD,
                                );
                            }
                            let x = rect.left() + self.timeline_view.cursor as f32 / 1000. * pps;
                            painter.line_segment(
                                [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                                Stroke::new(1.5, Color32::from_rgb(255, 105, 105)),
                            );
                            painter.add(egui::Shape::convex_polygon(
                                vec![
                                    Pos2::new(x - 5., rect.top() + 17.),
                                    Pos2::new(x + 5., rect.top() + 17.),
                                    Pos2::new(x, rect.top() + 24.),
                                ],
                                Color32::from_rgb(255, 105, 105),
                                Stroke::NONE,
                            ));
                        });
                });
            });
        if let Some((track, lock)) = track_toggle {
            let state = &mut self.project.timeline.tracks[track];
            if lock {
                state.locked = !state.locked;
                self.dirty = true;
            } else {
                if track == CAPTIONS && !self.project.subtitles.enabled {
                    state.muted = false;
                    self.project.subtitles.enabled = true;
                } else {
                    state.muted = !state.muted;
                }
                if matches!(track,VOICE|ORIGINAL|SFX|MUSIC) {
                    self.dirty=true;self.project.rendered=None;
                    self.player.update_audio(&self.project);
                } else {self.touch();}
            }
        }
        if let Some(d) = drag_start {
            self.timeline_view.drag = Some(d);
        }
        if let Some(d) = &mut self.timeline_view.drag {
            if let Some(pos) = ui.input(|i| i.pointer.latest_pos()) {
                d.delta = pos.x - d.origin_x;
            }
        }
        if finished {
            ui.ctx().request_repaint();
            if let Some(d) = self.timeline_view.drag.take() {
                let before = self.project.clone();
                let delta = (d.delta / pps * 1000.).round() as i64;
                let start = starts(&self.project)[d.index];
                let length = self.project.segments[d.index].duration_ms;
                let layer=composition.clips.iter().find(|c|c.id==d.clip_id).cloned();
                let result=if let Some(layer)=layer.filter(|c|!c.linked) {
                    if !layer.editable(&self.project){Err(anyhow::anyhow!("Layer đang khóa"))}
                    else {
                        let (new_start,trim,duration)=match d.edge {
                            Some(false)=>{let cut=delta.max(0).min(layer.duration_ms.saturating_sub(100) as i64) as u64;(layer.start_ms+cut,cut,layer.duration_ms-cut)},
                            Some(true)=>(layer.start_ms,0,(layer.duration_ms as i64+delta).clamp(100,layer.duration_ms as i64) as u64),
                            None=>((layer.start_ms as i64+delta).max(0) as u64,0,layer.duration_ms),
                        };
                        self.project.time_layer(&layer.id,new_start,trim,duration)
                    }
                } else if let Some(end) = d.edge {
                    let raw = if end { length as i64 + delta } else { delta };
                    let absolute = (start as i64 + raw).max(0) as u64;
                    let snapped = if self.project.timeline.snapping {
                        snap(absolute, &self.project, (7000. / pps) as u64)
                    } else {
                        absolute
                    };
                    let local = snapped.saturating_sub(start);
                    trim(
                        &mut self.project,
                        d.index,
                        if end {
                            0
                        } else {
                            local.min(length.saturating_sub(100))
                        },
                        if end {
                            local.clamp(100, length)
                        } else {
                            length
                        },
                    )
                } else {
                    let target = (start as i64 + delta + length as i64 / 2).max(0) as u64;
                    let to = locate(&self.project, target)
                        .map(|(i, _)| i)
                        .unwrap_or(d.index);
                    move_scene(&mut self.project, d.index, to).map(|_| self.selected = to)
                };
                match result {
                    Ok(()) => {
                        self.timeline_view.history.record(before);
                        self.touch();
                        self.seek_timeline(self.timeline_view.cursor);
                    }
                    Err(e) => self.message(format!("{e:#}"), true),
                }
            }
        }
        if let Some((index, track,id)) = action {
            self.layers.canvas_focused=false;
            self.timeline_view.track = track;
            self.selected = index;
            self.layers.selected=Some(id.clone());
            let ms=self.project.composition_snapshot().clips.iter().find(|c|c.id==id).map(|c|c.start_ms).unwrap_or(0);
            self.seek_timeline(ms);
        }
        if let Some(ms) = seek {
            self.seek_timeline(ms);
        }
        if let Some((index, track, cmd,id)) = context_command {
            self.selected = index;
            self.timeline_view.track = track;
            self.layers.selected=Some(id);
            self.apply_timeline_edit(cmd);
        }
    }
}
