//! Editorial preset and runtime constraints share the project's actual settings.
use crate::model::Project;

pub const LEGACY_REACTION_PROMPT: &str = "đọc và hiểu toàn bộ nội dung, tình huống trong video. Sau đó viết lại thành 1 kịch bản mới với nội dung dạng review, reaction ngắn khoảng {duration}. Phong cách kịch bản dạng bất ngờ cho nhân vật review, reaction thoại";

pub const DEFAULT_REACTION_PROMPT: &str = concat!(r#"Act as an evidence-led US True Crime and police bodycam commentary creator. Analyze all available source evidence and write an original, punchy reaction/review script lasting {duration}, in the project's selected review language.

FIVE RETENTION BEATS: These are editorial beats, not five required scenes or fixed timestamps. Scale them to the target duration, including all original-audio clips. Open with the strongest verified moment when hook is enabled, then a short high-stakes spoken hook. Establish who, where and why; escalate through observed action; reveal a twist only if supported; explain the human impact; finish with the verified legal outcome and one fair ethical/legal question. Distribute action and analysis across the full duration. Do not force a seventy-second structure onto a longer video.

NATURAL SPOKEN DELIVERY: For English, use natural American English, contractions, short sentences (aim for fewer than fifteen words), strong verbs and occasional natural pauses. Write for the ear. Use terms such as spike strips, Jaws of Life, PIT maneuver, floored it or miles an hour only when accurate for the source. Write spoken numbers and speeds in words for TTS. For Vietnamese, apply the same pacing with natural Vietnamese delivery; do not force English idioms. Never use "spiraled into a nightmare", "heavy hydraulic rescue tools", "extricate", "succumbed to catastrophic injuries", "little did they know" or "unbeknownst".


FACTUAL PRECISION: Match hook verbs to the exact visual action: a gently pushed door is not kicked open. State a specific supported setting immediately; omit an unknown location instead of inventing one. Preserve distinctive verified details instead of generic summaries. Separate allegations, charges, convictions and sentences. State an exact sentence only when provided by the source; otherwise use the latest verified legal action. Keep missing outcomes in internal evidence notes, without video meta-references or invented continued custody/pending trial. Do not invent weapons, prior crimes, identities, quotes, injuries, deaths, twists or police tactics to fit the structure. Avoid sensational language that implies an unverified event. Evidence cards, subtitles and on-screen text must match the selected language and verified source. Source dialogue remains faithful to the original. Do not rewrite a distinct statutory charge as resisting arrest.

US DELIVERY AND ENDING: Sentences under fifteen words, except faithful source quotes. Use booked into jail, fights back, stop sticks, spike strips or Jaws of Life only when accurate. Never use delve, testament, volatile dispute, erupt into violence, according to verified incident records, transported to county booking, or tire deflation devices. End with the latest supported legal action, when known, then one grounded debate question. Never close with a moral lesson or generic praise for police. Spell spoken numbers/units contextually for TTS, while cards can show 80 MPH. Viewer card lines are at most twenty-five Unicode characters including bullets/spaces. Keep labels one line and content at most three short lines; rephrase or split instead of truncating. Preserve full source quotes in internal proof fields. Use actual source scenes for raw-audio gaps; never speak [Play Raw Audio]. In automatic cadence mode, distribute these after fifteen to twenty seconds of narration. Fixed count and user switches still take priority.

PROJECT SETTINGS TAKE PRIORITY: Respect the selected language, duration, hook switch, intercut switch/count/length, included/excluded ranges, footage speed, evidence settings and locked scenes. Do not change settings to fit the story. Use the runtime narration budget below; count the actual words before returning JSON. Do not inflate duration_ms or add silence to disguise insufficient narration."#, crate::narration_policy::rules_text!());

pub fn runtime_contract(p: &Project) -> String {
    let rules = &p.rules;
    let resolved = rules.resolved_prompt();
    let policy = crate::narration_policy::supplement(&resolved);
    let hook = if rules.hook_enabled { 3. } else { 0. };
    let audio_available = !p.sources.iter().all(|s| s.duration_ms > 0 && !s.has_audio)
        || p.sources.is_empty();
    let intercuts = if rules.intercuts_enabled && audio_available { crate::us_editorial::intercut_count(rules) } else { 0 };
    let intercut_mid = (rules.intercut_min_seconds as f64 + rules.intercut_max_seconds as f64) / 2.;
    let reference_raw = hook + intercuts as f64 * intercut_mid;
    let target_mid = (rules.min_seconds as f64 + rules.max_seconds as f64) / 2.;
    let rate = crate::content_language::rate(&rules.language);
    let reference_words = ((target_mid - reference_raw).max(0.) * rate).round() as u64;
    let unit = if crate::content_language::english(&rules.language) { "spoken English words" } else { "Vietnamese whitespace-separated speech units" };
    let hook_rule = if rules.hook_enabled {
        "Hook ON: choose a 3-second source opening by default, within the allowed 3–7 seconds; narration empty, no avatar. Put the one-sentence spoken hook in the NEXT host/review scene."
    } else {
        "Hook OFF: do not add an opening source hook. Start with the spoken hook in a host/review scene."
    };
    let intercut_rule = if intercuts > 0 {
        format!("Intercuts ON: exactly {intercuts} source scenes AFTER the opening hook, each {}–{} seconds. Prefer 2–4 seconds only when that fits this configured range. Place spoken scenes before AND after each clip; do not add extra raw clips or silent gaps.",rules.intercut_min_seconds,rules.intercut_max_seconds)
    } else {
        "Intercuts OFF or all sources verified silent: do not insert additional source scenes between spoken scenes.".into()
    };
    let mut contract = format!("\nAIR3VIEW RUNTIME CONTRACT:\n{hook_rule}\n{intercut_rule}\nTOTAL TARGET: {}–{} seconds including all raw clips. Reference allocation: {:.1}s raw clips, approximately {reference_words} {unit} at {rate:.2} units/second, targeting the midpoint {:.1}s. This is a narration budget, not a requested word count on screen. After choosing source clips, recompute using their actual durations: spoken_units / {rate:.2} + sum(source.duration_ms)/1000 must be within {}–{} seconds. Count narration only, not captions, evidence, titles or stage notes. If the count is short, add grounded analysis and context; if long, tighten repeated ideas. Do this BEFORE returning JSON. Real WAV measurements remain authoritative; never stretch silence or speed up AI speech. Preserve locked scenes and count their existing speech/clip time in the total. If the constraints cannot be met from the evidence, explain the limitation rather than invent facts.\nOutput JSON only; no markdown or bracketed production cues inside narration/caption. The selected output language overrides this preset's instructional language. Legal outcomes absent from the source remain unknown in internal evidence notes; omit spoken video meta-references and never infer continued custody or pending trial.\n",rules.min_seconds,rules.max_seconds,reference_raw,target_mid,rules.min_seconds,rules.max_seconds);
    contract.push_str(policy);
    contract
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ScriptRules;

    #[test]
    fn narration_policy_is_shared_once_by_writer_and_full_repair() {
        for custom in [DEFAULT_REACTION_PROMPT.to_owned(), "My own tone. Target {duration}.".into(),
            format!("Keep my tone.\n{}",crate::narration_policy::LEGACY_CRITICAL_BLOCK)] {
            let mut p=Project::default();
            p.rules.prompt=custom;
            p.rules.editorial_profile=crate::us_editorial::PROFILE.into();
            p.rules.language="en".into();p.rules.min_seconds=95;p.rules.max_seconds=240;
            let before=serde_json::to_value(&p).unwrap();
            let writer=crate::providers::build_prompt(&p);
            let repair=crate::script_quality::repair_prompt(&p,&crate::script_quality::plan(&p),&Default::default());
            for prompt in [writer,repair] {
                assert_eq!(prompt.matches("DIRECT NARRATION POLICY v2").count(),1);
                assert!(!prompt.contains("CRITICAL PATCH RULES"));
                assert!(!prompt.contains("otherwise explicitly say"));
                assert!(!prompt.contains("say 'The video doesn't provide"));
                assert!(prompt.contains("TOTAL TARGET: 95–240 seconds"));
                assert!(prompt.contains("An arrest alone never establishes continued custody or pending trial"));
                assert!(!prompt.contains("{duration}"));
            }
            assert_eq!(before,serde_json::to_value(&p).unwrap());
        }
        let mut r=ScriptRules{prompt:crate::narration_policy::PREVIOUS_PRESET.into(),..Default::default()};
        r.migrate_prompt();assert_eq!(r.prompt,DEFAULT_REACTION_PROMPT);
    }

    #[test]
    fn old_presets_migrate_but_custom_prompts_survive() {
        for preset in [LEGACY_REACTION_PROMPT.to_string(), LEGACY_REACTION_PROMPT.replace("{duration}", "3-4p")] {
            let mut rules=ScriptRules { prompt:preset, ..Default::default() };
            rules.migrate_prompt();
            assert_eq!(rules.prompt,DEFAULT_REACTION_PROMPT);
        }
        let mut rules=ScriptRules { prompt:"My own script {duration}".into(),language:"en".into(),..Default::default() };
        rules.migrate_prompt();
        assert_eq!(rules.prompt,"My own script {duration}");
        assert!(rules.resolved_prompt().contains("180–240 seconds"));
    }

    #[test]
    fn runtime_uses_duration_language_and_switches_without_mutating_settings() {
        for language in ["vi","en"] {
            for (min,max) in [(50,55),(100,110),(180,240)] {
                let mut p=Project::default();
                p.rules.language=language.into();p.rules.min_seconds=min;p.rules.max_seconds=max;
                p.rules.intercut_count=5;p.rules.intercut_min_seconds=4;p.rules.intercut_max_seconds=6;
                let before=serde_json::to_vec(&p.rules).unwrap();
                let prompt=crate::providers::build_prompt(&p);
                assert!(!prompt.contains("{duration}"));
                assert!(prompt.contains(&format!("TOTAL TARGET: {min}–{max} seconds")));
                assert!(prompt.contains("exactly 5 source scenes")&&prompt.contains("each 4–6 seconds"));
                assert!(!prompt.contains("(50-70s)"));
                assert!(prompt.contains(&crate::content_language::contract(language)));
                assert!(!crate::content_language::prompt_conflict(language,DEFAULT_REACTION_PROMPT));
                assert_eq!(before,serde_json::to_vec(&p.rules).unwrap());
                p.rules.hook_enabled=false;p.rules.intercuts_enabled=false;
                let prompt=crate::providers::build_prompt(&p);
                assert!(prompt.contains("Hook OFF")&&prompt.contains("Intercuts OFF"));
                assert!(!prompt.contains("HOOK BẮT BUỘC")&&!prompt.contains("ĐOẠN GỐC XEN KẼ BẮT BUỘC"));
            }
        }
        let mut p=Project::default();p.rules.language="en".into();p.rules.min_seconds=100;p.rules.max_seconds=110;
        assert!(runtime_contract(&p).contains("approximately 229 spoken English words"));
        p.sources.push(crate::model::Source {duration_ms:1000,has_audio:false,..Default::default()});
        assert!(runtime_contract(&p).contains("all sources verified silent"));
    }

    #[test]
    fn production_tags_are_repaired_before_they_can_reach_tts() {
        let mut p=Project::default();p.rules.min_seconds=1;p.rules.max_seconds=20;
        p.rules.hook_enabled=false;p.rules.intercuts_enabled=false;p.evidence_settings.enabled=false;
        p.sources.push(crate::model::Source {id:"s".into(),duration_ms:20000,..Default::default()});
        p.segments.push(crate::model::Segment {kind:"host".into(),source_id:"s".into(),out_ms:10000,duration_ms:3000,
            narration:"[Avatar Emotion: Somber] The victim never made it home.".into(),..Default::default()});
        let plan=crate::script_quality::plan(&p);
        let report=crate::script_quality::inspect(&p,&plan,&Default::default(),false);
        assert!(report.issues.iter().any(|i|i.code=="NARRATION_CUE"&&i.affects_narration));
        p.segments[0].narration="The victim never made it home.".into();
        let report=crate::script_quality::inspect(&p,&crate::script_quality::plan(&p),&Default::default(),false);
        assert!(!report.issues.iter().any(|i|i.code=="NARRATION_CUE"));
        let repair=crate::script_quality::repair_prompt(&p,&plan,&report);
        assert!(repair.contains("FIVE RETENTION BEATS")&&repair.contains("AIR3VIEW RUNTIME CONTRACT"));
    }
}
