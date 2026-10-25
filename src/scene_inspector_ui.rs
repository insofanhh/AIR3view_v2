//! Responsive scene properties: primary editing first, optional detail below.
use super::*;

#[derive(Default)]
struct SceneGeometry {
    #[cfg(test)]
    rects: BTreeMap<String, Rect>,
}

impl SceneGeometry {
    fn record(&mut self, key: &str, rect: Rect) {
        #[cfg(test)]
        self.rects.insert(key.into(), rect);
        #[cfg(not(test))]
        let _ = (key, rect);
    }
}

fn scene_card() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD.gamma_multiply(0.55))
        .stroke(Stroke::new(1., EDGE))
        .corner_radius(8)
        .inner_margin(12)
}
fn label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(12.).color(MUTED));
}
fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(13.).strong());
}
fn kind_label(value: &str) -> &str {
    match value {
        "host" => "Nhân vật toàn màn hình",
        "review" => "Review phủ video",
        _ => "Video gốc + tiếng gốc",
    }
}
fn gesture_label(value: &str) -> &str {
    match value {
        "none" => "Nghỉ / không cử chỉ",
        "open_palms" => "Mở lòng bàn tay",
        "point" => "Chỉ về video",
        "beat" => "Nhấn luận điểm",
        "surprised" => "Nâng tay bất ngờ",
        _ => "Tự động theo lời",
    }
}
fn motion_label(value: &str) -> &str {
    match value {
        "push_in" => "Zoom nhẹ",
        "reaction_shake" => "Rung bất ngờ",
        _ => "Tĩnh",
    }
}

fn scene_actions(ui: &mut egui::Ui, enabled: bool, geo: &mut SceneGeometry) -> u8 {
    let mut action = 0;
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            for (id, title, hint) in [
                (1, "Lên", "Chuyển cảnh lên trước"),
                (2, "Xuống", "Chuyển cảnh xuống sau"),
                (3, "Tách", "Tách cảnh tại playhead"),
                (4, "Xóa", "Xóa cảnh khỏi timeline"),
            ] {
                let response = ui.button(title).on_hover_text(hint);
                geo.record(&format!("action_{id}"), response.rect);
                if response.clicked() {
                    action = id;
                }
            }
        });
    });
    action
}

fn source_card(ui: &mut egui::Ui, s: &mut Segment, sources: &[Source], geo: &mut SceneGeometry) {
    let card = scene_card().show(ui, |ui| {
        ui.set_min_height(184.);
        section_title(ui, "Nguồn & thời gian");
        let field_width = (ui.available_width() - 62.).max(100.);
        egui::Grid::new("scene_source_fields")
            .num_columns(2)
            .spacing([10., 8.])
            .show(ui, |ui| {
                label(ui, "Chế độ");
                let combo = egui::ComboBox::from_id_salt("kind")
                    .width(field_width)
                    .truncate()
                    .selected_text(kind_label(&s.kind))
                    .show_ui(ui, |ui| {
                        for kind in ["source", "review", "host"] {
                            let response =
                                ui.selectable_value(&mut s.kind, kind.into(), kind_label(kind));
                            geo.record(&format!("scene_kind_{kind}"), response.rect);
                        }
                    });
                geo.record("scene_kind", combo.response.rect);
                ui.end_row();
                label(ui, "Nguồn");
                if s.kind == "host" {
                    label(ui, "Không dùng video gốc");
                } else {
                    let title = sources
                        .iter()
                        .find(|src| src.id == s.source_id)
                        .map(|src| {
                            if src.title.is_empty() {
                                src.id.clone()
                            } else {
                                format!("{} · {}", src.id, src.title)
                            }
                        })
                        .unwrap_or_else(|| s.source_id.clone());
                    let combo = egui::ComboBox::from_id_salt("source_id")
                        .width(field_width)
                        .truncate()
                        .selected_text(egui::RichText::new(title.clone()))
                        .show_ui(ui, |ui| {
                            ui.set_max_width(460.);
                            for source in sources {
                                ui.selectable_value(
                                    &mut s.source_id,
                                    source.id.clone(),
                                    format!("{} · {}", source.id, source.title),
                                );
                            }
                        });
                    geo.record("scene_source", combo.response.rect);
                    combo.response.on_hover_text(title);
                }
                ui.end_row();
            });
        if s.kind != "host" {
            let max = sources
                .iter()
                .find(|src| src.id == s.source_id)
                .map(|src| src.duration_ms)
                .unwrap_or(0);
            let mut start = s.in_ms as f64 / 1000.;
            let mut end = s.out_ms as f64 / 1000.;
            ui.columns(2, |columns| {
                for (index, (title, value)) in
                    [("Bắt đầu (In)", &mut start), ("Kết thúc (Out)", &mut end)]
                        .into_iter()
                        .enumerate()
                {
                    let ui = &mut columns[index];
                    label(ui, title);
                    let response = ui.add_sized(
                        [ui.available_width(), 28.],
                        egui::DragValue::new(value)
                            .range(if index == 0 { 0. } else { 0.1 }..=max as f64 / 1000.)
                            .speed(0.1)
                            .fixed_decimals(2)
                            .suffix(" s"),
                    );
                    geo.record(
                        if index == 0 { "scene_in" } else { "scene_out" },
                        response.rect,
                    );
                }
            });
            s.in_ms = (start * 1000.).round() as u64;
            s.out_ms = (end * 1000.).round() as u64;
            if s.kind == "source" {
                s.duration_ms = (s.out_ms.saturating_sub(s.in_ms) as f64/s.source_rate).round() as u64;
            }
            label(
                ui,
                &format!(
                    "Đoạn nguồn: {:.2} s • mốc trong video gốc",
                    s.out_ms.saturating_sub(s.in_ms) as f64 / 1000.
                ),
            );
        } else {
            ui.add_space(8.);
            label(ui, "Hiển thị nhân vật và đọc lời thoại.");
            label(ui, "Thời lượng được khớp theo giọng đã tạo.");
        }
    });
    geo.record("scene_source_card", card.response.rect);
}

