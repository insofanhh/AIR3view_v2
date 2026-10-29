//! Shared authoring policy. Cheap style diagnostics never start another model job.
use crate::{model::*, script_quality::{Issue, Report}};

macro_rules! rules_text {
    () => {r#"
DIRECT NARRATION POLICY v2:
Priority: verified source facts, project settings/JSON schema, then writing style. Never invent facts to satisfy style or duration.
1. NO VIDEO META-REFERENCES: Narrate the incident directly. Do not say "In this clip", "The video does not provide..." or "The footage ends without...". If later legal outcomes are unknown, use the latest VERIFIED arrest, charge, hold, conviction or sentence and a grounded ending question. Keep unknown outcomes in internal evidence notes. An arrest alone never establishes continued custody or pending trial. Say "while awaiting trial" or "held in county jail" only when source evidence confirms that status. Never replace an unknown outcome with an invented one.
2. HOOK TO SETUP: Across host/review scenes, the first authored narration sentence is the spoken hook. The next sentence advances a verified physical action instead of repeating time, location or call information already stated. Raw source dialogue does not count. Add only essential missing context alongside that new action. Respect locked scenes.
3. NATURAL REPETITION CONTROL: Avoid redundant repetition of nouns and phrases within or between adjacent authored sentences. Repeated prepositions and grammar words are allowed when natural or necessary for precision. Preserve quotations, proper names and official charge titles. Never change facts merely to avoid repeating a word. This is not a blanket ban on repeating nouns or prepositions.
SELF-CHECK IN THIS RESPONSE: Before returning JSON, check the hook/setup pair, remove video meta-references, remove needless repetition and verify the narration budget. Keep sentences natural and preserve source meaning. Return only the required JSON, without a separate review or extra tool/model calls. Narration, subtitles and cards follow the selected language. Do not add scenes, silence, effects or change project settings to satisfy these writing rules.
"#};
}
pub(crate) use rules_text;
pub const CONTRACT: &str = rules_text!();
pub const PREVIOUS_PRESET: &str = include_str!("prompts/us_bodycam_pre_narration_v2.txt");

// Exact former user-facing block, not a fuzzy removal of custom instructions.
pub const LEGACY_CRITICAL_BLOCK: &str = r#"## CRITICAL PATCH RULES FOR AI SCRIPT GENERATION:
1. **NO META-REFERENCES TO THE VIDEO:** NEVER say "The video does not provide...", "In this clip...", or "The footage ends without...". If the final court sentence is unknown because the arrest is recent, simply state the booking charges and say "while awaiting trial" or "held in county jail."
2. **NO HOOK-TO-SETUP REPETITION:** Sentence 2 (Setup) must NEVER repeat the time, location, or "emergency call" already mentioned in Sentence 1 (Hook). Sentence 2 must immediately advance the physical action inside the scene.
3. **WORD REPETITION CHECK:** Never use the same preposition or noun twice in the same sentence (e.g., BANNED: "Inside, officers approach... from inside")."#;

pub fn migrate(prompt:&str)->String {
    let normalized=prompt.replace("\r\n","\n");
    if normalized.trim()==PREVIOUS_PRESET.replace("\r\n","\n").trim() {
        return crate::reaction_prompt::DEFAULT_REACTION_PROMPT.into();
    }
    let replaced=normalized.replace(LEGACY_CRITICAL_BLOCK, CONTRACT.trim());
    let changed=replaced!=normalized;
    if changed {replaced}else{prompt.to_owned()}
}

pub fn supplement(prompt:&str)->&'static str {
    if prompt.replace("\r\n","\n").contains(CONTRACT.trim()) {""}else{CONTRACT}
}

fn authored(text:&str)->String {
    let mut quoted=false;
    text.chars().map(|c|match c {
        '“'=>{quoted=true;' '},'”'=>{quoted=false;' '},
        '"'=>{quoted = !quoted;' '},
        _ if quoted=>' ', _=>c,
    }).collect()
}
fn words(text:&str)->Vec<String> {
    text.split(|c:char| !c.is_alphanumeric() && c!='\'' && c!='’')
        .filter(|v|!v.is_empty()).map(|v|v.to_lowercase().replace('’',"'")).collect()
}
fn phrase(text:&str, value:&str)->bool {
    let text=words(text);let value=words(value);
    text.windows(value.len()).any(|window|window==value)
}
fn warn(r:&mut Report,message:String) {
    if !r.warnings.contains(&message){r.warnings.push(message);}
}

