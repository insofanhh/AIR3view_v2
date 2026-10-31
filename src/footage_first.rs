//! Script generation policy when the reviewer avatar is disabled.
//! Existing/manual timelines are not rewritten when opened.
use crate::{model::*, script_quality::{Issue, Report}};

pub const VERSION: &str = "footage-first-v1";
pub const MAX_HOST_MS: u64 = 10_000;

pub fn enabled(p: &Project, s: &Settings) -> bool {
    !p.production.as_ref().map_or(s.avatar_enabled, |v| v.avatar_enabled)
}

pub fn contract() -> &'static str {
    "\nFOOTAGE-FIRST MODE — AVATAR DISABLED (overrides generic Host/avatar staging): Create exactly ONE short kind=host scene, normally the first spoken scene after the optional raw hook. Aim for five to eight seconds, at most ten seconds of actual speech. Use it for one punchy hook/context sentence, supported by a relevant source photo when evidence is enabled; no avatar is rendered. ALL remaining spoken scenes, including the twist, impact and ending question, must be kind=review over verified highlight footage from the main source. Select original source_id/in_ms/out_ms for EACH review based on the action/quote that supports its narration; never convert a Host with zero timestamps into random footage. Prefer concise action-matched highlights and analytical commentary. Keep source-audio intercuts at meaningful moments, respecting the configured hook, fixed count or cadence, source speed, exclusions, total duration and language. Do not add extra raw clips when disabled. Source clips contain no TTS. Voice stays at 1x. Do not stretch a still, repeat footage, invent events or pad silence to meet duration. All unlocked scenes use motion=none. Preserve locked scenes exactly; a locked Host counts toward the one Host allowance, and explain any unavoidable locked-scene exception. Other settings/factual/empathy requirements still apply."
}

/// Invisible avatar metadata must not consume a factual repair round.
/// Only fresh generated candidates pass here; never mutate locked/manual scenes.
pub fn normalize(p: &Project, s: &Settings, plan: &mut ScriptPlan) {
    if !enabled(p, s) { return; }
    for scene in plan.segments.iter_mut().filter(|v| !v.locked) {
        scene.gesture = "none".into();
        scene.motion = "none".into();
    }
}

/// A brief context scene still has a picture when evidence cards are disabled.
/// It is a sourced still, not an avatar placeholder or arbitrary video padding.
pub fn context_path(p:&Project,scene:&Segment)->Option<std::path::PathBuf> {
    if p.script_presentation!=VERSION || scene.kind!="host" || scene.avatar.is_some()
        || (p.evidence_settings.enabled&&scene.evidence_items.iter().any(|e|e.enabled&&e.asset.is_some())) {return None;}
    let source=p.source(&scene.source_id).ok()?;
    let stamp=std::fs::metadata(source.input_path()).ok().map(|m|(m.len(),m.modified().ok()));
    let key=crate::media::hash(format!("context-v2:{:?}:{stamp:?}:{}:{:?}:{}",source.input_path(),scene.in_ms,p.output.dimensions(),crate::source_masks::identity(p)));
    Some(p.root.join("assets/host-context").join(format!("{key}.png")))
}

pub fn prepare_contexts(p:&Project,s:&Settings,cancel:&crate::process::Cancel)->anyhow::Result<()> {
    use anyhow::ensure;
    for scene in &p.segments {
        let Some(path)=context_path(p,scene) else{continue;};
        let (w,h)=p.output.dimensions();
        if image::image_dimensions(&path).ok()==Some((w,h)){continue;}
        crate::process::check_cancel(cancel)?;
        let source=p.source(&scene.source_id)?;
        ensure!(scene.in_ms<source.duration_ms&&!p.rules.excluded_ranges.iter().any(|r|r.source_id==source.id&&r.start_ms<=scene.in_ms&&scene.in_ms<r.end_ms),
            "Host không nhân vật cần một mốc ảnh nguồn hợp lệ ngoài vùng đã loại; tạo lại kịch bản hoặc chỉnh mốc Host");
        let mut frame=crate::media::frame(s,source.input_path(),scene.in_ms,cancel)?;
        crate::source_masks::mask_image(p,&scene.source_id,scene.in_ms,&mut frame);
        let scale=(w as f64/frame.width() as f64).min(h as f64/frame.height() as f64);
        let fw=(frame.width() as f64*scale).round().max(1.) as u32;
        let fh=(frame.height() as f64*scale).round().max(1.) as u32;
        let fit=image::imageops::resize(&frame,fw,fh,image::imageops::FilterType::Triangle);
        let mut canvas=image::RgbaImage::from_pixel(w,h,image::Rgba([11,18,32,255]));
        image::imageops::overlay(&mut canvas,&fit,((w-fw)/2) as i64,((h-fh)/2) as i64);
        std::fs::create_dir_all(path.parent().unwrap())?;
        canvas.save(path)?;
    }
    Ok(())
}

