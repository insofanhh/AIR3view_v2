//! Bounded, durable recovery of measured speech/timeline duration. User rules stay immutable.
use crate::{diagnostics, media, model::*, process::Cancel};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fmt, fs, path::Path, sync::atomic::Ordering};

pub const MAX_RETRIES: u8 = 3;
pub const MAX_AI_REPAIRS: u8 = 2;
pub const SMALL_SHORTFALL_MS: u64 = 2000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mismatch { pub actual_ms: u64, pub min_ms: u64, pub max_ms: u64 }
impl Mismatch {
    pub fn shortfall(&self) -> u64 { self.min_ms.saturating_sub(self.actual_ms) }
    pub fn from_project(p: &Project) -> Option<Self> {
        let actual_ms = if p.composition.is_some() { p.composition_duration() } else { p.duration_ms() };
        let m = Self { actual_ms, min_ms: p.rules.min_seconds * 1000, max_ms: p.rules.max_seconds * 1000 };
        (actual_ms < m.min_ms || actual_ms > m.max_ms).then_some(m)
    }
}
impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Timeline dài {:.3}s ({} ms), mục tiêu {}–{}s; {} {} ms. Điều chỉnh cảnh trước khi xuất.",
            self.actual_ms as f64 / 1000., self.actual_ms, self.min_ms / 1000, self.max_ms / 1000,
            if self.shortfall() > 0 { "thiếu" } else { "thừa" },
            if self.shortfall() > 0 { self.shortfall() } else { self.actual_ms.saturating_sub(self.max_ms) })
    }
}
impl std::error::Error for Mismatch {}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Recovery {
    pub signature: String,
    pub auto_attempts: u8,
    pub ai_attempts: u8,
    pub status: String,
    pub last_actual_ms: u64,
}
fn signature(p: &Project, s: &Settings) -> String {
    let sources: Vec<_> = p.sources.iter().map(|v| (&v.id, &v.url, &v.path)).collect();
    media::hash(serde_json::to_vec(&(&p.id, &p.rules, &p.output, &p.production, sources,
        &s.provider, &s.cli_model, &s.gemini_model, &s.voice, s.voice_reference_enabled,
        crate::input_assets::file_fingerprint(&s.voice_reference_path))).unwrap_or_default())
}
fn read(path: &Path) -> Result<Recovery> {
    let backup = path.with_extension("json.bak");
    if !path.exists() && !backup.exists() { return Ok(Recovery::default()); }
    let result = [path, backup.as_path()].into_iter().find_map(|v| fs::read(v).ok().and_then(|b| serde_json::from_slice(&b).ok()));
    result.context("Không đọc được checkpoint phục hồi thời lượng; giữ nguyên dữ liệu để tránh thử lại vượt giới hạn")
}
impl Recovery {
    pub fn load(p: &Project, s: &Settings) -> Result<Self> {
        let old = read(&p.root.join("duration-recovery.json"))?;
        let signature = signature(p, s);
        Ok(if old.signature == signature { old } else { Self { signature, ..Self::default() } })
    }
    pub fn save(&self, p: &Project) -> Result<()> { write_json(&p.root.join("duration-recovery.json"), self) }
    pub fn retry(p: &Project, s: &Settings, m: &Mismatch, cancel: &Cancel) -> Result<Option<u8>> {
        crate::process::check_cancel(cancel)?;
        if m.shortfall() == 0 { return Ok(None); }
        let mut state = Self::load(p, s)?;
        state.last_actual_ms = m.actual_ms;
        if state.auto_attempts >= MAX_RETRIES {
            state.status = "exhausted".into(); state.save(p)?; return Ok(None);
        }
        state.auto_attempts += 1;
        state.status = "retrying".into();
        state.save(p)?; // Reserve the attempt before any worker/AI work, including after a crash.
        diagnostics::record(&p.root, "Duration", None, "auto_retry", serde_json::json!({
            "attempt":state.auto_attempts,"limit":MAX_RETRIES,"actual_ms":m.actual_ms,
            "min_ms":m.min_ms,"max_ms":m.max_ms,"shortfall_ms":m.shortfall()}))?;
        if cancel.load(Ordering::Relaxed) { crate::process::check_cancel(cancel)?; }
        Ok(Some(state.auto_attempts))
    }
    pub fn reserve_ai(p: &Project, s: &Settings) -> Result<Option<u8>> {
        let mut state = Self::load(p, s)?;
        if state.ai_attempts >= MAX_AI_REPAIRS { return Ok(None); }
        state.ai_attempts += 1; state.save(p)?;
        Ok(Some(state.ai_attempts))
    }
    pub fn complete(p: &Project, s: &Settings) -> Result<()> {
        let mut state = Self::load(p, s)?;
        state.status = "complete".into(); state.last_actual_ms = p.composition_duration(); state.save(p)
    }
    pub fn reset(p: &Project, s: &Settings) -> Result<()> {
        Self { signature:signature(p,s), ..Self::default() }.save(p)
    }
    /// A deliberate UI/CLI Retry starts a new bounded cycle. Resume after a
    /// crash and the internal automatic loop must never call this method.
    pub fn manual_retry(root: &Path) -> Result<()> {
        let path=root.join("duration-recovery.json");
        if !path.exists() && !path.with_extension("json.bak").exists() { return Ok(()); }
        let old=read(&path)?;
        let backup=root.join("analysis/duration-manual-retries").join(format!("{}.json",uuid::Uuid::new_v4()));
        write_json(&backup,&old)?;
        write_json(&path,&Self{signature:old.signature,status:"manual_retry".into(),..Self::default()})
    }
}

