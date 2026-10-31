use super::*;
use crate::{
    composition::LayerClip,
    source_masks::{self, Rect as SourceRect, Region},
};

#[derive(Default)]
pub(super) struct MaskView {
    pub selected: String,
    pub edit: bool,
    pub draw: bool,
    pub before: bool,
    drag: Option<(Project, SourceRect, [f32; 2], bool)>,
}
impl StudioApp {
    pub(super) fn mask_preview(&self) -> Project {
        let mut p = self.project.clone();
        if self.masks.before {
            p.source_masks.enabled = false;
            p.ensure_composition();
        }
        p
    }
    pub(super) fn source_masks_settings_ui(&mut self, ui: &mut egui::Ui) {
        let focus = std::env::args().any(|v| v == "--capture-source-masks");
        let response=egui::CollapsingHeader::new("Che chữ / logo nguồn · không dùng AI").id_salt("source_masks_settings").default_open(focus).show(ui,|ui|{
            let before=self.project.clone();let previous=self.project.source_masks.clone();
            let cfg=&mut self.project.source_masks;
            let _r=ui.add_enabled(!self.busy,egui::Checkbox::new(&mut cfg.enabled,"Bật lớp blur che chữ / logo nguồn"));#[cfg(test)]self.hitboxes.insert("masks_enabled".into(),_r.rect);
            ui.add_enabled_ui(cfg.enabled&&!self.busy,|ui|{
                ui.checkbox(&mut cfg.auto_scan,"Tự dò khi tạo video / xuất MP4");
                ui.horizontal_wrapped(|ui|{ui.checkbox(&mut cfg.subtitles,"Phụ đề nguồn");ui.checkbox(&mut cfg.logos,"Logo / watermark");ui.checkbox(&mut cfg.camera,"Thông số camera");});
                egui::ComboBox::from_id_salt("mask_sensitivity").selected_text(match cfg.sensitivity.as_str(){"careful"=>"Thận trọng","wide"=>"Quét rộng",_=>"Cân bằng"}).show_ui(ui,|ui|{
                    for (id,label) in [("careful","Thận trọng"),("balanced","Cân bằng"),("wide","Quét rộng")]{ui.selectable_value(&mut cfg.sensitivity,id.into(),label);}
                });
                ui.spacing_mut().slider_width=(ui.available_width()-150.).clamp(45.,130.);
                ui.add(egui::Slider::new(&mut cfg.strength,2. ..=60.).text("Độ blur"));
                let mut percent=cfg.padding*100.;ui.add(egui::Slider::new(&mut percent,0. ..=5.).text("Viền che").suffix(" %"));cfg.padding=percent/100.;
                ui.checkbox(&mut cfg.evidence,"Áp dụng cho ảnh dẫn chứng từ nguồn");
            });
            let changed=previous!=self.project.source_masks;
            if changed {self.timeline_view.history.record(before);self.media_settings_changed();}
            ui.label(RichText::new("Dò hình dạng chữ và vùng cố định; vùng chưa chắc chắn chỉ được đề xuất. Kiểm tra trên preview để tránh che nhầm tình huống.").small().color(MUTED));
            if !self.project.source_masks.report.is_empty(){ui.label(RichText::new(&self.project.source_masks.report).small().color(TEAL));}
            if self.project.source_masks.enabled&&!self.project.source_masks.scan_complete&&!self.project.source_masks.scan_key.is_empty(){ui.colored_label(Color32::YELLOW,"Quét chưa đầy đủ. Quét lại nguồn để dò tiếp các khoảng còn thiếu; ảnh mẫu đã có được dùng lại.");}
            ui.horizontal_wrapped(|ui|{
                let ready=self.project.source_masks.enabled&&!self.project.segments.is_empty()&&!self.busy;
                let r=ui.add_enabled(ready,egui::Button::new("Quét lại nguồn"));#[cfg(test)]self.hitboxes.insert("masks_scan".into(),r.rect);
                if r.clicked(){self.launch(Task::MaskScan(true));}
                if ui.add_enabled(!self.project.segments.is_empty()&&!self.busy,egui::Button::new("Áp dụng / cập nhật ảnh")).clicked(){self.launch(Task::MaskScan(false));}
                if ui.add_enabled(!self.busy&&self.project.source_masks.templates.len()<16,egui::Button::new("Nhập mẫu logo…")).clicked(){if let Some(path)=rfd::FileDialog::new().add_filter("Ảnh logo",&["png","jpg","jpeg"]).pick_file(){self.launch(Task::MaskTemplate(path));}}
            });
            let mut remove=None;
            for (i,t) in self.project.source_masks.templates.iter().enumerate(){ui.horizontal_wrapped(|ui|{
                ui.label(format!("Mẫu {} · {}",i+1,t.name));
                if !t.path.is_file(){ui.colored_label(Color32::YELLOW,"Thiếu ảnh mẫu");}
                if ui.add_enabled(!self.busy,egui::Button::new("Bỏ mẫu")).clicked(){remove=Some(i);}
            });}
            if let Some(i)=remove{self.timeline_view.history.record(self.project.clone());self.project.source_masks.templates.remove(i);self.project.source_masks.scan_key.clear();self.dirty=true;}
            ui.label(RichText::new("Nhớ tùy chọn và mẫu logo cho dự án sau. Vị trí / thời gian vùng che được dò riêng theo từng nguồn.").small().color(MUTED));
        });
        #[cfg(test)]
        self.hitboxes
            .insert("masks_header".into(), response.header_response.rect);
        if focus && self.capture_path.is_some() && self.frames == 3 {
            ui.scroll_to_rect(response.header_response.rect, Some(egui::Align::Min));
        }
    }
    pub(super) fn source_masks_layer_ui(&mut self, ui: &mut egui::Ui, c: &LayerClip) {
        if c.track != crate::timeline::VIDEO {
            return;
        }
        let Some(scene) = self.project.segments.iter().find(|s| s.id == c.scene_id) else {
            return;
        };
        let source = scene.source_id.clone();
        let source_time = c.source_at(self.timeline_view.cursor.saturating_sub(c.start_ms));
        let locked = !c.editable(&self.project) || self.busy;
        egui::CollapsingHeader::new("Vùng che chữ / logo nguồn").id_salt("source_mask_layers").default_open(std::env::args().any(|v|v=="--capture-source-masks")).show(ui,|ui|{
            let before=self.project.clone();let previous=self.project.source_masks.clone();
            ui.add_enabled_ui(!locked,|ui|{
                let _r=ui.checkbox(&mut self.project.source_masks.enabled,"Bật lớp che nguồn");#[cfg(test)]self.hitboxes.insert("mask_layer_enabled".into(),_r.rect);
                ui.horizontal_wrapped(|ui|{
                    if ui.selectable_label(self.masks.edit,"Chỉnh vùng trên preview").clicked(){self.masks.edit = !self.masks.edit;self.masks.before=false;}
                    if ui.selectable_label(self.masks.before,"Xem trước khi che").clicked(){self.masks.before = !self.masks.before;self.request_frame();}
                });
                ui.horizontal_wrapped(|ui|{
                    let r=ui.add_enabled(self.project.source_masks.regions.len()<512,egui::Button::new("Vẽ vùng mới"));#[cfg(test)]self.hitboxes.insert("mask_draw".into(),r.rect);
                    if r.clicked(){self.project.source_masks.enabled=true;self.masks.edit=true;self.masks.draw=true;self.masks.before=false;}
                    if ui.button("Thêm vùng dưới").clicked()&&self.project.source_masks.regions.len()<512 {
                        let r=Region{source_id:source.clone(),start_ms:c.media_in_ms,end_ms:c.source_at(c.duration_ms),..Default::default()};self.masks.selected=r.id.clone();self.project.source_masks.regions.push(r);self.project.source_masks.enabled=true;self.masks.edit=true;
                    }
                    if ui.button("Quét nguồn").clicked(){self.project.source_masks.enabled=true;self.launch(Task::MaskScan(true));}
                });
                ui.label(RichText::new(format!("Mốc nguồn: {:.2}s · vùng che dùng chung các cảnh từ nguồn này",source_time as f64/1000.)).small().color(MUTED));
                if self.project.source_masks.enabled {
                    let count=self.project.source_masks.regions.iter().filter(|r|r.source_id==source&&source_time>=r.start_ms&&source_time<r.end_ms&&source_masks::region_active(&self.project.source_masks,r)).count();
                    ui.label(RichText::new(if count>0{format!("{count} vùng che được bật tại mốc này")}else{"Chưa có vùng che được bật tại mốc này. Quét nguồn, bật đề xuất hoặc vẽ vùng mới nếu còn chữ / logo.".into()}).small().color(if count>0{TEAL}else{Color32::YELLOW}));
                }
                if self.masks.edit{ui.label(RichText::new(if self.masks.draw{"Kéo một hình chữ nhật trong video để vẽ vùng che."}else{"Kéo để di chuyển; kéo góc dưới phải để đổi kích thước. Crop, lật và zoom được tính theo nguồn."}).small().color(TEAL));}
                let entries:Vec<_>=self.project.source_masks.regions.iter().filter(|r|r.source_id==source).cloned().collect();
                egui::ScrollArea::vertical().id_salt("mask_regions").max_height(150.).show(ui,|ui|{
                    for r in &entries {ui.horizontal_wrapped(|ui|{
                        let mut on=r.enabled;if ui.checkbox(&mut on,"").changed(){if let Some(v)=self.project.source_masks.regions.iter_mut().find(|v|v.id==r.id){v.enabled=on;v.manual=true;}}
                        let kind=match r.kind.as_str(){"subtitle"=>"Phụ đề","camera"=>"Camera","logo"=>"Logo",_=>"Thủ công"};
                        let label=format!("{kind} · {:.1}–{:.1}s{}",r.start_ms as f64/1000.,r.end_ms as f64/1000.,if !r.enabled{" · đề xuất / tắt"}else{""});
                        if ui.selectable_label(self.masks.selected==r.id,label).on_hover_text(format!("Điểm thuật toán: {:.0}% · cần kiểm tra bằng hình ảnh",r.score*100.)).clicked(){self.masks.selected=r.id.clone();let local=((r.start_ms.saturating_sub(c.media_in_ms)) as f64/c.playback_rate) as u64;self.timeline_view.cursor=c.start_ms+local.min(c.duration_ms.saturating_sub(1));self.request_frame();}
                    });}
                });
                if let Some(index)=self.project.source_masks.regions.iter().position(|r|r.id==self.masks.selected&&r.source_id==source) {
                    let mut r=self.project.source_masks.regions[index].clone();let old=r.clone();
                    ui.spacing_mut().slider_width=70.;
                    egui::Grid::new("mask_geometry").num_columns(2).show(ui,|ui|{
                        for (label,value) in [("X",&mut r.rect.x),("Y",&mut r.rect.y),("Rộng",&mut r.rect.w),("Cao",&mut r.rect.h)]{ui.label(label);ui.add(egui::DragValue::new(value).speed(0.002).range(0. ..=1.));ui.end_row();}
                    });
                    ui.horizontal_wrapped(|ui|{ui.label("Từ / đến nguồn");ui.add(egui::DragValue::new(&mut r.start_ms).range(0..=86_399_999).speed(100.).suffix(" ms"));ui.add(egui::DragValue::new(&mut r.end_ms).range(1..=86_400_000).speed(100.).suffix(" ms"));});
                    if r!=old {r.rect.x=r.rect.x.clamp(0.,0.998);r.rect.y=r.rect.y.clamp(0.,0.998);r.rect.w=r.rect.w.clamp(0.002,1.-r.rect.x);r.rect.h=r.rect.h.clamp(0.002,1.-r.rect.y);r.end_ms=r.end_ms.max(r.start_ms+1).min(86_400_000);r.manual=true;self.project.source_masks.regions[index]=r.clone();}
                    ui.horizontal_wrapped(|ui|{
                        if ui.button("Bỏ qua / xóa vùng").clicked(){if self.project.source_masks.ignored.len()<512{self.project.source_masks.ignored.push(source_masks::Ignored{source_id:source.clone(),rect:r.rect});}self.project.source_masks.regions.remove(index);self.masks.selected.clear();}
                        if ui.add_enabled(self.project.source_masks.templates.len()<16,egui::Button::new("Lưu làm mẫu logo")).clicked(){self.launch(Task::MaskTemplateRegion(r.id.clone()));}
                    });
                }
            });
            if previous!=self.project.source_masks {self.timeline_view.history.record(before);self.media_settings_changed();}
        });
    }
    /// Source-space editing is an overlay tool; it does not add a synthetic
    /// timeline track or change source/voice clocks.
    pub(super) fn source_mask_canvas(
        &mut self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        canvas: Rect,
        response: &egui::Response,
        clips: &[LayerClip],
    ) -> bool {
        if !self.masks.edit {
            return false;
        }
        let Some(c) = clips
            .iter()
            .find(|c| {
                c.track == crate::timeline::VIDEO
                    && Some(&c.id) == self.layers.selected.as_ref()
                    && c.active(self.timeline_view.cursor)
            })
            .cloned()
        else {
            return false;
        };
        let Some(scene) = self.project.segments.iter().find(|s| s.id == c.scene_id) else {
            return false;
        };
        let source = scene.source_id.clone();
        let time = c.source_at(self.timeline_view.cursor.saturating_sub(c.start_ms));
        let t = crate::composition::evaluated(&self.project, &c, self.timeline_view.cursor);
        let center = canvas.min + Vec2::new(t.x * canvas.width(), t.y * canvas.height());
        let (sin, cos) = t.rotation.to_radians().sin_cos();
        let to_screen = |point: [f32; 2]| {
            let q = source_masks::source_to_layer(&self.project, &c, point, false);
            let x = (q[0] - 0.5) * t.width * canvas.width();
            let y = (q[1] - 0.5) * t.height * canvas.height();
            center + Vec2::new(x * cos - y * sin, x * sin + y * cos)
        };
        let mut hits = vec![];
        for r in self
            .project
            .source_masks
            .regions
            .iter()
            .filter(|r| r.source_id == source && time >= r.start_ms && time < r.end_ms)
        {
            let b = r.rect;
            let pts = [
                [b.x, b.y],
                [b.x + b.w, b.y],
                [b.x + b.w, b.y + b.h],
                [b.x, b.y + b.h],
            ]
            .map(to_screen);
            let color = if r.id == self.masks.selected {
                TEAL
            } else {
                Color32::YELLOW
            };
            let p = painter.with_clip_rect(canvas);
            for i in 0..4 {
                p.line_segment([pts[i], pts[(i + 1) % 4]], Stroke::new(1.5, color));
            }
            p.rect_filled(Rect::from_center_size(pts[2], Vec2::splat(9.)), 1., color);
            hits.push((r.id.clone(), b, pts[2]));
        }
        if self.busy || !c.editable(&self.project) {
            return true;
        }
        if let Some(pos) = response.interact_pointer_pos() {
            let d = pos - center;
            let x = d.x * cos + d.y * sin;
            let y = -d.x * sin + d.y * cos;
            let q = source_masks::source_to_layer(
                &self.project,
                &c,
                [
                    x / (t.width * canvas.width()) + 0.5,
                    y / (t.height * canvas.height()) + 0.5,
                ],
                true,
            );
            let inside = canvas.contains(pos)
                && q.iter().all(|v| (0. ..=1.).contains(v))
                && x.abs() <= t.width * canvas.width() / 2.
                && y.abs() <= t.height * canvas.height() / 2.;
            let point = [q[0].clamp(0., 1.), q[1].clamp(0., 1.)];
            if response.drag_started() || response.clicked() {
                let press=ui.input(|i|i.pointer.press_origin()).unwrap_or(pos);
                let d=press-center;let px=d.x*cos+d.y*sin;let py=-d.x*sin+d.y*cos;
                let raw=source_masks::source_to_layer(&self.project,&c,[px/(t.width*canvas.width())+0.5,py/(t.height*canvas.height())+0.5],true);
                let origin=[raw[0].clamp(0.,1.),raw[1].clamp(0.,1.)];
                if self.masks.draw
                    && inside
                    && canvas.contains(press)&&raw.iter().all(|v|(0. ..=1.).contains(v))
                    && response.drag_started()
                    && self.project.source_masks.regions.len() < 512
                {
                    let b = SourceRect {
                        x: origin[0].min(0.998),
                        y: origin[1].min(0.998),
                        w: 0.002,
                        h: 0.002,
                    };
                    let old = self.project.clone();
                    let r = Region {
                        source_id: source,
                        start_ms: c.media_in_ms,
                        end_ms: c.source_at(c.duration_ms),
                        rect: b,
                        ..Default::default()
                    };
                    self.masks.selected = r.id.clone();
                    self.project.source_masks.regions.push(r);
                    self.masks.drag = Some((old, b, origin, true));
                } else if let Some((id, b, corner)) = hits.iter().rev().find(|(_, b, corner)| {
                    press.distance(*corner) < 14.
                        || (origin[0] >= b.x
                            && origin[0] <= b.x + b.w
                            && origin[1] >= b.y
                            && origin[1] <= b.y + b.h)
                }) {
                    self.masks.selected = id.clone();
                    if response.drag_started() {
                        self.masks.drag =
                            Some((self.project.clone(), *b, origin, press.distance(*corner) < 14.));
                    }
                }
            }
            if response.dragged() {
                if let Some((_, b, origin, resize)) = &self.masks.drag {
                    let mut rect = *b;
                    let dx = point[0] - origin[0];
                    let dy = point[1] - origin[1];
                    if self.masks.draw {
                        rect.x = origin[0].min(point[0]).min(0.998);
                        rect.y = origin[1].min(point[1]).min(0.998);
                        rect.w = dx.abs().clamp(0.002, 1. - rect.x);
                        rect.h = dy.abs().clamp(0.002, 1. - rect.y);
                    } else if *resize {
                        rect.w = (b.w + dx).clamp(0.002, 1. - b.x);
                        rect.h = (b.h + dy).clamp(0.002, 1. - b.y);
                    } else {
                        rect.x = (b.x + dx).clamp(0., 1. - b.w);
                        rect.y = (b.y + dy).clamp(0., 1. - b.h);
                    }
                    if let Some(r) = self
                        .project
                        .source_masks
                        .regions
                        .iter_mut()
                        .find(|r| r.id == self.masks.selected)
                    {
                        r.rect = rect;
                        r.manual = true;
                    }
                    self.dirty = true;
                    self.project.rendered = None;
                }
            }
        }
        if response.drag_stopped() {
            if let Some((old, _, _, _)) = self.masks.drag.take() {
                self.timeline_view.history.record(old);
                self.masks.draw = false;
                self.media_settings_changed();
            }
        }
        let _ = ui;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mask_controls_fit_narrow_columns_and_enable_with_one_click() {
        for width in [280., 420., 1080.] {
            let ctx = egui::Context::default();
            ctx.style_mut(|s| s.animation_time = 0.);
            let mut app = StudioApp::from_context(&ctx);
            app.capture_path = None;
            app.project = Project::default();
            app.project.segments.push(Segment::default());
            let draw = |app: &mut StudioApp, events| {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 900.))),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default()
                            .show(ctx, |ui| app.source_masks_settings_ui(ui));
                    },
                );
            };
            for _ in 0..3 {
                draw(&mut app, vec![]);
            }
            for key in ["masks_header", "masks_enabled"] {
                let pos = app.hitboxes[key].center();
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
                draw(&mut app, vec![]);
            }
            assert!(app.project.source_masks.enabled);
            let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 900.));
            for key in ["masks_header", "masks_enabled", "masks_scan"] {
                assert!(
                    screen.contains_rect(app.hitboxes[key]),
                    "{width} {key}: {:?}",
                    app.hitboxes[key]
                );
            }
        }
    }
    #[test]
    fn mask_canvas_draw_move_resize_and_undo_keep_video_transform() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = StudioApp::from_context(&ctx);
        app.capture_path = None;
        app.project = Project {
            root: temp.path().into(),
            sources: vec![Source {
                id: "s".into(),
                width: 640,
                height: 360,
                duration_ms: 4000,
                ..Default::default()
            }],
            segments: vec![Segment {
                id: "scene".into(),
                source_id: "s".into(),
                kind: "source".into(),
                duration_ms: 4000,
                out_ms: 4000,
                ..Default::default()
            }],
            ..Default::default()
        };
        app.project.output.portrait = false;
        app.project.ensure_composition();
        let c = app
            .project
            .composition
            .as_ref()
            .unwrap()
            .clips
            .iter()
            .find(|c| c.track == crate::timeline::VIDEO)
            .unwrap()
            .clone();
        app.layers.selected = Some(c.id.clone());
        app.masks.edit = true;
        app.masks.draw = true;
        app.project.source_masks.enabled = true;
        let canvas = Rect::from_min_max(Pos2::new(20., 20.), Pos2::new(660., 380.));
        let draw = |app: &mut StudioApp, events| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(700., 430.))),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let response = ui.interact(
                            canvas,
                            egui::Id::new("mask-qa-canvas"),
                            egui::Sense::click_and_drag(),
                        );
                        let clips = app.project.composition.as_ref().unwrap().clips.clone();
                        app.source_mask_canvas(ui, ui.painter(), canvas, &response, &clips);
                    });
                },
            );
        };
        let drag = |app: &mut StudioApp, a: Pos2, b: Pos2| {
            draw(
                app,
                vec![
                    egui::Event::PointerMoved(a),
                    egui::Event::PointerButton {
                        pos: a,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            for i in 1..=8 {
                draw(
                    app,
                    vec![egui::Event::PointerMoved(a + (b - a) * i as f32 / 8.)],
                );
            }
            draw(
                app,
                vec![egui::Event::PointerButton {
                    pos: b,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        };
        draw(&mut app, vec![]);
        drag(&mut app, Pos2::new(500., 300.), Pos2::new(220., 200.));
        assert_eq!(app.project.source_masks.regions.len(), 1);
        let initial = app.project.source_masks.regions[0].rect;
        assert!(
            initial.valid() && initial.w > 0.3 && initial.h > 0.2,
            "{initial:?}"
        );
        let center = Pos2::new(
            20. + (initial.x + initial.w / 2.) * 640.,
            20. + (initial.y + initial.h / 2.) * 360.,
        );
        drag(&mut app, center, center + Vec2::new(30., -20.));
        let moved = app.project.source_masks.regions[0].rect;
        assert!(moved.x > initial.x && moved.y < initial.y);
        let corner = Pos2::new(
            20. + (moved.x + moved.w) * 640.,
            20. + (moved.y + moved.h) * 360.,
        );
        drag(&mut app, corner, corner + Vec2::new(30., 20.));
        assert!(app.project.source_masks.regions[0].rect.w > moved.w);
        assert!(app.timeline_view.history.undo(&mut app.project));
        assert_eq!(app.project.source_masks.regions[0].rect, moved);
        assert_eq!(
            app.project
                .composition
                .as_ref()
                .unwrap()
                .clips
                .iter()
                .find(|v| v.id == c.id)
                .unwrap()
                .transform,
            c.transform
        );
        app.project
            .composition
            .as_mut()
            .unwrap()
            .clips
            .iter_mut()
            .find(|v| v.id == c.id)
            .unwrap()
            .locked = true;
        drag(&mut app, center, center + Vec2::new(40., 40.));
        assert_eq!(app.project.source_masks.regions[0].rect, moved);
        app.masks.before = true;
        assert!(!app.mask_preview().source_masks.enabled);
        assert!(app.project.source_masks.enabled);
        app.player.stop();
    }
}
