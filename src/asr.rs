//! Shared ASR diagnostics for both evidence and subtitle alignment.
use crate::{model::Settings, process::{self, Cancel}};
use anyhow::Result;
use std::{fs, path::Path, time::Duration};

pub fn cuda_check(settings: &Settings, install: bool, cancel: &Cancel) -> Result<String> {
    let worker = crate::model::worker_path("transcribe.py");
    let output = process::run(&settings.python, &[
        worker.to_string_lossy().into_owned(),
        if install { "--setup-cuda" } else { "--check-cuda" }.into(),
    ], None, cancel, Duration::from_secs(if install { 1800 } else { 60 }), None)?;
    let report = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value = serde_json::from_str(report.lines().last().unwrap_or(""))?;
    Ok(format!("CUDA sẵn sàng: {} • cuBLAS {} • backend {} • {}", value["gpu"].as_str().unwrap_or(""),
        value["cublas"].as_str().unwrap_or(""), value["backend"].as_str().unwrap_or(""),
        value["supported_compute_types"]))
}

pub fn summary(words: &Path) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(&fs::read(words.with_extension("asr.json")).ok()?).ok()?;
    Some(format!("Whisper: {} / {} / {} • {} từ • {}s nhận dạng", value["model"].as_str()?,
        value["device"].as_str()?, value["compute_type"].as_str()?, value["word_count"], value["transcribe_seconds"]))
}