pub fn inspect(p: &Project, s: &Settings, plan: &ScriptPlan, r: &mut Report, measured: bool) {
    if !enabled(p, s) { return; }
    let hosts: Vec<_> = plan.segments.iter().enumerate().filter(|(_, v)| v.kind == "host").collect();
    let locked = hosts.iter().filter(|(_, v)| v.locked).count();
    let expected = locked.max(1);
    let rate = r.units_per_second;
    let mut add = |scene, message: String| r.issues.push(Issue {
        code: "FOOTAGE_FIRST".into(), scene, item: None, path: "segments".into(),
        label: String::new(), message, affects_narration: true,
    });
    if hosts.len() != expected {
        add(0, format!("Nhân vật đang tắt: chỉ giữ một Host mở đầu 5–8 giây (tối đa 10 giây). Hiện có {} Host. Phần lời còn lại phải là review với highlight nguồn đúng nội dung và đủ mốc in/out, giữ nguyên clip gốc/cảnh khóa/Độ dài. Không đổi loại cảnh mà giữ mốc 0/0.", hosts.len()));
    }
    if !plan.segments.iter().any(|v|v.kind=="review") && p.rules.min_seconds>10 {
        add(0,"Nhân vật đang tắt: cần các cảnh review dùng highlight nguồn cho phần lời sau Host; không dùng Host hoặc clip im lặng để lấp thời lượng.".into());
    }
    for (i, host) in &hosts {
        if host.locked { continue; }
        let duration = if measured || host.audio.is_some() { host.duration_ms }
            else { (crate::script_quality::units(&host.narration) as f64 / rate * 1000.).round() as u64 };
        if duration > MAX_HOST_MS {
            add(i + 1, format!("Host khi tắt nhân vật đang dài {:.1}s, tối đa 10s. Rút thành một câu hook/bối cảnh 5–8s; chuyển phân tích còn lại sang các review có footage khớp, không tăng tốc voice hoặc đổi Độ dài.", duration as f64 / 1000.));
        }
    }
    if locked == 0 && hosts.len() == 1 && plan.segments.iter().position(|v| v.kind != "source") != Some(hosts[0].0) {
        add(0, "Đặt Host ngắn ở cảnh lời đầu tiên sau hook; mọi phần lời sau đó dùng review trên highlight nguồn.".into());
    }
    if locked > 0 {
        r.warnings.push("Giữ nguyên Host đã khóa; giới hạn một Host ngắn áp dụng cho các cảnh được phép tạo lại.".into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan() -> ScriptPlan {
        ScriptPlan{title:"Incident".into(),analysis_summary:String::new(),segments:vec![
            Segment{kind:"source".into(),duration_ms:3000,..Default::default()},
            Segment{kind:"host".into(),narration:"A routine stop turns urgent when an officer spots a handgun.".into(),duration_ms:6000,..Default::default()},
            Segment{kind:"review".into(),in_ms:10000,out_ms:30000,narration:"Officers take cover behind their cruisers.".into(),..Default::default()},
        ]}
    }
    fn report(p:&Project,plan:&ScriptPlan,measured:bool)->Report {
        let mut r=Report{units_per_second:2.5,..Default::default()};
        inspect(p,&Settings::default(),plan,&mut r,measured);r
    }
    #[test]
    fn one_brief_host_then_source_highlights_and_measured_voice_limit() {
        let p=Project::default();let mut plan=plan();
        assert!(report(&p,&plan,false).issues.is_empty());
        plan.segments[1].duration_ms=11000;
        assert!(report(&p,&plan,false).issues.is_empty());
        assert_eq!(report(&p,&plan,true).issues[0].scene,2);
        plan.segments[1].duration_ms=7000;
        plan.segments[2].kind="host".into();
        assert!(report(&p,&plan,false).issues.iter().any(|i|i.scene==0));
        plan.segments[2].kind="review".into();plan.segments[1].kind="review".into();
        assert!(report(&p,&plan,false).issues.iter().any(|i|i.scene==0));
    }
    #[test]
    fn disabled_metadata_is_local_only_and_locked_source_and_rules_stay_exact() {
        let p=Project::default();let mut plan=plan();
        plan.segments[1].gesture="open_palms".into();
        plan.segments[2].locked=true;plan.segments[2].gesture="beat".into();
        let locked=serde_json::to_value(&plan.segments[2]).unwrap();
        let source=serde_json::to_value(&plan.segments[0]).unwrap();
        normalize(&p,&Settings::default(),&mut plan);
        assert_eq!(plan.segments[1].gesture,"none");
        assert_eq!(plan.segments[1].motion,"none");
        assert_eq!(serde_json::to_value(&plan.segments[2]).unwrap(),locked);
        assert_eq!(serde_json::to_value(&plan.segments[0]).unwrap()["duration_ms"],source["duration_ms"]);
        assert_eq!(plan.segments[2].in_ms,10000);assert_eq!(plan.segments[2].out_ms,30000);
    }
    #[test]
    fn project_overrides_global_and_enabled_avatar_keeps_legacy_staging() {
        let mut p=Project::default();let mut plan=plan();let s=Settings{avatar_enabled:true,..Default::default()};
        plan.segments[1].gesture="beat".into();
        normalize(&p,&s,&mut plan);assert_eq!(plan.segments[1].gesture,"beat");
        p.production=Some(ProductionSettings{avatar_enabled:false,..Default::default()});
        normalize(&p,&s,&mut plan);assert_eq!(plan.segments[1].gesture,"none");
        p.production.as_mut().unwrap().avatar_enabled=true;
        assert!(report(&p,&plan,true).issues.is_empty());
        let old:Project=serde_json::from_value(serde_json::json!({"title":"Legacy"})).unwrap();
        assert!(old.script_presentation.is_empty());
    }
    #[test]
    fn locked_host_exceptions_do_not_force_destructive_rewrites() {
        let p=Project::default();let mut plan=plan();
        plan.segments[1].locked=true;plan.segments[1].duration_ms=25000;
        plan.segments.push(plan.segments[1].clone());
        let r=report(&p,&plan,true);assert!(r.issues.is_empty());assert!(!r.warnings.is_empty());
    }
    #[test]
    fn shortage_never_creates_extra_hosts_or_changes_saved_speed_preferences() {
        let mut p=Project{script_presentation:VERSION.into(),..Default::default()};
        p.rules.hook_enabled=false;p.rules.intercuts_enabled=false;
        p.output.footage_speed.mode="auto".into();p.output.footage_speed.allow_host=true;
        p.sources.push(Source{id:"s".into(),duration_ms:30000,..Default::default()});
        p.segments.push(Segment{kind:"review".into(),source_id:"s".into(),in_ms:10000,out_ms:15000,duration_ms:10000,..Default::default()});
        let before=serde_json::to_vec(&p).unwrap();
        let error=crate::footage_speed::fit(&mut p).unwrap_err();
        assert!(error.to_string().contains("chỉ dùng một Host"));
        assert_eq!(before,serde_json::to_vec(&p).unwrap());
        p.script_presentation.clear();crate::footage_speed::fit(&mut p).unwrap();
        assert_eq!(p.segments[0].kind,"host");
    }
    #[test]
    fn host_without_avatar_or_evidence_cards_has_a_real_context_picture_and_voice() {
        use crate::{media,process,timeline};use std::time::Duration;
        let temp=tempfile::tempdir().unwrap();let cancel=process::cancel_token();let settings=Settings::default();
        let source=temp.path().join("source.mp4");let voice=temp.path().join("voice.wav");
        process::run(&settings.ffmpeg,&process::args(&["-y","-v","error","-f","lavfi","-i","color=c=blue:s=320x180:d=2","-c:v","libx264",&source.to_string_lossy()]),None,&cancel,Duration::from_secs(20),None).unwrap();
        process::run(&settings.ffmpeg,&process::args(&["-y","-v","error","-f","lavfi","-i","sine=frequency=600:duration=1","-c:a","pcm_s16le",&voice.to_string_lossy()]),None,&cancel,Duration::from_secs(20),None).unwrap();
        let mut p=Project{root:temp.path().into(),script_presentation:VERSION.into(),..Default::default()};
        p.rules.min_seconds=1;p.rules.max_seconds=10;p.rules.hook_enabled=false;p.rules.intercuts_enabled=false;
        p.evidence_settings.enabled=false;p.subtitles.enabled=false;p.output.resolution=360;p.output.fps=24;
        let mut src=media::probe(&settings,&source,&cancel).unwrap();src.id="s".into();p.sources.push(src);
        p.segments.push(Segment{kind:"host".into(),source_id:"s".into(),duration_ms:1000,narration:"Officers take cover.".into(),audio:Some(voice),..Default::default()});
        prepare_contexts(&p,&settings,&cancel).unwrap();p.ensure_composition();
        let c=p.composition_snapshot();assert!(!c.clips.iter().any(|c|c.track==timeline::AVATAR));
        assert!(c.clips.iter().any(|c|c.track==timeline::VOICE));
        let output=temp.path().join("final.mp4");media::render(&p,&settings,&output,&cancel,|_,_|{}).unwrap();
        let meta=media::probe(&settings,&output,&cancel).unwrap();assert!(meta.has_audio);assert!(meta.duration_ms.abs_diff(1000)<100);
        let image=media::frame(&settings,&output,500,&cancel).unwrap();let pixel=image.get_pixel(image.width()/2,image.height()/2);
        assert!(pixel[2]>170&&pixel[0]<35&&pixel[1]<35);
        // Upgrade a saved composition with the old empty avatar placeholder.
        let mut empty=c.clips.iter().find(|c|c.track==timeline::VOICE).unwrap().clone();empty.track=timeline::AVATAR;empty.media=None;
        p.composition.as_mut().unwrap().clips.push(empty);p.ensure_composition();
        assert!(!p.composition.as_ref().unwrap().clips.iter().any(|c|c.track==timeline::AVATAR));
    }
}