/// Internal target only: never write these bounds into ScriptRules.
pub fn target(p: &Project) -> (u64, u64) {
    let min = p.rules.min_seconds * 1000;
    let max = p.rules.max_seconds * 1000;
    let margin = 2000.min((max - min) / 4);
    (min + margin, max - margin)
}

/// Grow only an unlocked original clip, within the source, intercut and exclusion bounds.
/// This creates real picture/audio time and leaves every speech WAV and its clock intact.
pub fn extend_short_source(p: &mut Project) -> Result<bool> {
    let Some(m) = Mismatch::from_project(p) else { return Ok(false); };
    if m.shortfall() == 0 || m.shortfall() > SMALL_SHORTFALL_MS { return Ok(false); }
    let frame_ms = 1000u64.div_ceil(p.output.fps.max(1) as u64);
    let needed = m.shortfall() + frame_ms.min(m.max_ms.saturating_sub(m.min_ms));
    for i in (0..p.segments.len()).rev() {
        let seg = &p.segments[i];
        if seg.kind != "source" || crate::timeline::editable(p, i).is_err() || (i == 0 && p.rules.hook_enabled) { continue; }
        let duration = seg.duration_ms + needed;
        if p.rules.intercuts_enabled && duration > p.rules.intercut_max_seconds * 1000 { continue; }
        let out_ms = seg.in_ms + crate::footage_speed::scaled(duration, seg.source_rate);
        if out_ms > p.source(&seg.source_id)?.duration_ms { continue; }
        if p.rules.excluded_ranges.iter().any(|r| r.source_id == seg.source_id && r.start_ms < out_ms && r.end_ms > seg.in_ms) { continue; }
        let mut next = p.clone(); next.composition = None;
        next.segments[i].duration_ms = duration; next.segments[i].out_ms = out_ms;
        next.segments[i].words.clear(); next.segments[i].timing_estimated = true;
        next.segments[i].footage_spans.clear();
        if crate::footage_speed::enabled(&next) { crate::footage_speed::fit(&mut next)?; }
        if next.check_duration().is_err() || next.validate(true).is_err() || next.validate_intercuts().is_err() { continue; }
        diagnostics::record(&p.root, "Duration", Some(&p.segments[i].id), "extend_original", serde_json::json!({
            "before_ms":m.actual_ms,"after_ms":next.duration_ms(),"added_ms":next.duration_ms().saturating_sub(m.actual_ms)}))?;
        *p = next; return Ok(true);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(root:&Path)->Project {
        fs::write(root.join("source.mp4"),b"fixture").unwrap();
        let mut p=Project {root:root.into(),sources:vec![Source{id:"src".into(),path:root.join("source.mp4"),duration_ms:60000,has_audio:true,..Default::default()}],
            segments:vec![Segment{source_id:"src".into(),in_ms:0,out_ms:94980,duration_ms:94980,..Default::default()}],..Default::default()};
        p.sources[0].duration_ms=120000;p.rules.min_seconds=95;p.rules.max_seconds=240;p.rules.hook_enabled=false;p.rules.intercuts_enabled=false;p
    }
    #[test]
    fn retries_and_ai_budgets_survive_reload_and_do_not_retry_other_errors_or_cancel() {
        let temp=tempfile::tempdir().unwrap();let mut p=fixture(temp.path());let s=Settings::default();
        let mismatch=Mismatch::from_project(&p).unwrap();
        assert!(p.check_duration().unwrap_err().downcast_ref::<Mismatch>().is_some());
        assert!(mismatch.to_string().contains("94.980s (94980 ms)"));
        for i in 1..=3 {assert_eq!(Recovery::retry(&p,&s,&mismatch,&crate::process::cancel_token()).unwrap(),Some(i));}
        assert_eq!(Recovery::retry(&p,&s,&mismatch,&crate::process::cancel_token()).unwrap(),None);
        assert_eq!(Recovery::load(&p,&s).unwrap().auto_attempts,3);
        assert_eq!(Recovery::reserve_ai(&p,&s).unwrap(),Some(1));
        assert_eq!(Recovery::reserve_ai(&p,&s).unwrap(),Some(2));
        assert_eq!(Recovery::reserve_ai(&p,&s).unwrap(),None);
        Recovery::manual_retry(&p.root).unwrap();
        assert_eq!(Recovery::load(&p,&s).unwrap().auto_attempts,0);
        assert_eq!(Recovery::load(&p,&s).unwrap().ai_attempts,0);
        fs::remove_file(p.root.join("duration-recovery.json")).unwrap();
        Recovery::manual_retry(&p.root).unwrap();
        assert_eq!(Recovery::load(&p,&s).unwrap().auto_attempts,0);
        assert_eq!(Recovery::load(&p,&s).unwrap().ai_attempts,0);
        for _ in 0..3 {Recovery::retry(&p,&s,&mismatch,&crate::process::cancel_token()).unwrap();}
        let cancelled=crate::process::cancel_token();cancelled.store(true,Ordering::Relaxed);
        assert!(Recovery::retry(&p,&s,&mismatch,&cancelled).is_err());
        assert_eq!(Recovery::load(&p,&s).unwrap().auto_attempts,3);
        p.rules.min_seconds=96;assert_eq!(Recovery::load(&p,&s).unwrap().auto_attempts,0);
        Recovery::reset(&p,&s).unwrap();
        fs::write(p.root.join("duration-recovery.json"),"broken").unwrap();
        fs::write(p.root.join("duration-recovery.json.bak"),"broken").unwrap();
        assert!(Recovery::load(&p,&s).is_err());
    }
    #[test]
    fn small_deficit_uses_real_source_time_preserving_rules_and_protected_intervals() {
        let temp=tempfile::tempdir().unwrap();let mut p=fixture(temp.path());
        let rules=serde_json::to_vec(&p.rules).unwrap();
        assert!(extend_short_source(&mut p).unwrap());p.check_duration().unwrap();
        assert_eq!(p.segments[0].out_ms,p.segments[0].duration_ms);
        assert_eq!(serde_json::to_vec(&p.rules).unwrap(),rules);
        for mode in ["locked","track","excluded","eof","large"] {
            let mut p=fixture(temp.path());
            match mode {
                "locked"=>p.segments[0].locked=true,
                "track"=>p.timeline.tracks[crate::timeline::ORIGINAL].locked=true,
                "excluded"=>p.rules.excluded_ranges.push(TimeRange{source_id:"src".into(),start_ms:94980,end_ms:120000}),
                "eof"=>p.sources[0].duration_ms=94980,
                _=>{p.segments[0].duration_ms=84000;p.segments[0].out_ms=84000;}
            }
            let before=serde_json::to_vec(&p).unwrap();assert!(!extend_short_source(&mut p).unwrap(),"{mode}");
            assert_eq!(serde_json::to_vec(&p).unwrap(),before);
        }
    }
    #[test]
    fn internal_target_has_margin_without_changing_user_rules() {
        let temp=tempfile::tempdir().unwrap();let mut p=fixture(temp.path());
        assert_eq!(target(&p),(97000,238000));assert_eq!(p.rules.min_seconds,95);
        p.rules.max_seconds=96;assert_eq!(target(&p),(95250,95750));
        p.rules.max_seconds=95;assert_eq!(target(&p),(95000,95000));
    }
}
