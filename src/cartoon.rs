use crate::{
    media,
    model::*,
    process::{self, Cancel},
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

pub fn generate(s: &Settings, seg: &Segment, folder: &Path, cancel: &Cancel) -> Result<PathBuf> {
    ensure!(
        matches!(
            s.cartoon_character.as_str(),
            "portrait" | "studio" | "analyst" | "panda"
        ),
        "Chọn nhân vật 2D Chân dung, Studio, Analyst hoặc Gấu trúc"
    );
    let audio = seg
        .audio
        .as_ref()
        .context("Nhân vật 2D cần audio VieNeu đã tạo")?;
    fs::create_dir_all(folder)?;
    let audio_hash = media::hash(fs::read(audio)?);
    let lips_key = media::hash(format!(
        "rhubarb-phonetic-1.14-v1:{}:{}",
        s.rhubarb, audio_hash
    ));
    let cues_path = folder.join(format!("{lips_key}.mouth.json"));
    if !cues_path.is_file() {
        let wav = folder.join(format!("{lips_key}.mono.wav"));
        process::run(
            &s.ffmpeg,
            &process::args(&[
                "-y",
                "-v",
                "error",
                "-i",
                &audio.to_string_lossy(),
                "-vn",
                "-ar",
                "16000",
                "-ac",
                "1",
                "-c:a",
                "pcm_s16le",
                &wav.to_string_lossy(),
            ]),
            None,
            cancel,
            Duration::from_secs(s.timeout_seconds),
            None,
        )?;
        let pending = cues_path.with_extension("pending.json");
        process::run(&s.rhubarb, &process::args(&["-r", "phonetic", "-f", "json", "--extendedShapes", "GHX", "--quiet", "-o", &pending.to_string_lossy(), &wav.to_string_lossy()]), None, cancel, Duration::from_secs(s.timeout_seconds), None)
            .context("Không chạy được Rhubarb. Chạy scripts/setup-rhubarb.ps1 hoặc chọn đúng rhubarb.exe trong Cài đặt")?;
        let value: Value = serde_json::from_slice(&fs::read(&pending)?)?;
        let cues = value["mouthCues"]
            .as_array()
            .context("Rhubarb thiếu mouthCues")?;
        ensure!(!cues.is_empty(), "Rhubarb chưa trả khẩu hình");
        for cue in cues {
            ensure!(
                cue["start"]
                    .as_f64()
                    .is_some_and(|n| n.is_finite() && n >= 0.0)
                    && cue["end"].as_f64().is_some_and(|n| n.is_finite()
                        && n > cue["start"].as_f64().unwrap()
                        && n <= seg.duration_ms as f64 / 1000.0 + 0.15)
                    && cue["value"].as_str().is_some_and(|v| matches!(
                        v,
                        "A" | "B" | "C" | "D" | "E" | "F" | "G" | "H" | "X"
                    )),
                "Rhubarb trả khẩu hình không hợp lệ"
            );
        }
        fs::rename(pending, &cues_path)?;
        let _ = fs::remove_file(wav);
    }
    let worker = worker_path("cartoon_render.py");
    let head_asset = worker_path(if s.cartoon_character == "panda" {
        "panda-head.png"
    } else {
        "portrait-base.png"
    });
    let key = media::hash(format!(
        "cartoon-rig-v6-lipsync:{}:{}:{}:{}:{}:{}:{}:{}:{}",
        lips_key,
        s.cartoon_character,
        seg.duration_ms,
        crate::input_assets::file_fingerprint(&worker.to_string_lossy()),
        crate::input_assets::file_fingerprint(&head_asset.to_string_lossy()),
        crate::input_assets::file_fingerprint(&worker_path("gesture_rig.py").to_string_lossy()),
        seg.gesture,
        seg.narration,
        serde_json::to_string(&seg.words)?
    ));
    let output = folder.join(format!("{key}.mkv"));
    if output.is_file() && media::probe(s, &output, cancel).is_ok() {
        return Ok(output);
    }
    let job = folder.join(format!("{key}.render.json"));
    write_json(
        &job,
        &json!({"cues":cues_path,"output":output,"ffmpeg":s.ffmpeg,"duration_ms":seg.duration_ms,"fps":25,"size":512,"character":s.cartoon_character,"gesture":seg.gesture,"narration":seg.narration,"words":seg.words}),
    )?;
    process::run(
        &s.python,
        &process::args(&[&worker.to_string_lossy(), "--job", &job.to_string_lossy()]),
        None,
        cancel,
        Duration::from_secs(s.timeout_seconds),
        None,
    )?;
    let info = media::probe(s, &output, cancel)?;
    ensure!(
        info.width == 512
            && info.height == if s.cartoon_character == "panda" { 704 } else { 512 }
            && info.duration_ms.abs_diff(seg.duration_ms) < 100,
        "Video nhân vật 2D sai kích thước/thời lượng"
    );
    Ok(output)
}
