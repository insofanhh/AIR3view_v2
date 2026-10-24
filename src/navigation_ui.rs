//! Collapsible navigation with code-drawn icons, avoiding missing font glyphs.
use super::*;

fn navigation_icon(painter: &egui::Painter, page: Page, center: Pos2, color: Color32) {
    let stroke = Stroke::new(1.7, color);
    let line = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment(
            [center + Vec2::new(a.0, a.1), center + Vec2::new(b.0, b.1)],
            stroke,
        );
    };
    match page {
        Page::New => {
            line((-7., 0.), (7., 0.));
            line((0., -7.), (0., 7.));
        }
        Page::Projects => {
            for x in [-7., 2.] {
                for y in [-7., 2.] {
                    painter.rect_stroke(
                        Rect::from_min_size(center + Vec2::new(x, y), Vec2::splat(5.)),
                        1.,
                        stroke,
                        egui::StrokeKind::Inside,
                    );
                }
            }
        }
        Page::Editor => {
            painter.rect_stroke(
                Rect::from_center_size(center, Vec2::new(19., 16.)),
                3.,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    center + Vec2::new(-2., -4.),
                    center + Vec2::new(-2., 4.),
                    center + Vec2::new(4., 0.),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Page::Queue | Page::Batch => {
            for (y, x) in [(-6., 5.), (0., 8.), (6., 2.)] {
                line((-7., y), (x, y));
            }
        }
        Page::Settings => {
            for (x, y) in [(-6., -3.), (0., 4.), (6., -1.)] {
                line((x, -8.), (x, y - 2.));
                line((x, y + 2.), (x, 8.));
                painter.circle_stroke(center + Vec2::new(x, y), 2., stroke);
            }
        }
    }
}

impl StudioApp {
    pub(super) fn sidebar(&mut self, ctx: &egui::Context) {
        let collapsed = self.editor_layout.sidebar_collapsed;
        let width = if collapsed { 64. } else { 202. };
        let mut toggle = false;
        let panel = egui::SidePanel::left("navigation")
            .exact_width(width)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(if collapsed { 8 } else { 14 }, 14)),
            )
            .show(ctx, |ui| {
                ui.add_space(6.);
                ui.horizontal(|ui| {
                    ui.add(egui::Image::new((
                        self.branding_icon.id(),
                        Vec2::splat(40.),
                    )));
                    if !collapsed {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(crate::branding::APP_NAME).size(19.).strong());
                            ui.label(RichText::new("AI VIDEO REVIEW").color(TEAL).size(9.));
                        });
                    }
                });
                ui.add_space(8.);
                let response = ui.add_sized(
                    [ui.available_width(), 28.],
                    egui::Button::new(if collapsed { "" } else { "Thu gọn" }),
                );
                #[cfg(test)]
                self.hitboxes.insert("nav_toggle".into(), response.rect);
                let response = response.on_hover_text(if collapsed {
                    "Mở rộng menu"
                } else {
                    "Thu gọn menu"
                });
                let c = if collapsed {
                    response.rect.center()
                } else {
                    Pos2::new(response.rect.left() + 14., response.rect.center().y)
                };
                let direction = if collapsed { 1. } else { -1. };
                ui.painter().line_segment(
                    [
                        c + Vec2::new(-direction * 3., -5.),
                        c + Vec2::new(direction * 3., 0.),
                    ],
                    Stroke::new(1.6, MUTED),
                );
                ui.painter().line_segment(
                    [
                        c + Vec2::new(direction * 3., 0.),
                        c + Vec2::new(-direction * 3., 5.),
                    ],
                    Stroke::new(1.6, MUTED),
                );
                toggle = response.clicked();
                ui.add_space(18.);
                if !collapsed {
                    heading(ui, "KHÔNG GIAN LÀM VIỆC");
                }
                for (page, label) in [
                    (Page::New, "Dự án mới"),
                    (Page::Projects, "Thư viện dự án"),
                    (Page::Editor, "Biên tập video"),
                    (Page::Queue, "Tiến trình & nhật ký"),
                    (Page::Batch, "Xử lý hàng loạt"),
                    (Page::Settings, "Cài đặt kết nối"),
                ] {
                    let selected = self.page == page;
                    let response = ui.add_sized(
                        [ui.available_width(), 42.],
                        egui::Button::new("")
                            .fill(if selected {
                                BLUE.gamma_multiply(0.16)
                            } else {
                                Color32::TRANSPARENT
                            })
                            .stroke(Stroke::NONE)
                            .corner_radius(8),
                    );
                    #[cfg(test)]
                    self.hitboxes.insert(format!("nav_{page:?}"), response.rect);
                    let color = if selected { BLUE } else { MUTED };
                    navigation_icon(
                        ui.painter(),
                        page,
                        if collapsed {
                            response.rect.center()
                        } else {
                            Pos2::new(response.rect.left() + 18., response.rect.center().y)
                        },
                        color,
                    );
                    if !collapsed {
                        ui.painter().text(
                            Pos2::new(response.rect.left() + 38., response.rect.center().y),
                            Align2::LEFT_CENTER,
                            label,
                            FontId::proportional(12.),
                            color,
                        );
                    }
                    if response.on_hover_text(label).clicked() {
                        if page == Page::New && !self.busy {
                            self.new_project();
                        } else {
                            self.page = page;
                        }
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    if collapsed {
                        if ui.small_button("UPD").on_hover_text("Phiên bản & cập nhật").clicked(){self.updates.open=true;}
                        let (rect, response) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 26.),
                            Sense::hover(),
                        );
                        ui.painter().circle_filled(
                            rect.center(),
                            4.,
                            if self.busy { BLUE } else { TEAL },
                        );
                        response.on_hover_text(if self.busy {
                            "Đang xử lý"
                        } else {
                            "Sẵn sàng"
                        });
                    } else {
                        if ui.button(format!("v{} • Cập nhật",env!("CARGO_PKG_VERSION"))).clicked(){self.updates.open=true;}
                        ui.separator();
                        chip(
                            ui,
                            if self.busy {
                                "Đang xử lý"
                            } else {
                                "Sẵn sàng"
                            },
                            if self.busy { BLUE } else { TEAL },
                        );
                        ui.label(RichText::new("Video review AI").size(11.).color(MUTED));
                    }
                });
            });
        #[cfg(test)]
        self.hitboxes
            .insert("navigation_panel".into(), panel.response.rect);
        #[cfg(not(test))]
        let _ = panel;
        if toggle {
            self.editor_layout.sidebar_collapsed = !collapsed;
            self.remember_editor_layout();
        }
    }
}