pub fn inspect(plan:&ScriptPlan,r:&mut Report) {
    let mut opening=vec![];
    let mut previous:Option<(usize,Vec<String>)>=None;
    for (index,seg) in plan.segments.iter().enumerate().filter(|(_,s)|s.kind!="source") {
        let text=authored(&seg.narration);
        if !seg.locked {
            let banned=["in this clip","in this video","the video does not provide","the video doesn't provide",
                "the footage ends without","the footage does not provide","the source does not provide",
                "the source doesn't provide","trong video này","trong đoạn clip này","video không cung cấp",
                "video này không cung cấp","đoạn phim không cung cấp"];
            if let Some(found)=banned.iter().find(|s|phrase(&text,s)) {
                r.issues.push(Issue{code:"US_META_REFERENCE".into(),scene:index+1,item:None,
                    path:format!("segments[{index}].narration"),label:String::new(),
                    message:format!("Bỏ cách dẫn '{found}' trong lời đọc; kể trực tiếp sự kiện đã xác minh. Nếu chưa rõ kết quả pháp lý, ghi vào evidence nội bộ; không suy đoán đang giam hay chờ xét xử."),affects_narration:true});
            }
        }
        for sentence in text.split(['.','!','?',';','\n']).filter(|v|!v.trim().is_empty()) {
            let tokens=words(sentence);if tokens.is_empty(){continue;}
            if opening.len()<2 {opening.push((index,seg.locked,tokens.clone()));}
            if !seg.locked {
                if previous.as_ref().is_some_and(|(_,before)|*before==tokens) {
                    warn(r,format!("Cảnh {}: câu lời bình lặp nguyên văn câu trước; ưu tiên một hành động hoặc chi tiết mới. Lưu ý văn phong này không mở thêm lượt AI.",index+1));
                }
                if tokens.windows(2).any(|w|w[0]==w[1]&&w[0].len()>3&&!matches!(w[0].as_str(),"that"|"had")) {
                    warn(r,format!("Cảnh {}: có từ liền nhau bị lặp; rà lại lời bình, giữ nguyên ngữ pháp, trích dẫn và tên riêng khi cần.",index+1));
                }
            }
            previous=Some((index,tokens));
        }
    }
    if let [(first,_,hook),(second,false,setup)]=opening.as_slice() {
        if hook.len()>=5 && setup.len()>=5 && hook[..4]==setup[..4] && hook!=setup {
            warn(r,format!("Cảnh {} → {}: hook và setup mở đầu giống nhau; câu setup nên tiến tới hành động mới. Không đổi mốc nguồn hoặc tạo thêm lượt AI chỉ vì lưu ý văn phong.",first+1,second+1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn report(lines:&[(&str,&str,bool)])->Report {
        let p=Project::default();let mut plan=crate::script_quality::plan(&p);
        plan.segments=lines.iter().map(|(kind,text,locked)|Segment{kind:(*kind).into(),narration:(*text).into(),locked:*locked,..Default::default()}).collect();
        let mut r=Report::default();inspect(&plan,&mut r);r
    }
    #[test] fn known_block_migration_is_idempotent_and_preserves_custom_settings_text() {
        let custom=format!("Custom tone and {{duration}}.\r\n{}\r\nKeep my closing question.",LEGACY_CRITICAL_BLOCK.replace('\n',"\r\n"));
        let once=migrate(&custom);assert!(once.contains("Keep my closing question."));
        assert!(once.contains("Custom tone and {duration}"));assert!(!once.contains("CRITICAL PATCH RULES"));
        assert_eq!(migrate(&once),once);assert_eq!(once.matches("DIRECT NARRATION POLICY v2").count(),1);
        assert_eq!(migrate("My unrelated custom prompt"),"My unrelated custom prompt");
        let different=LEGACY_CRITICAL_BLOCK.replace("booking charges","my custom charges");
        assert!(migrate(&different).contains("my custom charges"));
    }
    #[test] fn meta_references_block_but_quotes_raw_dialogue_and_locks_remain_faithful() {
        for line in ["In this clip, police approach.","The video doesn't provide a final outcome.","Trong video này, cảnh sát bước vào."] {
            assert_eq!(report(&[("host",line,false)]).issues[0].code,"US_META_REFERENCE");
        }
        for (kind,line,locked) in [("source","In this clip",false),("host","In this clip",true),
            ("host","A witness said, “In this clip, I was outside.”",false),
            ("host","She was arrested on verified charges. Should police have waited?",false),
            ("host","The video game store closes at nine.",false)] {
            assert!(report(&[(kind,line,locked)]).issues.is_empty());
        }
    }
    #[test] fn grammar_repetition_is_allowed_and_opening_checks_cross_scene_boundaries() {
        let r=report(&[("source","A raw hook.",false),("host","Police received a call in Ohio.",false),
            ("review","Police received a call in Ohio. She argued with her mother, then fought with officers.",false)]);
        assert!(r.issues.is_empty());assert_eq!(r.warnings.len(),1);
        let r=report(&[("host","She argued with her mother, then fought with officers.",false)]);
        assert!(r.issues.is_empty()&&r.warnings.is_empty());
        let r=report(&[("host","He said, “No, no, no!” Police booked him.",false)]);
        assert!(r.warnings.is_empty());
    }
}
