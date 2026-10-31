//! Footage uses source time; speech, gestures and captions use output time.
use crate::model::*;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

pub fn one() -> f64 { 1. }
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub mode: String,
    pub preferred: f64,
    pub maximum: f64,
    pub hook: f64,
    pub intercut: f64,
    pub allow_host: bool,
}
impl Default for Settings {
    fn default() -> Self { Self { mode:"off".into(), preferred:1.25, maximum:1.5, hook:1., intercut:1., allow_host:false } }
}
impl Settings {
    pub fn validate(&self)->Result<()> {
        ensure!(matches!(self.mode.as_str(),"off"|"auto"|"fixed"),"Chế độ tốc độ footage không hợp lệ");
        for value in [self.preferred,self.maximum,self.hook,self.intercut] {ensure!(valid_rate(value),"Tốc độ footage phải trong 1–2×");}
        ensure!(self.preferred<=self.maximum,"Tốc độ ưu tiên không được lớn hơn tốc độ tối đa");
        Ok(())
    }
}
pub fn valid_rate(rate:f64)->bool {rate.is_finite() && (1. ..=2.).contains(&rate)}
pub fn scaled(ms:u64,rate:f64)->u64 {(ms as f64*rate).round() as u64}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Anchor {
    pub phrase:String,
    pub source_ms:u64,
    pub timeline_ms:u64,
    pub manual:bool,
    pub status:String,
    pub alignment_key:String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Span {
    pub start_ms:u64,
    pub duration_ms:u64,
    pub source_in_ms:u64,
    pub source_out_ms:u64,
}
impl Span {
    pub fn rate(&self)->f64 {self.source_out_ms.saturating_sub(self.source_in_ms) as f64/self.duration_ms.max(1) as f64}
}
pub fn anchor_status(status:&str)->&str {
    match status {"aligned"=>"Đã căn ASR", "manual"=>"Mốc tay", "estimated"=>"Ước lượng · chưa dùng", "ambiguous"=>"Cụm từ bị lặp", "conflict"=>"Xung đột tốc độ", "not_found"=>"Không tìm thấy cụm từ", _=>"Chờ căn từ"}
}
fn token(s:&str)->String {s.chars().filter(|c|c.is_alphanumeric()).flat_map(char::to_lowercase).collect()}
fn alignment_prefix(seg:&Segment)->String {
    crate::media::hash(serde_json::to_vec(&(&seg.asset_key,&seg.audio,seg.asset_start_ms,seg.duration_ms,&seg.narration)).unwrap_or_default())
}
pub fn align(seg:&mut Segment) {
    let prefix=alignment_prefix(seg);
    let words:Vec<_>=seg.words.iter().map(|w|token(&w.text)).collect();
    for a in &mut seg.footage_anchors {
        if a.manual {a.status="manual".into();continue;}
        let key=crate::media::hash(format!("{prefix}:{}",a.phrase));
        if matches!(a.status.as_str(),"aligned"|"conflict")&&a.alignment_key==key {a.status="aligned".into();continue;}
        a.alignment_key.clear();
        let needle:Vec<_>=a.phrase.split_whitespace().map(token).filter(|v|!v.is_empty()).collect();
        let hits:Vec<_>=if needle.is_empty(){vec![]}else{words.windows(needle.len()).enumerate().filter(|(_,w)|*w==needle).map(|(i,_)|i).collect()};
        if hits.len()==1 {
            a.timeline_ms=seg.words[hits[0]].start_ms;
            a.status=if seg.timing_estimated {"estimated"}else{"aligned"}.into();
            if !seg.timing_estimated {a.alignment_key=key;}
        }else{a.status=if hits.is_empty(){"not_found"}else{"ambiguous"}.into();}
    }
}
/// Verify each anchor on raw ASR, even when other caption words were interpolated.
pub fn align_measured(seg:&mut Segment,words:&[Word]) {
    let mut measured=seg.clone();measured.words=words.to_vec();measured.timing_estimated=false;
    for a in &mut measured.footage_anchors {a.alignment_key.clear();}
    align(&mut measured);seg.footage_anchors=measured.footage_anchors;
}
pub fn enabled(p:&Project)->bool {p.output.footage_speed.mode!="off"||p.segments.iter().any(|s|s.footage_rate_override.is_some())}

/// Atomic: an impossible anchor or a locked scene never partly edits the project.
pub fn fit(p:&mut Project)->Result<Vec<String>> {
    p.output.footage_speed.validate()?;
    let mut next=p.clone();let mut messages=vec![];
    for i in 0..next.segments.len() {
        let mut s=next.segments[i].clone();
        if s.locked || next.timeline.tracks.iter().enumerate().any(|(t,v)|v.locked&&crate::timeline::has_track(&s,t)) {continue;}
        if s.kind=="host" {continue;}
        ensure!(s.in_ms<s.out_ms && s.duration_ms>0,"Cảnh {}: điểm vào/ra hoặc thời lượng không hợp lệ",i+1);
        let src=next.source(&s.source_id)?;
        ensure!(src.duration_ms==0||s.out_ms<=src.duration_ms,"Cảnh {}: điểm ra vượt video nguồn",i+1);
        ensure!(!next.rules.excluded_ranges.iter().any(|r|r.source_id==s.source_id&&s.in_ms<r.end_ms&&r.start_ms<s.out_ms),"Cảnh {}: vùng nguồn chạm khoảng đã loại",i+1);
        let cfg=&next.output.footage_speed;
        let requested=s.footage_rate_override.unwrap_or(if cfg.mode=="off" {1.}else if s.kind=="source" {if i==0&&next.rules.hook_enabled {cfg.hook}else{cfg.intercut}}else{cfg.preferred});
        ensure!(valid_rate(requested),"Cảnh {}: tốc độ phải trong 1–2×",i+1);
        let maximum=if cfg.mode=="fixed"||s.footage_rate_override.is_some(){requested}else{cfg.maximum};
        let fixed=cfg.mode=="fixed"||s.footage_rate_override.is_some();
        s.footage_spans.clear();
        if s.kind=="source" {
            let min=if i==0&&next.rules.hook_enabled {3000}else if next.rules.intercuts_enabled {next.rules.intercut_min_seconds*1000}else{100};
            let rate=requested.min((s.out_ms-s.in_ms) as f64/min.max(1) as f64).max(1.);
            s.source_rate=rate;s.duration_ms=((s.out_ms-s.in_ms) as f64/rate).round() as u64;
            // Original captions must use the new output clock, not source time.
            if (rate-next.segments[i].source_rate).abs()>0.00001 {s.words.clear();s.timing_estimated=true;}
        }else{
            align(&mut s);
            let mut begin=s.in_ms;
            let mut end=s.in_ms+scaled(s.duration_ms,requested).min(s.out_ms-s.in_ms);
            if end-s.in_ms<s.duration_ms {
                if cfg.allow_host && next.script_presentation!=crate::footage_first::VERSION && !s.footage_anchors.iter().any(|a|a.manual) {
                    s.kind="host".into();s.footage_status="Chuyển Host: không đủ footage trong vùng đã chọn".into();
                    messages.push(format!("Cảnh {}: {}",i+1,s.footage_status));next.segments[i]=s;continue;
                }
                let advice=if next.script_presentation==crate::footage_first::VERSION {"Mở rộng điểm cắt trong cùng tình huống hoặc rút lời; chế độ không nhân vật chỉ dùng một Host ngắn"}else{"Mở rộng điểm cắt trong cùng tình huống hoặc bật chuyển Host"};
                anyhow::bail!("Cảnh {}: voice {:.2}s nhưng vùng nguồn chỉ {:.2}s. {advice}; voice được giữ nguyên.",i+1,s.duration_ms as f64/1000.,(s.out_ms-s.in_ms) as f64/1000.);
            }
            ensure!(!fixed || end-s.in_ms+2>=scaled(s.duration_ms,requested),"Cảnh {}: không đủ footage cho tốc độ cố định {requested:.2}×. Chọn Tự động hoặc mở rộng vùng nguồn; voice giữ nguyên.",i+1);
            let mut points=vec![];
            for a in &mut s.footage_anchors {
                ensure!(!a.manual || a.timeline_ms<=s.duration_ms&&(s.in_ms..=s.out_ms).contains(&a.source_ms),"Cảnh {}: mốc tay nằm ngoài cảnh hoặc vùng nguồn",i+1);
                if (cfg.mode!="off"||s.footage_rate_override.is_some()) && matches!(a.status.as_str(),"aligned"|"manual") && a.timeline_ms<=s.duration_ms {
                    let before_ok=a.source_ms>=s.in_ms.saturating_add(a.timeline_ms);
                    let after_ok=a.source_ms.saturating_add(s.duration_ms-a.timeline_ms)<=s.out_ms;
                    let previous_ok=points.last().is_none_or(|&(t,v)|a.timeline_ms>t&&a.source_ms>v&&(1.-0.002..=maximum+0.002).contains(&((a.source_ms-v) as f64/(a.timeline_ms-t) as f64)));
                    let fixed_ok=!fixed||a.source_ms.abs_diff(s.in_ms+scaled(a.timeline_ms,requested))<=2;
                    if !(before_ok&&after_ok&&previous_ok&&fixed_ok) {
                        ensure!(!a.manual&& !fixed,"Cảnh {}: mốc '{}' không thể khớp trong giới hạn tốc độ; chỉnh mốc hoặc điểm cắt trong Thuộc tính cảnh.",i+1,a.phrase);
                        a.status="conflict".into();continue;
                    }
                    points.push((a.timeline_ms,a.source_ms));
                }
            }
            points.sort_by_key(|v|v.0);
            if let Some(&(time,source))=points.first() {
                let low=s.in_ms.max(source.saturating_sub(scaled(time,maximum)));
                let high=source.saturating_sub(time);
                ensure!(high>=low,"Cảnh {}: không đủ hình trước mốc hành động; mở rộng điểm vào nguồn",i+1);
                begin=source.saturating_sub(scaled(time,requested)).clamp(low,high);
            }
            if let Some(&(time,source))=points.last() {
                let remain=s.duration_ms-time;
                let low=source+remain;let high=s.out_ms.min(source+scaled(remain,maximum));
                ensure!(high>=low,"Cảnh {}: không đủ hình sau mốc hành động; mở rộng điểm ra nguồn",i+1);
                end=(source+scaled(remain,requested)).clamp(low,high);
            }
            if points.first().is_none_or(|v|v.0>0) {points.insert(0,(0,begin));}
            if points.last().is_none_or(|v|v.0<s.duration_ms) {points.push((s.duration_ms,end));}
            for pair in points.windows(2) {
                let ((t0,v0),(t1,v1))=(pair[0],pair[1]);
                ensure!(t1>t0&&v1>v0,"Cảnh {}: mốc hành động trùng/ngược thứ tự; chỉnh mốc trong Thuộc tính cảnh",i+1);
                let span=Span{start_ms:t0,duration_ms:t1-t0,source_in_ms:v0,source_out_ms:v1};
                ensure!(span.rate()>=1.-0.002&&span.rate()<=maximum+0.002,"Cảnh {}: mốc hình/thoại cần tốc độ {:.2}×, ngoài giới hạn 1–{maximum:.2}×. Chỉnh mốc hoặc điểm cắt; không đổi tốc độ voice.",i+1,span.rate());
                ensure!(!fixed||(span.rate()-requested).abs()<=0.002,"Cảnh {}: mốc yêu cầu đổi tốc độ, không phù hợp chế độ Cố định; chọn Tự động hoặc chỉnh mốc",i+1);
                s.footage_spans.push(span);
            }
            let unresolved=s.footage_anchors.iter().filter(|a|!matches!(a.status.as_str(),"aligned"|"manual")).count();
            let used=(end-begin) as f64/s.duration_ms.max(1) as f64;
            s.footage_status=format!("{used:.2}× · {} đoạn · {} mốc cần kiểm tra",s.footage_spans.len(),unresolved);
            if used+0.002<requested {s.footage_status.push_str(" · giảm tốc để đủ footage");}
        }
        if s.kind=="source" {s.footage_status=format!("{:.2}× · {:.2}s đầu ra",s.source_rate,s.duration_ms as f64/1000.);}
        messages.push(format!("Cảnh {}: {}",i+1,s.footage_status));
        next.segments[i]=s;
    }
    for r in &next.rules.required_ranges {
        let mut ranges:Vec<_>=next.segments.iter().filter(|s|s.kind!="host"&&s.source_id==r.source_id).flat_map(|s|if s.footage_spans.is_empty(){vec![(s.in_ms,s.out_ms)]}else{s.footage_spans.iter().map(|v|(v.source_in_ms,v.source_out_ms)).collect()}).collect();
        ranges.sort_unstable();let mut end=r.start_ms;
        for (a,b) in ranges {if a<=end {end=end.max(b);}}
        ensure!(end>=r.end_ms,"Tăng tốc bỏ sót vùng bắt buộc {}–{} ms; mở rộng cảnh hoặc chỉnh mốc",r.start_ms,r.end_ms);
    }
    next.script_warnings.retain(|v|!v.starts_with("[Tốc độ]"));
    for (i,s) in next.segments.iter().enumerate() {
        for a in &s.footage_anchors {
            if !matches!(a.status.as_str(),"aligned"|"manual") {next.script_warnings.push(format!("[Tốc độ] Cảnh {}: mốc '{}' trạng thái {}. Kiểm tra mốc hình/thoại trong Thuộc tính cảnh; mốc chưa khớp không được dùng để ép tốc độ.",i+1,a.phrase,anchor_status(&a.status)));}
        }
    }
    p.segments=next.segments;p.script_warnings=next.script_warnings;
    Ok(messages)
}

/// Preserve the source clock through scene splits; generated assets remain 1×.
pub fn slice(s:&Segment,from:u64,to:u64)->Vec<Span> {
    s.footage_spans.iter().filter_map(|v|{
        let a=from.max(v.start_ms);let b=to.min(v.start_ms+v.duration_ms);if b<=a{return None;}
        Some(Span{start_ms:a-from,duration_ms:b-a,source_in_ms:v.source_in_ms+scaled(a-v.start_ms,v.rate()),source_out_ms:v.source_in_ms+scaled(b-v.start_ms,v.rate())})
    }).collect()
}

pub fn validate_scene(s:&Segment)->Result<()> {
    ensure!(valid_rate(s.source_rate)&&s.footage_rate_override.is_none_or(valid_rate),"Tốc độ cảnh phải trong 1–2×");
    ensure!(s.footage_anchors.len()<=8,"Tối đa 8 mốc hình/thoại trong một cảnh");
    let mut end=0;
    for v in &s.footage_spans {
        ensure!(v.start_ms==end&&v.duration_ms>0&&v.source_out_ms>v.source_in_ms&&v.source_in_ms>=s.in_ms&&v.source_out_ms<=s.out_ms,"Bản đồ tốc độ footage không hợp lệ");
        ensure!((0.998..=2.002).contains(&v.rate()),"Tốc độ đoạn footage vượt giới hạn");end+=v.duration_ms;
    }
    ensure!(s.footage_spans.is_empty()||end==s.duration_ms,"Bản đồ tốc độ chưa phủ đủ voice");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn project()->Project {
        let mut p=Project::default();p.rules.hook_enabled=false;p.rules.intercuts_enabled=false;
        p.output.footage_speed=Settings{mode:"auto".into(),maximum:2.,..Default::default()};
        p.sources.push(Source{id:"src".into(),duration_ms:30000,..Default::default()});
        p.segments.push(Segment{source_id:"src".into(),kind:"review".into(),in_ms:1000,out_ms:16000,duration_ms:10000,
            narration:"Một hành động xảy ra".into(),timing_estimated:false,
            words:vec![Word{text:"Một".into(),start_ms:0,end_ms:1000},Word{text:"hành".into(),start_ms:4000,end_ms:4400},Word{text:"động".into(),start_ms:4400,end_ms:4800}],
            footage_anchors:vec![Anchor{phrase:"hành động".into(),source_ms:6000,..Default::default()}],..Default::default()});p
    }
    #[test]
    fn anchors_map_measured_voice_without_changing_audio_words_or_avatar() {
        let mut p=project();p.output.blur_background=true;
        p.segments[0].audio=Some("voice.wav".into());p.segments[0].avatar=Some("avatar.mkv".into());
        let voice=serde_json::to_vec(&(&p.segments[0].words,&p.segments[0].audio,&p.segments[0].avatar,p.segments[0].duration_ms)).unwrap();
        fit(&mut p).unwrap();let s=&p.segments[0];
        assert_eq!(s.footage_anchors[0].status,"aligned");assert_eq!(s.footage_spans.len(),2);
        assert_eq!(s.footage_spans[0].source_out_ms,6000);assert_eq!(s.footage_spans[0].duration_ms,4000);
        assert_eq!(voice,serde_json::to_vec(&(&s.words,&s.audio,&s.avatar,s.duration_ms)).unwrap());
        let comp=crate::composition::Composition::from_project(&p);comp.validate().unwrap();
        for clip in &comp.clips {if matches!(clip.track,crate::timeline::VOICE|crate::timeline::AVATAR|crate::timeline::CAPTIONS){assert_eq!(clip.playback_rate,1.);}else if matches!(clip.track,crate::timeline::VIDEO|crate::timeline::BACKGROUND){assert!((clip.playback_rate-1.25).abs()<0.001);}}
        let sliced=slice(s,2000,8000);assert_eq!(sliced.len(),2);assert_eq!(sliced[0].source_in_ms,3500);assert_eq!(sliced[1].source_out_ms,11000);
        let mut reopened:Project=serde_json::from_slice(&serde_json::to_vec(&p).unwrap()).unwrap();fit(&mut reopened).unwrap();
        assert_eq!(serde_json::to_vec(&reopened.segments).unwrap(),serde_json::to_vec(&p.segments).unwrap());
    }
    #[test]
    fn shortage_reduces_auto_but_respects_limits_locks_and_exclusions() {
        let mut p=project();p.segments[0].footage_anchors.clear();p.segments[0].out_ms=12000;
        fit(&mut p).unwrap();assert!((p.segments[0].footage_spans[0].rate()-1.1).abs()<0.001);
        let before=serde_json::to_vec(&p.segments).unwrap();p.segments[0].locked=true;
        p.output.footage_speed.preferred=1.5;fit(&mut p).unwrap();p.segments[0].locked=false;
        assert_eq!(before,serde_json::to_vec(&p.segments).unwrap());
        p.segments[0].out_ms=9000;assert!(fit(&mut p).is_err());
        p.output.footage_speed.allow_host=true;fit(&mut p).unwrap();assert_eq!(p.segments[0].kind,"host");
        let legacy:Segment=serde_json::from_str("{}").unwrap();assert_eq!(legacy.source_rate,1.);
        p.output.footage_speed.maximum=f64::NAN;assert!(fit(&mut p).is_err());
    }
    #[test]
    fn hook_can_speed_up_without_shrinking_below_three_seconds() {
        let mut p=project();p.rules.hook_enabled=true;p.output.footage_speed.hook=2.;
        p.segments.insert(0,Segment{source_id:"src".into(),in_ms:0,out_ms:6000,duration_ms:6000,..Default::default()});
        fit(&mut p).unwrap();assert_eq!(p.segments[0].duration_ms,3000);assert_eq!(p.segments[0].source_rate,2.);
        p.segments[0].out_ms=4000;fit(&mut p).unwrap();assert_eq!(p.segments[0].duration_ms,3000);assert!((p.segments[0].source_rate-4./3.).abs()<0.001);
    }
    #[test]
    fn contradictory_anchor_fails_atomically_and_reports_scene() {
        let mut p=project();p.segments[0].footage_anchors[0].source_ms=2000;p.segments[0].footage_anchors[0].manual=true;p.segments[0].footage_anchors[0].timeline_ms=4000;
        let before=serde_json::to_vec(&p.segments).unwrap();let err=fit(&mut p).unwrap_err();
        assert!(format!("{err}").contains("Cảnh 1"));assert_eq!(before,serde_json::to_vec(&p.segments).unwrap());
    }
    #[test]
    fn raw_asr_anchor_survives_partial_caption_estimates_but_invalidates_with_voice() {
        let mut p=project();let words=p.segments[0].words.clone();p.segments[0].timing_estimated=true;
        p.segments[0].words[1].start_ms=8000;
        align_measured(&mut p.segments[0],&words);fit(&mut p).unwrap();
        assert_eq!(p.segments[0].footage_anchors[0].status,"aligned");assert_eq!(p.segments[0].footage_anchors[0].timeline_ms,4000);
        p.segments[0].asset_key="different voice".into();fit(&mut p).unwrap();
        assert_eq!(p.segments[0].footage_anchors[0].status,"estimated");assert_eq!(p.segments[0].footage_spans.len(),1);
    }
    #[test]
    fn fixed_speed_rejects_shortage_and_auto_reports_unusable_anchors() {
        let mut p=project();p.output.footage_speed.mode="fixed".into();p.segments[0].footage_anchors.clear();
        p.segments[0].out_ms=12000;assert!(fit(&mut p).unwrap_err().to_string().contains("cố định"));
        p.output.footage_speed.mode="auto".into();p.segments[0].out_ms=16000;
        p.segments[0].footage_anchors.push(Anchor{phrase:"hành động".into(),source_ms:2000,..Default::default()});
        fit(&mut p).unwrap();assert_eq!(p.segments[0].footage_anchors[0].status,"conflict");
        assert!(!p.script_warnings.is_empty());assert_eq!(p.segments[0].footage_spans.len(),1);
        p.segments[0].in_ms=17000;assert!(fit(&mut p).is_err());
    }
    #[test]
    fn real_export_and_preview_keep_source_clock_and_unmodified_voice_after_seek_split_and_cache() {
        use crate::{media,process,timeline};use std::time::Duration;
        let temp=tempfile::tempdir().unwrap();let cancel=process::cancel_token();
        let settings=crate::model::Settings{preview_long_edge:640,..Default::default()};
        let src=temp.path().join("source.mp4");let voice=temp.path().join("voice.wav");
        process::run(&settings.ffmpeg,&process::args(&["-y","-v","error","-f","lavfi","-i","testsrc2=size=640x360:rate=24:duration=24","-f","lavfi","-i","sine=frequency=440:sample_rate=48000:duration=24","-c:v","libx264","-preset","ultrafast","-c:a","aac","-shortest",&src.to_string_lossy()]),None,&cancel,Duration::from_secs(30),None).unwrap();
        process::run(&settings.ffmpeg,&process::args(&["-y","-v","error","-f","lavfi","-i","aevalsrc=0.2*sin(2*PI*(220*t+40*t*t))*exp(-mod(t\\,0.3)*4):s=48000:d=8","-c:a","pcm_s16le",&voice.to_string_lossy()]),None,&cancel,Duration::from_secs(30),None).unwrap();
        let hash=media::hash(std::fs::read(&voice).unwrap());let mut p=project();
        p.root=temp.path().join("project");p.output.resolution=360;p.output.fps=24;p.output.portrait=false;p.subtitles.enabled=false;
        let mut source=media::probe(&settings,&src,&cancel).unwrap();source.id="src".into();source.proxy=src.clone();p.sources=vec![source];
        p.rules.min_seconds=1;p.rules.max_seconds=60;p.rules.hook_enabled=true;p.output.footage_speed.hook=2.;
        p.segments[0].in_ms=6000;p.segments[0].out_ms=20000;p.segments[0].duration_ms=8000;p.segments[0].audio=Some(voice.clone());
        p.segments[0].footage_anchors[0].source_ms=11000;
        p.segments.insert(0,Segment{source_id:"src".into(),in_ms:0,out_ms:6000,duration_ms:6000,evidence:"0–6s test hook".into(),..Default::default()});
        fit(&mut p).unwrap();p.ensure_composition();
        let output=temp.path().join("output.mp4");media::render(&p,&settings,&output,&cancel,|_,_|{}).unwrap();
        assert!(media::probe(&settings,&output,&cancel).unwrap().duration_ms.abs_diff(11000)<100);
        let difference=|a:&image::RgbaImage,b:&image::RgbaImage| {
            let b=image::imageops::resize(b,a.width(),a.height(),image::imageops::FilterType::Triangle);
            a.pixels().zip(b.pixels()).map(|(a,b)|(0..3).map(|i|a[i].abs_diff(b[i]) as f64).sum::<f64>()).sum::<f64>()/(a.width()*a.height()*3) as f64
        };
        for ms in [500,2000,3500,5000,7500,9500] {
            let comp=p.composition_snapshot();let clip=comp.clips.iter().find(|c|c.track==timeline::VIDEO&&c.active(ms)).unwrap();
            let expected=media::frame(&settings,&src,clip.source_at(ms-clip.start_ms),&cancel).unwrap();
            let exported=media::frame(&settings,&output,ms,&cancel).unwrap();
            assert!(difference(&expected,&exported)<13.,"Export source clock mismatch at {ms}: {}",difference(&expected,&exported));
            let preview=crate::player::decode_test_frame(&p,&settings,clip,ms-clip.start_ms).unwrap();
            assert!(difference(&expected,&preview)<13.,"Preview source clock mismatch at {ms}: {}",difference(&expected,&preview));
        }
        for (seek,expected) in [("0.5",440usize),("3.5",300usize)] {
            let audio=process::run(&settings.ffmpeg,&process::args(&["-v","error","-ss",seek,"-i",&output.to_string_lossy(),"-t","1","-vn","-ar","48000","-ac","1","-f","s16le","pipe:1"]),None,&cancel,Duration::from_secs(20),None).unwrap();
            let samples:Vec<_>=audio.stdout.chunks_exact(2).map(|v|i16::from_le_bytes([v[0],v[1]])).collect();
            let rising=samples.windows(2).filter(|v|v[0]<=0&&v[1]>0).count();assert!(rising.abs_diff(expected)<8,"Audio pitch changed at {seek}: {rising}, expected {expected}");
            if expected==300 {
                let raw=process::run(&settings.ffmpeg,&process::args(&["-v","error","-ss","0.5","-i",&voice.to_string_lossy(),"-t","1","-vn","-ar","48000","-ac","1","-f","s16le","pipe:1"]),None,&cancel,Duration::from_secs(20),None).unwrap();
                let raw:Vec<_>=raw.stdout.chunks_exact(2).map(|v|i16::from_le_bytes([v[0],v[1]]) as f64).collect();
                let mut best=0f64;
                for lag in -48i32..=48 {
                    let a=&samples[lag.max(0) as usize..samples.len().saturating_sub((-lag).max(0) as usize)];
                    let b=&raw[(-lag).max(0) as usize..raw.len().saturating_sub(lag.max(0) as usize)];
                    let (mut dot,mut aa,mut bb)=(0.,0.,0.);
                    for (a,b) in a.iter().zip(b) {let a=*a as f64;dot+=a*b;aa+=a*a;bb+=b*b;}
                    best=best.max(dot/(aa*bb).sqrt().max(1.));
                }
                assert!(best>0.97,"Unity-rate speech waveform was altered: correlation {best}");
            }

        }
        assert_eq!(hash,media::hash(std::fs::read(&voice).unwrap()));
        let mut split=p.clone();timeline::split(&mut split,1,3000).unwrap();
        assert_eq!(split.segments[2].asset_start_ms,3000);assert_eq!(split.segments[2].in_ms,9750);
        validate_scene(&split.segments[2]).unwrap();
        let mut independent=p.clone();let video=independent.composition.as_mut().unwrap().clips.iter_mut().find(|c|c.track==timeline::VIDEO&&c.start_ms==0).unwrap();
        video.linked=false;let id=video.id.clone();
        independent.time_layer(&id,500,500,2000).unwrap();
        let video=independent.composition.as_ref().unwrap().clips.iter().find(|c|c.id==id).unwrap();
        assert_eq!(video.media_in_ms,1000);assert_eq!(video.playback_rate,2.);
        let mut clipboard=None;let right=independent.edit_independent_layer(&id,"split",1500,&mut clipboard).unwrap().unwrap();
        let right=independent.composition.as_ref().unwrap().clips.iter().find(|c|c.id==right).unwrap();
        assert_eq!(right.media_in_ms,3000);assert_eq!(right.duration_ms,1000);
        let scene_preview=media::render_segment(&p,&p.segments[0],&settings,&temp.path().join("scene-preview"),&cancel).unwrap();
        assert!(media::probe(&settings,&scene_preview,&cancel).unwrap().duration_ms.abs_diff(3000)<100);
        media::render(&p,&settings,&output,&cancel,|_,_|{}).unwrap();
        let report:serde_json::Value=serde_json::from_slice(&std::fs::read(p.root.join("cache/composition-export/render-report.json")).unwrap()).unwrap();
        assert_eq!(report["cache_hits"],report["intervals"]);
        p.output.portrait=true;p.output.blur_background=true;p.ensure_composition();
        media::render(&p,&settings,&temp.path().join("portrait.mp4"),&cancel,|_,_|{}).unwrap();
    }
}
