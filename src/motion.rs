use crate::{model::Project, subtitle};
use anyhow::Result;
use std::{fs, path::Path};
use eframe::egui::{self, Color32, FontData, FontDefinitions, FontId};

fn metrics_context(font: &str) -> egui::Context {
    let ctx = egui::Context::default();
    let path = if font.to_lowercase().contains("arial") {
        "C:/Windows/Fonts/arialbd.ttf"
    } else { "C:/Windows/Fonts/segoeuib.ttf" };
    if let Ok(bytes) = fs::read(path) {
        let mut defs = FontDefinitions::default();
        defs.font_data.insert("card".into(), FontData::from_owned(bytes).into());
        defs.families.get_mut(&egui::FontFamily::Proportional).unwrap().insert(0, "card".into());
        ctx.set_fonts(defs);
    }
    let _ = ctx.run(Default::default(), |_| {});
    ctx
}

fn fit_text(ctx: &egui::Context, text: &str, max_size: f64, width: f64, height: f64) -> (String, f64) {
    let text = subtitle::safe(text);
    let mut size = max_size;
    loop {
        let galley = ctx.fonts(|fonts| fonts.layout(text.clone(), FontId::proportional(size as f32), Color32::BLACK, width as f32));
        if (galley.size().y as f64 <= height && galley.size().x as f64 <= width + 0.5) || size <= max_size * 0.08 {
            return (galley.rows.iter().map(|row| row.text()).collect::<Vec<_>>().join("\\N"), size);
        }
        size *= 0.90;
    }
}

/// ASS vector cards and deterministic pop/slide animations; text is always escaped.
pub fn write(p: &Project, folder: &Path) -> Result<bool> {
    let (w, h) = p.output.dimensions();
    let scale = w.min(h) as f64 / 1080.0;
    let font = p.subtitles.font.replace([',', '\r', '\n'], " ");
    let mut ass = format!("[Script Info]\nScriptType: v4.00+\nPlayResX: {w}\nPlayResY: {h}\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Card,{font},40,&H00203346,&H00203346,&H00FFFFFF,&H88000000,-1,0,0,0,100,100,0,0,1,0,0,8,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n");
    let mut offset = 0;
    let mut count = 0;
    let metrics = p.segments.iter().any(|s| !s.graphics.is_empty()).then(|| metrics_context(&font));
    for seg in &p.segments {
        for cue in &seg.graphics {
            let start = offset + cue.start_ms;
            let end = offset + cue.end_ms;
            let ms = cue.end_ms - cue.start_ms;
            let transition = 220.min(ms / 3).max(1);
            let center = w as f64 / 2.0;
            let top = h as f64 * 0.10;
            let cw = (w as f64 * 0.88).min(960.0 * scale);
            let ch = 210.0 * scale;
            let movement = format!(
                "\\move({:.1},{:.1},{:.1},{:.1},0,{transition})\\fad({transition},{transition})",
                center,
                top + 25.0 * scale,
                center,
                top
            );
            let box_shape = format!(
                "m {:.1} 0 l {:.1} 0 {:.1} {:.1} {:.1} {:.1}",
                -cw / 2.0,
                cw / 2.0,
                cw / 2.0,
                ch,
                -cw / 2.0,
                ch
            );
            let event = |layer: u8, a: u64, b: u64, text: &str| {
                format!(
                    "Dialogue: {layer},{},{},Card,,0,0,0,,{text}\n",
                    subtitle::ass_time(a),
                    subtitle::ass_time(b)
                )
            };
            ass.push_str(&event(
                0,
                start,
                end,
                &format!("{{\\an7{movement}\\p1\\c&HFFFFFF&\\bord0\\shad2}}{box_shape}{{\\p0}}"),
            ));
            let text_y = top + 24.0 * scale;
            let (label, label_size) = fit_text(metrics.as_ref().unwrap(), &cue.label, 38.0*scale, cw*0.88, 55.0*scale);
            let label_tags = format!("\\an8\\move({center:.1},{:.1},{center:.1},{text_y:.1},0,{transition})\\fad({transition},{transition})\\fs{label_size:.1}\\c&H463320&", text_y+25.0*scale);
            ass.push_str(&event(
                1,
                start,
                end,
                &format!("{{{label_tags}}}{label}"),
            ));
            let value_y = top + 85.0 * scale;
            let value = if cue.value.is_empty() {
                "REACTION"
            } else {
                &cue.value
            };
            let (fitted_value, value_size) = fit_text(metrics.as_ref().unwrap(), value, 90.0*scale, cw*0.88, 105.0*scale);
            let value_tags = format!("\\an8\\pos({center:.1},{value_y:.1})\\fs{value_size:.1}\\c&H3B55E8&\\fscx80\\fscy80\\t(0,{transition},\\fscx100\\fscy100)\\fad({transition},{transition})");
            // Count-up is only for simple integer values; dates and formatted numbers stay exact.
            if cue.kind == "number"
                && value.len() <= 9
                && value.bytes().all(|c| c.is_ascii_digit())
                && ms >= 1000
            {
                let number: u64 = value.parse().unwrap_or(0);
                for step in 0..10u64 {
                    let a = start + step * 60;
                    let b = start + (step + 1) * 60;
                    ass.push_str(&event(
                        2,
                        a,
                        b,
                        &format!(
                            "{{\\an8\\pos({center:.1},{value_y:.1})\\fs{:.1}\\c&H3B55E8&}}{}",
                            value_size,
                            number * step / 10
                        ),
                    ));
                }
                ass.push_str(&event(
                    2,
                    start + 600,
                    end,
                    &format!("{{{value_tags}}}{fitted_value}"),
                ));
            } else {
                ass.push_str(&event(
                    2,
                    start,
                    end,
                    &format!("{{{value_tags}}}{fitted_value}"),
                ));
            }
            count += 1;
        }
        offset += seg.duration_ms;
    }
    fs::create_dir_all(folder)?;
    fs::write(folder.join("graphics.ass"), ass)?;
    Ok(count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_vietnamese_cards_wrap_and_fit_inside_the_card() {
        let ctx = metrics_context("Segoe UI");
        for (text, max_size, width, height) in [
            ("Khống chế đối tượng mang hung khí trên đường phố", 30.0, 278.0, 35.0),
            ("Lời bình phân tích tình huống và cách giữ khoảng cách an toàn", 12.6, 278.0, 18.3),
        ] {
            let (fitted, size) = fit_text(&ctx, text, max_size, width, height);
            let layout = ctx.fonts(|f| f.layout(fitted.replace("\\N", "\n"), FontId::proportional(size as f32), Color32::BLACK, width as f32));
            assert!(layout.size().x <= width as f32 + 0.5 && layout.size().y <= height as f32);
            assert_eq!(fitted.replace("\\N", " ").split_whitespace().collect::<Vec<_>>(), text.split_whitespace().collect::<Vec<_>>());
            assert!(size < max_size);
        }
    }
}
