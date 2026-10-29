//! Local evidence preparation for the headless CLI: ASR plus timestamped frame sheets.
use crate::{
    media,
    model::*,
    process::{self, Cancel},
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, time::Duration};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Evidence {
    pub source_id: String,
    pub duration_ms: u64,
    pub transcript: Vec<TranscriptLine>,
    pub contact_sheets: Vec<PathBuf>,
    pub sample_seconds: u32,
    #[serde(default)]
    pub quality_flags: Vec<String>,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub frame_index: Option<PathBuf>,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct IndexedFrame {
    pub frame_id:String,pub source_id:String,pub source_time_ms:u64,pub sheet:PathBuf,pub cell_index:usize,
    pub coordinate_space:String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TranscriptLine {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Exact token match across adjacent ASR chunks, anchored to this source/time.
/// Chunk boundaries are not sentence boundaries. Never search unrelated footage
/// or bridge a long silence just to make a quote pass.
pub fn quote_supported(evidence:&[Evidence],source_id:&str,time:u64,quote:&str)->bool {
    let tokens=|s:&str|s.split(|c:char|!c.is_alphanumeric()).filter(|s|!s.is_empty()).map(str::to_lowercase).collect::<Vec<_>>();
    let needle=tokens(quote);
    if needle.is_empty(){return false;}
    let near=|line:&TranscriptLine|time.saturating_add(10000)>=line.start_ms && time<=line.end_ms.saturating_add(10000);
    for source in evidence.iter().filter(|e|e.source_id==source_id) {
        for (start,first) in source.transcript.iter().enumerate().filter(|(_,t)|near(t)) {
            let mut words=vec![];let mut origins=vec![];let mut previous=first.end_ms;
            for (offset,line) in source.transcript[start..].iter().take(6).enumerate() {
                if line.end_ms<line.start_ms || line.end_ms.saturating_sub(first.start_ms)>30000
                    || (offset>0&&(line.start_ms<source.transcript[start+offset-1].start_ms || line.start_ms.saturating_sub(previous)>8000)){break;}
                let part=tokens(&line.text);origins.extend(std::iter::repeat_n(start+offset,part.len()));words.extend(part);
                previous=line.end_ms;
                if words.windows(needle.len()).enumerate().any(|(index,w)|w==needle && near(&source.transcript[origins[index]])) {return true;}
            }
        }
    }
    false
}

pub fn resolve_frame_ids(p:&Project,plan:&mut ScriptPlan)->Result<()> {
    let evidence:Vec<Evidence>=fs::read(p.root.join("analysis/local-evidence.json")).ok()
        .and_then(|v|serde_json::from_slice(&v).ok()).unwrap_or_default();
    let mut frames=std::collections::BTreeMap::new();
    for e in evidence {if let Some(path)=e.frame_index {if let Ok(bytes)=fs::read(path){
        if let Ok(entries)=serde_json::from_slice::<Vec<IndexedFrame>>(&bytes){for frame in entries{frames.insert(frame.frame_id.clone(),frame);}}
    }}}
    for item in plan.segments.iter_mut().flat_map(|s|&mut s.evidence_items) {
        if item.frame_id.is_empty(){continue;}
        if let Some(frame)=frames.get(&item.frame_id).filter(|f|f.source_id==item.source_id){item.source_time_ms=frame.source_time_ms;}
    }Ok(())
}

pub fn prepare<F: FnMut(&str)>(
    p: &Project,
    s: &Settings,
    cancel: &Cancel,
    mut progress: F,
) -> Result<Vec<Evidence>> {
    ensure!(
        (1..=10).contains(&s.visual_sample_seconds),
        "Khoảng ảnh mẫu cần từ 1 đến 10 giây"
    );
    let mut evidence = vec![];
    for source in &p.sources {
        process::check_cancel(cancel)?;
        ensure!(
            source.path.is_file(),
            "Cần tải video trước khi phân tích bằng Antigravity CLI"
        );
        let meta = fs::metadata(&source.path)?;
        let cache = media::hash(format!(
            "evidence-v3:{}:{}:{}:{:?}:{}:{}:{}",
            source.id,
            source.path.display(),
            meta.len(),
            meta.modified().ok(),
            s.visual_sample_seconds,
            s.asr_model,
            s.asr_language
        ));
        let folder = p.root.join("analysis/evidence").join(cache);
        fs::create_dir_all(&folder)?;
        let folder = PathBuf::from(
            fs::canonicalize(folder)?
                .to_string_lossy()
                .trim_start_matches("\\\\?\\")
                .replace('\\', "/"),
        );
        let manifest = folder.join("evidence.json");
        if let Ok(bytes) = fs::read(&manifest) {
            if let Ok(mut saved) = serde_json::from_slice::<Evidence>(&bytes) {
                if !saved.contact_sheets.is_empty()
                    && saved.contact_sheets.iter().all(|path| path.is_file())
                    && saved.frame_index.as_ref().is_some_and(|path|path.is_file())
                {
                    // Upgrade grouping using saved word timestamps; no ASR/download redo.
                    if let Ok(bytes)=fs::read(folder.join("words.json")) {
                        if let Ok(words)=serde_json::from_slice::<Vec<Word>>(&bytes) {
                            saved.transcript=group_words(&words);
                            saved.quality_flags=quality_flags(&words,saved.duration_ms);
                        }
                    }
                    write_json(&manifest,&saved)?;
                    evidence.push(saved);
                    continue;
                }
            }
        }
        progress(&format!("{}: nhận dạng tiếng nói • {} / {} / {}", source.id, s.asr_model, s.asr_device, s.asr_compute_type));
        let transcript = folder.join("words.json");
        if !transcript.is_file() {
            if source.has_audio {
                let audio = folder.join("source.wav");
                let args = vec![
                    "-y".into(),
                    "-v".into(),
                    "error".into(),
                    "-i".into(),
                    source.path.to_string_lossy().into_owned(),
                    "-vn".into(),
                    "-ar".into(),
                    "16000".into(),
                    "-ac".into(),
                    "1".into(),
                    audio.to_string_lossy().into_owned(),
                ];
                process::run(
                    &s.ffmpeg,
                    &args,
                    None,
                    cancel,
                    Duration::from_secs(s.timeout_seconds),
                    None,
                )?;
                let args = vec![
                    worker_path("transcribe.py").to_string_lossy().into_owned(),
                    "--input".into(),
                    audio.to_string_lossy().into_owned(),
                    "--output".into(),
                    transcript.to_string_lossy().into_owned(),
                    "--language".into(),
                    s.asr_language.clone(),
                    "--model".into(),
                    s.asr_model.clone(),
                    "--device".into(),
                    s.asr_device.clone(),
                    "--compute-type".into(),
                    s.asr_compute_type.clone(),
                ];
                process::run(
                    &s.python,
                    &args,
                    None,
                    cancel,
                    Duration::from_secs(s.timeout_seconds),
                    None,
                )
                .map_err(|error| {
                    let message=format!("{error:#}").to_lowercase();
                    if s.asr_device == "cuda" {
                        error.context("Whisper CUDA thất bại. Mở Cài đặt kết nối → Kiểm tra CUDA; chi tiết bên dưới. Video đã tải được giữ lại.")
                    } else if ["mkl_malloc","memoryerror","out of memory","failed to allocate memory"].iter().any(|v|message.contains(v)) {
                        error.context("Whisper thiếu RAM. Chọn model ASR base trong Cài đặt kết nối, lưu cấu hình rồi Tiếp tục / sửa phần lỗi; video đã tải được giữ lại.")
                    }else{error.context("Không tạo được transcript. Kiểm tra Python worker và model Whisper.")}
                })?;
                if let Some(summary) = crate::asr::summary(&transcript) { progress(&summary); }
                let _ = fs::remove_file(audio);
            } else {
                write_json(&transcript, &Vec::<Word>::new())?;
            }
        }
        let words: Vec<Word> = serde_json::from_slice(&fs::read(&transcript)?)?;
        ensure!(words.iter().all(|word|word.start_ms<word.end_ms&&word.end_ms<=source.duration_ms+1000),"Transcript có timestamp ngoài nguồn");
        progress(&format!(
            "{}: trích ảnh mỗi {} giây",
            source.id, s.visual_sample_seconds
        ));
        let font = folder.join("font.ttf");
        let font_source = [
            "C:/Windows/Fonts/segoeuib.ttf",
            "C:/Windows/Fonts/arial.ttf",
        ]
        .into_iter()
        .find(|path| std::path::Path::new(path).is_file())
        .context("Không thấy font để ghi timestamp")?;
        fs::copy(font_source, &font)?;
        let filter=format!("setpts=PTS-STARTPTS,select='isnan(prev_selected_t)+gte(t-prev_selected_t\\,{})',showinfo,scale=320:180:force_original_aspect_ratio=decrease,pad=320:208:(ow-iw)/2:0:black,drawtext=fontfile=font.ttf:text='F%{{eif\\:n+1\\:d\\:5}} %{{pts\\:hms}}':fontcolor=white:fontsize=16:x=4:y=185,tile=4x4:padding=4:margin=4",s.visual_sample_seconds);
        fs::write(folder.join("sheets.filter"), &filter)?;
        let args = vec![
            "-y".into(),
            "-v".into(),
            "info".into(),
            "-i".into(),
            source.path.to_string_lossy().into_owned(),
            "-an".into(),
            "-vf".into(),
            filter,
            "-fps_mode".into(),
            "vfr".into(),
            "-q:v".into(),
            "3".into(),
            "sheet-%04d.jpg".into(),
        ];
        let output=process::run(
            &s.ffmpeg,
            &args,
            Some(&folder),
            cancel,
            Duration::from_secs(s.timeout_seconds),
            None,
        )?;
        let mut contact_sheets: Vec<_> = fs::read_dir(&folder)?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("sheet-"))
                    && path.extension().is_some_and(|ext| ext == "jpg")
            })
            .collect();
        contact_sheets.sort();
        ensure!(!contact_sheets.is_empty(), "Không trích được ảnh từ video");
        let frames:Vec<_>=String::from_utf8_lossy(&output.stderr).lines().filter(|l|l.contains("showinfo")&&l.contains("pts_time:"))
            .filter_map(|l|l.split("pts_time:").nth(1)?.split_whitespace().next()?.parse::<f64>().ok())
            .enumerate().map(|(i,t)|IndexedFrame{frame_id:format!("{}:F{:05}",source.id,i+1),source_id:source.id.clone(),source_time_ms:(t*1000.).round() as u64,
                sheet:folder.join(format!("sheet-{:04}.jpg",i/16+1)),cell_index:i%16,coordinate_space:"source_frame".into()}).collect();
        ensure!(!frames.is_empty(),"Không có PTS thực tế cho chỉ mục ảnh mẫu");
        let frame_index=folder.join("frame-index.json");write_json(&frame_index,&frames)?;
        let transcript = group_words(&words);
        let item = Evidence {
            source_id: source.id.clone(),
            duration_ms: source.duration_ms,
            transcript,
            contact_sheets,
            sample_seconds: s.visual_sample_seconds,
            quality_flags:quality_flags(&words,source.duration_ms),
            frame_index:Some(frame_index),
        };
        write_json(&manifest, &item)?;
        evidence.push(item);
    }
    write_json(&p.root.join("analysis/local-evidence.json"), &evidence)?;
    Ok(evidence)
}