fn narration_card(ui: &mut egui::Ui, s: &mut Segment, geo: &mut SceneGeometry) {
    let card = scene_card().show(ui, |ui| {
        ui.set_min_height(184.);
        ui.horizontal(|ui| {
            section_title(ui, "Lời thoại");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                label(
                    ui,
                    if s.kind == "source" {
                        "Tiếng gốc"
                    } else {
                        "Giọng VieNeu"
                    },
                );
            });
        });
        ui.add_enabled_ui(s.kind != "source", |ui| {
            egui::ScrollArea::vertical()
                .id_salt("scene_narration_scroll")
                .max_height(100.)
                .min_scrolled_height(100.)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let response = ui.add(
                        egui::TextEdit::multiline(&mut s.narration)
                            .desired_width(f32::INFINITY)
                            .desired_rows(5)
                            .hint_text("Nhập lời review để VieNeu đọc..."),
                    );
                    geo.record("scene_narration", response.rect);
                });
        });
        label(
            ui,
            if s.kind == "source" {
                "Cảnh này phát tiếng gốc; chuyển sang Review để thêm lời bình."
            } else {
                "Đổi lời thoại cần tạo lại giọng."
            },
        );
        if s.kind != "source" {
            label(
                ui,
                &format!(
                    "{} từ • {:.2} s trên timeline",
                    s.narration.split_whitespace().count(),
                    s.duration_ms as f64 / 1000.
                ),
            );
        }
    });
    geo.record("scene_narration_card", card.response.rect);
}

