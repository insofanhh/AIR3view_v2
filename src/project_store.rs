//! Deletion is limited to a validated, direct child of the managed project library.
use crate::model::Project;
use anyhow::{ensure, Context, Result};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct Summary {
    pub id: String,
    pub title: String,
    pub status: String,
    pub source_count: usize,
    pub segment_count: usize,
    pub duration_ms: u64,
    pub created_at: Option<u64>,
    pub updated_at: Option<u64>,
}

#[derive(serde::Deserialize)]
struct SummaryFile {
    version: u32,
    id: String,
    title: String,
    #[serde(default)] status: String,
    #[serde(default)] sources: Vec<serde::de::IgnoredAny>,
    #[serde(default)] segments: Vec<SummarySegment>,
}
#[derive(serde::Deserialize)]
struct SummarySegment { #[serde(default)] duration_ms: u64 }

/// The library only reads manifest metadata, never migrates projects or probes
/// media. Cache by manifest size/mtime so unchanged rows do no JSON work.
#[derive(Default)]
pub struct SummaryCache {
    entries: BTreeMap<PathBuf, (u64, Option<std::time::SystemTime>, Option<Summary>)>,
    #[cfg(test)] pub reads: usize,
}
impl SummaryCache {
    pub fn clear(&mut self) { self.entries.clear(); }
    pub fn get(&mut self, path: &Path) -> Option<Summary> {
        let metadata = fs::metadata(path).ok()?;
        let stamp = (metadata.len(), metadata.modified().ok());
        if let Some((size, modified, summary)) = self.entries.get(path) {
            if (*size, *modified) == stamp { return summary.clone(); }
        }
        #[cfg(test)] { self.reads += 1; }
        // write_json replaces the manifest on every save. Directory birth time
        // is stable across those saves; never use the manifest's birth time.
        let created_at=path.parent().and_then(|p|fs::metadata(p).ok()).and_then(|m|m.created().ok())
            .or_else(||metadata.created().ok()).and_then(crate::date_filter::unix_time);
        let updated_at=stamp.1.and_then(crate::date_filter::unix_time);
        let summary = fs::read(path).ok().and_then(|bytes| serde_json::from_slice::<SummaryFile>(&bytes).ok())
            .filter(|p|p.version==1).map(|p| Summary {
                id:p.id, title:p.title, status:p.status, source_count:p.sources.len(),
                segment_count:p.segments.len(), duration_ms:p.segments.iter().fold(0u64, |sum,s|sum.saturating_add(s.duration_ms)),
                created_at,updated_at,
            });
        self.entries.insert(path.to_owned(), (stamp.0, stamp.1, summary.clone()));
        summary
    }
}

#[derive(Clone)]
pub struct DeleteInfo {
    pub path: PathBuf,
    pub root: PathBuf,
    pub title: String,
    pub bytes: u64,
}

pub fn list(root: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok().map(|entry| entry.path().join("project.json")))
        .filter(|path| path.is_file())
        .collect();
    paths.sort_by_key(|path| std::cmp::Reverse(fs::metadata(path).and_then(|m| m.modified()).ok()));
    paths
}

fn reject_link(path: &Path) -> Result<fs::Metadata> {
    let meta = fs::symlink_metadata(path)?;
    let mut link = meta.file_type().is_symlink();
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        link |= meta.file_attributes() & 0x400 != 0; // junction / reparse point
    }
    ensure!(
        !link,
        "Không xóa thư mục có liên kết/junction: {}",
        path.display()
    );
    Ok(meta)
}

fn tree_bytes(path: &Path) -> Result<u64> {
    let meta = reject_link(path)?;
    if !meta.is_dir() {
        return Ok(meta.len());
    }
    let mut bytes = 0u64;
    for entry in fs::read_dir(path)? {
        bytes = bytes.saturating_add(tree_bytes(&entry?.path())?);
    }
    Ok(bytes)
}

pub fn inspect(library: &Path, path: &Path) -> Result<DeleteInfo> {
    reject_link(library)?;
    let library = fs::canonicalize(library)?;
    ensure!(
        path.file_name().is_some_and(|name| name == "project.json"),
        "Chỉ xóa thư mục dự án được quản lý"
    );
    let parent = path.parent().context("Thiếu thư mục dự án")?;
    reject_link(parent)?;
    reject_link(path)?;
    let root = fs::canonicalize(parent)?;
    ensure!(
        root.parent() == Some(library.as_path()),
        "Dự án nằm ngoài thư viện; không xóa thư mục này"
    );
    let project = Project::load(&root.join("project.json"))?;
    let name = root
        .file_name()
        .context("Thiếu tên thư mục dự án")?
        .to_string_lossy();
    ensure!(
        uuid::Uuid::parse_str(&name).is_ok() && name == project.id,
        "ID không khớp thư mục dự án; không xóa"
    );
    let bytes = tree_bytes(&root)?;
    Ok(DeleteInfo {
        path: root.join("project.json"),
        root,
        title: project.title,
        bytes,
    })
}

