//! A generated project cannot finish with an empty Host evidence track.
//! Repair pictures independently of narration, WAVs and avatar checkpoints.
use crate::{evidence::EvidenceItem, model::*, process::Cancel};
use anyhow::{ensure, Result};
use serde_json::json;

pub const VERSION:&str="host-photo-coverage-v1";
fn auto_id(scene:&str)->String {format!("host-photo:{scene}")}
pub fn automatic(item:&EvidenceItem,scene:&str)->bool {item.id==auto_id(scene)}
fn photo(item:&EvidenceItem)->bool {
    item.enabled && !item.selection.text_only && item.selection.status!="text_only"
}
fn verified(p:&Project,seg:&Segment,item:&EvidenceItem)->bool {
    photo(item) && item.selection.passed()
        && item.selection.key==crate::evidence_selection::request_key(p,seg,item)
}
/// Add one neutral photographic illustration, not an invented factual card.
/// The existing closed-candidate selector must still approve its relevance.
pub fn seed_missing(p:&mut Project)->usize {
    if !p.evidence_settings.enabled{return 0;}
    let mut pending=vec![];
    for (index,seg) in p.segments.iter().enumerate().filter(|(_,s)|s.kind=="host") {
        if seg.evidence_items.iter().any(photo) || seg.locked
            || p.timeline.tracks[crate::timeline::EVIDENCE].locked
            || p.timeline.tracks[crate::timeline::ANNOTATION].locked
            || seg.evidence_items.len()>=4
            || seg.evidence_items.iter().any(|e|automatic(e,&seg.id)) {continue;}
        let Ok(source)=p.source(&seg.source_id) else{continue;};
        let end=seg.out_ms.min(source.duration_ms);
        let start=seg.in_ms.min(end);
        let time=[start+(end-start)/2,start,end.saturating_sub(100),start+(end-start)/3]
            .into_iter().find(|t|*t<source.duration_ms && !p.rules.excluded_ranges.iter()
                .any(|r|r.source_id==source.id && r.start_ms<=*t && *t<r.end_ms));
        let Some(time)=time else{continue;};
        pending.push((index,EvidenceItem{
            id:auto_id(&seg.id),source_id:source.id.clone(),source_time_ms:time,
            label:crate::content_language::text(&p.rules.language,"Ảnh hiện trường","Scene evidence").into(),
            visual_query:"A clear view of a person, action or location from this incident, relevant to the host narration. Illustrate observed source context only; do not claim the image proves off-screen actions, identities, diagnoses or legal outcomes.".into(),
            duration_ms:p.evidence_settings.default_duration_ms.min(seg.duration_ms.max(1)),
            ..Default::default()
        }));
    }
    let count=pending.len();
    for (index,item) in pending{p.segments[index].evidence_items.push(item);}
    count
}
fn missing(p:&Project,prepared:bool)->Vec<usize> {
    if !p.evidence_settings.enabled{return vec![];}
    let comp=prepared.then(||p.composition_snapshot());
    p.segments.iter().enumerate().filter(|(_,s)|s.kind=="host")
        .filter(|(index,seg)|!seg.evidence_items.iter().any(|item| {
            if !verified(p,seg,item){return false;}
            if !prepared{return true;}
            let start:u64=p.segments[..*index].iter().map(|s|s.duration_ms).sum();
            let end=start.saturating_add(seg.duration_ms);
            item.asset.as_ref().is_some_and(|a|image::image_dimensions(p.root.join(a)).is_ok())
                && comp.as_ref().is_some_and(|c|c.clips.iter().any(|clip|
                    clip.scene_id==seg.id && clip.evidence_id.as_deref()==Some(&item.id)
                    && clip.track==crate::timeline::EVIDENCE && clip.enabled(p)
                    && clip.duration_ms>0 && clip.transform.opacity>0.
                    && clip.start_ms<end && clip.start_ms.saturating_add(clip.duration_ms)>start
                    && clip.transform.width>0. && clip.transform.height>0.
                    && clip.media==item.asset.as_ref().map(|a|p.root.join(a))
                    && clip.transform.x+clip.transform.width/2.>0. && clip.transform.x-clip.transform.width/2.<1.
                    && clip.transform.y+clip.transform.height/2.>0. && clip.transform.y-clip.transform.height/2.<1.))
        })).map(|(i,_)|i+1).collect()
}
pub fn validate(p:&Project,prepared:bool)->Result<()> {
    let missing=missing(p,prepared);
    ensure!(missing.is_empty(),"Cảnh Host {} chưa có ảnh dẫn chứng phù hợp đã đối chiếu{}. Giọng và avatar đã lưu; bấm Kiểm tra / chọn lại ảnh dẫn chứng hoặc Thử lại để chỉ bổ sung ảnh. Kiểm tra cảnh/track bị khóa, ảnh bị ẩn, vùng nguồn bị loại hoặc đã đủ 4 thẻ. Tool không dùng ảnh ngẫu nhiên để báo hoàn tất.",
        missing.iter().map(usize::to_string).collect::<Vec<_>>().join(", "),if prepared{" và hiển thị trên timeline"}else{""});
    Ok(())
}
pub fn report(p:&Project,prepared:bool)->Result<()> {
    write_json(&p.root.join("analysis/host-evidence-report.json"),&json!({
        "version":VERSION,"enabled":p.evidence_settings.enabled,"prepared":prepared,
        "missing_host_scenes":missing(p,prepared),
        "scenes":p.segments.iter().enumerate().filter(|(_,s)|s.kind=="host").map(|(i,s)|json!({
            "scene":i+1,"scene_id":s.id,"cards":s.evidence_items.len(),
            "verified_photos":s.evidence_items.iter().filter(|e|verified(p,s,e)).count(),"locked":s.locked
        })).collect::<Vec<_>>()
    }))
}
pub fn prepare(p:&mut Project,s:&Settings,cancel:&Cancel,mut progress:impl FnMut(&str))->Result<()> {
    let added=seed_missing(p);
    if added>0{progress(&format!("Bổ sung ảnh cho {added} cảnh Host còn trống; giữ nguyên giọng và avatar"));}
    crate::evidence_selection::prepare(p,s,cancel,&mut progress)?;
    // An authored card may be rejected or downgraded to text-only. One bounded
    // fallback uses scene context, without retrying a rejected automatic photo.
    if seed_missing(p)>0 {crate::evidence_selection::prepare(p,s,cancel,&mut progress)?;}
    report(p,false)?;
    validate(p,false)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn project()->Project {
        let mut p=Project::default();p.rules.language="en".into();
        p.sources=vec![Source{id:"src".into(),duration_ms:10000,..Default::default()}];
        p.segments=vec![Segment{id:"host".into(),kind:"host".into(),source_id:"src".into(),in_ms:1000,out_ms:9000,
            duration_ms:5000,narration:"Officers approach the doorway.".into(),..Default::default()}];p
    }
    #[test] fn missing_hosts_get_one_stable_neutral_candidate_without_touching_voice_or_locks() {
        let mut p=project();p.segments[0].audio=Some("voice.wav".into());p.segments[0].avatar=Some("avatar.mkv".into());
        let before=serde_json::to_value(&p.segments[0]).unwrap();
        assert_eq!(seed_missing(&mut p),1);assert_eq!(seed_missing(&mut p),0);
        let item=p.segments[0].evidence_items.pop().unwrap();assert_eq!(item.kind,"frame");assert!(item.claim.is_empty()&&item.proof.quote.is_empty());
        assert_eq!(before,serde_json::to_value(&p.segments[0]).unwrap());
        p.segments[0].locked=true;assert_eq!(seed_missing(&mut p),0);assert!(validate(&p,false).is_err());
        p.segments[0].locked=false;p.evidence_settings.enabled=false;assert_eq!(seed_missing(&mut p),0);assert!(validate(&p,true).is_ok());
    }
    #[test] fn text_only_and_rejected_photos_never_satisfy_coverage_or_loop_forever() {
        let mut p=project();seed_missing(&mut p);
        let item=&mut p.segments[0].evidence_items[0];item.selection.text_only=true;item.selection.status="text_only".into();
        assert!(validate(&p,false).is_err());assert_eq!(seed_missing(&mut p),0);
        p.segments[0].evidence_items[0].enabled=false;assert!(validate(&p,false).is_err());
        assert_eq!(seed_missing(&mut p),0);
    }
    #[test] fn each_host_uses_its_own_source_and_respects_exclusions_and_card_limits() {
        let mut p=project();
        p.sources.push(Source{id:"other".into(),duration_ms:10000,..Default::default()});
        let mut second=p.segments[0].clone();second.id="host2".into();second.source_id="other".into();p.segments.push(second);
        p.rules.excluded_ranges.push(crate::model::TimeRange{source_id:"src".into(),start_ms:4000,end_ms:6000});
        assert_eq!(seed_missing(&mut p),2);
        assert_eq!(p.segments[0].evidence_items[0].source_time_ms,1000);
        assert_eq!(p.segments[1].evidence_items[0].source_id,"other");
        assert_eq!(p.segments[1].evidence_items[0].source_time_ms,5000);
        p.segments[0].evidence_items=vec![EvidenceItem{enabled:false,..Default::default()};4];
        assert_eq!(seed_missing(&mut p),0);
        assert!(validate(&p,false).is_err());
    }
}
