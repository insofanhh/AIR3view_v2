use anyhow::Result;
use serde_json::{json, Value};
use std::{fs, io::Write, path::Path, sync::Mutex, time::SystemTime};

static LOG_LOCK: Mutex<()> = Mutex::new(());

/// Recognize explicit storage failures, never bare FFmpeg exit numbers (-28
/// can mean something else in a different tool). Keep the raw error for logs.
pub fn disk_full(text: &str) -> bool {
    let text = text.to_lowercase();
    ["no space left on device", "disk full", "disk is full",
        "not enough space on the disk", "not enough space on disk",
        "disk quota exceeded", "không đủ dung lượng", "ổ đĩa đã đầy",
        "os error 112", "os error 39", "os error 28"]
        .iter().any(|phrase| text.contains(phrase))
}

/// Apply at display time so persisted errors from older releases benefit too.
/// Unknown errors remain intact instead of guessing their cause.
pub fn friendly_error(text: &str) -> Option<String> {
    if !disk_full(text) { return None; }
    let lower = text.to_lowercase();
    let step = if lower.contains("avatar 2d") || lower.contains("cartoon_render") || lower.contains("/ffv1") {
        " khi tạo nhân vật 2D"
    } else if lower.contains("xuất mp4") || lower.contains("export") {
        " khi xuất video"
    } else { " khi xử lý video" };
    let scene = lower.split("cảnh ").nth(1).and_then(|s| s.split_whitespace().next())
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
        .map(|n| format!("Cảnh {n}: ")).unwrap_or_default();
    Some(format!("{scene}Không đủ dung lượng ổ đĩa{step}.\nKiểm tra ổ chứa dự án/cache và thư mục xuất video. Xóa dự án không cần hoặc giải phóng dung lượng, rồi bấm Thử lại / Tiếp tục. Đổi nơi lưu MP4 không chuyển cache sang ổ đó. Chi tiết trong Nhật ký."))
}

/// Do not lose the failure category when a long subprocess traceback is
/// redacted/truncated for durable workflow and queue messages.
pub fn failure_text(text: &str) -> String {
    let safe = safe_text(text, &[]);
    if disk_full(text) && !disk_full(&safe) {format!("Không đủ dung lượng ổ đĩa. {safe}")} else {safe}
}

/// Bound provider text and omit URLs, credentials, encoded media and long opaque tokens.
pub fn safe_text(text: &str, secrets: &[String]) -> String {
    let mut clean = text.chars().take(4096).collect::<String>();
    for secret in secrets.iter().filter(|s| !s.is_empty()) {
        clean = clean.replace(secret, "[redacted]");
    }
    clean
        .split_whitespace()
        .take(80)
        .map(|word| {
            if word.contains("://")
                || word.contains("www.")
                || word.contains("data:")
                || word.contains("sk_")
                || word.contains("sk-")
                || word.contains('@')
                || word.len() > 64
            {
                "[redacted]"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(600)
        .collect()
}

pub fn record(
    root: &Path,
    stage: &str,
    scene: Option<&str>,
    event: &str,
    details: Value,
) -> Result<()> {
    let _lock = LOG_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("Log lock error"))?;
    let folder = root.join("logs");
    fs::create_dir_all(&folder)?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(folder.join("pipeline.jsonl"))?;
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_millis();
    serde_json::to_writer(
        &mut file,
        &json!({"timestamp_ms":timestamp,"stage":stage,"scene_id":scene,"event":event,"details":details}),
    )?;
    file.write_all(b"\n")?;
    file.flush()?;
    Ok(())
}

pub fn failure(root: &Path, stage: &str, scene: Option<&str>, error: &anyhow::Error) -> Result<()> {
    let raw = format!("{error:#}");
    let mut details = json!({"message":failure_text(&raw)});
    if let Some(message) = friendly_error(&raw) {
        details["user_message"] = message.into();
        details["error_code"] = "disk_full".into();
    }
    if let Some(http) = error.downcast_ref::<crate::providers::HttpFailure>() {
        details["http"] = serde_json::to_value(http)?;
    }
    record(root, stage, scene, "failed", details)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disk_errors_explain_recovery_without_confusing_other_resources() {
        for text in [
            "Avatar 2D: [vost#0:0/ffv1] Error submitting a packet to the muxer: No space left on device; error code: -28",
            "There is not enough space on the disk. (os error 112)",
            "The disk is full (os error 39)", "Disk quota exceeded", "Không đủ dung lượng trống",
        ] {
            let message = friendly_error(text).unwrap();
            assert!(message.contains("Không đủ dung lượng ổ đĩa") && message.contains("cache") && message.contains("Thử lại"));
            assert!(!message.contains("muxer") && !message.contains("-28"));
        }
        assert!(friendly_error("Avatar 2D: No space left on device").unwrap().contains("tạo nhân vật 2D"));
        for other in ["CUDA out of memory", "Permission denied", "Invalid data found when processing input", "error code: -28", "HTTP 429 quota exceeded", "YouTube HTTP 403"] {
            assert!(friendly_error(other).is_none(), "Must not diagnose {other} as a full disk");
        }
    }
    #[test]
    fn storage_failure_log_keeps_technical_message_and_readable_diagnosis() {
        let temp = tempfile::tempdir().unwrap();
        failure(temp.path(), "Avatar 2D", Some("scene-8"), &anyhow::anyhow!("ffv1: No space left on device; error code: -28")).unwrap();
        let log: Value = serde_json::from_slice(&fs::read(temp.path().join("logs/pipeline.jsonl")).unwrap()).unwrap();
        assert_eq!(log["scene_id"], "scene-8");
        assert_eq!(log["details"]["error_code"], "disk_full");
        assert!(log["details"]["message"].as_str().unwrap().contains("-28"));
        assert!(log["details"]["user_message"].as_str().unwrap().contains("Thử lại"));
    }
    #[test]
    fn persisted_errors_keep_disk_diagnosis_when_the_trace_is_truncated() {
        let raw=format!("{} No space left on device", "FFmpeg trace ".repeat(500));
        assert!(!disk_full(&safe_text(&raw,&[])));
        let persisted=failure_text(&raw);
        assert!(disk_full(&persisted) && friendly_error(&persisted).is_some());
        assert!(friendly_error("Cảnh 8 / Avatar 2D: No space left on device").unwrap().starts_with("Cảnh 8:"));
        assert!(!friendly_error("Cảnh 8cbb0448-f953-4934-ba28-73cf2a6f6a56 / Avatar 2D: No space left on device").unwrap().starts_with("Cảnh 8:"));
    }
    #[cfg(windows)]
    #[test]
    fn subprocess_disk_failure_reaches_the_readable_diagnosis() {
        // Simulate stderr, not an actual full disk; no user files are changed.
        let error=crate::process::run("powershell.exe", &crate::process::args(&[
            "-NoProfile", "-NonInteractive", "-Command",
            "[Console]::Error.WriteLine('[vost#0:0/ffv1] Error submitting a packet to the muxer: No space left on device; error code: -28'); exit 1",
        ]),None,&crate::process::cancel_token(),std::time::Duration::from_secs(20),None).unwrap_err();
        let raw=format!("{error:#}");
        assert!(raw.contains("No space left on device"));
        assert!(friendly_error(&failure_text(&raw)).unwrap().contains("tạo nhân vật 2D"));
    }
}