pub fn uses_directory(project: &Project, root: &Path) -> bool {
    let paths = project
        .sources
        .iter()
        .flat_map(|source| [&source.path, &source.proxy])
        .chain(
            project
                .segments
                .iter()
                .flat_map(|seg| seg.audio.iter().chain(seg.avatar.iter())),
        )
        .chain(project.rendered.iter());
    paths
        .filter(|path| !path.as_os_str().is_empty())
        .any(|path| {
            let path = if path.is_absolute() {
                path.clone()
            } else {
                project.root.join(path)
            };
            fs::canonicalize(&path).unwrap_or(path).starts_with(root)
        })
}

pub fn delete(library: &Path, path: &Path, active: Option<&Project>) -> Result<u64> {
    if let (Some(data),Some(id))=(library.parent(),path.parent().and_then(|p|p.file_name()).and_then(|v|v.to_str())) {
        ensure!(!crate::batch::reserved_project(data,id),"Dự án đang ở hàng đợi chạy; tạm dừng trước khi xóa");
    }
    // Recheck on confirmation; do not trust the path/size captured by the dialog.
    let info = inspect(library, path)?;
    for other_path in list(library) {
        let other_root = fs::canonicalize(other_path.parent().context("Thiếu thư mục")?)?;
        if other_root == info.root {
            continue;
        }
        let other = Project::load(&other_path)
            .context("Có dự án không đọc được; chưa thể kiểm tra tài nguyên dùng chung")?;
        ensure!(
            !uses_directory(&other, &info.root),
            "Dự án “{}” đang dùng file trong thư mục này; cần chuyển tài nguyên trước khi xóa",
            other.title
        );
    }
    if let Some(active) = active {
        if fs::canonicalize(&active.root).ok().as_ref() != Some(&info.root) {
            ensure!(
                !uses_directory(active, &info.root),
                "Dự án đang mở sử dụng file trong thư mục cần xóa"
            );
        }
    }
    // All absolute boundaries and the complete tree were checked above. Never
    // follow paths from project JSON to delete imported files outside this tree.
    let data = library.parent().context("Thiếu thư mục dữ liệu thư viện")?;
    let project_id = info.root.file_name().context("Thiếu ID dự án")?.to_string_lossy();
    crate::batch::remove_project(data, &project_id, || {
    let manifest = fs::read(&info.path)?;
    let removal = (|| -> std::io::Result<()> {
        // Keep project.json until the data is gone, so a locked cache file
        // leaves a visible project which the user can retry deleting.
        for entry in fs::read_dir(&info.root)? {
            let entry = entry?;
            if entry.file_name() == "project.json" {
                continue;
            }
            if entry.file_type()?.is_dir() {
                fs::remove_dir_all(entry.path())?;
            } else {
                fs::remove_file(entry.path())?;
            }
        }
        fs::remove_file(&info.path)?;
        fs::remove_dir(&info.root)
    })();
    if removal.is_err() && info.root.is_dir() && !info.path.exists() {
        fs::write(&info.path, manifest)
            .context("Không xóa hết được dự án và không phục hồi được project.json")?;
    }
    removal.context("Không xóa hết được dự án; đóng file đang mở rồi thử lại")?;
    Ok(info.bytes)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn library_summary_is_read_only_cached_and_refreshes_changed_manifests() {
        let temp=tempfile::tempdir().unwrap();
        let path=temp.path().join("project.json");
        let mut p=Project{title:"Dự án cũ có media không truy cập được".into(),status:"Sẵn sàng".into(),..Default::default()};
        p.sources.push(crate::model::Source::default());
        p.segments.push(crate::model::Segment{duration_ms:12345,avatar:Some(temp.path().join("missing-avatar.mkv")),..Default::default()});
        let bytes=serde_json::to_vec(&p).unwrap();fs::write(&path,&bytes).unwrap();
        let mut cache=SummaryCache::default();
        let created_at=cache.get(&path).unwrap().created_at;
        for _ in 0..100 {
            let summary=cache.get(&path).unwrap();
            assert_eq!(summary.title,p.title);assert_eq!(summary.source_count,1);
            assert_eq!(summary.segment_count,1);assert_eq!(summary.duration_ms,12345);
        }
        assert_eq!(cache.reads,1);
        assert_eq!(fs::read(&path).unwrap(),bytes);
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(),1,"Library must not migrate or create backups/cache");
        p.status="Đã xuất video mới".into();
        fs::write(&path,serde_json::to_vec(&p).unwrap()).unwrap();
        assert_eq!(cache.get(&path).unwrap().status,p.status);assert_eq!(cache.reads,2);
        assert_eq!(cache.get(&path).unwrap().created_at,created_at,"Saving a manifest must not change the project creation date");
        fs::write(&path,b"invalid manifest").unwrap();
        assert!(cache.get(&path).is_none());assert!(cache.get(&path).is_none());assert_eq!(cache.reads,3);
        cache.clear();assert!(cache.get(&path).is_none());assert_eq!(cache.reads,4);
        fs::remove_file(&path).unwrap();assert!(cache.get(&path).is_none());
    }
    use crate::model::Source;

    fn project(library: &Path) -> Project {
        let mut p = Project::default();
        p.root = library.join(&p.id);
        p.save().unwrap();
        p
    }

    #[test]
    fn deletes_owned_tree_but_keeps_imported_files_and_other_projects() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("projects");
        let mut p = project(&library);
        let other = project(&library);
        let external = temp.path().join("imported.mp4");
        fs::write(&external, b"original video").unwrap();
        p.sources.push(Source {
            path: external.clone(),
            ..Default::default()
        });
        fs::create_dir(p.root.join("cache")).unwrap();
        fs::write(p.root.join("cache/voice.wav"), b"audio").unwrap();
        p.save().unwrap();
        let info = inspect(&library, &p.root.join("project.json")).unwrap();
        assert!(info.bytes > 5);
        assert_eq!(delete(&library, &info.path, Some(&p)).unwrap(), info.bytes);
        assert!(!p.root.exists());
        assert_eq!(fs::read(external).unwrap(), b"original video");
        assert!(other.root.join("project.json").is_file());
    }

    #[test]
    fn refuses_external_root_id_mismatch_and_shared_resources() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("projects");
        let p = project(&library);
        let outside = project(temp.path());
        assert!(delete(&library, &outside.root.join("project.json"), None).is_err());
        assert!(delete(&library, &library.join("../project.json"), None).is_err());
        assert!(outside.root.exists());
        let resource = p.root.join("source.mp4");
        fs::write(&resource, b"shared").unwrap();
        let mut other = project(&library);
        other.sources.push(Source {
            path: resource,
            ..Default::default()
        });
        other.save().unwrap();
        assert!(delete(&library, &p.root.join("project.json"), None)
            .unwrap_err()
            .to_string()
            .contains("đang dùng file"));
        assert!(p.root.exists());
        let mut bad = p.clone();
        bad.id = uuid::Uuid::new_v4().to_string();
        bad.save().unwrap();
        assert!(inspect(&library, &bad.root.join("project.json")).is_err());
    }

    #[test]
    fn refuses_directory_links_before_recursive_deletion() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("projects");
        let p = project(&library);
        let external = temp.path().join("external");
        fs::create_dir(&external).unwrap();
        fs::write(external.join("keep.txt"), b"keep").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&external, p.root.join("linked")).unwrap();
        #[cfg(windows)]
        {
            // Junctions work without Developer Mode or symbolic-link privilege.
            let status = crate::process::command("cmd.exe")
                .args(["/C", "mklink", "/J"])
                .arg(p.root.join("linked"))
                .arg(&external)
                .stdout(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(status.success(), "Create test junction");
        }
        assert!(delete(&library, &p.root.join("project.json"), None).is_err());
        assert!(external.join("keep.txt").is_file());
    }

    #[cfg(windows)]
    #[test]
    fn locked_file_leaves_project_visible_for_retry() {
        use std::os::windows::fs::OpenOptionsExt;
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("projects");
        let p = project(&library);
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .share_mode(0)
            .open(p.root.join("locked.wav"))
            .unwrap();
        assert!(delete(&library, &p.root.join("project.json"), None).is_err());
        assert!(Project::load(&p.root.join("project.json")).is_ok());
        assert_eq!(list(&library).len(), 1);
        drop(file);
        delete(&library, &p.root.join("project.json"), None).unwrap();
        assert!(!p.root.exists());
    }
}
