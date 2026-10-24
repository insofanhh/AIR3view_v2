//! Destination for new YouTube originals, independent of the MP4 export folder.
use crate::model::{write_json, Project, Settings};
use anyhow::{ensure, Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn needs_download(p: &Project) -> bool {
    p.sources
        .iter()
        .any(|s| !s.url.is_empty() && !s.path.is_file())
}

pub fn validate(settings: &Settings) -> Result<()> {
    let value = settings.youtube_download_folder.trim();
    if value.is_empty() {
        return Ok(());
    }
    let base = Path::new(value);
    ensure!(base.is_absolute() && base.is_dir(),
        "Thư mục tải video nguồn {} không khả dụng. Kết nối lại ổ đĩa hoặc chọn Nơi lưu / metadata → Đổi thư mục video nguồn.", base.display());
    Ok(())
}

pub fn folder(p: &Project, settings: &Settings) -> Result<PathBuf> {
    validate(settings)?;
    if settings.youtube_download_folder.trim().is_empty() {
        return Ok(p.root.join("sources"));
    }
    let id =
        uuid::Uuid::parse_str(&p.id).context("ID dự án không hợp lệ để tạo thư mục video nguồn")?;
    Ok(Path::new(&settings.youtube_download_folder)
        .join(format!("AIR3view-{id}"))
        .join("sources"))
}

/// Save only the folder field: unsaved connection edits must not be committed by a picker.
pub fn remember(settings: &mut Settings, config: &Path, folder: Option<PathBuf>) -> Result<()> {
    let value = folder
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let candidate = Settings {
        youtube_download_folder: value.clone(),
        ..settings.clone()
    };
    validate(&candidate)?;
    if !value.is_empty() {
        let _probe = tempfile::Builder::new()
            .prefix(".air3view-write-")
            .tempfile_in(&value)
            .with_context(|| {
                format!(
                    "Không ghi được vào thư mục video nguồn {}. Chọn thư mục có quyền ghi.",
                    value
                )
            })?;
    }
    let mut stored = if config.is_file() {
        serde_json::from_slice::<serde_json::Value>(&fs::read(config)?)
            .context("Không đọc được cấu hình đã lưu; chưa đổi thư mục video nguồn")?
    } else {
        serde_json::to_value(&*settings)?
    };
    let fields = stored
        .as_object_mut()
        .context("Cấu hình đã lưu không hợp lệ")?;
    fields.insert(
        "youtube_download_folder".into(),
        serde_json::Value::String(value.clone()),
    );
    write_json(config, &stored).context("Không lưu được thư mục video nguồn")?;
    settings.youtube_download_folder = value;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{media, model::Source, process};

    #[test]
    fn selection_survives_restart_preserves_other_settings_and_reset_keeps_old_media() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("settings.json");
        fs::write(&config, br#"{"voice":"saved voice","future_field":42}"#).unwrap();
        let mut s = Settings {
            voice: "unsaved voice".into(),
            ..Default::default()
        };
        let base = temp.path().join("Video nguồn có dấu");
        fs::create_dir(&base).unwrap();
        remember(&mut s, &config, Some(base.clone())).unwrap();
        assert_eq!(s.voice, "unsaved voice");
        let saved = Settings::load(&config).unwrap();
        assert_eq!(saved.voice, "saved voice");
        assert_eq!(saved.youtube_download_folder, base.to_string_lossy());
        let json: serde_json::Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
        assert_eq!(json["future_field"], 42);
        let mut p = Project {
            root: temp.path().join("project"),
            ..Default::default()
        };
        let destination = folder(&p, &saved).unwrap();
        fs::create_dir_all(&destination).unwrap();
        let old = destination.join("old.mp4");
        fs::write(&old, b"existing original").unwrap();
        p.sources.push(Source {
            url: "https://youtu.be/dBy-8s6ly08".into(),
            path: old.clone(),
            ..Default::default()
        });
        assert!(!needs_download(&p));
        let mut reopened = saved;
        remember(&mut reopened, &config, None).unwrap();
        assert_eq!(folder(&p, &reopened).unwrap(), p.root.join("sources"));
        assert_eq!(fs::read(old).unwrap(), b"existing original");
        assert!(Settings::load(&config)
            .unwrap()
            .youtube_download_folder
            .is_empty());
        let other = Project::default();
        assert_ne!(folder(&p, &s).unwrap(), folder(&other, &s).unwrap());
    }

    #[test]
    fn missing_drive_invalid_paths_and_failed_save_do_not_replace_a_selection() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().join("removable");
        fs::create_dir(&base).unwrap();
        let config = temp.path().join("settings.json");
        let mut s = Settings::default();
        remember(&mut s, &config, Some(base.clone())).unwrap();
        let before = fs::read(&config).unwrap();
        assert!(remember(&mut s, &config, Some("relative".into())).is_err());
        assert_eq!(fs::read(&config).unwrap(), before);
        let blocked = temp.path().join("file");
        fs::write(&blocked, b"not a directory").unwrap();
        assert!(remember(&mut s, &blocked.join("settings.json"), None).is_err());
        assert_eq!(s.youtube_download_folder, base.to_string_lossy());
        fs::remove_dir(&base).unwrap();
        let error = folder(&Project::default(), &s).unwrap_err().to_string();
        assert!(error.contains("video nguồn") && error.contains("Nơi lưu / metadata"));
        assert!(
            !base.exists(),
            "Never silently recreate an unavailable destination"
        );
        assert_eq!(fs::read(&config).unwrap(), before);
        let legacy: Settings = serde_json::from_str("{}").unwrap();
        assert!(legacy.youtube_download_folder.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn download_pipeline_writes_originals_to_selected_drive_and_reopens_absolute_paths() {
        use std::{sync::mpsc, time::Duration};
        let temp = tempfile::tempdir().unwrap();
        let mut s = Settings::default();
        let cancel = process::cancel_token();
        let input = temp.path().join("fixture.mp4");
        process::run(
            &s.ffmpeg,
            &[
                "-y".into(),
                "-v".into(),
                "error".into(),
                "-f".into(),
                "lavfi".into(),
                "-i".into(),
                "testsrc2=size=320x180:rate=24:duration=1".into(),
                "-c:v".into(),
                "libx264".into(),
                "-preset".into(),
                "ultrafast".into(),
                input.to_string_lossy().into_owned(),
            ],
            None,
            &cancel,
            Duration::from_secs(30),
            None,
        )
        .unwrap();
        // Simulate only the network boundary. Probe, proxy, manifest and frame decoding are real.
        let script = temp.path().join("downloader.rs");
        fs::write(
            &script,
            format!(
                r##"use std::{{env,fs,path::PathBuf}};
fn main() {{
    let args:Vec<_>=env::args().collect();
    let template=&args[args.iter().position(|s|s=="-o").unwrap()+1];
    let p=PathBuf::from(template.replace("%(ext)s","mp4"));
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::copy({:?},&p).unwrap();
    fs::write(p.with_extension("info.json"),br#"{{"title":"YouTube fixture"}}"#).unwrap();
    println!("{{}}",p.display());
}}
"##,
                input
            ),
        )
        .unwrap();
        let shim = temp.path().join("download.exe");
        let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
        process::run(
            &rustc,
            &[
                script.to_string_lossy().into_owned(),
                "--crate-name".into(),
                "fake_downloader".into(),
                "-o".into(),
                shim.to_string_lossy().into_owned(),
            ],
            None,
            &cancel,
            Duration::from_secs(60),
            None,
        )
        .unwrap();
        let base = temp.path().join("Ổ lưu video nguồn");
        fs::create_dir(&base).unwrap();
        s.youtube_download_folder = base.to_string_lossy().into_owned();
        s.ytdlp = shim.to_string_lossy().into_owned();
        s.provider = "offline-test".into();
        let mut original_paths = vec![];
        for _ in 0..2 {
            let mut p = Project::default();
            p.root = temp.path().join("projects").join(&p.id);
            p.sources = vec![Source {
                id: "src_1".into(),
                url: "https://youtu.be/dBy-8s6ly08".into(),
                ..Default::default()
            }];
            let destination = folder(&p, &s).unwrap();
            let root = p.root.clone();
            let (tx, _rx) = mpsc::channel();
            // The intentional AI-provider error leaves the successful download committed for retry.
            let analysis_error =
                crate::pipeline::prepare(p, s.clone(), tx, cancel.clone()).unwrap_err();
            let saved = Project::load(&root.join("project.json")).unwrap();
            let src = &saved.sources[0];
            assert!(
                src.path.starts_with(&destination) && src.path.is_file(),
                "{}; pipeline result: {analysis_error:#}",
                src.path.display()
            );
            assert_eq!(src.title, "YouTube fixture");
            assert!(!root.join("sources").exists());
            assert!(src.proxy.starts_with(root.join("cache/proxy")) && src.proxy.is_file());
            assert_eq!(src.width, 320);
            assert!(media::frame(&s, &src.path, 0, &cancel).unwrap().width() > 0);
            let cached = media::download(
                &Settings {
                    ytdlp: "missing-downloader.exe".into(),
                    ..s.clone()
                },
                &src.url,
                &destination,
                "src_2".into(),
                &cancel,
            )
            .unwrap();
            assert_eq!(cached.path, src.path);
            original_paths.push(src.path.clone());
        }
        assert_ne!(
            original_paths[0], original_paths[1],
            "Projects never overwrite each other's originals"
        );
    }
}
