//! Project setup lives in the editor; one immutable job owns the automatic run.
use super::*;
use crate::workflow::{JobState, Phase, Status};

fn muted(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).size(12.).color(MUTED));
}
fn section(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    summary: String,
    body: impl FnOnce(&mut egui::Ui),
) {
    let _card = egui::Frame::new()
        .fill(CARD.gamma_multiply(0.55))
        .stroke(Stroke::new(1., EDGE))
        .corner_radius(8)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().slider_width=(ui.available_width()*0.35).clamp(70.,150.);
            let _header = egui::CollapsingHeader::new(RichText::new(title).strong())
                .id_salt(id)
                .default_open(std::env::args().any(|arg|arg==format!("--setup-section={id}")))
                .show(ui, body);
            #[cfg(test)]
            ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new(("setup_header", id)), _header.header_response.rect));
            ui.add(egui::Label::new(RichText::new(&summary).size(12.).color(MUTED)).truncate())
                .on_hover_text(&summary);
        });
    #[cfg(test)]
    ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new(("setup_card", id)), _card.response.rect));
}
impl StudioApp {
    pub(super) fn original_audio_settings(&mut self,ui:&mut egui::Ui) {
        heading(ui,"ÂM THANH VIDEO GỐC");
        let o=&mut self.project.output;
        let enabled=ui.checkbox(&mut o.original_audio_enabled,"Bật tiếng gốc làm nền khi review");
        #[cfg(test)] self.hitboxes.insert("original_audio_enabled".into(),enabled.rect);
        let mut changed=enabled.changed();
        ui.add_enabled_ui(o.original_audio_enabled,|ui| {
            ui.label("Âm lượng nền");
            ui.spacing_mut().slider_width=(ui.available_width()-95.).clamp(60.,180.);
            let mut gain=o.original_audio_gain*100.;
            let response=ui.add(egui::Slider::new(&mut gain,0.0..=200.).suffix(" %"));
            #[cfg(test)] self.hitboxes.insert("original_audio_gain".into(),response.rect);
            if response.changed(){o.original_audio_gain=gain/100.;changed=true;}
        });
        muted(ui,"Áp dụng khi chiếu video nguồn cùng lời review. Hook / đoạn gốc xen kẽ dùng âm lượng riêng của cảnh.");
        if o.original_audio_enabled && self.project.timeline.tracks[crate::timeline::ORIGINAL].muted {
            ui.colored_label(egui::Color32::YELLOW,"Track Tiếng gốc đang mute; bật lại nút M trên timeline để nghe.");
        }
        if changed {
            self.project.ensure_composition();
            if self.preview_final {self.touch();} else {
                self.dirty=true;self.project.rendered=None;self.player.update_audio(&self.project);
            }
        }
    }
    pub(super) fn planned_sources(&self) -> anyhow::Result<Vec<Source>> {
        let mut sources: Vec<Source> = self
            .project
            .sources
            .iter()
            .filter(|src| src.url.is_empty())
            .cloned()
            .collect();
        let mut next = 1;
        for line in self
            .urls
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            let url = validate_youtube_url(line)?;
            if sources.iter().any(|src| src.url == url) {
                continue;
            }
            if let Some(old) = self.project.sources.iter().find(|src| src.url == url) {
                sources.push(old.clone());
            } else {
                while sources
                    .iter()
                    .chain(&self.project.sources)
                    .any(|src| src.id == format!("src_{next}"))
                {
                    next += 1;
                }
                sources.push(Source {
                    id: format!("src_{next}"),
                    url,
                    ..Default::default()
                });
                next += 1;
            }
        }
        Ok(sources)
    }
    pub(super) fn setup_editor(&mut self, ctx: &egui::Context) {
        let max_width = (ctx.available_rect().width() - 430.).clamp(260., 440.);
        let width = if self.project.output.portrait {
            ((ctx.available_rect().height() - 160.) * 9. / 16. + 24.)
                .clamp(280., max_width.max(280.))
        } else {
            max_width.min(if ctx.available_rect().width()<1000. {280.} else {360.})
        };
        let preview = egui::SidePanel::right("setup_preview")
            .default_width(width)
            .min_width(260.)
            .max_width(max_width.max(280.))
            .resizable(true)
            .frame(egui::Frame::new().fill(BG).inner_margin(12))
            .show(ctx, |ui| self.preview_panel(ui, true));
        #[cfg(test)]
        self.hitboxes
            .insert("preview_panel".into(), preview.response.rect);
        #[cfg(not(test))]
        let _ = preview;
        egui::TopBottomPanel::bottom("setup_timeline_strip")
            .frame(egui::Frame::new().fill(PANEL).inner_margin(8))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    muted(
                        ui,
                        format!(
                            "Kịch bản đã lưu • {} cảnh • {:.1} s",
                            self.project.segments.len(),
                            self.project.duration_ms() as f64 / 1000.
                        ),
                    );
                    if ui
                        .add_enabled(
                            !self.project.segments.is_empty(),
                            egui::Button::new("Mở biên tập"),
                        )
                        .clicked()
                    {
                        self.setup_open = false;
                    }
                });
            });
        egui::TopBottomPanel::bottom("setup_run_bar")
            .frame(egui::Frame::new().fill(PANEL).inner_margin(12))
            .show(ctx, |ui| self.setup_run_bar(ui));
        let before = serde_json::to_vec(&self.project).ok();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(14))
            .show(ctx, |ui| {
                let _form = egui::ScrollArea::vertical()
                    .id_salt("project_setup_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().slider_width =
                            (ui.available_width() * 0.35).clamp(90., 150.);
                        ui.spacing_mut().button_padding = Vec2::new(8., 5.);
                        ui.add_enabled_ui(!self.busy, |ui| {
                            field(ui, "Tên dự án", &mut self.project.title);
                            if ui.available_width() >= 540. {
                                ui.columns(2, |cols| {
                                    self.setup_script(&mut cols[0]);
                                    self.setup_image(&mut cols[1]);
                                });
                                ui.add_space(8.);
                                ui.columns(2, |cols| {
                                    self.setup_production(&mut cols[0]);
                                    self.setup_subtitles(&mut cols[1]);
                                });
                            } else {
                                self.setup_script(ui);
                                self.setup_image(ui);
                                self.setup_production(ui);
                                self.setup_subtitles(ui);
                            }
                            ui.add_space(8.);
                            self.setup_sources(ui);
                            let _advanced = egui::CollapsingHeader::new(
                                "Công cụ nâng cao • chạy riêng từng bước",
                            )
                            .id_salt("setup_advanced")
                            .show(ui, |ui| self.editor_actions(ui));
                            #[cfg(test)]
                            self.hitboxes
                                .insert("setup_advanced".into(), _advanced.header_response.rect);
                        });
                    });
                #[cfg(test)] self.hitboxes.insert("setup_form_visible".into(),_form.inner_rect);
            });
        if before != serde_json::to_vec(&self.project).ok() {
            self.touch();
        }
    }
    fn setup_script(&mut self, ui: &mut egui::Ui) {
        let r = &self.project.rules;
        let summary = format!(
            "{}–{} giây · {} · hook {} · {} đoạn gốc xen kẽ",
            r.min_seconds,
            r.max_seconds,
            crate::content_language::label(&r.language),
            if r.hook_enabled { "3–7 s" } else { "tắt" },
            if r.intercuts_enabled {r.intercut_count}else{0}
        );
        section(ui, "setup_script", "01 / Kịch bản", summary, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Độ dài");
                let response = ui.add(
                    egui::DragValue::new(&mut self.project.rules.min_seconds)
                        .range(1..=7200)
                        .suffix(" s"),
                );
                #[cfg(test)]
                self.hitboxes
                    .insert("setup_min_duration".into(), response.rect);
                #[cfg(not(test))]
                let _ = response;
                ui.label("đến");
                ui.add(
                    egui::DragValue::new(&mut self.project.rules.max_seconds)
                        .range(1..=7200)
                        .suffix(" s"),
                );
            });
            let previous = self.project.rules.language.clone();
            let response = crate::content_language::selector(ui, &mut self.project.rules.language);
            #[cfg(test)] self.hitboxes.insert("setup_language".into(), response.rect);
            #[cfg(not(test))] let _ = response;
            if previous != self.project.rules.language && !self.project.segments.is_empty() {
                self.notice = "Đã đổi ngôn ngữ yêu cầu. Tạo lại kịch bản để áp dụng; cảnh đã khóa được giữ nguyên.".into();
            }
            if !self.project.segments.is_empty() {
                muted(ui, "Đổi ngôn ngữ áp dụng khi tạo lại kịch bản; không tự dịch cảnh đã có hoặc cảnh khóa.");
            }
            if crate::content_language::prompt_conflict(&self.project.rules.language, &self.project.rules.prompt) {
                ui.colored_label(egui::Color32::YELLOW, "Prompt có yêu cầu ngôn ngữ khác. AI sẽ ưu tiên Ngôn ngữ lời review đã chọn.");
            }
            ui.label("Prompt / phong cách / yêu cầu riêng");
            ui.add(
                egui::TextEdit::multiline(&mut self.project.rules.prompt)
                    .desired_width(f32::INFINITY)
                    .desired_rows(5),
            );
            if ui.small_button("Dùng prompt review mẫu").clicked() {
                self.project.rules.prompt = DEFAULT_REACTION_PROMPT.into();
            }
            prompt_duration_hint(ui, &self.project.rules);
            hook_settings(ui, &mut self.project.rules);
            egui::CollapsingHeader::new("Cảnh ưu tiên / cảnh loại bỏ").show(ui, |ui| {
                field(ui, "Ưu tiên lấy cảnh", &mut self.project.rules.include);
                field(ui, "Loại bỏ cảnh", &mut self.project.rules.exclude);
                muted(
                    ui,
                    "Quy tắc này được gửi cho AI. Vùng timestamp được đặt tại Video nguồn.",
                );
            });
        });
    }
    fn setup_image(&mut self, ui: &mut egui::Ui) {
        let summary = format!(
            "{} · {}p · {}",
            if self.project.output.portrait {
                "9:16"
            } else {
                "16:9"
            },
            self.project.output.resolution,
            if self.project.output.blur_background {
                "nền video mờ"
            } else {
                "nền theo mẫu"
            }
        );
        section(
            ui,
            "setup_image",
            "02 / Hình ảnh đầu ra",
            summary,
            |ui| {self.output_settings(ui);ui.separator();self.evidence_settings_ui(ui);},
        );
    }
    fn setup_subtitles(&mut self, ui: &mut egui::Ui) {
        let summary = if self.project.subtitles.enabled {
            format!(
                "{} · cỡ {:.0} · {}",
                self.project.subtitles.font,
                self.project.subtitles.size,
                if self.project.subtitles.karaoke {
                    "highlight theo từ"
                } else {
                    "chữ thường"
                }
            )
        } else {
            "Đang tắt phụ đề".into()
        };
        section(ui, "setup_subtitles", "04 / Phụ đề", summary, |ui| {
            self.subtitle_settings(ui);
            let s = &self.project.subtitles;
            egui::Frame::new()
                .fill(Color32::from_rgb(8, 12, 20))
                .corner_radius(6)
                .inner_margin(10)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new("Khoảnh khắc")
                                .size(s.size.clamp(16., 28.))
                                .color(Color32::from_rgb(
                                    s.highlight[0],
                                    s.highlight[1],
                                    s.highlight[2],
                                )),
                        );
                        ui.label(
                            RichText::new("bất ngờ trong video")
                                .size(s.size.clamp(16., 28.))
                                .color(Color32::from_rgb(s.color[0], s.color[1], s.color[2])),
                        );
                    });
                });
            muted(
                ui,
                "Mẫu màu minh họa; preview dựng cảnh dùng đúng font, cỡ và viền đã chọn.",
            );
        });
    }
    fn setup_production(&mut self, ui: &mut egui::Ui) {
        let p = self
            .project
            .production
            .get_or_insert_with(|| ProductionSettings::from_settings(&self.settings));
        let summary = format!(
            "{} · {} · {}",
            if p.voice_reference_enabled {
                "Giọng tham chiếu"
            } else {
                p.voice.as_str()
            },
            if !p.avatar_enabled {
                "Tắt nhân vật"
            } else if p.avatar_provider == "cartoon2d" {
                "2D local"
            } else {
                "HeyGen"
            },
            if self.project.output.original_audio_enabled {format!("Tiếng gốc {:.0}%",self.project.output.original_audio_gain*100.)}else{"Tắt nền tiếng gốc".into()}
        );
        let assets = self
            .projects_root
            .parent()
            .unwrap_or(&self.projects_root)
            .join("assets");
        let mut note = None;
        section(
            ui,
            "setup_production",
            "03 / Giọng, nhân vật & âm thanh",
            summary,
            |ui| {
                self.original_audio_settings(ui);
                ui.separator();
                let p = self.project.production.as_mut().unwrap();
                field(ui, "Giọng preset VieNeu", &mut p.voice);
                ui.checkbox(
                    &mut p.voice_reference_enabled,
                    "Dùng audio giọng tham chiếu",
                );
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Chọn audio…").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Audio", &["wav", "mp3", "m4a", "flac", "ogg"])
                            .pick_file()
                        {
                            match crate::input_assets::reference_audio(
                                &self.settings,
                                &path,
                                &assets.join("voices"),
                            ) {
                                Ok(path) => {
                                    p.voice_reference_path = path.to_string_lossy().into_owned();
                                    p.voice_reference_enabled = true;
                                }
                                Err(e) => note = Some(format!("Không nhập được audio: {e:#}")),
                            }
                        }
                    }
                    if ui
                        .add_enabled(
                            !p.voice_reference_path.is_empty(),
                            egui::Button::new("Bỏ audio"),
                        )
                        .clicked()
                    {
                        p.voice_reference_path.clear();
                        p.voice_reference_enabled = false;
                    }
                });
                if !p.voice_reference_path.is_empty() {
                    ui.label(
                        egui::RichText::new(
                            if std::path::Path::new(&p.voice_reference_path).is_file() {
                                "Đã chọn audio tham chiếu"
                            } else {
                                "Audio không còn tồn tại"
                            },
                        )
                        .color(TEAL),
                    )
                    .on_hover_text(&p.voice_reference_path);
                }
                ui.add_enabled_ui(p.voice_reference_enabled, |ui| {
                    ui.checkbox(&mut p.voice_reference_denoise, "Lọc nhiễu giọng mẫu");
                });
                muted(
                    ui,
                    "VieNeu v3 dùng tối đa 8 giây đầu; không cần nhập lời mẫu.",
                );
                ui.separator();
                ui.checkbox(&mut p.avatar_enabled, "Tạo nhân vật review / reaction");
                ui.add_enabled_ui(p.avatar_enabled, |ui| {
                    egui::ComboBox::from_id_salt("project_avatar_provider")
                        .width(ui.available_width() - 4.)
                        .truncate()
                        .selected_text(if p.avatar_provider == "cartoon2d" {
                            "2D local • miễn phí API"
                        } else {
                            "HeyGen Avatar IV • tính phí"
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut p.avatar_provider,
                                "cartoon2d".into(),
                                "2D local • Rhubarb",
                            );
                            ui.selectable_value(
                                &mut p.avatar_provider,
                                "heygen".into(),
                                "HeyGen Avatar IV • tính phí",
                            );
                        });
                    let preview = if p.avatar_provider == "cartoon2d" {
                        egui::ComboBox::from_id_salt("project_character")
                            .width(ui.available_width() - 4.)
                            .truncate()
                            .selected_text(match p.cartoon_character.as_str() {
                                "analyst" => "Analyst • kính / áo teal",
                                "studio" => "Studio • tai nghe",
                                "portrait" => "Bán thân • hoodie xám",
                                _ => "Chọn nhân vật",
                            })
                            .show_ui(ui, |ui| {
                                for (value, label) in [
                                    ("portrait", "Bán thân • hoodie xám"),
                                    ("studio", "Studio • tai nghe"),
                                    ("analyst", "Analyst • kính / áo teal"),
                                ] {
                                    ui.selectable_value(
                                        &mut p.cartoon_character,
                                        value.into(),
                                        label,
                                    );
                                }
                            });
                        muted(ui, "Nhép miệng và cử chỉ chạy trên máy, dùng voice VieNeu.");
                        Some(worker_path(&format!("cartoon-{}.png", p.cartoon_character)))
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Chọn ảnh nhân vật…").clicked() {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("PNG / JPEG", &["png", "jpg", "jpeg"])
                                    .pick_file()
                                {
                                    match crate::input_assets::avatar_image(
                                        &path,
                                        &assets.join("characters"),
                                    ) {
                                        Ok(path) => {
                                            p.avatar_image_path =
                                                path.to_string_lossy().into_owned()
                                        }
                                        Err(e) => {
                                            note = Some(format!("Không nhập được ảnh: {e:#}"))
                                        }
                                    }
                                }
                            }
                            if ui
                                .add_enabled(
                                    !p.avatar_image_path.is_empty(),
                                    egui::Button::new("Bỏ ảnh"),
                                )
                                .clicked()
                            {
                                p.avatar_image_path.clear();
                            }
                        });
                        field(ui, "Hoặc avatar ID", &mut p.avatar_id);
                        field(ui, "Hoặc URL ảnh", &mut p.avatar_image_url);
                        if p.avatar_image_path.is_empty() {
                            None
                        } else {
                            Some(PathBuf::from(&p.avatar_image_path))
                        }
                    };
                    if let Some(path) = preview {
                        let key = path.to_string_lossy().into_owned();
                        if self.avatar_preview.as_ref().map(|(p, _)| p.as_str())
                            != Some(key.as_str())
                        {
                            self.avatar_preview = None;
                            if let Ok(image) = image::open(path) {
                                let image = image.thumbnail(130, 110).to_rgba8();
                                self.avatar_preview = Some((
                                    key,
                                    ui.ctx().load_texture(
                                        "project-character",
                                        egui::ColorImage::from_rgba_unmultiplied(
                                            [image.width() as usize, image.height() as usize],
                                            image.as_raw(),
                                        ),
                                        Default::default(),
                                    ),
                                ));
                            }
                        }
                        if let Some((_, texture)) = &self.avatar_preview {
                            ui.add(egui::Image::new(texture).max_size(Vec2::new(130., 110.)));
                        }
                    }
                });
                ui.checkbox(&mut p.align_subtitles, "Tự căn từ phụ đề bằng Whisper");
                muted(
                    ui,
                    "Các lựa chọn được lưu riêng trong dự án và kế thừa cho dự án mới.",
                );
                if ui
                    .small_button("Mở cài đặt kết nối / API / công cụ")
                    .clicked()
                {
                    self.page = Page::Settings;
                }
            },
        );
        if let Some(note) = note {
            self.message(note, true);
        }
    }
    fn setup_sources(&mut self, ui: &mut egui::Ui) {
        panel().inner_margin(12).show(ui,|ui| {
            heading(ui,"05 / VIDEO NGUỒN");
            self.batch_mode_ui(ui);
            let edit=ui.add(egui::TextEdit::multiline(&mut self.urls).desired_width(f32::INFINITY).desired_rows(2).hint_text("URL YouTube, mỗi dòng một video"));
            #[cfg(test)] self.hitboxes.insert("setup_urls".into(),edit.rect);
            if edit.changed(){self.project.input_urls=self.urls.clone();}
            if self.batch_view.prefs.enabled{self.batch_options_ui(ui);self.batch_sources_ui(ui);return;}
            if ui.button("+ Video từ máy…").clicked(){self.import_dialog();}
            match self.planned_sources() {
                Err(e)=>{ui.colored_label(Color32::LIGHT_RED,format!("{e:#}"));}
                Ok(sources)=>{
                    let mut remove=None;
                    egui::ScrollArea::vertical().id_salt("setup_source_list").max_height(120.).show(ui,|ui| {
                        for source in &sources {
                            ui.horizontal(|ui| {
                                if ui.small_button("Xóa").on_hover_text("Bỏ khỏi danh sách nguồn; giữ file trên máy").clicked(){remove=Some(source.clone());}
                                let title=if source.title.is_empty(){source.url.as_str()}else{source.title.as_str()};
                                ui.add(egui::Label::new(format!("{} · {} · {}",source.id,if source.path.is_file(){"đã tải"}else{"chờ tải"},title)).truncate()).on_hover_text(title);
                            });
                        }
                    });
                    if let Some(source)=remove {
                        if source.url.is_empty(){self.project.sources.retain(|s|s.id!=source.id);self.project.segments.retain(|s|s.source_id!=source.id);}
                        else{self.urls=self.urls.lines().filter(|line|validate_youtube_url(line).ok().as_deref()!=Some(source.url.as_str())).collect::<Vec<_>>().join("\n");self.project.input_urls=self.urls.clone();}
                    }
                    egui::CollapsingHeader::new("Vùng bắt buộc giữ / loại bỏ (timestamp)").id_salt("setup_ranges").show(ui,|ui|{
                        if !sources.iter().any(|src|src.id==self.range_source){self.range_source=sources.first().map(|s|s.id.clone()).unwrap_or_default();}
                        muted(ui,"Mốc thời gian tính theo video nguồn. Chọn nguồn trước khi thêm vùng.");
                        egui::ComboBox::from_id_salt("range_source_picker").selected_text(&self.range_source).show_ui(ui,|ui|{for source in &sources{ui.selectable_value(&mut self.range_source,source.id.clone(),&source.id);}});
                        self.range_rules(ui);
                    });
                }
            }
            if self.settings.provider=="antigravity" && ui.small_button("Thử đọc URL trực tiếp lại ở lần chạy tới").clicked() {
                let _=std::fs::remove_file(self.project.root.join("analysis/url/capability.json"));
            }
            muted(ui,if self.settings.provider=="antigravity"{"Antigravity: thử URL song song tải; tự động dùng transcript + ảnh nếu URL không đọc được."}else{"Gemini API: phân tích URL song song với tải video."});
        });
    }
    fn setup_run_bar(&mut self, ui: &mut egui::Ui) {
        if self.batch_view.prefs.enabled{self.batch_run_bar(ui);return;}
        let production = self.project.production.as_ref();
        muted(
            ui,
            format!(
                "{}–{} s · {} · {} · {}",
                self.project.rules.min_seconds,
                self.project.rules.max_seconds,
                if self.project.output.portrait {
                    "9:16"
                } else {
                    "16:9"
                },
                production
                    .map(|p| if p.voice_reference_enabled {
                        "Giọng tham chiếu"
                    } else {
                        p.voice.as_str()
                    })
                    .unwrap_or(&self.settings.voice),
                production
                    .map(|p| if !p.avatar_enabled {
                        "Tắt nhân vật"
                    } else if p.avatar_provider == "cartoon2d" {
                        "2D local"
                    } else {
                        "HeyGen"
                    })
                    .unwrap_or("Nhân vật")
            ),
        );
        if let Some(job) = &self.workflow_state {
            let done = |step| {
                job.status==Status::Complete || match step {
                    0 => job.phase!=Phase::Prepare || self.lanes.get("Download").is_some_and(|(_,done)|*done),
                    1 => job.phase!=Phase::Prepare,
                    2 => matches!(job.phase,Phase::Captions|Phase::Ready) || (job.phase==Phase::Assets && self.lanes.get("Audio").is_some_and(|(_,done)|*done)),
                    3 => matches!(job.phase,Phase::Captions|Phase::Ready),
                    4 => job.phase==Phase::Ready,
                    _ => false,
                }
            };
            ui.horizontal_wrapped(|ui| {
                for (i, title) in ["Tải nguồn", "Kịch bản", "Giọng", "Nhân vật", "Phụ đề", "Sẵn sàng"]
                    .into_iter()
                    .enumerate()
                {
                    chip(
                        ui,
                        title,
                        if job.status == Status::Complete || done(i) {
                            TEAL
                        } else {
                            MUTED
                        },
                    );
                }
            });
            let relevant_lanes: &[&str] = match job.phase {
                Phase::Prepare => &["Analysis", "Gemini", "Download"],
                Phase::Assets => &["Avatar", "Audio"],
                Phase::Captions => &["Subtitle"],
                Phase::Ready => &[],
            };
            let message = if self.busy {
                relevant_lanes.iter().filter_map(|lane|self.lanes.get(*lane))
                    .find(|(_, done)| !*done)
                    .map(|(message, _)| message.as_str())
                    .unwrap_or(&job.message)
            } else {
                &job.message
            };
            if !self.is_error || self.busy {
                ui.add(egui::Label::new(RichText::new(message).size(12.)).wrap());
            }
            if job.phase==Phase::Prepare && job.status==Status::Failed && !self.project.segments.is_empty() {
                muted(ui,"Lần phân tích mới chưa được áp dụng. Timeline bên dưới là kịch bản đã lưu trước đó.");
            }
        }
        if self.is_error && !self.notice.is_empty() {
            let summary=self.notice.chars().take(190).collect::<String>();
            let _error=ui.add(egui::Label::new(RichText::new(summary).color(Color32::LIGHT_RED).size(12.)).wrap());
            #[cfg(test)] self.hitboxes.insert("setup_error".into(),_error.rect);
            egui::CollapsingHeader::new("Chi tiết lỗi / vị trí cần sửa").show(ui,|ui|{
                egui::ScrollArea::vertical().max_height(130.).show(ui,|ui|{ui.label(&self.notice);});
            });
        }
        if !self.project.script_warnings.is_empty() {
            egui::CollapsingHeader::new(format!("{} cảnh báo dẫn chứng",self.project.script_warnings.len()))
                .show(ui,|ui|{for warning in &self.project.script_warnings {ui.label(RichText::new(warning).color(Color32::YELLOW));}});
        }
        ui.horizontal_wrapped(|ui| {
            if self.busy {
                ui.spinner();
                if ui
                    .button(if self.cancel.load(Ordering::Relaxed) {
                        "Đang hủy…"
                    } else {
                        "Hủy tác vụ"
                    })
                    .clicked()
                {
                    self.cancel.store(true, Ordering::Relaxed);
                }
            } else {
                let available = self.planned_sources().is_ok_and(|s| !s.is_empty());
                let response = ui.add_enabled(
                    available,
                    egui::Button::new(
                        RichText::new("Tạo review tự động")
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(BLUE)
                    .min_size(Vec2::new(190., 36.)),
                );
                #[cfg(test)]
                self.hitboxes.insert("setup_run".into(), response.rect);
                if response.clicked() {
                    self.launch(Task::Automatic { resume: false });
                }
                if self
                    .workflow_state
                    .as_ref()
                    .is_some_and(JobState::can_resume)
                {
                    let response = ui.add(egui::Button::new("Tiếp tục / sửa phần lỗi").min_size(Vec2::new(160.,36.)));
                    #[cfg(test)]
                    self.hitboxes.insert("setup_resume".into(), response.rect);
                    if response.clicked() {
                        self.launch(Task::Automatic { resume: true });
                    }
                }
            }
            let log=ui.add(egui::Button::new("Xem nhật ký").min_size(Vec2::new(105.,36.)));
            #[cfg(test)] self.hitboxes.insert("setup_logs".into(),log.rect);
            if log.clicked() {self.page=Page::Queue;}
        });
        muted(
            ui,
            "Phân tích → kiểm tra / sửa kịch bản → giọng → nhân vật → phụ đề → timeline.",
        );
    }
}
