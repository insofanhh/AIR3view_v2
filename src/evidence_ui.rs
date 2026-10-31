//! Native evidence controls. Extraction and thumbnails run outside the UI thread.
use super::*;
use crate::{
    evidence::{self, Annotation, EvidenceItem},
    timeline,
    timeline_media::{self, Data, MediaCache, Request},
};

#[derive(Default)]
pub(super) struct EvidenceView {
    selected: usize,
    media: MediaCache,
    textures: BTreeMap<String, egui::TextureHandle>,
    source_key: String,
    source_label: String,
    source_crop: [f32; 4],
    source_open: bool,
    sound_notice:String,
}
fn choose(ui: &mut egui::Ui, id: &str, label: &str, value: &mut String, options: &[(&str, &str)]) {
    ui.horizontal_wrapped(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(id)
            .selected_text(
                options
                    .iter()
                    .find(|(v, _)| *v == value)
                    .map(|(_, l)| *l)
                    .unwrap_or(value),
            )
            .show_ui(ui, |ui| {
                for (v, l) in options {
                    ui.selectable_value(value, (*v).into(), *l);
                }
            });
    });
}
fn crop_ui(ui: &mut egui::Ui, id: &str, c: &mut [f32; 4]) {
    ui.push_id(id, |ui| {
        ui.horizontal_wrapped(|ui| {
            for (i, label) in ["X", "Y", "Rộng", "Cao"].iter().enumerate() {
                ui.label(*label);
                ui.add(
                    egui::DragValue::new(&mut c[i])
                        .speed(0.005)
                        .range(if i < 2 { 0.0..=0.99 } else { 0.01..=1.0 }),
                );
            }
        });
    });
    c[2] = c[2].min(1. - c[0]);
    c[3] = c[3].min(1. - c[1]);
}
impl StudioApp {
    pub(super) fn card_sound_settings_ui(&mut self,ui:&mut egui::Ui) {
        self.sound_controls(ui,false);
    }
    fn card_sound_controls(&mut self,ui:&mut egui::Ui) {self.sound_controls(ui,false);}
    pub(super) fn evidence_sound_settings_ui(&mut self,ui:&mut egui::Ui) {self.sound_controls(ui,true);}
    fn evidence_sound_controls(&mut self,ui:&mut egui::Ui) {self.sound_controls(ui,true);}
    fn sound_controls(&mut self,ui:&mut egui::Ui,evidence:bool) {
        use crate::evidence_sfx::{self,Scope};
        let old_card=self.project.card_sound.clone();let old_evidence=self.project.evidence_settings.clone();
        let count=self.project.composition.as_ref().map(|c|c.clips.iter().filter(|c|c.track==timeline::SFX&&c.evidence_id.is_some()==evidence).count()).unwrap_or(0);
        ui.label(RichText::new(format!("{} · {count}",if evidence{"ẢNH DẪN CHỨNG"}else{"THẺ THÔNG TIN / SỐ LIỆU"})).strong().color(MUTED));
        if !evidence {
            let sync=ui.checkbox(&mut self.project.card_sound.sync_style,"Đồng bộ loại SFX cho cả hai nhóm");
            #[cfg(test)] self.hitboxes.insert("sfx_sync_style".into(),sync.rect);
            let _=sync;
        }
        let (style,enabled,gain)=if evidence {(&mut self.project.evidence_settings.sound_style,&mut self.project.evidence_settings.sound_enabled,&mut self.project.evidence_settings.sound_gain)}else{(&mut self.project.card_sound.style,&mut self.project.card_sound.enabled,&mut self.project.card_sound.gain)};
        let enabled_response=ui.checkbox(enabled,if evidence{"Phát SFX khi ảnh xuất hiện"}else{"Phát SFX khi thẻ xuất hiện"});
        #[cfg(test)] self.hitboxes.insert(if evidence{"evidence_sound_enabled"}else{"card_sound_enabled"}.into(),enabled_response.rect);
        let _=enabled_response;
        let id=if evidence{"evidence_sound"}else{"card_sound"};let mut listen=false;let mut force=false;
        ui.add_enabled_ui(*enabled,|ui| {
            ui.horizontal_wrapped(|ui| {
                let picker=egui::ComboBox::from_id_salt(format!("{id}_style")).width(142.)
                    .selected_text(evidence_sfx::style_label(style)).show_ui(ui,|ui|{
                        for value in ["auto","whoosh","pop","ting"] {
                            let r=ui.selectable_value(style,value.into(),evidence_sfx::style_label(value));
                            #[cfg(test)] self.hitboxes.insert(format!("{id}_option_{value}"),r.rect);
                            let _=r;
                        }
                    });
                #[cfg(test)] self.hitboxes.insert(format!("{id}_style"),picker.response.rect);
                let _=picker;
                let r=ui.button("▶ Nghe");listen=r.clicked();
                #[cfg(test)] self.hitboxes.insert(format!("{id}_listen"),r.rect);
            });
            ui.label("Âm lượng");ui.spacing_mut().slider_width=(ui.available_width()-95.).clamp(60.,180.);
            let mut percent=*gain*100.;
            let r=ui.add(egui::Slider::new(&mut percent,0.0..=if evidence{100.}else{200.}).suffix(" %"));
            #[cfg(test)] self.hitboxes.insert(format!("{id}_gain"),r.rect);
            if r.changed(){*gain=percent/100.;}
            let r=ui.small_button("Áp dụng lại cho nhóm");force=r.clicked();
            #[cfg(test)] self.hitboxes.insert(format!("{id}_apply"),r.rect);
        });
        let old_style=if evidence{&old_evidence.sound_style}else{&old_card.style};
        let selected=if evidence{self.project.evidence_settings.sound_style.clone()}else{self.project.card_sound.style.clone()};
        let linked=self.project.card_sound.sync_style&&(old_style!=&selected||!old_card.sync_style||force);
        if linked {self.project.card_sound.style=selected.clone();self.project.evidence_settings.sound_style=selected.clone();}
        if force || old_card!=self.project.card_sound || serde_json::to_vec(&old_evidence).ok()!=serde_json::to_vec(&self.project.evidence_settings).ok() {
            let mut before=self.project.clone();before.card_sound=old_card;before.evidence_settings=old_evidence.clone();
            let old_gain=(old_evidence.sound_gain!=self.project.evidence_settings.sound_gain).then_some(old_evidence.sound_gain);
            let report=evidence_sfx::apply_settings(&mut self.project,if linked{Scope::Both}else if evidence{Scope::Evidence}else{Scope::Cards},old_gain);
            self.timeline_view.history.record(before);self.dirty=true;self.layers.managed_edit=true;
            self.evidence_view.sound_notice=format!("Áp dụng {}/{} SFX · khóa: {} · âm riêng: {}",report.updated,report.total,report.locked,report.custom);
            if self.preview_final {
                let playing=self.player.playing;self.preview_final=false;self.preview_scene=None;self.texture=None;self.blurred_texture=None;
                self.player.composition(self.mask_preview(),self.settings.clone(),self.timeline_view.cursor,playing);
            }else{self.player.refresh_sfx(&self.project);}
        }
        if listen {
            let gain=if evidence{self.project.evidence_settings.sound_gain}else{self.project.card_sound.gain};
            if let Err(error)=self.player.audition(&selected,if evidence{"fact"}else{"title"},gain){self.message(format!("{error:#}"),true);}
        }
        ui.label(RichText::new(if self.project.card_sound.sync_style{"Loại âm đồng bộ; âm lượng chỉnh riêng từng nhóm."}else if evidence{"Chỉ áp dụng cho ảnh dẫn chứng."}else{"Chỉ áp dụng cho thẻ chữ / số liệu."}).size(11.).color(MUTED));
        if !self.evidence_view.sound_notice.is_empty(){ui.label(RichText::new(&self.evidence_view.sound_notice).size(11.).color(MUTED));}
        if self.project.timeline.tracks[timeline::SFX].muted||self.project.timeline.tracks[if evidence{timeline::EVIDENCE}else{timeline::GRAPHICS}].muted {
            ui.colored_label(egui::Color32::YELLOW,"Track của nhóm hoặc SFX đang mute.");
        }
    }
    pub(super) fn evidence_settings_ui(&mut self, ui: &mut egui::Ui) {
        heading(ui, "DẪN CHỨNG CHO HOST");
        self.evidence_settings_controls(ui);
    }
    pub(super) fn evidence_settings_controls(&mut self,ui:&mut egui::Ui) {
        if self.player.preparing_backgrounds>0 {
            ui.horizontal_wrapped(|ui|{ui.spinner();ui.label(format!("Đang cập nhật nền cho {} cảnh Host từ video đã tải…",self.player.preparing_backgrounds));});
        }
        let old=serde_json::to_vec(&self.project.evidence_settings).unwrap_or_default();
        let old_crop=self.project.evidence_settings.crop_percent;
        let s = &mut self.project.evidence_settings;
        ui.checkbox(&mut s.enabled, "Ảnh / số liệu phía sau Host");
        ui.add_enabled_ui(s.enabled, |ui| {
            choose(ui,"evidence_presentation","Trình bày",&mut s.presentation,&[
                ("stack","Thẻ chồng · motion graphic"),("classic","Các ô cố định")]);
            if s.presentation=="stack" {
                ui.label(RichText::new("Ảnh sau chồng lên ảnh trước. Các ảnh giữ đến hết cảnh, tiếp tục chuyển động; ảnh phía sau lùi mờ. Giữ thời điểm đã căn tay.").size(11.).color(MUTED));
            }
            choose(ui,"evidence_motion","Chuyển động khi hiển thị",&mut s.motion,&[
                ("zoom","Zoom vào nhẹ · 1 → 1,04"),("drift","Trôi nhẹ"),("none","Tĩnh")]);
            choose(
                ui,
                "evidence_preset",
                "Bố cục",
                &mut s.preset,
                &[
                    ("case_file", "Hồ sơ tình huống"),
                    ("comparison", "So sánh dẫn chứng"),
                    ("center", "Host trung tâm"),
                ],
            );
            choose(
                ui,
                "evidence_background",
                "Nền",
                &mut s.background,
                &[
                    ("paper", "Giấy sáng"),
                    ("dark", "Tối"),
                    ("blur", "Ảnh nguồn làm mờ"),
                ],
            );
            ui.add_enabled_ui(s.presentation!="stack",|ui| {ui.horizontal_wrapped(|ui| {
                ui.label("Tối đa thẻ cùng lúc");
                ui.add(egui::DragValue::new(&mut s.density).range(1..=3));
            });});
            ui.horizontal_wrapped(|ui|{
                ui.label("Crop ảnh dẫn chứng");
                let _crop=ui.add(egui::DragValue::new(&mut s.crop_percent).speed(0.5).range(0.0..=30.0).suffix(" % / cạnh"));
                #[cfg(test)] self.hitboxes.insert("evidence_crop_percent".into(),_crop.rect);
            });
            ui.label(RichText::new("Cắt đều mỗi cạnh ảnh gốc; 5% giữ vùng giữa 90% × 90%. Kết hợp vùng crop riêng của từng thẻ. Không đổi video nguồn.").size(11.).color(MUTED));
            ui.add_enabled_ui(s.presentation!="stack",|ui| {ui.horizontal_wrapped(|ui| {
                ui.label("Hiển thị mặc định");
                ui.add(
                    egui::DragValue::new(&mut s.default_duration_ms)
                        .speed(100)
                        .range(500..=30000)
                        .suffix(" ms"),
                );
            });});
            if s.presentation=="stack" {
                ui.label(RichText::new("Chia đều thời điểm xuất hiện trong cảnh; các thẻ cùng kết thúc ở cuối cảnh. Chỉnh riêng tại Căn thời điểm thủ công.").size(11.).color(MUTED));
            }
        });
        if serde_json::to_vec(&self.project.evidence_settings).unwrap_or_default()!=old {
            if self.project.evidence_settings.crop_percent!=old_crop {
                for seg in &mut self.project.segments {for item in &mut seg.evidence_items {
                    item.asset=None;item.annotation_asset=None;item.asset_key.clear();
                }}
            }
            self.project.ensure_composition();
            self.transform_changed();self.request_frame();
        }
        ui.separator();
        settings_group(ui, "output_card_sound", "Âm thanh thẻ thông tin", false, |ui| self.card_sound_controls(ui));
        settings_group(ui, "output_evidence_sound", "Âm thanh ảnh dẫn chứng", false, |ui| self.evidence_sound_controls(ui));
        if old_crop!=self.project.evidence_settings.crop_percent && !self.project.segments.is_empty() {
            self.message("Đã đổi crop. Bấm Tạo thẻ / cập nhật ảnh dẫn chứng để áp dụng cho preview và xuất MP4.",false);
        }
        if ui.add_enabled(!self.busy&&!self.project.segments.is_empty(),egui::Button::new("Tạo thẻ / cập nhật ảnh dẫn chứng")).clicked(){self.launch(Task::Evidence);}
        let apply=ui.add_enabled(!self.project.segments.is_empty(),egui::Button::new("Áp dụng dàn cảnh dẫn chứng"));
        #[cfg(test)] self.hitboxes.insert("evidence_apply_motion".into(),apply.rect);
        if apply.clicked(){let before=self.project.clone();evidence::reflow(&mut self.project);self.timeline_view.history.record(before);self.transform_changed();self.request_frame();}
        ui.label(RichText::new("AI đề xuất khi phân tích kịch bản. Dự án cũ thêm thẻ tại tab Dẫn chứng; giữ nguyên giọng/avatar.").size(11.).color(MUTED));
    }
    pub(super) fn evidence_source_window(&mut self, ctx: &egui::Context) {
        self.evidence_view.media.poll();
        for (key, data) in &self.evidence_view.media.ready {
            if let Data::Thumbnail(img) = data {
                if !self.evidence_view.textures.contains_key(key) {
                    self.evidence_view.textures.insert(
                        key.clone(),
                        ctx.load_texture(
                            key,
                            egui::ColorImage::from_rgba_unmultiplied(
                                [img.width() as usize, img.height() as usize],
                                img.as_raw(),
                            ),
                            Default::default(),
                        ),
                    );
                }
            }
        }
        if !self.evidence_view.source_open {
            return;
        }
        egui::Window::new("Đối chiếu khung hình nguồn").open(&mut self.evidence_view.source_open).default_width(840.).show(ctx,|ui|{
            ui.label(&self.evidence_view.source_label);
            if let Some(tex)=self.evidence_view.textures.get(&self.evidence_view.source_key){
                let c=self.evidence_view.source_crop;
                ui.columns(2,|cols|{
                    cols[0].label("Khung nguồn · vùng xanh là ảnh cuối");
                    let sz=tex.size_vec2();let size=sz*(cols[0].available_width()/sz.x).min(1.);
                    let r=cols[0].add(egui::Image::new(tex).fit_to_exact_size(size));
                    let crop=Rect::from_min_size(r.rect.min+Vec2::new(c[0]*size.x,c[1]*size.y),Vec2::new(c[2]*size.x,c[3]*size.y));
                    cols[0].painter().rect_stroke(crop,0.,Stroke::new(2.,TEAL),egui::StrokeKind::Inside);
                    cols[1].label("Ảnh sau crop AI + crop% dự án");
                    let crop_size=Vec2::new(sz.x*c[2],sz.y*c[3]);let fit=crop_size*(cols[1].available_width()/crop_size.x.max(1.)).min(360./crop_size.y.max(1.)).min(2.);
                    cols[1].add(egui::Image::new(tex).uv(Rect::from_min_max(Pos2::new(c[0],c[1]),Pos2::new(c[0]+c[2],c[1]+c[3]))).fit_to_exact_size(fit));
                });
                ui.label("Đối chiếu chủ thể và chữ/số nhìn thấy trước khi xác nhận dữ kiện.");
            }else if matches!(self.evidence_view.media.ready.get(&self.evidence_view.source_key),Some(Data::Failed)){ui.colored_label(Color32::LIGHT_RED,"Không trích được ảnh nguồn. Kiểm tra đường dẫn và timestamp.");}
            else{ui.spinner();ui.label("Đang lấy khung hình…");}
        });
    }
    pub(super) fn view_evidence_source(&mut self, item: &EvidenceItem, second: bool) {
        let sid = if second && !item.second_source_id.is_empty() {
            &item.second_source_id
        } else {
            &item.source_id
        };
        let time = if second {
            item.second_time_ms.unwrap_or(0)
        } else {
            item.source_time_ms
        };
        if let Ok(src) = self.project.source(sid) {
            let key = timeline_media::identity(&src.path, time, 0, false);
            self.evidence_view.media.request(Request {
                key: key.clone(),
                path: src.path.clone(),
                start: time,
                duration: 0,
                waveform: false,
                folder: self.project.root.join("cache/evidence-ui-source"),
                settings: self.settings.clone(),
            });
            self.evidence_view.source_key = key;
            self.evidence_view.source_label =
                format!("{} · {} · {}", sid, evidence::time(time), src.title);
            self.evidence_view.source_crop = evidence::effective_crop(if second {item.second_crop}else{item.crop},self.project.evidence_settings.crop_percent)
                .unwrap_or([0.,0.,1.,1.]);
            self.evidence_view.source_open = true;
        }
    }
    pub(super) fn evidence_inspector(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("DẪN CHỨNG THEO CẢNH").strong().color(MUTED));
            let build = ui.add_enabled(
                !self.project.segments.is_empty(),
                egui::Button::new("Tạo thẻ / cập nhật"),
            );
            #[cfg(test)]
            self.hitboxes
                .insert("evidence_build_top".into(), build.rect);
            if build.clicked() {
                self.launch(Task::Evidence);
            }
            if ui.add_enabled(!self.busy&&!self.project.segments.is_empty(),egui::Button::new("Kiểm tra / chọn lại ảnh dẫn chứng")).clicked(){
                self.timeline_view.history.record(self.project.clone());self.launch(Task::EvidenceReview(None));
            }
        });
        egui::CollapsingHeader::new("Thiết lập bố cục / nền")
            .show(ui, |ui| self.evidence_settings_ui(ui));
        let Some(seg) = self.project.segments.get(self.selected).cloned() else {
            ui.label("Chọn cảnh Host để thêm dẫn chứng.");
            return;
        };
        if seg.kind != "host" {
            ui.label("Dẫn chứng dùng tốt nhất với cảnh Host. Có thể thêm thẻ vào cảnh review.");
        }
        let editable = !seg.locked
            && ![timeline::EVIDENCE, timeline::ANNOTATION]
                .iter()
                .any(|i| self.project.timeline.tracks[*i].locked);
        ui.add_enabled_ui(editable, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (kind, label) in [
                    ("frame", "+ Khung hình"),
                    ("stat", "+ Số liệu"),
                    ("fact", "+ Trích dẫn"),
                    ("comparison", "+ So sánh"),
                ] {
                    let r = ui.add_enabled(seg.evidence_items.len() < 4, egui::Button::new(label));
                    #[cfg(test)]
                    self.hitboxes.insert(format!("evidence_add_{kind}"), r.rect);
                    if r.clicked() {
                        self.project.segments[self.selected]
                            .evidence_items
                            .push(EvidenceItem {
                                label: crate::content_language::text(&self.project.rules.language,"Dẫn chứng từ video","Evidence from source video").into(),
                                kind: kind.into(),
                                source_id: seg.source_id.clone(),
                                source_time_ms: seg.in_ms,
                                second_time_ms: if kind == "comparison" {
                                    Some(seg.in_ms)
                                } else {
                                    None
                                },
                                duration_ms: self
                                    .project
                                    .evidence_settings
                                    .default_duration_ms
                                    .min(seg.duration_ms)
                                    .max(100),
                                ..Default::default()
                            });
                        self.evidence_view.selected = seg.evidence_items.len();
                        self.touch();
                    }
                }
            });
        });
        if seg.evidence_items.is_empty() {
            ui.label("Chưa có dẫn chứng. Thêm khung hình hoặc phân tích lại để AI đề xuất theo lời thoại.");
            return;
        }
        self.evidence_view.selected = self
            .evidence_view
            .selected
            .min(seg.evidence_items.len() - 1);
        ui.horizontal_wrapped(|ui| {
            for (i, item) in seg.evidence_items.iter().enumerate() {
                let mut texture = None;
                if let Some(asset) = &item.asset {
                    let path = self.project.root.join(asset);
                    let key = timeline_media::identity(&path, 0, 0, false);
                    self.evidence_view.media.request(Request {
                        key: key.clone(),
                        path,
                        start: 0,
                        duration: 0,
                        waveform: false,
                        folder: self.project.root.join("cache/evidence-ui-thumbs"),
                        settings: self.settings.clone(),
                    });
                    texture = self.evidence_view.textures.get(&key);
                }
                let label = format!("{} · {}", i + 1, item.label);
                let button = if let Some(tex) = texture {
                    egui::Button::image_and_text(
                        egui::Image::new(tex).fit_to_exact_size(Vec2::new(48., 36.)),
                        label,
                    )
                } else {
                    egui::Button::new(label)
                };
                let response = ui.add_sized(
                    Vec2::new(180f32.min(ui.available_width()), 52.),
                    button.wrap().selected(self.evidence_view.selected == i),
                );
                if response.clicked() {
                    self.evidence_view.selected = i;
                    self.layers.selected = self
                        .project
                        .composition_snapshot()
                        .clips
                        .iter()
                        .find(|c| {
                            c.evidence_id.as_ref() == Some(&item.id)
                                && c.track == timeline::EVIDENCE
                        })
                        .map(|c| c.id.clone());
                }
            }
        });
        let index = self.evidence_view.selected;
        let original = &seg.evidence_items[index];
        let mut item = original.clone();
        ui.separator();
        let color=if item.selection.passed(){TEAL}else{Color32::YELLOW};
        ui.colored_label(color,item.selection.label());
        if !item.selection.reason.is_empty(){ui.label(RichText::new(&item.selection.reason).size(11.).color(MUTED));}
        settings_group(ui,"evidence_candidates","Ảnh ứng viên / chọn lại",false,|ui|{
            ui.label(RichText::new("Ảnh đã áp dụng crop% dự án. Bấm ảnh để dùng thủ công; đối chiếu nguồn trước khi xác nhận dữ kiện.").size(11.).color(MUTED));
            // Wrap inside the inspector's existing vertical scroll area. A
            // nested horizontal scrollbar can paint over the transport below
            // a short portrait inspector.
            ui.horizontal_wrapped(|ui|{for choice in &item.selection.choices {
                    let path=self.project.root.join(&choice.image);let key=timeline_media::identity(&path,0,0,false);
                    self.evidence_view.media.request(Request{key:key.clone(),path,start:0,duration:0,waveform:false,
                        folder:self.project.root.join("cache/evidence-ui-choices"),settings:self.settings.clone()});
                    ui.allocate_ui_with_layout(Vec2::new(144.,116.),egui::Layout::top_down(egui::Align::Min),|ui|{
                        let response=if let Some(tex)=self.evidence_view.textures.get(&key){ui.add(egui::Button::image(
                            egui::Image::new(tex).fit_to_exact_size(Vec2::new(132.,80.))).selected(item.selection.chosen_id==choice.id))}
                        else{ui.add_sized([132.,80.],egui::Button::new("Đang tải ảnh…"))};
                        ui.label(RichText::new(evidence::time(choice.time_ms)).size(11.));
                        if response.clicked()&&editable&&!self.busy {
                            self.timeline_view.history.record(self.project.clone());
                            self.launch(Task::EvidencePick{item_id:item.id.clone(),choice_id:Some(choice.id.clone())});
                        }
                    });
                }});
            ui.add_enabled_ui(editable&&!self.busy,|ui|{ui.horizontal_wrapped(|ui|{
                if ui.button("Chọn lại bằng AI").clicked(){self.timeline_view.history.record(self.project.clone());self.launch(Task::EvidenceReview(Some(item.id.clone())));}
                if ui.button("Giữ lựa chọn này").clicked(){self.timeline_view.history.record(self.project.clone());self.launch(Task::EvidencePick{item_id:item.id.clone(),choice_id:None});}
            });});
        });
        ui.add_enabled_ui(editable, |ui| {
            settings_group(ui, "evidence_item_source", "1 / Nguồn & crop ảnh", true, |ui| {
            ui.checkbox(&mut item.enabled, "Hiện dẫn chứng");
            let source_ids: Vec<_> = self.project.sources.iter().map(|s| s.id.clone()).collect();
            let sources: Vec<_> = source_ids
                .iter()
                .map(|s| (s.as_str(), s.as_str()))
                .collect();
            choose(
                ui,
                "evidence_source",
                "Video",
                &mut item.source_id,
                &sources,
            );
            let max = self
                .project
                .source(&item.source_id)
                .map(|s| s.duration_ms.saturating_sub(1))
                .unwrap_or(7_200_000);
            ui.add(
                egui::DragValue::new(&mut item.source_time_ms)
                    .speed(100)
                    .range(0..=max)
                    .prefix("Timestamp ")
                    .suffix(" ms"),
            );
            egui::CollapsingHeader::new("Crop ảnh [0–1]")
                .show(ui, |ui| crop_ui(ui, "crop1", &mut item.crop));
            if ui.small_button("Mở rộng crop về toàn ảnh").clicked(){item.crop=[0.,0.,1.,1.];}
            if ui.button("Xem nguồn").clicked() {
                self.view_evidence_source(&item, false);
            }
            if item.kind == "comparison" || item.proof.formula == "delta_ms" {
                if item.second_time_ms.is_none() {
                    item.second_time_ms = Some(item.source_time_ms);
                }
                if item.second_source_id.is_empty() {
                    item.second_source_id = item.source_id.clone();
                }
                choose(
                    ui,
                    "evidence_source2",
                    "Nguồn B",
                    &mut item.second_source_id,
                    &sources,
                );
                let max = self
                    .project
                    .source(&item.second_source_id)
                    .map(|s| s.duration_ms.saturating_sub(1))
                    .unwrap_or(7_200_000);
                ui.add(
                    egui::DragValue::new(item.second_time_ms.as_mut().unwrap())
                        .speed(100)
                        .range(0..=max)
                        .prefix("Timestamp B ")
                        .suffix(" ms"),
                );
                egui::CollapsingHeader::new("Crop ảnh B")
                    .show(ui, |ui| crop_ui(ui, "crop2", &mut item.second_crop));
                if ui.button("Xem nguồn B").clicked() {
                    self.view_evidence_source(&item, true);
                }
            }
            });
            settings_group(ui, "evidence_item_content", "2 / Nội dung & chú thích", false, |ui| {
            choose(
                ui,
                "evidence_kind",
                "Loại",
                &mut item.kind,
                &[
                    ("frame", "Ảnh"),
                    ("stat", "Số liệu"),
                    ("fact", "Trích dẫn"),
                    ("comparison", "So sánh"),
                ],
            );
            field(ui, "Nhãn thẻ", &mut item.label);
            field(ui, "Ý dẫn chứng", &mut item.claim);
            if matches!(item.kind.as_str(), "stat" | "fact") {
                choose(
                    ui,
                    "evidence_formula",
                    "Dữ kiện",
                    &mut item.proof.formula,
                    &[("", "Trích dẫn"), ("delta_ms", "Chênh lệch timestamp")],
                );
                if item.proof.formula.is_empty() {
                    field(ui, "Giá trị", &mut item.value);
                    field(ui, "Đơn vị", &mut item.unit);
                }
                ui.label("Trích nguyên câu/chữ làm bằng chứng");
                ui.text_edit_multiline(&mut item.proof.quote);
                ui.checkbox(&mut item.proof.confirmed, "Đã đối chiếu dữ kiện với nguồn");
            }
            egui::CollapsingHeader::new("Chú thích trên ảnh").show(ui, |ui| {
                let mut remove = None;
                for (i, a) in item.annotations.iter_mut().enumerate() {
                    ui.push_id(i, |ui| {
                        choose(
                            ui,
                            "annkind",
                            "Kiểu",
                            &mut a.kind,
                            &[
                                ("circle", "Khoanh tròn"),
                                ("arrow", "Mũi tên"),
                                ("underline", "Gạch chân"),
                                ("label", "Nhãn"),
                            ],
                        );
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Từ X/Y");
                            for v in &mut a.from {
                                ui.add(egui::DragValue::new(v).speed(0.01).range(0.0..=1.0));
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Đến X/Y");
                            for v in &mut a.to {
                                ui.add(egui::DragValue::new(v).speed(0.01).range(0.0..=1.0));
                            }
                        });
                        if a.kind == "label" {
                            field(ui, "Chữ", &mut a.text);
                        }
                        if ui.small_button("Xóa chú thích").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    item.annotations.remove(i);
                }
                if ui
                    .add_enabled(item.annotations.len() < 4, egui::Button::new("+ Chú thích"))
                    .clicked()
                {
                    item.annotations.push(Annotation::default());
                }
            });
            });
            settings_group(ui, "evidence_item_motion", "3 / Vị trí, chuyển động & thời điểm", false, |ui| {
            choose(
                ui,
                "evidence_slot",
                "Vị trí",
                &mut item.slot,
                &[
                    ("auto", "Tự động"),
                    ("upper_left", "Trên trái"),
                    ("upper_right", "Trên phải"),
                    ("lower_left", "Dưới trái"),
                    ("lower_right", "Dưới phải"),
                ],
            );
            choose(
                ui,
                "evidence_transition",
                "Hiệu ứng",
                &mut item.transition,
                &[
                    ("slide_fade", "Trượt + mờ"),
                    ("fade", "Mờ dần"),
                    ("none", "Không"),
                ],
            );
            ui.checkbox(&mut item.zoom, "Thêm zoom nhẹ cho riêng thẻ này");
            field(ui, "Cụm từ trong lời thoại", &mut item.anchor_phrase);
            ui.add(
                egui::DragValue::new(&mut item.occurrence)
                    .range(0..=99)
                    .prefix("Lần xuất hiện (0=tự chọn) "),
            );
            ui.checkbox(&mut item.timing_manual, "Căn thời điểm thủ công");
            ui.add_enabled_ui(item.timing_manual, |ui| {
                ui.add(
                    egui::DragValue::new(&mut item.start_ms)
                        .speed(50)
                        .range(0..=seg.duration_ms.saturating_sub(100))
                        .prefix("Vào cảnh ")
                        .suffix(" ms"),
                );
            });
            ui.add(
                egui::DragValue::new(&mut item.duration_ms)
                    .speed(100)
                    .range(100..=60000)
                    .prefix("Hiển thị ")
                    .suffix(" ms"),
            );
            ui.label(format!(
                "Căn thoại: {}",
                match item.timing_status.as_str() {
                    "aligned" => "đã khớp từ thật",
                    "ambiguous" => "trùng cụm từ; chọn lần xuất hiện",
                    "not_found" => "không tìm thấy; cần căn tay",
                    "manual" => "thủ công",
                    _ => "ước lượng; chạy Căn từ để chính xác",
                }
            ));
            });
        });
        let changed = serde_json::to_vec(&item).ok() != serde_json::to_vec(original).ok();
        if changed && editable {
            let mut prior = original.clone();
            prior.proof.confirmed = item.proof.confirmed;
            if evidence::content_identity(&item) != evidence::content_identity(&prior) {
                item.proof.confirmed = false;
            }
            if evidence::content_identity(&item) != evidence::content_identity(original) {
                item.selection.status="pending".into();item.selection.key.clear();
                item.asset = None;
                item.annotation_asset = None;
                item.asset_key.clear();
            }
            self.project.segments[self.selected].evidence_items[index] = item.clone();
            self.project.rendered = None;
            self.touch();
        }
        if item.asset.is_none() {
            ui.colored_label(
                Color32::YELLOW,
                "Thẻ chưa tạo hoặc nội dung đã đổi. Bấm Tạo thẻ / cập nhật.",
            );
        }
        ui.add_enabled_ui(editable, |ui| {
            ui.horizontal_wrapped(|ui| {
                let r = ui.button("Tạo thẻ / cập nhật");
                #[cfg(test)]
                self.hitboxes.insert("evidence_build".into(), r.rect);
                if r.clicked() {
                    self.launch(Task::Evidence);
                }
                if ui.button("Sắp xếp lại theo khung").clicked() {
                    let before = self.project.clone();
                    evidence::reflow(&mut self.project);
                    self.timeline_view.history.record(before);
                    self.transform_changed();
                    self.request_frame();
                }
                if ui.button("Xóa thẻ").clicked() {
                    self.project.segments[self.selected]
                        .evidence_items
                        .remove(index);
                    self.project.ensure_composition();
                    self.touch();
                    self.request_frame();
                }
            });
        });
        ui.label(RichText::new("Chỉnh kéo / kích thước / xoay trong tab Cảnh. Tạo thẻ chỉ cập nhật ảnh và chú thích.").size(11.).color(MUTED));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sound_controls_fit_adjust_volume_keep_voice_and_undo() {
        for width in [240.,320.,560.] {
            let ctx=egui::Context::default();let mut app=StudioApp::from_context(&ctx);
            app.capture_path=None;app.project=Project::default();
            app.project.segments=vec![Segment{id:"s".into(),audio:Some("keep.wav".into()),asset_key:"keep-voice".into(),..Default::default()}];
            let screen=Rect::from_min_size(Pos2::ZERO,Vec2::new(width+24.,720.));
            let draw=|app:&mut StudioApp,events|{let _=ctx.run(egui::RawInput{screen_rect:Some(screen),events,..Default::default()},|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{app.card_sound_settings_ui(ui);ui.separator();app.evidence_sound_settings_ui(ui);});
            });};
            for _ in 0..3{draw(&mut app,vec![]);}
            for key in ["sfx_sync_style","card_sound_enabled","card_sound_gain","evidence_sound_gain","card_sound_style","evidence_sound_style","card_sound_listen","evidence_sound_listen","card_sound_apply","evidence_sound_apply"] {assert!(screen.contains_rect(app.hitboxes[key]),"{key} outside {width}px panel: {:?}",app.hitboxes[key]);}
            let slider=app.hitboxes["card_sound_gain"];
            let pos=egui::pos2(slider.left()+40.,slider.center().y);
            for pressed in [true,false]{draw(&mut app,vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed,modifiers:egui::Modifiers::NONE}]);}
            assert_ne!(app.project.card_sound.gain,0.25);assert!(app.dirty);
            assert!(app.timeline_view.history.undo(&mut app.project));assert_eq!(app.project.card_sound.gain,0.25);
            let pos=app.hitboxes["card_sound_enabled"].center();
            for pressed in [true,false]{draw(&mut app,vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed,modifiers:egui::Modifiers::NONE}]);}
            assert!(!app.project.card_sound.enabled);
            assert_eq!(app.project.segments[0].asset_key,"keep-voice");assert_eq!(app.project.segments[0].audio.as_deref(),Some(std::path::Path::new("keep.wav")));
        }
    }
    #[test]
    fn sound_dropdown_pointer_sync_persistence_and_undo() {
        let ctx=egui::Context::default();ctx.style_mut(|s|s.animation_time=0.);
        let mut app=StudioApp::from_context(&ctx);app.capture_path=None;app.project=Project::default();
        let screen=Rect::from_min_size(Pos2::ZERO,Vec2::new(390.,720.));
        let draw=|app:&mut StudioApp,events|{let _=ctx.run(egui::RawInput{screen_rect:Some(screen),events,..Default::default()},|ctx|{egui::CentralPanel::default().show(ctx,|ui|{app.card_sound_settings_ui(ui);ui.separator();app.evidence_sound_settings_ui(ui);});});};
        let click=|app:&mut StudioApp,key:&str|{let pos=app.hitboxes[key].center();for pressed in [true,false]{draw(app,vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed,modifiers:egui::Modifiers::NONE}]);}};
        for _ in 0..3{draw(&mut app,vec![]);}
        click(&mut app,"card_sound_style");draw(&mut app,vec![]);click(&mut app,"card_sound_option_ting");
        assert_eq!(app.project.card_sound.style,"ting");assert_eq!(app.project.evidence_settings.sound_style,"whoosh");
        click(&mut app,"sfx_sync_style");assert!(app.project.card_sound.sync_style);assert_eq!(app.project.evidence_settings.sound_style,"ting");
        click(&mut app,"evidence_sound_style");draw(&mut app,vec![]);click(&mut app,"evidence_sound_option_pop");
        assert_eq!(app.project.card_sound.style,"pop");assert_eq!(app.project.evidence_settings.sound_style,"pop");
        let saved=serde_json::to_vec(&app.project).unwrap();let loaded:Project=serde_json::from_slice(&saved).unwrap();assert!(loaded.card_sound.sync_style);assert_eq!(loaded.card_sound.style,"pop");
        assert!(app.timeline_view.history.undo(&mut app.project));assert_eq!(app.project.card_sound.style,"ting");assert_eq!(app.project.evidence_settings.sound_style,"ting");
    }
    #[test]
    fn sound_toolbar_popup_fits_small_and_large_editor() {
        for (width,height) in [(1080.,720.),(1440.,900.)] {
            let ctx=egui::Context::default();ctx.style_mut(|s|s.animation_time=0.);
            let mut app=StudioApp::from_context(&ctx);app.capture_path=None;app.page=Page::Editor;app.setup_open=false;
            app.project=Project{segments:vec![Segment{id:"host".into(),kind:"host".into(),duration_ms:5000,..Default::default()}],..Default::default()};app.project.ensure_composition();
            let screen=Rect::from_min_size(Pos2::ZERO,Vec2::new(width,height));
            let draw=|app:&mut StudioApp,events|{let _=ctx.run(egui::RawInput{screen_rect:Some(screen),events,..Default::default()},|ctx|app.draw(ctx));};
            for _ in 0..3{draw(&mut app,vec![]);}
            let pos=app.hitboxes["tl_sfx"].center();for pressed in [true,false]{draw(&mut app,vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed,modifiers:egui::Modifiers::NONE}]);}
            for _ in 0..3{draw(&mut app,vec![]);}
            for key in ["sfx_sync_style","card_sound_style","card_sound_listen","card_sound_apply","evidence_sound_style","evidence_sound_listen","evidence_sound_apply"] {
                assert!(screen.contains_rect(app.hitboxes[key]),"{width}x{height} popup clips {key}: {:?}",app.hitboxes[key]);
            }
        }
    }
    #[test]
    fn static_layers_stay_visible_after_initial_texture_upload_during_playback() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = StudioApp::from_context(&ctx);
        app.player.stop();
        app.capture_path = None;
        let path = temp.path().join("still.png");
        image::RgbaImage::from_pixel(80, 60, image::Rgba([30, 150, 90, 255]))
            .save(&path)
            .unwrap();
        app.project = Project {
            root: temp.path().into(),
            sources: vec![Source {
                id: "s".into(),
                ..Default::default()
            }],
            segments: vec![Segment {
                id: "host".into(),
                kind: "host".into(),
                source_id: "s".into(),
                duration_ms: 1500,
                avatar: Some(path.clone()),
                evidence_items: vec![EvidenceItem {
                    source_id: "s".into(),
                    asset: Some("still.png".into()),
                    annotation_asset: Some("still.png".into()),
                    annotations: vec![Annotation::default()],
                    duration_ms: 1500,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        let bg = evidence::background_path(&app.project, &app.project.segments[0]);
        fs::create_dir_all(bg.parent().unwrap()).unwrap();
        fs::copy(&path, bg).unwrap();
        let mut second=app.project.segments[0].evidence_items[0].clone();second.id="second-static-card".into();
        app.project.segments[0].evidence_items.push(second);
        app.project.ensure_composition();
        app.player
            .composition(app.project.clone(), app.settings.clone(), 0, true);
        let clock = Instant::now();
        while clock.elapsed() < Duration::from_secs(3) && app.timeline_view.cursor < 700 {
            thread::sleep(Duration::from_millis(30));
            app.poll(&ctx);
        }
        assert!(app.timeline_view.cursor >= 700);
        let comp = app.project.composition_snapshot();
        for c in comp.clips.iter().filter(|c| !c.audio() && c.active(700)) {
            assert!(
                app.layers.textures.contains_key(&c.id),
                "Static texture {} disappeared",
                c.id
            );
            if c.motion.backdrop_ms.is_some(){assert!(app.layers.textures.contains_key(&format!("{}#blur",c.id)),"Blur texture must survive static-frame delivery");}
        }
        app.player.stop();
    }
    #[test]
    fn evidence_panel_wraps_native_controls_and_adds_cards_with_undo() {
        for (width, height) in [(1080., 720.), (1440., 900.)] {
            for portrait in [true, false] {
                for collapsed in [true, false] {
                    let temp = tempfile::tempdir().unwrap();
                    let ctx = egui::Context::default();
                    ctx.style_mut(|s| s.animation_time = 0.);
                    let mut app = StudioApp::from_context(&ctx);
                    app.player.stop();
                    app.capture_path = None;
                    app.page = Page::Editor;
                    app.setup_open = false;
                    app.project = Project {
                        root: temp.path().into(),
                        sources: vec![Source {
                            id: "s".into(),
                            duration_ms: 4000,
                            width: 320,
                            height: 180,
                            ..Default::default()
                        }],
                        segments: vec![Segment {
                            id: "host".into(),
                            source_id: "s".into(),
                            kind: "host".into(),
                            duration_ms: 4000,
                            narration: "Thử UI dẫn chứng".into(),
                            ..Default::default()
                        }],
                        ..Default::default()
                    };
                    app.project.output.portrait = portrait;
                    app.project.ensure_composition();
                    app.inspector = 3;
                    app.selected = 0;
                    app.editor_layout.inspector_open = true;
                    app.editor_layout.sidebar_collapsed = collapsed;
                    app.editor_layout.mode = if portrait {
                        editor_ui::PreviewLayout::PortraitRight
                    } else {
                        editor_ui::PreviewLayout::Standard
                    };
                    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height));
                    for _ in 0..3 {
                        let _ = ctx.run(
                            egui::RawInput {
                                screen_rect: Some(screen),
                                ..Default::default()
                            },
                            |ctx| app.draw(ctx),
                        );
                    }
                    for key in [
                        "inspector_tab_3",
                        "evidence_build_top",
                        "evidence_add_frame",
                        "evidence_add_stat",
                        "evidence_add_comparison",
                        "preview_seek",
                    ] {
                        assert!(
                            screen.contains_rect(app.hitboxes[key]),
                            "{width}x{height} portrait={portrait} {key}: {:?}",
                            app.hitboxes[key]
                        );
                    }
                    let button = app.hitboxes["evidence_add_frame"].center();
                    for pressed in [true, false] {
                        let _ = ctx.run(
                            egui::RawInput {
                                screen_rect: Some(screen),
                                events: vec![
                                    egui::Event::PointerMoved(button),
                                    egui::Event::PointerButton {
                                        pos: button,
                                        button: egui::PointerButton::Primary,
                                        pressed,
                                        modifiers: egui::Modifiers::NONE,
                                    },
                                ],
                                ..Default::default()
                            },
                            |ctx| app.draw(ctx),
                        );
                    }
                    assert_eq!(app.project.segments[0].evidence_items.len(), 1);
                    assert_eq!(app.project.segments[0].evidence_items[0].source_id, "s");
                    assert!(app.timeline_view.history.undo(&mut app.project));
                    assert!(app.project.segments[0].evidence_items.is_empty());
                    app.player.stop();
                }
            }
        }
    }
}
