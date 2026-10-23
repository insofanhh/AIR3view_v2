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
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TranscriptLine {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
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
        let filter=format!("setpts=PTS-STARTPTS,select='isnan(prev_selected_t)+gte(t-prev_selected_t\\,{})',scale=320:180:force_original_aspect_ratio=decrease,pad=320:208:(ow-iw)/2:0:black,drawtext=fontfile=font.ttf:text='%{{pts\\:hms}}':fontcolor=white:fontsize=19:x=8:y=185,tile=4x4:padding=4:margin=4",s.visual_sample_seconds);
        fs::write(folder.join("sheets.filter"), &filter)?;
        let args = vec![
            "-y".into(),
            "-v".into(),
            "error".into(),
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
        process::run(
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
        let transcript = group_words(&words);
        let item = Evidence {
            source_id: source.id.clone(),
            duration_ms: source.duration_ms,
            transcript,
            contact_sheets,
            sample_seconds: s.visual_sample_seconds,
            quality_flags:quality_flags(&words,source.duration_ms),
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
    if let Some(hook)=plan.segments.first().filter(|v|v.kind=="source") {
        for t in [hook.in_ms,hook.in_ms+(hook.out_ms.saturating_sub(hook.in_ms))/2,hook.out_ms.saturating_sub(1)] {
            points.insert((hook.source_id.clone(),t));
        }
    }
    for item in plan.segments.iter().flat_map(|v|&v.evidence_items).filter(|v|v.enabled) {
        points.insert((item.source_id.clone(),item.source_time_ms));
        if let Some(t)=item.second_time_ms {points.insert((if item.second_source_id.is_empty(){&item.source_id}else{&item.second_source_id}.clone(),t));}
    }
    let mut frames=vec![];
    for (id,t) in points.into_iter().take(16) {
        process::check_cancel(cancel)?;
        if let Ok(src)=p.source(&id) {
            if !src.path.is_file() || t>=src.duration_ms || p.rules.excluded_ranges.iter().any(|r|r.source_id==id&&r.start_ms<=t&&t<r.end_ms){continue;}
            let path=crate::evidence::source_frame(p,s,src,t,cancel)?;
            frames.push(serde_json::json!({"source_id":id,"source_time_ms":t,"image_path":path,"note":"Frame đúng mốc, không chứng minh riêng ý nghĩa toàn bộ sự kiện"}));
        }
    }
    Ok(frames)
}