pub fn group_words(words:&[Word])->Vec<TranscriptLine> {
    let mut lines=vec![];
    let mut start=0;
    for end in 1..=words.len() {
        if end==words.len() || end-start>=24 || words[end-1].end_ms.saturating_sub(words[start].start_ms)>=10000
            || words[end].start_ms.saturating_sub(words[end-1].end_ms)>2000 {
            lines.push(TranscriptLine{start_ms:words[start].start_ms,end_ms:words[end-1].end_ms,
                text:words[start..end].iter().map(|w|w.text.as_str()).collect::<Vec<_>>().join(" ")});
            start=end;
        }
    }
    lines
}
fn quality_flags(words:&[Word],duration:u64)->Vec<String> {
    let spoken:u64=words.iter().map(|w|w.end_ms.saturating_sub(w.start_ms)).sum();
    let mut flags=vec!["Ảnh được lấy mẫu, không phải toàn bộ frame. Transcript ASR cần đối chiếu.".into()];
    if spoken<duration/5 {flags.push("ASR thưa/ít lời hoặc nhiều khoảng trống; không coi khoảng trống là không có sự kiện".into());}
    let repeated=words.windows(4).filter(|w|w[0].text==w[1].text&&w[1].text==w[2].text&&w[2].text==w[3].text).count();
    if repeated>0 {flags.push("ASR có từ lặp bất thường; đối chiếu audio trước khi dùng làm proof".into());}
    flags
}

