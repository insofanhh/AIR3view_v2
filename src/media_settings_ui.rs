//! Native controls for source normalization, picture effects and background music.
use super::*;
use crate::visual_effects::Effect;

fn effect_fields(ui:&mut egui::Ui,e:&mut Effect)->bool {
    let before=e.clone();
    ui.horizontal_wrapped(|ui|{ui.checkbox(&mut e.mirror_x,"Lật ngang");ui.checkbox(&mut e.mirror_y,"Lật dọc");});
    egui::ComboBox::from_id_salt(ui.id().with("lut_preset")).selected_text(match e.lut.as_str(){"off"=>"Tắt LUT","warm"=>"Ấm nhẹ","cool"=>"Lạnh nhẹ","documentary"=>"Tài liệu nhẹ",_=>"LUT đã nhập (.cube)"}).show_ui(ui,|ui|{
        for (id,name) in [("off","Tắt LUT"),("warm","Ấm nhẹ"),("cool","Lạnh nhẹ"),("documentary","Tài liệu nhẹ")] {ui.selectable_value(&mut e.lut,id.into(),name);}
    });
    ui.add_enabled_ui(e.lut!="off",|ui|{ui.label("Cường độ LUT");ui.add(egui::Slider::new(&mut e.strength,0.0..=1.));});
    before!=*e
}

