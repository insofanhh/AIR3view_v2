//! App-wide export destination, independent of projects and their templates.
use crate::model::{write_json, Project};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Default, Serialize, Deserialize)]
struct Preferences {
    folder: Option<PathBuf>,
}

pub struct ExportLocation {
    config: PathBuf,
    preferences: Preferences,
}

impl ExportLocation {
    pub fn load(config: PathBuf) -> Self {
        let preferences = [config.clone(), config.with_extension("json.bak")]
            .iter()
            .find_map(|path| {
                fs::read(path)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            })
            .unwrap_or_default();
        Self {
            config,
            preferences,
        }
    }

    pub fn folder(&self) -> Option<&Path> {
        self.preferences.folder.as_deref()
    }

    pub fn remember(&mut self, folder: PathBuf) -> Result<()> {
        ensure!(
            folder.is_absolute() && folder.is_dir(),
            "Thư mục lưu không khả dụng. Kết nối lại ổ đĩa hoặc chọn Nơi lưu → Đổi thư mục."
        );
        let next = Preferences {
            folder: Some(folder),
        };
        write_json(&self.config, &next).context("Không ghi nhớ được thư mục xuất video")?;
        self.preferences = next;
        Ok(())
    }

    /// The picker runs only for the first export. A disconnected remembered
    /// drive is an actionable error, never a silent fallback to another drive.
    pub fn resolve(
        &mut self,
        project: &Project,
        pick: impl FnOnce() -> Option<PathBuf>,
    ) -> Result<Option<PathBuf>> {
        if self.folder().is_none() {
            let Some(folder) = pick() else {
                return Ok(None);
            };
            self.remember(folder)?;
        }
        let folder = self.folder().context("Chưa chọn thư mục lưu")?;
        ensure!(folder.is_absolute() && folder.is_dir(), "Ổ đĩa/thư mục lưu {} không khả dụng. Kết nối lại ổ đĩa hoặc chọn Nơi lưu → Đổi thư mục.", folder.display());
        let id = uuid::Uuid::parse_str(&project.id).context("ID dự án không hợp lệ")?;
        // Keep sidecars and equal filenames from different projects separate.
        Ok(Some(
            folder.join(format!("AIR3view-{id}")).join("final.mp4"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_location_remembers_across_restart_and_projects_and_cancel_keeps_it() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("settings/export-location.json");
        let folder = temp.path().join("Video có dấu");
        fs::create_dir(&folder).unwrap();
        let p = Project::default();
        let mut location = ExportLocation::load(config.clone());
        assert!(location.resolve(&p, || None).unwrap().is_none());
        assert!(!config.exists());
        let path = location
            .resolve(&p, || Some(folder.clone()))
            .unwrap()
            .unwrap();
        assert_eq!(path.parent().unwrap().parent().unwrap(), folder);
        assert!(path.ends_with("final.mp4"));
        let mut reopened = ExportLocation::load(config.clone());
        assert_eq!(
            reopened
                .resolve(&p, || panic!("must not ask again"))
                .unwrap(),
            Some(path.clone())
        );
        let next = reopened
            .resolve(&Project::default(), || {
                panic!("new project must reuse folder")
            })
            .unwrap()
            .unwrap();
        assert_ne!(next, path);
        assert_eq!(next.parent().unwrap().parent().unwrap(), folder);
        let other = temp.path().join("Other drive");
        fs::create_dir(&other).unwrap();
        reopened.remember(other.clone()).unwrap();
        assert_eq!(ExportLocation::load(config).folder(), Some(other.as_path()));
    }

    #[test]
    fn export_location_missing_drive_and_failed_save_preserve_selection() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("removable");
        fs::create_dir(&folder).unwrap();
        let config = temp.path().join("export-location.json");
        let mut location = ExportLocation::load(config.clone());
        location.remember(folder.clone()).unwrap();
        fs::remove_dir(&folder).unwrap();
        assert!(location
            .resolve(&Project::default(), || panic!(
                "do not ask on missing drive"
            ))
            .is_err());
        assert_eq!(
            ExportLocation::load(config).folder(),
            Some(folder.as_path())
        );
        assert!(location.remember(PathBuf::from("relative")).is_err());
        assert_eq!(location.folder(), Some(folder.as_path()));
        let blocked = temp.path().join("not-a-directory");
        fs::write(&blocked, b"file").unwrap();
        let mut bad = ExportLocation::load(blocked.join("prefs.json"));
        assert!(bad.remember(temp.path().into()).is_err());
        assert!(bad.folder().is_none());
    }
}
