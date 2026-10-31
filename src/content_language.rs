//! Output language is independent of source ASR and of the Vietnamese application UI.
use serde::{Deserialize, Deserializer};

pub const CONTRACT_VERSION: &str = "review-language-v1";
// Writing metadata policy must not invalidate previously generated speech takes.
pub const WRITING_POLICY_VERSION: &str = "script-language-v2-internal-summary";
pub fn canonical(value: &str) -> String {
    match value.trim().to_lowercase().replace('_', "-").as_str() {
        "vi" | "vie" | "vi-vn" | "vietnamese" | "tiếng việt" | "tieng viet" => "vi".into(),
        "en" | "eng" | "en-us" | "en-gb" | "english" | "tiếng anh" | "tieng anh" => "en".into(),
        _ => value.trim().into(),
    }
}
pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(canonical(&String::deserialize(d)?))
}
pub fn supported(value: &str) -> bool { matches!(canonical(value).as_str(), "vi" | "en") }
pub fn english(value: &str) -> bool { canonical(value) == "en" }
pub fn label(value: &str) -> String {
    match canonical(value).as_str() {
        "vi" => "Tiếng Việt".into(), "en" => "English".into(), _ => format!("Chưa hỗ trợ ({value})"),
    }
}
pub fn rate(value: &str) -> f64 { if english(value) { 2.5 } else { 5.2 } }
pub fn text<'a>(value: &str, vi: &'a str, en: &'a str) -> &'a str { if english(value) { en } else { vi } }

pub fn contract(value: &str) -> String {
    let language = if english(value) { "English" } else { "Vietnamese (Tiếng Việt)" };
    format!("OUTPUT LANGUAGE CONTRACT {CONTRACT_VERSION}, WRITING POLICY {WRITING_POLICY_VERSION}: {language}. The selected output language takes precedence over conflicting language instructions in the style prompt. Generate directly in {language}, without an intermediate translation pass. Viewer-facing title, narration, caption (including source scenes), graphics.label/value, evidence_items.label/claim/unit and annotations.text must use {language}. analysis_summary is INTERNAL EDITOR METADATA: Vietnamese or the selected language is acceptable. It is never narration, subtitles or a video card; do not translate it or change scenes just to match the viewer language. Keep proper names and verified numbers unchanged; use unambiguous month names for dates. JSON keys, enums, IDs and timestamps stay unchanged. proof.quote, proof.start_quote and proof.end_quote MUST remain exact original source quotations for verification; never translate these internal proof fields. Internal evidence explanations may retain their language. Use claim for the translated viewer-facing explanation. Source pixels/audio retain their original language. For a source scene with intelligible speech, supply a faithful {language} caption from the evidence; otherwise leave caption empty rather than inventing speech. anchor_phrase must be an exact substring of that scene's {language} narration. Preserve locked scenes even when their language differs; do not silently translate locked edits.")
}

pub fn field_label(path: &str) -> &'static str {
    if path=="title" {"Tiêu đề dự án"}
    else if path.ends_with(".narration") {"Lời đọc"}
    else if path.ends_with(".caption") {"Phụ đề"}
    else if path.contains(".annotations[") {"Chú thích ảnh dẫn chứng"}
    else if path.contains(".evidence_items[") {"Nội dung thẻ dẫn chứng"}
    else {"Nội dung thẻ thông tin"}
}

/// Conservative statistical check: short names, acronyms, numbers and quoted source
/// fragments are not sufficient evidence to reject an otherwise valid script.
pub fn wrong_language(value: &str, text: &str) -> bool {
    if !supported(value) { return false; }
    let mut clean = String::new();
    let mut quoted = false;
    for c in text.chars() {
        if matches!(c, '"' | '“' | '”') { quoted = !quoted; clean.push(' '); }
        else if !quoted { clean.push(c); }
    }
    let words = clean.split_whitespace().filter(|s|s.chars().any(char::is_alphabetic)).count();
    if english(value) {
        let vi_words = ["cảnh", "dẫn", "chứng", "người", "giây", "phút", "đang", "trong", "đã", "với", "chiếc", "có", "của", "không", "này", "chiếu"];
        let hits = clean.split(|c:char|!c.is_alphabetic()).map(str::to_lowercase)
            .filter(|w|vi_words.contains(&w.as_str())).collect::<std::collections::HashSet<_>>();
        if hits.len() >= 2 { return true; }
    }
    if words < 8 { return false; }
    let expected = if english(value) {whatlang::Lang::Eng} else {whatlang::Lang::Vie};
    // Inspect individual sentences as well as complete fields: a long valid English
    // paragraph must not hide a complete Vietnamese sentence in the middle.
    std::iter::once(clean.as_str()).chain(clean.split(['.', '!', '?', '\n']))
        .filter(|s|s.split_whitespace().count() >= 8)
        .any(|s|whatlang::detect(s).is_some_and(|i|i.lang()!=expected && i.is_reliable() && i.confidence()>=0.85))
}