/// Bounded close-up sampling for repair of hooks and measurement endpoints.
pub fn focus_evidence(p:&Project,s:&Settings,plan:&ScriptPlan,cancel:&Cancel)->Result<Vec<serde_json::Value>> {
    let mut points=std::collections::BTreeSet::new();
    let mut priority=vec![];
    if let Some(hook)=plan.segments.first().filter(|v|v.kind=="source") {
        for t in [hook.in_ms,hook.in_ms+(hook.out_ms.saturating_sub(hook.in_ms))/2,hook.out_ms.saturating_sub(1)] {
            points.insert((hook.source_id.clone(),t));
            priority.push((hook.source_id.clone(),t));
        }
    }
    // Reserve evidence from the ending as well as the opening, so the bounded
    // review can see on-screen charges/outcomes even on a long source video.
    if let Some(last)=plan.segments.iter().rfind(|v|v.kind!="source") {
        for item in last.evidence_items.iter().filter(|e|e.enabled).take(2) {
            priority.push((item.source_id.clone(),item.source_time_ms));
        }
        priority.push((last.source_id.clone(),last.in_ms+(last.out_ms.saturating_sub(last.in_ms))/2));
    }
    for item in plan.segments.iter().flat_map(|v|&v.evidence_items).filter(|v|v.enabled) {
        points.insert((item.source_id.clone(),item.source_time_ms));
        if let Some(t)=item.second_time_ms {points.insert((if item.second_source_id.is_empty(){&item.source_id}else{&item.second_source_id}.clone(),t));}
    }
    let mut frames=vec![];
    let ordered:Vec<_>=priority.iter().cloned().chain(points.into_iter().filter(|v|!priority.contains(v))).collect();
    for (id,t) in ordered.into_iter().take(16) {
        process::check_cancel(cancel)?;
        if let Ok(src)=p.source(&id) {
            if !src.path.is_file() || t>=src.duration_ms || p.rules.excluded_ranges.iter().any(|r|r.source_id==id&&r.start_ms<=t&&t<r.end_ms){continue;}
            let path=crate::evidence::source_frame(p,s,src,t,cancel)?;
            frames.push(serde_json::json!({"source_id":id,"source_time_ms":t,"image_path":path,"note":"Frame đúng mốc, không chứng minh riêng ý nghĩa toàn bộ sự kiện"}));
        }
    }
    Ok(frames)
}