fn scene_details(ui: &mut egui::Ui, s: &mut Segment, geo: &mut SceneGeometry, language: &str) {
    let actor = egui::CollapsingHeader::new("Nhân vật, chuyển động & âm thanh")
        .id_salt("scene_actor")
        .show(ui, |ui| {
            ui.add_enabled_ui(s.kind != "source", |ui| {
                label(ui, "Cảm xúc / cách đọc");
                let response =
                    ui.add(egui::TextEdit::singleline(&mut s.emotion).desired_width(f32::INFINITY));
                geo.record("scene_emotion", response.rect);
                label(ui, "Cử chỉ tay");
                egui::ComboBox::from_id_salt("scene_gesture")
                    .width(ui.available_width() - 4.)
                    .selected_text(gesture_label(&s.gesture))
                    .show_ui(ui, |ui| {
                        for value in ["auto", "none", "open_palms", "point", "beat", "surprised"] {
                            ui.selectable_value(&mut s.gesture, value.into(), gesture_label(value));
                        }
                    });
                label(ui, "Kích thước nhân vật");
                egui::ComboBox::from_id_salt("scene_avatar_size")
                    .width(ui.available_width() - 4.)
                    .selected_text(if s.avatar_size == "large" {
                        "Lớn (nhấn reaction)"
                    } else {
                        "Nhỏ (theo khung hình)"
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut s.avatar_size,
                            "small".into(),
                            "Nhỏ (theo khung hình)",
                        );
                        ui.selectable_value(
                            &mut s.avatar_size,
                            "large".into(),
                            "Lớn (nhấn reaction)",
                        );
                    });
            });
            label(ui, "Chuyển động hình ảnh");
            egui::ComboBox::from_id_salt("scene_motion")
                .width(ui.available_width() - 4.)
                .selected_text(motion_label(&s.motion))
                .show_ui(ui, |ui| {
                    for value in ["none", "push_in", "reaction_shake"] {
                        ui.selectable_value(&mut s.motion, value.into(), motion_label(value));
                    }
                });
            label(
                ui,
                if s.kind == "source" {
                    "Âm lượng tiếng gốc"
                } else {
                    "Âm lượng lời thoại"
                },
            );
            let mut gain = s.audio_gain * 100.;
            let response = ui.add(egui::Slider::new(&mut gain, 0. ..=200.).suffix(" %"));
            if response.changed() {
                s.audio_gain = gain / 100.;
            }
            geo.record("scene_gain", response.rect);
        });
    geo.record("scene_actor_header", actor.header_response.rect);
    let captions = egui::CollapsingHeader::new("Phụ đề của cảnh")
        .id_salt("scene_captions")
        .show(ui, |ui| {
            label(ui, "Nội dung riêng (để trống để dùng lời thoại)");
            let response = ui.add(
                egui::TextEdit::multiline(&mut s.caption)
                    .desired_width(f32::INFINITY)
                    .desired_rows(3),
            );
            geo.record("scene_caption", response.rect);
            label(
                ui,
                if s.timing_estimated {
                    "Thời gian: ước lượng • dùng Căn từ để chính xác"
                } else {
                    "Thời gian: đã căn bằng ASR"
                },
            );
            label(ui, "Font, cỡ và màu chữ: tab Phụ đề phía trên.");
        });
    geo.record("scene_captions_header", captions.header_response.rect);
    let graphics =
        egui::CollapsingHeader::new(format!("Thẻ thông tin & số liệu ({})", s.graphics.len()))
            .id_salt("scene_graphics")
            .show(ui, |ui| {
                label(ui,"Âm thanh thẻ: nút SFX trên thanh timeline (hiệu ứng / âm lượng).");
                let mut remove = None;
                for (index, cue) in s.graphics.iter_mut().enumerate() {
                    ui.push_id(index, |ui| {
                        label(ui, &format!("Thẻ {}", index + 1));
                        egui::ComboBox::from_id_salt("graphic_kind")
                            .selected_text(&cue.kind)
                            .show_ui(ui, |ui| {
                                for kind in ["title", "fact", "number", "date"] {
                                    ui.selectable_value(&mut cue.kind, kind.into(), kind);
                                }
                            });
                        field(ui, "Nhãn", &mut cue.label);
                        field(ui, "Giá trị hiển thị", &mut cue.value);
                        label(ui, "Bắt đầu / kết thúc trong cảnh");
                        ui.horizontal_wrapped(|ui| {
                            ui.add(
                                egui::DragValue::new(&mut cue.start_ms)
                                    .range(0..=s.duration_ms.saturating_sub(1))
                                    .suffix(" ms"),
                            );
                            ui.add(
                                egui::DragValue::new(&mut cue.end_ms)
                                    .range(1..=s.duration_ms)
                                    .suffix(" ms"),
                            );
                        });
                        field(ui, "Dẫn chứng số liệu / ngày", &mut cue.evidence);
                        if ui.button("Xóa thẻ").clicked() {
                            remove = Some(index);
                        }
                        ui.separator();
                    });
                }
                if let Some(index) = remove {
                    s.graphics.remove(index);
                }
                if s.graphics.len() < 8 && ui.button("+ Thẻ thông tin").clicked() {
                    s.graphics.push(GraphicCue {
                        label: crate::content_language::text(language,"Điểm đáng chú ý","Key observation").into(),
                        end_ms: s.duration_ms.min(2500),
                        ..Default::default()
                    });
                }
            });
    geo.record("scene_graphics_header", graphics.header_response.rect);
    let evidence = egui::CollapsingHeader::new("Dẫn chứng & ghi chú nguồn")
        .id_salt("scene_evidence")
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut s.evidence)
                    .desired_width(f32::INFINITY)
                    .desired_rows(3),
            );
        });
    geo.record("scene_evidence_header", evidence.header_response.rect);
}