pub fn prompt_conflict(value: &str, prompt: &str) -> bool {
    let p = prompt.to_lowercase();
    if english(value) { ["bằng tiếng việt", "in vietnamese", "ngôn ngữ tiếng việt"].iter().any(|s|p.contains(s)) }
    else { ["bằng tiếng anh", "in english", "ngôn ngữ tiếng anh"].iter().any(|s|p.contains(s)) }
}

pub fn source_is_english(words: &[crate::model::Word]) -> bool {
    let text = words.iter().map(|w|w.text.as_str()).collect::<Vec<_>>().join(" ");
    !wrong_language("en", &text) && whatlang::detect_lang(&text)==Some(whatlang::Lang::Eng)
}

/// A mixed soundtrack may retain unclassified original speech. Do not label it
/// as entirely English just because the newly generated host is English.
pub fn mux_args(p: &crate::model::Project) -> Vec<String> {
    if !english(&p.rules.language) { return vec![]; }
    let original = p.composition_snapshot().clips.iter().any(|c|c.enabled(p) && c.audio() && c.effective_gain(p)>0.
        && (c.track==crate::timeline::ORIGINAL || (c.track==crate::timeline::VOICE &&
            p.segments.iter().any(|s|s.id==c.scene_id && !english(&s.asset_language)))));
    vec!["-metadata:s:a:0".into(),format!("language={}",if original {"und"} else {"eng"})]
}

pub fn selector(ui: &mut eframe::egui::Ui, value: &mut String) -> eframe::egui::Response {
    ui.label("Ngôn ngữ lời review");
    let response = eframe::egui::ComboBox::from_id_salt("review_language")
        .selected_text(label(value)).width(175.).show_ui(ui, |ui| {
            ui.selectable_value(value, "vi".into(), "Tiếng Việt");
            let en = ui.selectable_value(value, "en".into(), "English");
            #[cfg(test)] ui.ctx().data_mut(|d|d.insert_temp(eframe::egui::Id::new("language_en_option"),en.rect));
            #[cfg(not(test))] let _ = en;
        }).response;
    if !supported(value) { ui.colored_label(eframe::egui::Color32::YELLOW, "Chọn Tiếng Việt hoặc English trước khi tạo kịch bản."); }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{self, Pos2, Rect, Vec2};
    #[test]
    fn language_dropdown_selects_english_and_keeps_narrow_layout_and_defaults() {
        for width in [240., 400.] {
            let ctx=egui::Context::default();let mut language="vi".to_string();
            let screen=Rect::from_min_size(Pos2::ZERO,Vec2::new(width,720.));
            let draw=|events,language:&mut String| {
                let mut combo=Rect::NOTHING;
                let _=ctx.run(egui::RawInput{screen_rect:Some(screen),events,..Default::default()},|ctx|{
                    egui::CentralPanel::default().show(ctx,|ui|{combo=selector(ui,language).rect;});
                });combo
            };
            let combo=draw(vec![],&mut language);assert!(screen.contains_rect(combo));
            let click=|pos| vec![egui::Event::PointerMoved(pos),egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:true,modifiers:egui::Modifiers::NONE},egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:false,modifiers:egui::Modifiers::NONE}];
            draw(click(combo.center()),&mut language);draw(vec![],&mut language);
            let option:Rect=ctx.data(|d|d.get_temp(egui::Id::new("language_en_option")).unwrap());
            assert!(screen.contains_rect(option));draw(click(option.center()),&mut language);
            assert_eq!(language,"en");
            let mut p=crate::model::Project::default();p.rules.language=language;
            let temp=tempfile::tempdir().unwrap();let file=temp.path().join("defaults.json");
            crate::model::write_json(&file,&crate::model::ProjectDefaults::from_project(&p)).unwrap();
            let mut reopened=crate::model::Project::default();crate::model::ProjectDefaults::load(&file).unwrap().apply(&mut reopened);
            assert_eq!(reopened.rules.language,"en");
        }
    }
}
