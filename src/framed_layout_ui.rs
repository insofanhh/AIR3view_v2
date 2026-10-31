use super::*;
use crate::framed_layout::{self, Framing};

fn percent(ui: &mut egui::Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, label: &str) -> egui::Response {
    ui.add(
        egui::Slider::new(value, range)
            .text(label)
            .custom_formatter(|v, _| format!("{:.1}%", v * 100.))
            .custom_parser(|s| {
                s.trim()
                    .trim_end_matches('%')
                    .parse::<f64>()
                    .ok()
                    .map(|v| v / 100.)
            }),
    )
}

fn crop_fields(ui: &mut egui::Ui, f: &mut Framing) -> [Rect; 4] {
    let mut bounds=[Rect::NOTHING;4];
    let columns=if ui.available_width()>=420. {2}else{1};
    ui.spacing_mut().slider_width=((ui.available_width()/columns as f32)-135.).clamp(55.,160.);
    egui::Grid::new(ui.id().with("crop_edges")).num_columns(columns).spacing(Vec2::new(16.,8.)).show(ui,|ui|{
        for (n,(i,label)) in [(0,"Trái"),(2,"Phải"),(1,"Trên"),(3,"Dưới")].into_iter().enumerate() {
            let other=if i<2 {i+2}else{i-2};
            let max=(0.8-f.trim[other]).max(0.);
            bounds[i]=percent(ui,&mut f.trim[i],0. ..=max,label).rect;
            if (n+1)%columns==0 {ui.end_row();}
        }
    });
    bounds
}
fn framing_position_fields(ui:&mut egui::Ui,f:&mut Framing) {
    ui.add(egui::Slider::new(&mut f.zoom, 1. ..=4.).text("Zoom trong khung"));
    percent(ui, &mut f.x, 0. ..=1., "Vùng nhìn X");
    percent(ui, &mut f.y, 0. ..=1., "Vùng nhìn Y");
    if ui.button("Khôi phục crop / căn giữa").clicked() {
        *f = Default::default();
    }
}
fn framing_fields(ui: &mut egui::Ui, f: &mut Framing) {
    crop_fields(ui,f);
    framing_position_fields(ui,f);
}
impl StudioApp {
    pub(super) fn framed_layout_settings(&mut self, ui: &mut egui::Ui) {
        let before = self.project.clone();
        let old = self.project.output.layout.clone();
        let mut upload = false;
        let focus = std::env::args().any(|a| a == "--capture-framed-layout");
        let header=egui::CollapsingHeader::new("Video trên · Ảnh nền dưới").id_salt("framed_layout").default_open(focus).show(ui,|ui|{
            ui.spacing_mut().slider_width=(ui.available_width()*0.4).clamp(55.,110.);
            let cfg=&mut self.project.output.layout;
            let _r=ui.checkbox(&mut cfg.enabled,"Bật bố cục video trên · ảnh nền dưới");#[cfg(test)]self.hitboxes.insert("framed_enabled".into(),_r.rect);
            ui.label(RichText::new("Ẩn avatar góc trong footage. Cảnh Host giữ nhân vật và dẫn chứng.").small().color(MUTED));
            ui.horizontal_wrapped(|ui|{
                let r=ui.button("Giống video mẫu");#[cfg(test)]self.hitboxes.insert("framed_sample".into(),r.rect);
                if r.clicked(){cfg.enabled=true;cfg.top=0.10;cfg.height=0.475;cfg.left=0.;cfg.width=1.;}
                if ui.button("Ưu tiên tình huống").clicked(){cfg.enabled=true;cfg.top=0.03;cfg.height=0.57;cfg.left=0.;cfg.width=1.;}
            });
            ui.add_enabled_ui(cfg.enabled,|ui|{
                settings_group(ui,"framed_background","1 / Ảnh nền phía dưới",true,|ui|{
                    let r=ui.button("Upload ảnh nền PNG / JPEG…");upload=r.clicked();#[cfg(test)]self.hitboxes.insert("framed_upload".into(),r.rect);
                    if cfg.background.as_os_str().is_empty(){ui.label(RichText::new("Chưa chọn ảnh · tạm dùng nền màu đen").small().color(MUTED));}
                    else{ui.add(egui::Label::new(cfg.background.file_name().unwrap_or_default().to_string_lossy()).truncate()).on_hover_text(cfg.background.display().to_string());if ui.button("Bỏ ảnh nền").clicked(){cfg.background.clear();}}
                    ui.horizontal_wrapped(|ui|{ui.selectable_value(&mut cfg.background_fit,"cover".into(),"Lấp đầy");ui.selectable_value(&mut cfg.background_fit,"contain".into(),"Toàn ảnh");});
                    if cfg.background_fit=="cover"{ui.add(egui::Slider::new(&mut cfg.background_zoom,1. ..=4.).text("Zoom ảnh nền"));}
                    percent(ui,&mut cfg.background_x,0. ..=1.,"Ảnh nền X");percent(ui,&mut cfg.background_y,0. ..=1.,"Ảnh nền Y");
                    percent(ui,&mut cfg.brightness,0. ..=1.,"Độ sáng ảnh");
                });
                settings_group(ui,"framed_viewport","2 / Khung video phía trên",false,|ui|{
                    let max_top=(0.96-cfg.height).max(0.);percent(ui,&mut cfg.top,0. ..=max_top,"Khoảng trên");
                    percent(ui,&mut cfg.height,0.15..=(0.96-cfg.top),"Chiều cao khung");
                    percent(ui,&mut cfg.width,0.2..=(1.-cfg.left),"Chiều rộng khung");
                    percent(ui,&mut cfg.left,0. ..=(1.-cfg.width).max(0.),"Lề trái");
                    ui.label(RichText::new("Trong preview: chọn layer Video để chỉnh khung hoặc kéo nội dung bên trong.").small().color(MUTED));
                });
                settings_group(ui,"framed_crop","3 / Crop mặc định / thông số camera",false,|ui|{
                    framing_fields(ui,&mut cfg.framing);
                    ui.label(RichText::new("Crop có thể cắt mất hành động ở mép. Dùng phần Layer để chỉnh riêng nguồn hoặc cảnh.").small().color(MUTED));
                });
                settings_group(ui,"framed_labels","4 / Phụ đề & nhãn",false,|ui|{
                    ui.checkbox(&mut cfg.captions_in_frame,"Neo phụ đề theo khung video");
                    ui.checkbox(&mut cfg.title_enabled,"Hiện thanh tiêu đề tại ranh giới");
                    ui.add_enabled_ui(cfg.title_enabled,|ui|{ui.label("Tiêu đề · để trống dùng tên dự án");ui.add(egui::TextEdit::multiline(&mut cfg.title).char_limit(300).desired_rows(2).desired_width(f32::INFINITY));});
                    ui.label("Nhãn phần · để trống để ẩn");ui.add(egui::TextEdit::singleline(&mut cfg.part).char_limit(40).desired_width(f32::INFINITY));
                });
            });
        });
        #[cfg(test)]
        self.hitboxes
            .insert("framed_header".into(), header.header_response.rect);
        if focus && self.capture_path.is_some() && self.frames == 3 {
            ui.scroll_to_rect(header.header_response.rect, Some(egui::Align::Min));
        }
        if old != self.project.output.layout {
            if self.project.output.layout.enabled {
                self.project.output.blur_background = false;
            }
            let reset = old.enabled != self.project.output.layout.enabled
                || old.viewport() != self.project.output.layout.viewport();
            framed_layout::refresh(&mut self.project, reset);
            self.timeline_view.history.record(before);
            self.layers.managed_edit = true;
            self.media_settings_changed();
        }
        if upload {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Ảnh nền", &["png", "jpg", "jpeg"])
                .pick_file()
            {
                self.launch(Task::BackgroundImport(path));
            }
        }
    }
    pub(super) fn footage_framing_ui(
        &mut self,
        ui: &mut egui::Ui,
        c: &crate::composition::LayerClip,
    ) {
        if c.track != crate::timeline::VIDEO {
            return;
        }
        let Some(seg) = self.project.segments.iter().find(|s| s.id == c.scene_id) else {
            return;
        };
        let source = seg.source_id.clone();
        if framed_layout::footage(&self.project,c) {ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.layers.content_edit, false, "Chỉnh khung");
            ui.selectable_value(&mut self.layers.content_edit, true, "Kéo nội dung");
        });}
        let header=egui::CollapsingHeader::new("Crop video · 4 cạnh")
            .id_salt("video_four_edge_crop")
            .default_open(true)
            .show(ui, |ui| {
                let mut reset_crop=false;
                ui.horizontal_wrapped(|ui| {
                    for (value,label) in [(2,"Riêng cảnh"),(1,"Cùng nguồn"),(0,"Toàn dự án")] {
                        let _r=ui.selectable_value(&mut self.layers.framing_scope,value,label);
                        #[cfg(test)] self.hitboxes.insert(format!("video_crop_scope_{value}"),_r.rect);
                    }
                    let _reset=ui.add_enabled(c.editable(&self.project)&&!self.busy,egui::Button::new("Đặt lại"))
                        .on_hover_text("Đưa cả 4 cạnh về 0%; giữ nguyên zoom và vùng nhìn.");
                    reset_crop=_reset.clicked();
                    #[cfg(test)] self.hitboxes.insert("video_crop_reset".into(),_reset.rect);
                });
                let before = self.project.clone();
                let cfg = &self.project.output.layout;
                let inactive=!cfg.enabled&&!cfg.crop_unframed;
                let old = if inactive {Framing::default()}else{match self.layers.framing_scope {
                    0 => &cfg.framing,
                    1 => cfg.sources.get(&source).unwrap_or(&cfg.framing),
                    _ => framed_layout::framing(&self.project, c),
                }
                .clone()};
                let mut f = old.clone();
                if reset_crop {f.trim=[0.;4];}
                if !inactive&&((self.layers.framing_scope < 2 && cfg.scenes.contains_key(&c.scene_id))
                    || (self.layers.framing_scope==0&&cfg.sources.contains_key(&source))) {
                    ui.label(
                        RichText::new(
                            "Cảnh / nguồn này có crop riêng ưu tiên hơn. Chọn phạm vi tương ứng để chỉnh hoặc bỏ ghi đè.",
                        )
                        .small()
                        .color(MUTED),
                    );
                }
                ui.add_enabled_ui(c.editable(&self.project) && !self.busy, |ui| {
                    let _rects=crop_fields(ui,&mut f);
                    #[cfg(test)] for (i,rect) in _rects.into_iter().enumerate(){self.hitboxes.insert(format!("video_crop_edge_{i}"),rect);}
                    egui::CollapsingHeader::new("Zoom & vùng nhìn").id_salt("crop_position").show(ui,|ui|framing_position_fields(ui,&mut f));
                });
                if f != old {
                    self.write_framing(c, &source, f);
                    self.timeline_view.history.record(before);
                }
                let has_override = match self.layers.framing_scope {
                    1=>self.project.output.layout.sources.contains_key(&source),
                    2=>self.project.output.layout.scenes.contains_key(&c.scene_id),
                    _=>false,
                };
                if has_override
                    && c.editable(&self.project)
                    && ui.button("Bỏ ghi đè cảnh / nguồn này").clicked()
                {
                    let before = self.project.clone();
                    if self.layers.framing_scope == 1 {
                        self.project.output.layout.sources.remove(&source);
                    } else {
                        self.project.output.layout.scenes.remove(&c.scene_id);
                    }
                    self.timeline_view.history.record(before);
                    self.media_settings_changed();
                }
            });
        header.header_response.clone().on_hover_text("Cắt 4 cạnh theo % kích thước nguồn. Hình giữ tỉ lệ và lấp đầy khung; không đổi âm thanh hoặc thời lượng.");
        #[cfg(test)] self.hitboxes.insert("video_crop_header".into(),header.header_response.rect);
        if self.capture_path.is_some()&&self.frames==3&&std::env::args().any(|a|a=="--capture-video-crop") {
            ui.scroll_to_rect(header.header_response.rect,Some(egui::Align::Min));
        }
    }
    pub(super) fn write_framing(
        &mut self,
        c: &crate::composition::LayerClip,
        _source: &str,
        f: Framing,
    ) {
        if !framed_layout::set_framing(&mut self.project,c,self.layers.framing_scope,f) {
            self.message("Không đổi crop: cảnh hoặc layer cùng cảnh đang khóa.",true);
            return;
        }
        self.layers.managed_edit = true;
        self.media_settings_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn four_edge_controls_fit_columns_edit_reset_and_undo_in_both_layouts() {
        for width in [240.,308.,560.,900.] {
            for framed in [false,true] {
                let temp=tempfile::tempdir().unwrap();let ctx=egui::Context::default();ctx.style_mut(|s|s.animation_time=0.);
                let mut app=StudioApp::from_context(&ctx);app.player.stop();app.capture_path=None;
                app.project=Project{root:temp.path().into(),sources:vec![Source{id:"source".into(),width:320,height:180,..Default::default()}],segments:vec![Segment{id:"scene".into(),source_id:"source".into(),kind:"source".into(),duration_ms:2000,out_ms:2000,..Default::default()}],..Default::default()};
                app.project.output.layout.enabled=framed;app.project.ensure_composition();
                let clip=app.project.composition.as_ref().unwrap().clips.iter().find(|c|c.track==crate::timeline::VIDEO).unwrap().clone();
                let screen=Rect::from_min_size(Pos2::ZERO,Vec2::new(width,1000.));
                let draw=|app:&mut StudioApp,events|{let _=ctx.run(egui::RawInput{screen_rect:Some(screen),events,..Default::default()},|ctx|{egui::CentralPanel::default().show(ctx,|ui|app.footage_framing_ui(ui,&clip));});};
                let click=|app:&mut StudioApp,pos|{
                    draw(app,vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:true,modifiers:egui::Modifiers::NONE}]);
                    draw(app,vec![egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:false,modifiers:egui::Modifiers::NONE}]);draw(app,vec![]);
                };
                draw(&mut app,vec![]);draw(&mut app,vec![]);
                for i in 0..4{assert!(screen.contains_rect(app.hitboxes[&format!("video_crop_edge_{i}")]),"edge {i} outside {width}px");}
                assert_eq!(app.layers.framing_scope,2);
                let slider=app.hitboxes["video_crop_edge_0"];click(&mut app,Pos2::new(slider.left()+35.,slider.center().y));
                assert!(framed_layout::framing(&app.project,&clip).trim[0]>0.,"slider must edit actual scene crop at {width}px, framed={framed}");
                assert_eq!(app.project.duration_ms(),2000);assert!(app.project.rendered.is_none());
                assert!(!temp.path().join("preview.mp4").exists());
                let reset=app.hitboxes["video_crop_reset"].center();click(&mut app,reset);
                assert_eq!(framed_layout::framing(&app.project,&clip).trim,[0.;4]);
                assert!(app.timeline_view.history.undo(&mut app.project));assert!(framed_layout::framing(&app.project,&clip).trim[0]>0.);
                assert!(app.timeline_view.history.redo(&mut app.project));assert_eq!(framed_layout::framing(&app.project,&clip).trim,[0.;4]);app.player.stop();
            }
        }
    }
    #[test]
    fn resizing_a_viewport_refreshes_decoders_without_creating_preview_mp4() {
        let temp=tempfile::tempdir().unwrap();let ctx=egui::Context::default();
        let mut app=StudioApp::from_context(&ctx);app.player.stop();app.capture_path=None;
        app.project=Project{root:temp.path().into(),sources:vec![Source{id:"source".into(),width:320,height:180,..Default::default()}],segments:vec![Segment{id:"scene".into(),source_id:"source".into(),kind:"source".into(),duration_ms:2000,out_ms:2000,..Default::default()}],..Default::default()};
        app.project.output.layout.enabled=true;app.project.ensure_composition();
        let v=app.project.composition.as_ref().unwrap().clips.iter().find(|c|c.track==crate::timeline::VIDEO).unwrap().clone();
        let generation=app.player.generation;let mut transform=v.transform.clone();transform.width=0.7;
        app.set_layer_transform(&v.id,transform);
        assert!(app.player.generation>generation&&app.layers.live&&app.project.rendered.is_none());
        assert!(!temp.path().join("preview.mp4").exists());app.player.stop();
    }
    #[test]
    fn layout_controls_fit_columns_and_preset_is_undoable() {
        for width in [308., 448., 560.] {
            let temp = tempfile::tempdir().unwrap();
            let ctx = egui::Context::default();
            ctx.style_mut(|s| s.animation_time = 0.);
            let mut app = StudioApp::from_context(&ctx);
            app.capture_path = None;
            app.player.stop();
            app.project = Project {
                root: temp.path().into(),
                ..Default::default()
            };
            let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 1200.));
            let draw = |app: &mut StudioApp, events| {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default()
                            .show(ctx, |ui| app.framed_layout_settings(ui));
                    },
                );
            };
            let click = |pos| {
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]
            };
            draw(&mut app, vec![]);
            let header = app.hitboxes["framed_header"];
            assert!(screen.contains_rect(header));
            draw(&mut app, click(header.center()));
            draw(&mut app, vec![]);
            for key in ["framed_enabled", "framed_sample", "framed_upload"] {
                assert!(
                    screen.contains_rect(app.hitboxes[key]),
                    "{key} outside {width}px: {:?}",
                    app.hitboxes[key]
                );
            }
            let pos = app.hitboxes["framed_sample"].center();
            draw(&mut app, click(pos));
            assert!(app.project.output.layout.enabled && app.dirty);
            assert!(!app.project.output.blur_background);
            assert!(app.timeline_view.history.undo(&mut app.project));
            assert!(!app.project.output.layout.enabled);
            assert!(app.timeline_view.history.redo(&mut app.project));
            assert!(app.project.output.layout.enabled);
        }
    }
}
