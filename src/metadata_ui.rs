use super::*;
use crate::export_metadata::{self, Preset, Scope, Tag};

impl StudioApp {
    pub(super) fn metadata_window(&mut self,ctx:&egui::Context) {
        if !self.metadata_open {return;}
        let mut open=true;let mut save=false;let mut apply=false;let mut inspect=false;
        let before=serde_json::to_vec(&self.project.export_metadata).ok();
        let _window=egui::Window::new("Metadata video / Xuất MP4")
            .id(egui::Id::new("export_metadata_window"))
            .anchor(Align2::CENTER_CENTER,Vec2::ZERO).default_width(660.)
            .collapsible(false).resizable(false).open(&mut open).show(ctx,|ui| {
                ui.set_max_width(660.);ui.spacing_mut().interact_size.y=32.;
                egui::ScrollArea::vertical().id_salt("metadata_settings_scroll")
                    .max_height((ctx.screen_rect().height()-220.).clamp(260.,650.)).show(ui,|ui| {
                    ui.add_enabled_ui(!self.busy,|ui| {
                        let m=&mut self.project.export_metadata;
                        let enabled=ui.checkbox(&mut m.enabled,"Ghi metadata khi xuất MP4");
                        #[cfg(test)] self.hitboxes.insert("metadata_enabled".into(),enabled.rect);
                        #[cfg(not(test))] let _=enabled;
                        ui.add(egui::Label::new(RichText::new("Lựa chọn được nhớ cho dự án sau. Chỉ đổi metadata không cần render lại cảnh.").size(12.).color(MUTED)).wrap());
                        ui.add_enabled_ui(m.enabled,|ui| {
                            ui.label("Preset");
                            let preset=egui::ComboBox::from_id_salt("metadata_preset").width((ui.available_width()-12.).min(610.))
                                .selected_text(m.preset.label()).show_ui(ui,|ui|{
                                for p in [Preset::Air3view,Preset::CapcutDesktop,Preset::Custom] {
                                    let choice=ui.selectable_value(&mut m.preset,p,p.label());
                                    #[cfg(test)] self.hitboxes.insert(format!("metadata_choice_{p:?}"),choice.rect);
                                    #[cfg(not(test))] let _=choice;
                                }
                            });
                            #[cfg(test)] self.hitboxes.insert("metadata_preset".into(),preset.response.rect);
                            #[cfg(not(test))] let _=preset;
                            if m.preset==Preset::CapcutDesktop {
                                ui.add(egui::Label::new(RichText::new("Mô phỏng metadata CapCut Desktop; chưa đối chiếu file gốc.").size(12.).color(TEAL)).wrap());
                            }
                            ui.separator();
                            ui.label("Tiêu đề — để trống dùng tên dự án");
                            let title=ui.add(egui::TextEdit::singleline(&mut m.title).desired_width(f32::INFINITY));
                            #[cfg(test)] self.hitboxes.insert("metadata_title".into(),title.rect);
                            #[cfg(not(test))] let _=title;
                            field(ui,"Tác giả / kênh",&mut m.author);
                            ui.label("Mô tả");ui.add(egui::TextEdit::multiline(&mut m.description).desired_width(f32::INFINITY).desired_rows(2));
                            ui.checkbox(&mut m.write_creation_time,"Ghi thời gian xuất");
                            if m.write_creation_time {
                                ui.add(egui::TextEdit::singleline(&mut m.creation_time).desired_width(f32::INFINITY).hint_text("Tự động; hoặc UTC: 2026-10-23T08:00:00Z"));
                            }
                            if m.preset==Preset::Custom {
                                ui.separator();ui.label("Tag nâng cao • chọn đúng cấp File / Video / Audio");
                                let mut remove=None;
                                for (i,tag) in m.custom_tags.iter_mut().enumerate() {
                                    ui.push_id(i,|ui| {
                                        ui.horizontal(|ui| {
                                            egui::ComboBox::from_id_salt("scope").width(75.).selected_text(tag.scope.label()).show_ui(ui,|ui| {
                                                for scope in [Scope::Container,Scope::Video,Scope::Audio] {ui.selectable_value(&mut tag.scope,scope,scope.label());}
                                            });
                                            ui.add(egui::TextEdit::singleline(&mut tag.key).desired_width((ui.available_width()-60.).max(80.)).hint_text("Tên tag, ví dụ comment"));
                                            if ui.button("Xóa").clicked(){remove=Some(i);}
                                        });
                                        ui.add(egui::TextEdit::multiline(&mut tag.value).desired_width(f32::INFINITY).desired_rows(1).hint_text("Giá trị"));
                                    });
                                }
                                if let Some(i)=remove {m.custom_tags.remove(i);}
                                if ui.add_enabled(m.custom_tags.len()<32,egui::Button::new("+ Thêm tag")).clicked(){m.custom_tags.push(Tag::default());}
                            }
                        });
                    });
                    egui::CollapsingHeader::new("Các tag sẽ ghi / trạng thái kiểm chứng").id_salt("metadata_planned").show(ui,|ui| {
                        match export_metadata::prepare(&self.project,&self.settings) {
                            Ok(plan)=>{
                                for t in plan.tags {ui.add(egui::Label::new(format!("{} / {} = {}",t.scope.label(),t.key,t.value)).wrap());}
                                for warning in plan.warnings {ui.add(egui::Label::new(RichText::new(warning).size(12.).color(MUTED)).wrap());}
                            }
                            Err(e)=>{ui.colored_label(Color32::LIGHT_RED,format!("{e:#}"));}
                        }
                    });
                    let actual=egui::CollapsingHeader::new("Metadata thực tế / báo cáo lần xử lý gần nhất").id_salt("metadata_actual").show(ui,|ui| {
                        if ui.button("Nạp báo cáo đã lưu của dự án").clicked() {
                            let path=self.project.root.join("metadata-report.json");
                            self.metadata_actual=fs::metadata(&path).ok().filter(|m|m.len()<1024*1024).and_then(|_|fs::read_to_string(&path).ok()).unwrap_or_else(||"Chưa có báo cáo metadata đã lưu.".into());
                        }
                        if self.metadata_actual.is_empty() {ui.label("Xuất video hoặc bấm Đọc MP4 để xem dữ liệu thực tế.");}
                        else {ui.add(egui::TextEdit::multiline(&mut self.metadata_actual.as_str()).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY).desired_rows(9));}
                    });
                    #[cfg(test)] self.hitboxes.insert("metadata_report_header".into(),actual.header_response.rect);
                    #[cfg(not(test))] let _=actual;
                });
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    let button_size=Vec2::new(194.,34.);
                    let a=ui.add_enabled(!self.busy,egui::Button::new("Lưu cấu hình").min_size(button_size));save=a.clicked();
                    let b=ui.add_enabled(!self.busy&&self.project.export_metadata.enabled,egui::Button::new("Áp dụng cho MP4…").min_size(button_size));apply=b.clicked();
                    let c=ui.add_enabled(!self.busy,egui::Button::new("Đọc metadata MP4…").min_size(button_size));inspect=c.clicked();
                    #[cfg(test)] {self.hitboxes.insert("metadata_save".into(),a.rect);self.hitboxes.insert("metadata_apply".into(),b.rect);self.hitboxes.insert("metadata_inspect".into(),c.rect);}
                });
                ui.label(RichText::new("Áp dụng cho MP4 tạo file mới, sao chép nguyên luồng hình và tiếng.").color(MUTED).size(12.));
            });
        #[cfg(test)] if let Some(window)=_window {self.hitboxes.insert("metadata_window".into(),window.response.rect);}
        self.metadata_open=open;
        if before!=serde_json::to_vec(&self.project.export_metadata).ok() {
            // Metadata edits preserve final preview, picture caches and edit history.
            self.dirty=true;self.layers.managed_edit=true;
        }
        if save {
            match export_metadata::prepare(&self.project,&self.settings) {
                Ok(_)=>{self.dirty=true;self.save();if !self.dirty {self.message("Đã lưu metadata và cấu hình mặc định cho dự án sau.",false);}}
                Err(e)=>self.message(format!("{e:#}"),true),
            }
        }
        if apply {
            if let Some(input)=rfd::FileDialog::new().set_title("Chọn MP4 cần cập nhật metadata").add_filter("MP4",&["mp4"]).pick_file() {
                let stem=input.file_stem().unwrap_or_default().to_string_lossy();
                if let Some(output)=rfd::FileDialog::new().set_title("Lưu bản MP4 mới có metadata")
                    .set_directory(input.parent().unwrap_or_else(||std::path::Path::new(".")))
                    .set_file_name(format!("{stem}.metadata-{}.mp4",&uuid::Uuid::new_v4().to_string()[..8])).add_filter("MP4",&["mp4"]).save_file() {
                    self.launch(Task::Metadata{input,output});
                }
            }
        }
        if inspect {
            if let Some(path)=rfd::FileDialog::new().set_title("Đọc metadata thực tế từ MP4").add_filter("MP4",&["mp4"]).pick_file(){self.launch(Task::InspectMetadata(path));}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn draw(app:&mut StudioApp,ctx:&egui::Context,size:Vec2,events:Vec<egui::Event>) {
        let _=ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,size)),events,..Default::default()},|ctx|app.metadata_window(ctx));
    }
    fn click(app:&mut StudioApp,ctx:&egui::Context,size:Vec2,key:&str) {
        let pos=app.hitboxes[key].center();
        for pressed in [true,false] {draw(app,ctx,size,vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed,modifiers:egui::Modifiers::NONE}]);}
    }
    #[test]
    fn native_metadata_layout_equal_buttons_edit_preserves_video_and_defaults() {
        for size in [Vec2::new(1080.,720.),Vec2::new(1440.,900.)] {
            let temp=tempfile::tempdir().unwrap();let ctx=egui::Context::default();let mut app=StudioApp::from_context(&ctx);
            app.capture_path=None;app.metadata_open=true;app.metadata_actual=String::new();
            app.projects_root=temp.path().join("projects");
            let original=temp.path().join("existing.mp4");fs::write(&original,b"existing video").unwrap();
            app.project=Project{root:temp.path().join("project"),rendered:Some(original.clone()),..Default::default()};
            for _ in 0..4 {draw(&mut app,&ctx,size,vec![]);}
            let screen=Rect::from_min_size(Pos2::ZERO,size);
            assert!(screen.contains_rect(app.hitboxes["metadata_window"]));
            let a=app.hitboxes["metadata_save"];
            for key in ["metadata_enabled","metadata_preset","metadata_title","metadata_save","metadata_apply","metadata_inspect"] {
                assert!(screen.contains_rect(app.hitboxes[key]),"{key} outside {size:?}");
            }
            for key in ["metadata_apply","metadata_inspect"] {assert!((a.height()-app.hitboxes[key].height()).abs()<0.5);assert!(!a.intersects(app.hitboxes[key]));}
            click(&mut app,&ctx,size,"metadata_title");draw(&mut app,&ctx,size,vec![egui::Event::Text("English & tiếng Việt".into())]);
            assert_eq!(app.project.export_metadata.title,"English & tiếng Việt");assert!(app.dirty);
            assert_eq!(app.project.rendered,Some(original.clone()));assert_eq!(fs::read(original).unwrap(),b"existing video");
            click(&mut app,&ctx,size,"metadata_preset");
            for _ in 0..3 {draw(&mut app,&ctx,size,vec![]);}
            click(&mut app,&ctx,size,"metadata_choice_CapcutDesktop");
            for _ in 0..3 {draw(&mut app,&ctx,size,vec![]);}
            assert_eq!(app.project.export_metadata.preset,Preset::CapcutDesktop);
            click(&mut app,&ctx,size,"metadata_save");assert!(!app.dirty,"{}",app.notice);
            let saved=Project::load(&app.project.root.join("project.json")).unwrap();assert_eq!(saved.export_metadata.preset,Preset::CapcutDesktop);assert!(saved.rendered.is_some());
            let defaults=ProjectDefaults::load(&temp.path().join("project-defaults.json")).unwrap();assert_eq!(defaults.export_metadata.preset,Preset::CapcutDesktop);assert!(defaults.export_metadata.title.is_empty());
            app.project.export_metadata.preset=Preset::Custom;
            app.project.export_metadata.custom_tags=(0..15).map(|i|Tag{scope:Scope::Container,key:format!("tag_{i}"),value:"Giá trị dài để kiểm tra cuộn và bố cục".repeat(8)}).collect();
            for _ in 0..3 {draw(&mut app,&ctx,size,vec![]);}
            for key in ["metadata_window","metadata_save","metadata_apply","metadata_inspect"] {assert!(screen.contains_rect(app.hitboxes[key]),"Long form: {key} outside {size:?}");}
        }
    }
}