impl StudioApp {
    pub(super) fn media_settings_changed(&mut self){self.dirty=true;self.project.rendered=None;self.project.ensure_composition();self.player.stop();self.layers.textures.clear();self.request_frame();}
    pub(super) fn footage_effects_ui(&mut self,ui:&mut egui::Ui){
        let focus=std::env::args().any(|v|v=="--capture-media=effects");
        let response=egui::CollapsingHeader::new("Màu sắc & lật footage").id_salt("footage_effects").default_open(focus).show(ui,|ui|{
            ui.label(RichText::new("Chỉ footage và nền video; host, phụ đề và thẻ giữ nguyên. Lật footage sẽ đảo cả chữ có sẵn trong nguồn.").small().color(MUTED));
            if effect_fields(ui,&mut self.project.effects.footage){self.media_settings_changed();}
            let r=ui.button("Nhập LUT .cube…");#[cfg(test)] self.hitboxes.insert("import_lut".into(),r.rect);
            if r.clicked(){if let Some(path)=rfd::FileDialog::new().add_filter("3D LUT",&["cube"]).pick_file(){self.launch(Task::LutImport(path,None));}}
        });
        if focus&&self.capture_path.is_some()&&self.frames==3{ui.scroll_to_rect(response.header_response.rect,Some(egui::Align::Min));}
    }
    pub(super) fn layer_effects_ui(&mut self,ui:&mut egui::Ui,c:&crate::composition::LayerClip){
        if c.audio()||c.media.is_none(){return;}
        egui::CollapsingHeader::new("Màu & lật riêng layer").id_salt(("layer_effects",&c.id)).show(ui,|ui|{
            let role=crate::layer_defaults::LayerDefaults::key(&self.project,c);
            let mut override_on=self.project.effects.layers.contains_key(&c.id)||role.as_ref().is_some_and(|key|self.project.layer_defaults.effects.contains_key(key));
            if ui.checkbox(&mut override_on,"Ghi đè thiết lập dự án").changed(){
                let before=self.project.clone();if override_on{let effect=crate::visual_effects::selected(&self.project,c).cloned().unwrap_or_default();self.project.effects.layers.insert(c.id.clone(),effect);}else{self.project.effects.layers.remove(&c.id);if let Some(key)=&role{self.project.layer_defaults.effects.remove(key);}}self.timeline_view.history.record(before);self.media_settings_changed();
            }
            if override_on {
                let before=self.project.clone();let mut effect=crate::visual_effects::selected(&self.project,c).cloned().unwrap_or_default();let changed=effect_fields(ui,&mut effect);
                if changed {
                    self.project.effects.layers.insert(c.id.clone(),effect);
                    if self.project.timeline.tracks[c.track].sync_parameters {
                        let effect=self.project.effects.layers[&c.id].clone();let targets=self.project.layer_parameter_targets(&c.id,false);
                        for id in targets{self.project.effects.layers.insert(id,effect.clone());}
                    }
                    self.timeline_view.history.record(before);self.media_settings_changed();
                }
                if ui.button("Nhập LUT cho layer…").clicked(){if let Some(path)=rfd::FileDialog::new().add_filter("3D LUT",&["cube"]).pick_file(){self.launch(Task::LutImport(path,Some(c.id.clone())));}}
            }
        });
    }
    pub(super) fn music_settings_ui(&mut self,ui:&mut egui::Ui){
        let response=egui::CollapsingHeader::new("Nhạc nền độc lập").id_salt("music_settings").default_open(true).show(ui,|ui|{
            ui.spacing_mut().slider_width=(ui.available_width()*0.3).clamp(55.,110.);
            let previous=self.project.music.clone();let before=serde_json::to_vec(&previous).unwrap_or_default();
            let ready=self.project.music.file.is_file();
            let _r=ui.add_enabled(ready,egui::Checkbox::new(&mut self.project.music.enabled,"Bật nhạc nền"));#[cfg(test)] self.hitboxes.insert("music_enabled".into(),_r.rect);
            if ready{let name=if self.project.music.name.is_empty(){self.project.music.file.file_name().unwrap_or_default().to_string_lossy().into_owned()}else{self.project.music.name.clone()};ui.add(egui::Label::new(name).truncate()).on_hover_text(self.project.music.file.to_string_lossy());}
            let mut choose=false;
            ui.horizontal_wrapped(|ui|{
                let r=ui.button("Chọn nhạc MP3 / WAV…");choose=r.clicked();#[cfg(test)] self.hitboxes.insert("music_import".into(),r.rect);
                if ui.add_enabled(ready,egui::Button::new("Bỏ nhạc")).clicked(){self.project.music=Default::default();}
            });
            ui.add_enabled_ui(ready,|ui|{
                ui.label("Âm lượng nhạc");let mut volume=self.project.music.gain*100.;let _r=ui.add(egui::Slider::new(&mut volume,0.0..=100.).suffix(" %"));self.project.music.gain=volume/100.;#[cfg(test)] self.hitboxes.insert("music_volume".into(),_r.rect);
                let m=&mut self.project.music;
                ui.horizontal_wrapped(|ui|{ui.label("Bắt đầu bài");ui.add(egui::DragValue::new(&mut m.offset_ms).range(0..=86_400_000).suffix(" ms"));});
                ui.horizontal_wrapped(|ui|{ui.label("Fade vào / ra");ui.add(egui::DragValue::new(&mut m.fade_in_ms).range(0..=60_000).suffix(" ms"));ui.add(egui::DragValue::new(&mut m.fade_out_ms).range(0..=60_000).suffix(" ms"));});
                ui.checkbox(&mut m.looping,"Lặp nhạc với nối chuyển tiếp nhẹ");ui.checkbox(&mut m.duck,"Tự giảm nhạc khi host nói");
                ui.add_enabled_ui(m.duck,|ui|{ui.label("Mức nhạc dưới lời nói");ui.add(egui::Slider::new(&mut m.duck_gain,0.0..=1.));});
                ui.checkbox(&mut m.mute_source,"Tắt nhạc trong hook / cảnh nghe tiếng gốc");
            });
            ui.label(RichText::new("Nhạc được lưu trong asset của app, nhớ cho dự án sau. Khi đổi bài, điểm vào hoặc ducking, preview chuẩn bị lại audio ở luồng nền.").small().color(MUTED));
            if before!=serde_json::to_vec(&self.project.music).unwrap_or_default(){
                if !previous.enabled&&self.project.music.enabled{if let Some(comp)=&mut self.project.composition{comp.removed.retain(|id|id!="music-bed");}}
                let mut gain_only=previous;gain_only.gain=self.project.music.gain;
                if serde_json::to_vec(&gain_only).ok()==serde_json::to_vec(&self.project.music).ok(){self.dirty=true;self.project.rendered=None;self.project.ensure_composition();self.player.update_audio(&self.project);}else{self.media_settings_changed();}
            }
            if choose{if let Some(path)=rfd::FileDialog::new().add_filter("Nhạc nền",&["mp3","wav","m4a","flac","ogg"]).pick_file(){self.launch(Task::MusicImport(path));}}
        });
        if self.capture_path.is_some()&&self.frames==3&&std::env::args().any(|v|v=="--capture-media=music"){ui.scroll_to_rect(response.header_response.rect,Some(egui::Align::Min));}
    }
    pub(super) fn normalization_ui(&mut self,ui:&mut egui::Ui){
        egui::CollapsingHeader::new("Chuẩn hóa video đầu vào").id_salt("normalization").show(ui,|ui|{
            let cfg=&mut self.project.normalization;
            egui::ComboBox::from_id_salt("normalization_mode").selected_text(match cfg.mode.as_str(){"keep"=>"Giữ nguyên","remux"=>"Đóng gói lại","transcode"=>"Chuyển mã H.264/AAC",_=>"Tự động (khuyến nghị)"}).show_ui(ui,|ui|{for(id,label)in[("auto","Tự động (khuyến nghị)"),("keep","Giữ nguyên"),("remux","Đóng gói lại"),("transcode","Chuyển mã H.264/AAC")]{ui.selectable_value(&mut cfg.mode,id.into(),label);}});
            ui.checkbox(&mut cfg.proxy,"Tạo proxy nhẹ cho preview");
            if ui.add_enabled(!self.project.sources.is_empty()&&self.project.sources.iter().all(|s|s.path.is_file()),egui::Button::new("Áp dụng cho nguồn hiện có")).clicked(){self.launch(Task::Normalize);}
            ui.label(RichText::new("Áp dụng khi tải / nhập nguồn hoặc chạy lại phân tích. Giữ nguyên file gốc; báo cáo nằm trong cache/normalized.").small().color(MUTED));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_music_and_effect_controls_fit_narrow_panels_and_preserve_old_defaults(){
        for width in[280.,420.,1080.] {
            let ctx=egui::Context::default();let mut app=StudioApp::from_context(&ctx);app.capture_path=None;
            let temp=tempfile::tempdir().unwrap();let audio=temp.path().join("music.wav");fs::write(&audio,b"UI-only fixture").unwrap();app.project.music.file=audio;
            let screen=Rect::from_min_size(Pos2::ZERO,Vec2::new(width,1000.));
            for _ in 0..3{let _=ctx.run(egui::RawInput{screen_rect:Some(screen),..Default::default()},|ctx|{egui::CentralPanel::default().show(ctx,|ui|{app.music_settings_ui(ui);effect_fields(ui,&mut app.project.effects.footage);});});}
            for key in["music_enabled","music_import","music_volume"]{assert!(screen.contains_rect(app.hitboxes[key]),"{key} clipped at {width}: {:?}",app.hitboxes[key]);}
            assert!(!app.project.music.enabled);assert!(!app.project.effects.footage.active());
        }
    }
}
