use anyhow::Result;
use serde_json::{json, Value};
use std::{fs, io::Write, path::Path, sync::Mutex, time::SystemTime};

static LOG_LOCK: Mutex<()> = Mutex::new(());

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
    let mut details = json!({"message":safe_text(&format!("{error:#}"), &[])});
    if let Some(http) = error.downcast_ref::<crate::providers::HttpFailure>() {
        details["http"] = serde_json::to_value(http)?;
    }
    record(root, stage, scene, "failed", details)
}
