//! Native editor docking. Layout preferences are independent of project media.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum PreviewLayout {
    #[default]
    Auto,
    Standard,
    PortraitRight,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct EditorLayout {
    pub mode: PreviewLayout,
    pub inspector_open: bool,
    pub sidebar_collapsed: bool,
}
impl Default for EditorLayout {
    fn default() -> Self {
        Self {
            mode: PreviewLayout::Auto,
            inspector_open: true,
            sidebar_collapsed: false,
        }
    }
}
impl EditorLayout {
    pub fn load(path: &std::path::Path) -> Self {
        fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    pub fn portrait_right(&self, portrait: bool) -> bool {
        self.mode == PreviewLayout::PortraitRight || (self.mode == PreviewLayout::Auto && portrait)
    }
    pub fn apply_args(&mut self, args: impl Iterator<Item = String>) {
        for arg in args {
            match arg.as_str() {
                "--editor-layout=standard" => self.mode = PreviewLayout::Standard,
                "--editor-layout=portrait" => self.mode = PreviewLayout::PortraitRight,
                "--editor-layout=auto" => self.mode = PreviewLayout::Auto,
                "--inspector=collapsed" => self.inspector_open = false,
                "--sidebar=collapsed" => self.sidebar_collapsed = true,
                "--sidebar=expanded" => self.sidebar_collapsed = false,
                _ => {}
            }
        }
    }
}

impl StudioApp {
    pub(super) fn remember_editor_layout(&mut self) {
        if let Some(root) = self.projects_root.parent() {
            if let Err(e) = write_json(&root.join("editor-layout.json"), &self.editor_layout) {
                self.message(format!("Không lưu được bố cục: {e:#}"), true);
            }
        }
    }
    pub(super) fn editor_layout_toolbar(&mut self, ctx: &egui::Context) {
        let before = self.editor_layout.clone();
        egui::TopBottomPanel::top("editor_layout_toolbar")
            .frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(12, 5)))
            .show(ctx, |ui| {
                ui.spacing_mut().button_padding = Vec2::new(9., 5.);
                ui.horizontal(|ui| {
                    for (setup,title,key) in [(true,"Thiết lập dự án","setup_tab"),(false,"Biên tập","edit_tab")] {
                        let response=ui.add_enabled(!self.auto_running || setup, egui::SelectableLabel::new(self.setup_open==setup,title));
                        #[cfg(test)] self.hitboxes.insert(key.into(),response.rect);
                        #[cfg(not(test))] let _=key;
                        if response.clicked(){self.setup_open=setup;}
                    }
                });
                if self.setup_open { return; }
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Bố cục").color(MUTED).size(12.));
                    for (mode, text, key) in [
                        (PreviewLayout::Auto, "Tự động", "auto"),
                        (PreviewLayout::Standard, "Ngang", "standard"),
                        (PreviewLayout::PortraitRight, "Dọc bên phải", "portrait"),
                    ] {
                        let response = ui.selectable_label(self.editor_layout.mode == mode, text)
                            .on_hover_text("Sắp xếp không gian làm việc; tỉ lệ và kích thước video xuất giữ theo mục Khung hình.");
                        #[cfg(test)]
                        self.hitboxes.insert(format!("layout_{key}"), response.rect);
                        #[cfg(not(test))]
                        let _ = key;
                        if response.clicked() { self.editor_layout.mode = mode; }
                    }
                    ui.separator();
                    let response = ui.checkbox(&mut self.editor_layout.inspector_open, "Bảng chỉnh sửa");
                    #[cfg(test)]
                    self.hitboxes.insert("layout_inspector_toggle".into(), response.rect);
                    #[cfg(not(test))]
                    let _ = response;
                    ui.label(RichText::new("Tự động: 9:16 → preview bên phải").color(MUTED).size(11.));
                });
            });
        if before != self.editor_layout {
            self.remember_editor_layout();
        }
    }
    pub(super) fn editor(&mut self, ctx: &egui::Context) {
        if self.setup_open { self.setup_editor(ctx); return; }
        let portrait = self
            .editor_layout
            .portrait_right(self.project.output.portrait);
        if portrait {
            // Dock before the timeline: the preview keeps the full work-area
            // height while the timeline occupies the remaining column.
            let height = ctx.available_rect().height();
            let max_width = (ctx.available_rect().width() - 400.).clamp(280., 460.);
            let width = ((height - 160.) * 9. / 16. + 24.).clamp(280., max_width);
            let panel = egui::SidePanel::right("portrait_preview")
                .default_width(width)
                .min_width(260.)
                .max_width(max_width)
                .resizable(true)
                .frame(egui::Frame::new().fill(BG).inner_margin(12))
                .show(ctx, |ui| self.preview_panel(ui, true));
            #[cfg(test)]
            self.hitboxes
                .insert("preview_panel".into(), panel.response.rect);
            #[cfg(not(test))]
            let _ = panel;
        }
        egui::TopBottomPanel::top("editor_tools")
            .frame(
                egui::Frame::new()
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(12, 5)),
            )
            .show(ctx, |ui| {
                let workflow =
                    egui::CollapsingHeader::new("Công cụ nâng cao • chạy riêng từng bước")
                        .id_salt("editor_workflow")
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(160.)
                                .show(ui, |ui| self.editor_actions(ui));
                        });
                #[cfg(test)]
                self.hitboxes
                    .insert("workflow_header".into(), workflow.header_response.rect);
                #[cfg(not(test))]
                let _ = workflow;
            });
        let available = ctx.available_rect().height();
        let timeline_min = if portrait { 220. } else { 144. };
        // Standard docking shares its vertical space with the preview. Reserve
        // room for the canvas as well as its header and transport controls.
        let timeline_max = if portrait {
            (available - 235.).clamp(timeline_min, 460.)
        } else {
            (available * 0.4).min(available - 330.).clamp(timeline_min, 460.)
        };
        egui::TopBottomPanel::bottom("timeline")
            .default_height((if portrait&&self.inspector==3{230.0_f32}else{350.0_f32}).min(timeline_max))
            .min_height(timeline_min)
            .max_height(timeline_max)
            .resizable(true)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(10))
            .show(ctx, |ui| {
                ui.spacing_mut().button_padding = Vec2::new(8., 4.);
                ui.spacing_mut().item_spacing = Vec2::new(6., 5.);
                ui.spacing_mut().slider_width = 70.;
                self.timeline(ui);
            });
        if portrait {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(PANEL).inner_margin(12))
                .show(ctx, |ui| {
                    if self.editor_layout.inspector_open {
                        self.inspector_panel(ui);
                    } else {
                        self.scene_outline(ui);
                    }
                });
        } else {
            let width = if self.editor_layout.inspector_open {
                308.
            } else {
                64.
            };
            egui::SidePanel::right("inspector")
                .exact_width(width)
                .resizable(false)
                .frame(egui::Frame::new().fill(PANEL).inner_margin(10))
                .show(ctx, |ui| {
                    if self.editor_layout.inspector_open {
                        self.inspector_panel(ui);
                    } else {
                        ui.spacing_mut().button_padding = Vec2::new(4., 5.);
                        if ui.button("Mở").on_hover_text("Mở bảng chỉnh sửa").clicked() {
                            self.editor_layout.inspector_open = true;
                            self.remember_editor_layout();
                        }
                        for (i, title) in ["Cảnh", "Khung", "Sub", "Ảnh"].iter().enumerate() {
                            let button = ui.add(
                                egui::Button::new(RichText::new(*title).size(11.))
                                    .min_size(Vec2::new(40., 27.)),
                            );
                            #[cfg(test)]
                            self.hitboxes
                                .insert(format!("inspector_rail_{i}"), button.rect);
                            if button.clicked() {
                                self.inspector = i;
                                self.editor_layout.inspector_open = true;
                                self.remember_editor_layout();
                            }
                        }
                    }
                });
            let panel = egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(BG).inner_margin(16))
                .show(ctx, |ui| self.preview_panel(ui, false));
            #[cfg(test)]
            self.hitboxes
                .insert("preview_panel".into(), panel.response.rect);
            #[cfg(not(test))]
            let _ = panel;
        }
    }
    fn inspector_panel(&mut self, ui: &mut egui::Ui) {
        ui.scope(|ui| {
            ui.spacing_mut().button_padding = Vec2::new(7., 5.);
            ui.spacing_mut().item_spacing.x = 5.;
            ui.horizontal_wrapped(|ui| {
                for (i, label) in ["Cảnh", "Khung", "Sub", "Dẫn chứng"].iter().enumerate() {
                    let response = ui.selectable_label(self.inspector == i, *label);
                    #[cfg(test)]
                    self.hitboxes
                        .insert(format!("inspector_tab_{i}"), response.rect);
                    if response.clicked() {
                        self.inspector = i;
                    }
                }
                let hide = ui
                    .button("Ẩn")
                    .on_hover_text("Thu gọn bảng chỉnh sửa; mở lại từ thanh Bố cục.");
                #[cfg(test)]
                self.hitboxes.insert("inspector_collapse".into(), hide.rect);
                if hide.clicked() {
                    self.editor_layout.inspector_open = false;
                    self.remember_editor_layout();
                }
            });
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("inspector_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_enabled_ui(!self.busy, |ui| match self.inspector {
                    0 => {self.layer_inspector(ui);self.segment_inspector(ui);},
                    1 => self.output_settings(ui),
                    2 => self.subtitle_settings(ui),
                    _=>self.evidence_inspector(ui),
                });
            });
    }
    fn scene_outline(&mut self, ui: &mut egui::Ui) {
        heading(ui, "DANH SÁCH CẢNH");
        ui.label(
            RichText::new(
                "Chọn cảnh tại đây hoặc trên timeline. Bật Bảng chỉnh sửa để sửa chi tiết.",
            )
            .color(MUTED)
            .size(12.),
        );
        let mut selection = None;
        egui::ScrollArea::vertical()
            .id_salt("scene_outline")
            .show(ui, |ui| {
                let starts = crate::timeline::starts(&self.project);
                for (i, scene) in self.project.segments.iter().enumerate() {
                    let text = if scene.narration.is_empty() {
                        &scene.evidence
                    } else {
                        &scene.narration
                    };
                    let summary: String = text.chars().take(90).collect();
                    let response = ui.selectable_label(
                        self.selected == i,
                        format!(
                            "{:02}  {}  •  {}  •  {:.1}s\n{}",
                            i + 1,
                            scene.kind.to_uppercase(),
                            seconds(starts[i]),
                            scene.duration_ms as f32 / 1000.,
                            summary
                        ),
                    );
                    if response.clicked() {
                        selection = Some(starts[i]);
                    }
                }
            });
        if let Some(ms) = selection {
            self.seek_timeline(ms);
        }
    }
    pub(super) fn preview_panel(&mut self, ui: &mut egui::Ui, compact: bool) {
        ui.spacing_mut().button_padding = Vec2::new(8., 5.);
        ui.horizontal(|ui| {
            chip(ui, "PROGRAM", TEAL);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                chip(
                    ui,
                    if self.project.output.portrait {
                        "9:16"
                    } else {
                        "16:9"
                    },
                    BLUE,
                );
            });
        });
        ui.label(
            RichText::new(if self.preview_final {
                "Bản render chính xác"
            } else {
                "Layer trực tiếp • không dựng MP4 preview"
            })
            .color(MUTED)
            .size(11.),
        );
        let before_quality=self.settings.preview_long_edge;
        ui.horizontal(|ui|{
            ui.label(RichText::new("Preview").size(11.).color(MUTED));
            egui::ComboBox::from_id_salt("preview_quality").width(90.).selected_text(format!("{} px",self.settings.preview_long_edge)).show_ui(ui,|ui|{
                for n in [360,540,720]{ui.selectable_value(&mut self.settings.preview_long_edge,n,format!("{n} px"));}
            });
            ui.label(RichText::new("24 fps").size(10.).color(MUTED));
        });
        if before_quality!=self.settings.preview_long_edge {
            let playing=self.player.playing;self.request_frame();
            if playing{self.play_timeline();}
            if let Some(root)=self.projects_root.parent(){let _=write_json(&root.join("settings.json"),&self.settings);}
        }
        let reserve = if compact { 113. } else { 80. };
        let size = Vec2::new(
            ui.available_width(),
            (ui.available_height() - reserve).max(50.),
        );
        self.preview(ui, size);
        ui.add_space(6.);
        ui.horizontal_wrapped(|ui| {
            let play = ui.button(if self.player.playing {
                "Ⅱ Dừng"
            } else {
                "▶ Phát layer"
            });
            #[cfg(test)]
            self.hitboxes
                .insert("preview_source_play".into(), play.rect);
            if play.clicked() {self.play_timeline();}
            ui.label(
                RichText::new(seconds(self.playhead))
                    .monospace()
                    .color(TEAL)
                    .size(12.),
            );
            if ui
                .add_enabled(
                    self.project.rendered.is_some(),
                    egui::Button::new(if compact { "Output" } else { "Xem output" }),
                )
                .clicked()
            {
                self.preview_final = true;
                self.timeline_view.full_preview = true;
                self.preview_scene = None;
                self.playhead = 0;
                self.request_frame();
            }
            if !compact {
                self.preview_scene_button(ui);
            }
        });
        if compact {
            ui.horizontal(|ui| {
                self.preview_scene_button(ui);
                if ui
                    .add_enabled(
                        !self.busy && !self.project.segments.is_empty(),
                        egui::Button::new("▶ Timeline"),
                    )
                    .clicked()
                {
                    self.play_timeline();
                }
            });
        }
        let (min,max)=(0,self.project.composition_duration().max(1));
        let mut seek = self.playhead as f64 / 1000.;
        let response = ui.add(
            egui::Slider::new(
                &mut seek,
                min as f64 / 1000. ..=max.max(min + 1) as f64 / 1000.,
            )
            .show_value(false)
            .text("Seek"),
        );
        #[cfg(test)]
        self.hitboxes.insert("preview_seek".into(), response.rect);
        if response.drag_stopped() || response.clicked() {
            self.seek_timeline((seek*1000.) as u64);
        }
    }
    fn preview_scene_button(&mut self, ui: &mut egui::Ui) {
        if ui
            .add_enabled(
                !self.busy && !self.project.segments.is_empty(),
                egui::Button::new("Dựng cảnh"),
            )
            .on_hover_text("Dựng preview chính xác của cảnh đang chọn")
            .clicked()
        {
            self.launch_preview_selected();
        }
    }
}