impl StudioApp {
    fn scene_footage_speed(&mut self,ui:&mut egui::Ui) {
        if self.project.segments[self.selected].kind=="host" {return;}
        let before=self.project.clone();let s=&mut self.project.segments[self.selected];
        let locked=s.locked||self.project.timeline.tracks.iter().enumerate().any(|(i,t)|t.locked&&crate::timeline::has_track(s,i));
        ui.add_enabled_ui(!locked,|ui|{
            egui::CollapsingHeader::new("Tốc độ hình & mốc khớp lời").id_salt("scene_speed").show(ui,|ui|{
                let mut own=s.footage_rate_override.is_some();
                if ui.checkbox(&mut own,"Tốc độ riêng cho cảnh").changed(){s.footage_rate_override=own.then_some(1.25);}
                if let Some(rate)=&mut s.footage_rate_override {ui.add(egui::Slider::new(rate,1. ..=2.).step_by(0.05).suffix("×"));}
                ui.add(egui::Label::new(&s.footage_status).wrap());
                label(ui,"Đổi hình giữ nguyên voice/sub. Mốc tự động được căn sau khi tạo giọng và Căn từ.");
                let mut remove=None;
                for (index,a) in s.footage_anchors.iter_mut().enumerate() {
                    ui.push_id(index,|ui|{
                        field(ui,"Cụm từ trong lời review",&mut a.phrase);
                        ui.horizontal_wrapped(|ui|{
                            ui.label("Nguồn");ui.add(egui::DragValue::new(&mut a.source_ms).range(s.in_ms..=s.out_ms).suffix(" ms"));
                            ui.checkbox(&mut a.manual,"Mốc tay");
                            if a.manual {ui.label("Trong cảnh");ui.add(egui::DragValue::new(&mut a.timeline_ms).range(0..=s.duration_ms).suffix(" ms"));}
                            ui.label(crate::footage_speed::anchor_status(&a.status));if ui.button("Xóa mốc").clicked(){remove=Some(index);}
                        });
                    });
                }
                if let Some(i)=remove{s.footage_anchors.remove(i);}
                if s.kind=="review"&&s.footage_anchors.len()<8&&ui.button("+ Mốc hành động").clicked(){s.footage_anchors.push(crate::footage_speed::Anchor{source_ms:s.in_ms,..Default::default()});}
            });
        });
        if serde_json::to_vec(&before.segments[self.selected]).ok()!=serde_json::to_vec(&self.project.segments[self.selected]).ok() {
            match crate::footage_speed::fit(&mut self.project) {
                Ok(_)=>{self.touch();self.request_frame();},
                Err(e)=>{self.project=before;self.message(format!("Chưa đổi tốc độ: {e:#}"),true);}
            }
        }
    }
    pub(super) fn segment_inspector(&mut self, ui: &mut egui::Ui) {
        if self.selected >= self.project.segments.len() {
            heading(ui, "THUỘC TÍNH CẢNH");
            ui.label("Chọn một cảnh trên timeline để chỉnh sửa.");
            return;
        }
        ui.spacing_mut().item_spacing = Vec2::new(10., 8.);
        ui.spacing_mut().button_padding = Vec2::new(8., 5.);
        ui.spacing_mut().interact_size.y = 28.;
        let old = self.project.segments[self.selected].clone();
        let s = &mut self.project.segments[self.selected];
        let linked_locked = self
            .project
            .timeline
            .tracks
            .iter()
            .enumerate()
            .any(|(i, t)| t.locked && crate::timeline::has_track(s, i));
        let mut geo = SceneGeometry::default();
        let mut action = 0;
        let wide = ui.available_width() >= 650.;
        ui.horizontal(|ui| {
            chip(ui, &format!("CẢNH {:02}", self.selected + 1), BLUE);
            label(ui, &format!("{:.2} s", s.duration_ms as f64 / 1000.));
            let lock = ui.checkbox(&mut s.locked, "Khóa cảnh");
            geo.record("lock", lock.rect);
            if wide {
                action = scene_actions(ui, !s.locked && !linked_locked, &mut geo);
            }
        });
        if !wide {
            action = scene_actions(ui, !s.locked && !linked_locked, &mut geo);
        }
        if linked_locked {
            label(
                ui,
                "Track liên kết đang khóa; mở khóa track trên timeline để sửa cảnh.",
            );
        }
        ui.add_space(2.);
        ui.add_enabled_ui(!s.locked && !linked_locked, |ui| {
            if ui.available_width() >= 650. {
                let width = ui.available_width();
                let source_width = 296.;
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(source_width, 0.),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            source_card(ui, s, &self.project.sources, &mut geo);
                        },
                    );
                    ui.allocate_ui_with_layout(
                        Vec2::new(width - source_width - 10., 0.),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            narration_card(ui, s, &mut geo);
                        },
                    );
                });
            } else {
                source_card(ui, s, &self.project.sources, &mut geo);
                narration_card(ui, s, &mut geo);
            }
            ui.add_space(2.);
            scene_details(ui, s, &mut geo, &self.project.rules.language);
        });
        #[cfg(test)]
        self.hitboxes.extend(geo.rects);
        let changed = serde_json::to_string(&old).ok() != serde_json::to_string(s).ok();
        if s.narration != old.narration || s.kind != old.kind {
            s.invalidate_voice();
        }
        if s.gesture != old.gesture || s.emotion != old.emotion {
            s.avatar = None;
            s.avatar_key.clear();
        }
        if s.caption != old.caption || s.kind=="source"&&(s.in_ms != old.in_ms || s.out_ms != old.out_ms) {
            s.suppress_captions = false;
            s.words.clear();
            s.timing_estimated = true;
        }
        if changed {
            if s.in_ms!=old.in_ms||s.out_ms!=old.out_ms||s.narration!=old.narration {s.footage_spans.clear();}
            if crate::footage_speed::enabled(&self.project) {
                if let Err(e)=crate::footage_speed::fit(&mut self.project) {self.project.segments[self.selected]=old.clone();self.message(format!("Chưa đổi cảnh: {e:#}"),true);}
            }
            self.touch();
        }
        self.scene_footage_speed(ui);
        if action > 0 && !old.locked {
            if action == 3 {
                let start = crate::timeline::starts(&self.project)
                    .get(self.selected)
                    .copied()
                    .unwrap_or(0);
                if self.timeline_view.cursor <= start
                    || self.timeline_view.cursor >= start + old.duration_ms
                {
                    self.timeline_view.cursor = start + old.duration_ms / 2;
                }
            }
            self.apply_timeline_edit(match action {
                1 => "up",
                2 => "down",
                3 => "split",
                _ => "delete",
            });
        }
        ui.add_space(8.0);
        if ui.button("+ Thêm cảnh từ nguồn").clicked() {
            if let Some(src) = self.project.sources.first() {
                let duration = src.duration_ms.min(5000);
                let scene = Segment {
                    source_id: src.id.clone(),
                    out_ms: duration,
                    duration_ms: duration,
                    ..Default::default()
                };
                let after = self.project.segments.len().saturating_sub(1);
                match crate::timeline::insert(&mut self.project, after, scene) {
                    Ok(index) => {
                        self.selected = index;
                        self.touch();
                        self.request_frame();
                    }
                    Err(error) => self.message(format!("{error:#}"), true),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(app: &mut StudioApp, ctx: &egui::Context, width: f32, events: Vec<egui::Event>) {
        app.hitboxes.clear();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 1800.))),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.segment_inspector(ui));
            },
        );
    }
    fn click_at(app: &mut StudioApp, ctx: &egui::Context, width: f32, pos: Pos2) {
        for pressed in [true, false] {
            draw(
                app,
                ctx,
                width,
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
        draw(app, ctx, width, vec![]);
    }
    fn sample(ctx: &egui::Context, root: &std::path::Path) -> StudioApp {
        let mut app = StudioApp::from_context(ctx);
        app.capture_path = None;
        app.projects_root = root.join("projects");
        app.project = Project {
            root: root.into(),
            sources: vec![Source { id: "src_1".into(), title: "Tên video nguồn rất dài để kiểm tra giao diện không tràn khung ở cả bố cục hẹp và rộng. ".repeat(6), duration_ms: 120_000, ..Default::default() }],
            segments: vec![Segment { kind: "review".into(), source_id: "src_1".into(), in_ms: 66_125, out_ms: 84_375, duration_ms: 18_250,
                narration: "Hãy cùng quan sát tình huống trong video. ".repeat(80), audio: Some(root.join("voice.wav")), avatar: Some(root.join("avatar.mov")), asset_key: "voice-cache".into(), avatar_key: "avatar-cache".into(),
                ..Default::default() }],
            ..Default::default()
        };
        app.dirty = false;
        app
    }
    #[test]
    fn scene_properties_responsive_cards_and_long_content_stay_inside_panel() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = sample(&ctx, temp.path());
        let before = serde_json::to_vec(&app.project).unwrap();
        for width in [280., 420., 640., 700., 1080.] {
            for _ in 0..5 {
                draw(&mut app, &ctx, width, vec![]);
            }
            for key in [
                "lock",
                "action_1",
                "action_2",
                "action_3",
                "action_4",
                "scene_source_card",
                "scene_narration_card",
                "scene_kind",
                "scene_source",
                "scene_in",
                "scene_out",
            ] {
                let rect = app.hitboxes[key];
                assert!(
                    rect.left() >= 0. && rect.right() <= width,
                    "{width}: {key}: {rect:?}"
                );
            }
            for key in ["action_2", "action_3", "action_4"] {
                assert!(
                    (app.hitboxes[key].top() - app.hitboxes["action_1"].top()).abs() < 1.,
                    "Action buttons must share one row at {width}"
                );
            }
            let source = app.hitboxes["scene_source_card"];
            let narration = app.hitboxes["scene_narration_card"];
            assert!(
                source.height() < 260. && narration.height() < 260.,
                "Long titles and dialogue must not stretch the cards: {source:?}, {narration:?}"
            );
            if width >= 700. {
                assert!((source.top() - narration.top()).abs() < 1.);
                assert!(source.right() < narration.left());
                assert!(
                    (source.height() - narration.height()).abs() < 1.,
                    "{width}: {source:?}, {narration:?}"
                );
            } else {
                assert!(narration.top() > source.bottom());
            }
            assert_eq!(serde_json::to_vec(&app.project).unwrap(), before);
            assert!(!app.dirty);
        }
    }
    #[test]
    fn scene_properties_edits_invalidate_assets_and_respect_scene_and_track_locks() {
        let temp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = sample(&ctx, temp.path());
        let width = 1000.;
        for _ in 0..4 {
            draw(&mut app, &ctx, width, vec![]);
        }
        let pos = app.hitboxes["scene_narration"].left_top() + Vec2::new(20., 10.);
        click_at(&mut app, &ctx, width, pos);
        draw(
            &mut app,
            &ctx,
            width,
            vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers {
                        ctrl: true,
                        command: true,
                        ..Default::default()
                    },
                },
                egui::Event::Text("Lời thoại đã sửa.".into()),
            ],
        );
        assert_eq!(app.project.segments[0].narration, "Lời thoại đã sửa.");
        assert!(
            app.dirty
                && app.project.segments[0].audio.is_none()
                && app.project.segments[0].avatar.is_none()
        );
        assert!(app.project.segments[0].asset_key.is_empty());
        // A scene lock and a linked track lock each block text and deletion.
        for track_lock in [false, true] {
            app.project.segments[0].locked = !track_lock;
            app.project.timeline.tracks[crate::timeline::VIDEO].locked = track_lock;
            app.dirty = false;
            draw(&mut app, &ctx, width, vec![]);
            let pos = app.hitboxes["scene_narration"].left_top() + Vec2::new(20., 10.);
            click_at(&mut app, &ctx, width, pos);
            draw(
                &mut app,
                &ctx,
                width,
                vec![egui::Event::Text("Không được thêm".into())],
            );
            let pos = app.hitboxes["action_4"].center();
            click_at(&mut app, &ctx, width, pos);
            assert_eq!(app.project.segments.len(), 1);
            assert_eq!(app.project.segments[0].narration, "Lời thoại đã sửa.");
            assert!(!app.dirty);
        }
    }
}