#[cfg(test)]
mod frame_tests {
    use super::*;
    #[test]
    fn exact_quotes_cross_adjacent_chunks_but_not_distant_or_unrelated_speech() {
        let source=|lines:Vec<(u64,u64,&str)>|vec![Evidence{source_id:"src".into(),duration_ms:300000,
            transcript:lines.into_iter().map(|(start_ms,end_ms,text)|TranscriptLine{start_ms,end_ms,text:text.into()}).collect(),
            contact_sheets:vec![],sample_seconds:5,quality_flags:vec![],frame_index:None}];
        let e=source(vec![(0,4200,"A"),(4300,15000,"desperate mother called 911 after her daughter became violent inside the home.")]);
        assert!(quote_supported(&e,"src",6000,"A desperate mother called 911 after her daughter became violent inside the home."));
        let e=source(vec![(44410,45630,"Don't touch me, ma 'am. Don't"),(52060,60980,"touch me.")]);
        assert!(quote_supported(&e,"src",42000,"Don't touch me, ma 'am. Don't touch me."));
        assert!(!quote_supported(&e,"wrong",42000,"Don't touch me, ma 'am. Don't touch me."));
        assert!(!quote_supported(&e,"src",12000,"Don't touch me, ma 'am. Don't touch me."));
        assert!(!quote_supported(&e,"src",42000,"Don't touch me and don't touch me."));
        let e=source(vec![(1000,2000,"That's my daughter."),(11000,12000,"She's psycho.")]);
        assert!(!quote_supported(&e,"src",1000,"That's my daughter. She's psycho."));
        assert!(!quote_supported(&e,"src",1000,""));
        let e=source(vec![(205190,207000,"That's my daughter."),(212770,215000,"She's psycho.")]);
        assert!(quote_supported(&e,"src",210000,"That's my daughter. She's psycho."));
        let e=source(vec![(1000,2000,"start"),(3000,31000,"end")]);
        assert!(quote_supported(&e,"src",1000,"start end"));
        let e=source(vec![(1000,2000,"start"),(3000,31001,"end")]);
        assert!(!quote_supported(&e,"src",1000,"start end"));
    }
    #[test]
    fn silent_vfr_portrait_and_multiple_sources_keep_real_pts_and_reuse_index() {
        let tmp = tempfile::tempdir().unwrap();
        let s = Settings {visual_sample_seconds: 1, ..Settings::default()};
        let cancel = process::cancel_token();
        let source = tmp.path().join("vfr.mkv");
        process::run(&s.ffmpeg,&process::args(&["-y","-v","error","-f","lavfi","-i",
            "testsrc2=size=180x320:rate=10:duration=4.1",
            "-vf","select='eq(mod(n,3),0)',setpts=PTS+2/TB","-fps_mode","vfr",
            "-c:v","libx264","-preset","ultrafast",&source.to_string_lossy()]),
            None,&cancel,Duration::from_secs(30),None).unwrap();
        let p = Project {root:tmp.path().join("project"), sources:["src_1","src_2"].into_iter().map(|id|Source {
            id:id.into(),path:source.clone(),duration_ms:4100,has_audio:false,..Default::default()
        }).collect(), ..Default::default()};
        let evidence = prepare(&p,&s,&cancel, |_|{}).unwrap();
        assert_eq!(evidence.len(),2);
        let mut ids=std::collections::HashSet::new();
        for e in &evidence {
            assert!(e.transcript.is_empty());
            let frames:Vec<IndexedFrame>=serde_json::from_slice(&fs::read(e.frame_index.as_ref().unwrap()).unwrap()).unwrap();
            assert_eq!(frames.len(),4);
            // Actual selected VFR timestamps are 0, 1.2, 2.4, 3.6;
            // neither interval multiplication nor the container start of 2s.
            assert_eq!(frames.iter().map(|f|f.source_time_ms).collect::<Vec<_>>(),vec![0,1200,2400,3600]);
            for f in &frames {assert!(ids.insert(f.frame_id.clone()));assert!(f.sheet.is_file());}
            assert_eq!(image::open(&e.contact_sheets[0]).unwrap().width(),1300);
        }
        let index=evidence[0].frame_index.as_ref().unwrap();
        let modified=fs::metadata(index).unwrap().modified().unwrap();
        prepare(&p,&s,&cancel, |_|panic!("must reuse valid sheets/index")).unwrap();
        assert_eq!(modified,fs::metadata(index).unwrap().modified().unwrap());
        let mut plan=ScriptPlan {segments:vec![Segment {evidence_items:vec![crate::evidence::EvidenceItem {
            source_id:"src_2".into(),frame_id:"src_2:F00002".into(),source_time_ms:999,..Default::default()
        }],..Default::default()}],title:"fixture".into(),analysis_summary:String::new()};
        resolve_frame_ids(&p,&mut plan).unwrap();
        assert_eq!(plan.segments[0].evidence_items[0].source_time_ms,1200);
    }
}
