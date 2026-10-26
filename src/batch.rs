//! Durable, serial batch queue. The supervisor and each project run outside the GUI.
use crate::{diagnostics, media, model::*, pipeline, process, workflow};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Stdio},
    sync::{atomic::Ordering, mpsc},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn ensure_capacity(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetDiskFreeSpaceExW(
                path: *const u16,
                available: *mut u64,
                total: *mut u64,
                free: *mut u64,
            ) -> i32;
        }
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut available = 0;
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                path.as_ptr(),
                &mut available,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        ensure!(ok != 0, "Ổ đĩa/thư mục không khả dụng");
        ensure!(
            available >= 512 * 1024 * 1024,
            "Không đủ dung lượng trống (cần ít nhất 512 MB để tiếp tục)"
        );
    }
    #[cfg(not(windows))]
    let _ = path;
    Ok(())
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum State {
    #[default]
    Pending,
    Running,
    Ready,
    Exported,
    Failed,
    Interrupted,
    Cancelled,
    Skipped,
}
impl State {
    pub fn label(&self) -> &str {
        match self {
            Self::Pending => "Đang chờ",
            Self::Running => "Đang chạy",
            Self::Ready => "Sẵn sàng biên tập",
            Self::Exported => "Đã xuất MP4",
            Self::Failed => "Lỗi",
            Self::Interrupted => "Bị gián đoạn",
            Self::Cancelled => "Đã dừng",
            Self::Skipped => "Đã bỏ qua",
        }
    }
    pub fn complete(&self) -> bool {
        matches!(self, Self::Ready | Self::Exported)
    }
    pub fn retryable(&self) -> bool {
        matches!(self, Self::Failed | Self::Interrupted | Self::Cancelled)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub project_id: String,
    pub url: String,
    pub title: String,
    pub state: State,
    pub stage: String,
    pub message: String,
    pub attempt_id: String,
    pub attempts: u32,
    pub auto_retries: u32,
    pub retry_at: u64,
    pub output: Option<PathBuf>,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
    #[serde(default)]
    pub export_only: bool,
    #[serde(default)]
    pub force_export: bool,
    #[serde(default)]
    pub export_existing: Option<ExportFile>,
}
impl Item {
    pub fn can_export(&self) -> bool {
        self.state.complete() || (self.export_only && (self.state.retryable() || self.state == State::Pending))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Batch {
    pub version: u32,
    pub id: String,
    pub title: String,
    pub created_at: u64,
    pub status: String,
    pub auto_export: bool,
    pub output_folder: Option<PathBuf>,
    pub items: Vec<Item>,
    /// A manual export runs only these items; unrelated pending URLs stay queued.
    #[serde(default)]
    pub run_items: Option<Vec<String>>,
}
#[derive(Clone, Debug, Default)]
pub struct UrlList {
    pub urls: Vec<String>,
    pub duplicates: usize,
    pub invalid: Vec<(usize, String)>,
}
pub fn parse_urls(text: &str) -> UrlList {
    let mut list = UrlList::default();
    for (line, s) in text
        .lines()
        .enumerate()
        .filter(|(_, s)| !s.trim().is_empty())
    {
        match validate_youtube_url(s) {
            Ok(url) => {
                if list.urls.contains(&url) {
                    list.duplicates += 1;
                } else {
                    list.urls.push(url);
                }
            }
            Err(_) => list.invalid.push((line + 1, s.trim().into())),
        }
    }
    list
}
pub fn root(data: &Path, id: &str) -> Result<PathBuf> {
    let id = uuid::Uuid::parse_str(id)?;
    Ok(data.join("batches").join(id.to_string()))
}
pub fn project_path(data: &Path, item: &Item) -> Result<PathBuf> {
    let id = uuid::Uuid::parse_str(&item.project_id)?;
    Ok(data
        .join("projects")
        .join(id.to_string())
        .join("project.json"))
}
impl Batch {
    pub fn load(path: &Path) -> Result<Self> {
        let batch: Self = [path.to_owned(), path.with_extension("json.bak")]
            .iter()
            .find_map(|p| {
                fs::read(p)
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok())
            })
            .context("Không đọc được hàng đợi hoặc bản khôi phục")?;
        ensure!(batch.version == 1, "Phiên bản hàng đợi chưa hỗ trợ");
        uuid::Uuid::parse_str(&batch.id)?;
        let mut ids = std::collections::HashSet::new();
        for item in &batch.items {
            uuid::Uuid::parse_str(&item.id)?;
            uuid::Uuid::parse_str(&item.project_id)?;
            ensure!(ids.insert(&item.project_id), "Trùng dự án trong hàng đợi");
        }
        if let Some(scope) = &batch.run_items {
            ensure!(scope.iter().all(|id| batch.items.iter().any(|i| &i.id == id)), "Phạm vi xuất chứa dự án không có trong phiên");
        }
        Ok(batch)
    }
    pub fn save(&self, data: &Path) -> Result<()> {
        write_json(&root(data, &self.id)?.join("batch.json"), self)
    }
    pub fn scheduled(&self, item: &Item) -> bool {
        self.run_items.as_ref().is_none_or(|ids| ids.contains(&item.id))
    }
    pub fn resumable(&self) -> bool {
        self.items.iter().any(|i| self.scheduled(i) && matches!(i.state, State::Pending | State::Interrupted | State::Cancelled))
    }
    pub fn reserved(&self, project_id: &str) -> bool {
        matches!(self.status.as_str(), "running" | "pausing")
            && self.items.iter().any(|i| {
                self.scheduled(i) && i.project_id == project_id && matches!(i.state, State::Pending | State::Running)
            })
    }
}
fn snapshot_file(path: &mut String, folder: &Path) -> Result<()> {
    if path.is_empty() {
        return Ok(());
    }
    let src = PathBuf::from(&*path);
    let bytes = fs::read(&src).context("Không đọc được ảnh/giọng tham chiếu")?;
    let name = format!(
        "{}.{}",
        media::hash(&bytes),
        src.extension().and_then(|v| v.to_str()).unwrap_or("bin")
    );
    fs::create_dir_all(folder)?;
    let target = folder.join(name);
    if !target.is_file() {
        fs::write(&target, bytes)?;
    }
    *path = target.to_string_lossy().into_owned();
    Ok(())
}
pub fn create(
    data: &Path,
    title: &str,
    urls: &str,
    template: &Project,
    settings: &Settings,
    auto_export: bool,
    output: Option<PathBuf>,
) -> Result<Batch> {
    let list = parse_urls(urls);
    ensure!(
        list.invalid.is_empty(),
        "Có URL sai ở dòng {:?}",
        list.invalid.iter().map(|v| v.0).collect::<Vec<_>>()
    );
    ensure!(!list.urls.is_empty(), "Thêm ít nhất một URL YouTube");
    ensure!(list.urls.len() <= 200, "Mỗi đợt tối đa 200 video");
    if auto_export {
        ensure!(
            output
                .as_ref()
                .is_some_and(|p| p.is_absolute() && p.is_dir()),
            "Chọn thư mục xuất MP4 còn khả dụng"
        );
    }
    let mut batch = Batch {
        version: 1,
        id: uuid::Uuid::new_v4().to_string(),
        title: if title.trim().is_empty() {
            "Phiên hàng loạt".into()
        } else {
            title.trim().into()
        },
        created_at: now(),
        status: "queued".into(),
        auto_export,
        output_folder: output,
        items: vec![],
        run_items: None,
    };
    let mut defaults = ProjectDefaults::from_project(template);
    if defaults.production.is_none() {
        defaults.production = Some(ProductionSettings::from_settings(settings));
    }
    let mut projects = vec![];
    for (index, url) in list.urls.into_iter().enumerate() {
        let mut p = Project::default();
        defaults.apply(&mut p);
        p.root = data.join("projects").join(&p.id);
        p.title = format!("{} / {:02}", batch.title, index + 1);
        p.input_urls = url.clone();
        p.sources = vec![Source {
            id: "src_1".into(),
            url: url.clone(),
            ..Default::default()
        }];
        if let Some(prod) = &mut p.production {
            let assets = p.root.join("inputs");
            if prod.voice_reference_enabled {
                snapshot_file(&mut prod.voice_reference_path, &assets)?;
            }
            if prod.avatar_enabled && !prod.avatar_image_path.is_empty() {
                snapshot_file(&mut prod.avatar_image_path, &assets)?;
            }
        }
        workflow::preflight(&p, &workflow::effective_settings(&p, settings))?;
        batch.items.push(Item {
            id: uuid::Uuid::new_v4().to_string(),
            project_id: p.id.clone(),
            url,
            title: p.title.clone(),
            state: State::Pending,
            stage: "prepare".into(),
            message: String::new(),
            attempt_id: String::new(),
            attempts: 0,
            auto_retries: 0,
            retry_at: 0,
            output: None,
            started_at: None,
            finished_at: None,
            export_only: false,
            force_export: false,
            export_existing: None,
        });
        projects.push(p);
    }
    // Publish the queue only after all project manifests and the immutable template exist.
    for p in &projects {
        p.save()?;
    }
    let folder = root(data, &batch.id)?;
    fs::create_dir_all(&folder)?;
    write_json(&folder.join("template.json"), &defaults)?;
    write_json(&folder.join("settings.json"), settings)?;
    batch.save(data)?;
    Ok(batch)
}
pub fn list(data: &Path) -> Vec<Batch> {
    let mut batches: Vec<_> = fs::read_dir(data.join("batches"))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| Batch::load(&e.path().join("batch.json")).ok())
        .collect();
    batches.sort_by_key(|b| std::cmp::Reverse((b.created_at,b.id.clone())));
    batches
}
pub struct Lease {
    _file: fs::File,
}
/// Windows closes these handles even when the GUI/supervisor crashes, killing
/// the worker tree before another queue may acquire the released leases.
pub struct KillJob {
    #[cfg(windows)]
    handle: *mut std::ffi::c_void,
}
impl KillJob {
    pub fn attach(child: &Child) -> Result<Self> {
        #[cfg(windows)]
        {
            use std::{ffi::c_void, os::windows::io::AsRawHandle};
            #[repr(C)]
            #[derive(Default)]
            struct Basic {
                process_time: i64,
                job_time: i64,
                flags: u32,
                min_working: usize,
                max_working: usize,
                active: u32,
                affinity: usize,
                priority: u32,
                scheduling: u32,
            }
            #[repr(C)]
            #[derive(Default)]
            struct Limits {
                basic: Basic,
                io: [u64; 6],
                process_memory: usize,
                job_memory: usize,
                peak_process: usize,
                peak_job: usize,
            }
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn CreateJobObjectW(attrs: *const c_void, name: *const u16) -> *mut c_void;
                fn SetInformationJobObject(
                    job: *mut c_void,
                    class: i32,
                    info: *const c_void,
                    len: u32,
                ) -> i32;
                fn AssignProcessToJobObject(job: *mut c_void, process: *mut c_void) -> i32;
            }
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            ensure!(!handle.is_null(), "Không tạo được Windows Job Object");
            let job = Self { handle };
            let limits = Limits {
                basic: Basic {
                    flags: 0x2000,
                    ..Default::default()
                },
                ..Default::default()
            };
            ensure!(
                unsafe {
                    SetInformationJobObject(
                        handle,
                        9,
                        &limits as *const _ as *const c_void,
                        std::mem::size_of::<Limits>() as u32,
                    )
                } != 0,
                "Không thiết lập được vòng đời worker"
            );
            ensure!(
                unsafe { AssignProcessToJobObject(handle, child.as_raw_handle()) } != 0,
                "Không gắn được worker vào Windows Job Object"
            );
            Ok(job)
        }
        #[cfg(not(windows))]
        {
            let _ = child;
            Ok(Self {})
        }
    }
}
impl Drop for KillJob {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
            }
            unsafe {
                CloseHandle(self.handle);
            }
        }
    }
}
impl Lease {
    pub fn acquire(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut opts = fs::OpenOptions::new();
        opts.read(true).write(true).create(true).truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            opts.share_mode(0);
        }
        Ok(Self {
            _file: opts
                .open(path)
                .context("Hàng đợi/dự án đang được worker khác xử lý")?,
        })
    }
}
pub fn running(data: &Path) -> bool {
    Lease::acquire(&data.join("runtime/batch.lock")).is_err()
}
pub fn reserved_project(data: &Path, id: &str) -> bool {
    list(data).iter().any(|b| b.reserved(id)) && running(data)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", content = "item_id", rename_all = "snake_case")]
pub enum Action {
    Pause,
    Stop,
    RetryAll,
    Retry(String),
    Skip(String),
    Up(String),
}
pub fn send(data: &Path, id: &str, action: Action) -> Result<()> {
    let folder = root(data, id)?.join("commands");
    fs::create_dir_all(&folder)?;
    write_json(
        &folder.join(format!("{}-{}.json", now(), uuid::Uuid::new_v4())),
        &action,
    )
}
fn apply_action(batch: &mut Batch, action: Action) -> bool {
    match action {
        Action::Pause => {
            batch.status = "pausing".into();
            false
        }
        Action::Stop => {
            batch.status = "pausing".into();
            true
        }
        Action::RetryAll => {
            batch.run_items = None;
            for i in &mut batch.items {
                if i.state.retryable() {
                    i.state = State::Pending;
                    i.auto_retries = 0;
                    i.retry_at = 0;
                    i.message.clear();
                }
            }
            false
        }
        Action::Retry(id) => {
            if let Some(i) = batch
                .items
                .iter_mut()
                .find(|i| i.id == id && i.state.retryable())
            {
                i.state = State::Pending;
                i.auto_retries = 0;
                i.retry_at = 0;
                i.message.clear();
                if let Some(ids) = &mut batch.run_items {
                    if !ids.contains(&id) { ids.push(id); }
                }
            }
            false
        }
        Action::Skip(id) => {
            if let Some(i) = batch
                .items
                .iter_mut()
                .find(|i| i.id == id && i.state == State::Pending)
            {
                i.state = State::Skipped;
            }
            false
        }
        Action::Up(id) => {
            if let Some(n) = batch
                .items
                .iter()
                .position(|i| i.id == id && i.state == State::Pending)
            {
                if let Some(other) = (0..n)
                    .rev()
                    .find(|j| batch.items[*j].state == State::Pending)
                {
                    batch.items.swap(n, other);
                }
            }
            false
        }
    }
}
fn commands(data: &Path, batch: &mut Batch) -> Result<bool> {
    let folder = root(data, &batch.id)?.join("commands");
    let mut paths: Vec<_> = fs::read_dir(folder)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    let changed = !paths.is_empty();
    let mut stop = false;
    for path in paths {
        let action: Action = serde_json::from_slice(&fs::read(&path)?)?;
        stop |= apply_action(batch, action);
        fs::remove_file(path)?;
    }
    if changed {
        batch.save(data)?;
    }
    Ok(stop)
}
pub fn action(data: &Path, id: &str, action: Action) -> Result<()> {
    match Lease::acquire(&root(data, id)?.join("queue.lock")) {
        Ok(_lease) => {
            let mut b = Batch::load(&root(data, id)?.join("batch.json"))?;
            apply_action(&mut b, action);
            b.save(data)
        }
        Err(_) => send(data, id, action),
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportFile {
    pub path: PathBuf,
    pub bytes: u64,
    pub modified_ns: u128,
}
impl ExportFile {
    fn read(path: &Path) -> Result<Option<Self>> {
        let metadata = match fs::metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("Không đọc được MP4 đích: {}", path.display())),
        };
        ensure!(metadata.is_file(), "Đường dẫn MP4 đích không phải file: {}", path.display());
        Ok(Some(Self { path: path.to_owned(), bytes: metadata.len(),
            modified_ns: metadata.modified()?.duration_since(UNIX_EPOCH)?.as_nanos() }))
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportTarget {
    pub item_id: String,
    pub title: String,
    pub output: PathBuf,
    pub existing: Option<ExportFile>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportPlan {
    pub batch_id: String,
    pub item_id: Option<String>,
    pub folder: PathBuf,
    pub targets: Vec<ExportTarget>,
    pub skipped: Vec<String>,
}
impl ExportPlan {
    pub fn existing_count(&self) -> usize { self.targets.iter().filter(|t|t.existing.is_some()).count() }
}
/// Read without migrations/backups: inspection must not recreate deleted projects.
fn export_project(path: &Path, project_id: &str) -> Result<Project> {
    let bytes = fs::read(path).with_context(|| format!("Không đọc được dữ liệu dự án: {}. Nếu đã xóa, hãy khôi phục thư mục dự án hoặc tạo lại từ URL; MP4 cũ vẫn được giữ.", path.display()))?;
    let mut p: Project = serde_json::from_slice(&bytes).context("File project.json bị lỗi định dạng")?;
    ensure!(p.version == 1 && p.id == project_id, "File dự án sai phiên bản hoặc không khớp mã dự án");
    p.root = std::path::absolute(path)?.parent().context("Thiếu thư mục dự án")?.to_owned();
    p.rules.migrate_prompt();
    if let Some(c) = &p.composition { c.validate()?; }
    p.validate(true)?;
    p.ensure_composition();
    p.check_duration()?;
    crate::layer_media::validate_assets(&p)?;
    Ok(p)
}
/// Build a read-only review of valid projects, missing files and existing outputs.
pub fn preview_export(data: &Path, id: &str, item_id: Option<&str>, output: &Path) -> Result<ExportPlan> {
    ensure!(output.is_absolute() && output.is_dir(), "Thư mục xuất MP4 không khả dụng. Chọn lại trong Nơi lưu / metadata.");
    let b = Batch::load(&root(data, id)?.join("batch.json"))?;
    ensure!(!matches!(b.status.as_str(), "running" | "pausing"), "Phiên đang chạy; tạm dừng trước khi xuất MP4.");
    if let Some(id) = item_id {
        ensure!(b.items.iter().any(|i| i.id == id && i.can_export()), "Dự án chưa sẵn sàng xuất. Hoàn thành xử lý và kiểm tra timeline trước.");
    }
    let mut plan = ExportPlan { batch_id: id.into(), item_id: item_id.map(str::to_owned), folder: output.to_owned(), targets: vec![], skipped: vec![] };
    for item in b.items.iter().filter(|i|item_id.is_none_or(|id| i.id == id)) {
        let checked = (|| -> Result<ExportTarget> {
            ensure!(item.can_export(), "Chưa có timeline sẵn sàng xuất");
            export_project(&project_path(data, item)?, &item.project_id)?;
            let output = output.join(format!("AIR3view-{}", item.project_id)).join("final.mp4");
            let existing = ExportFile::read(&output)?;
            Ok(ExportTarget { item_id: item.id.clone(), title: item.title.clone(), output, existing })
        })();
        match checked {
            Ok(t) => plan.targets.push(t),
            Err(e) => plan.skipped.push(format!("{}: {e:#}", item.title)),
        }
    }
    ensure!(!plan.targets.is_empty(), "Không có dự án hợp lệ để xuất MP4.\n{}", plan.skipped.join("\n\n"));
    Ok(plan)
}
pub fn request_export(data: &Path, id: &str, item_id: Option<&str>, output: &Path) -> Result<Batch> {
    let plan = preview_export(data, id, item_id, output)?;
    queue_export(data, &plan, false)
}
/// Confirmation is tied to the inspected file versions and checked again by the worker.
pub fn queue_export(data: &Path, plan: &ExportPlan, overwrite: bool) -> Result<Batch> {
    let _global = Lease::acquire(&data.join("runtime/batch.lock"))?;
    let _queue = Lease::acquire(&root(data, &plan.batch_id)?.join("queue.lock"))?;
    let current = preview_export(data, &plan.batch_id, plan.item_id.as_deref(), &plan.folder)?;
    ensure!(&current == plan, "Dữ liệu dự án hoặc MP4 đích đã thay đổi. Bấm Xuất để kiểm tra và xác nhận lại.");
    ensure!(overwrite || plan.existing_count() == 0, "Đã có {} file MP4 đích. Cần xác nhận xuất lại trước khi tiếp tục.", plan.existing_count());
    let mut b = Batch::load(&root(data, &plan.batch_id)?.join("batch.json"))?;
    let ids: Vec<_> = plan.targets.iter().map(|t|t.item_id.clone()).collect();
    // Do not discard a stopped export when a different subset is requested.
    for i in &mut b.items {
        if i.export_only && matches!(i.state, State::Pending | State::Interrupted | State::Cancelled) && !ids.contains(&i.id) {
            i.state = State::Ready;
            i.stage = "ready".into();
            i.message = "Chưa xuất MP4; timeline được giữ lại.".into();
        }
        if ids.contains(&i.id) {
            i.export_only = true;
            i.force_export = true;
            i.export_existing = plan.targets.iter().find(|t|t.item_id == i.id).and_then(|t|t.existing.clone());
            i.state = State::Pending;
            i.stage = "Export".into();
            i.message = "Chờ xuất MP4 từ timeline đã lưu".into();
            i.auto_retries = 0;
            i.retry_at = 0;
        }
    }
    b.run_items = Some(ids);
    b.output_folder = Some(plan.folder.clone());
    b.status = "queued".into();
    b.save(data)?;
    Ok(b)
}
pub fn recover(data: &Path, batch: &mut Batch) -> Result<()> {
    if running(data) {
        return Ok(());
    }
    if matches!(batch.status.as_str(), "running" | "pausing") {
        batch.status = "interrupted".into();
        for item in &mut batch.items {
            if item.state == State::Running {
                item.state = State::Interrupted;
                item.message = "Worker bị gián đoạn; có thể tiếp tục từ checkpoint".into();
            }
        }
        batch.save(data)?;
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Progress {
    pub batch_id: String,
    pub item_id: String,
    pub project_id: String,
    pub attempt_id: String,
    pub stage: String,
    pub message: String,
}
impl Progress {
    fn belongs_to(&self, batch_id: &str, item: &Item) -> bool {
        self.batch_id == batch_id
            && self.item_id == item.id
            && self.project_id == item.project_id
            && self.attempt_id == item.attempt_id
    }
}
fn progress(item: &Item, batch_id: &str, stage: &str, message: &str) -> Progress {
    Progress {
        batch_id: batch_id.into(),
        item_id: item.id.clone(),
        project_id: item.project_id.clone(),
        attempt_id: item.attempt_id.clone(),
        stage: stage.into(),
        message: diagnostics::safe_text(message, &[]),
    }
}
#[derive(Serialize, Deserialize)]
struct Outcome {
    ok: bool,
    cancelled: bool,
    error: String,
    output: Option<PathBuf>,
    title: String,
}
fn receipt_key(p: &Project) -> String {
    media::hash(
        serde_json::to_vec(&(
            &p.segments,
            &p.composition,
            &p.output,
            &p.subtitles,
            &p.evidence_settings,
            &p.card_sound,
            &p.export_metadata,
            &p.music,&p.effects,
            crate::visual_effects::fingerprint(&p.music.file),
            p.effects.layers.values().chain(std::iter::once(&p.effects.footage)).map(|e|crate::visual_effects::fingerprint(Path::new(&e.lut))).collect::<Vec<_>>(),
        ))
        .unwrap_or_default(),
    )
}
pub fn run_item(data: &Path, id: &str, item_id: &str) -> Result<()> {
    let batch = Batch::load(&root(data, id)?.join("batch.json"))?;
    let item = batch
        .items
        .iter()
        .find(|i| i.id == item_id)
        .context("Không tìm thấy mục hàng đợi")?
        .clone();
    ensure!(
        item.state == State::Running,
        "Mục chưa được bộ điều phối nhận"
    );
    let folder = root(data, id)?.join("attempts").join(&item.attempt_id);
    fs::create_dir_all(&folder)?;
    let path = project_path(data, &item)?;
    let cancel = process::cancel_token();
    let finished = process::cancel_token();
    let watcher = {
        let cancel = cancel.clone();
        let done = finished.clone();
        let marker = folder.join("cancel");
        thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                if marker.is_file() {
                    cancel.store(true, Ordering::Relaxed);
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
        })
    };
    let (tx, rx) = mpsc::channel();
    let report = {
        let item = item.clone();
        let id = id.to_owned();
        thread::spawn(move || {
            let mut out = std::io::stdout().lock();
            while let Ok(e) = rx.recv() {
                let fields = match e {
                    pipeline::Event::Stage { lane, message, .. } => Some((lane, message)),
                    pipeline::Event::Workflow(job) => {
                        Some((format!("{:?}", job.phase), job.message))
                    }
                    _ => None,
                };
                if let Some((stage, message)) = fields {
                    let event = progress(&item, &id, &stage, &message);
                    let _ = writeln!(out, "{}", serde_json::to_string(&event).unwrap_or_default());
                    let _ = out.flush();
                }
            }
        })
    };
    let mut title = item.title.clone();
    let result = (|| -> Result<Option<PathBuf>> {
        // Check before taking a lease: Lease::acquire creates its parent folder.
        ensure!(path.is_file(), "Không tìm thấy dữ liệu dự án: {}. Khôi phục thư mục dự án hoặc tạo lại từ URL; MP4 cũ vẫn được giữ.", path.display());
        let _lease = Lease::acquire(&path.parent().unwrap().join("batch-owner.lock"))?;
        let mut settings = Settings::load(&root(data, id)?.join("settings.json"))?;
        // Authentication and cookie fixes can be applied without changing production choices.
        if let Ok(live) = Settings::load(&data.join("settings.json")) {
            settings.youtube_cookie_path = live.youtube_cookie_path;
            settings.youtube_download_folder = live.youtube_download_folder;
        }
        let mut p = if item.export_only { export_project(&path, &item.project_id)? } else {
            Project::load(&path).with_context(||format!("Không mở được dự án: {}", path.display()))?
        };
        ensure_capacity(&p.root)?;
        let job = workflow::JobState::load(&p.root);
        if item.export_only {
            p.validate(true).context("Không thể xuất timeline đã lưu")?;
            p.check_duration()?;
        } else if job
            .as_ref()
            .is_none_or(|j| j.status != workflow::Status::Complete)
        {
            p = workflow::run(
                p,
                settings.clone(),
                tx.clone(),
                cancel.clone(),
                job.is_some(),
            )?;
        }
        title = p.title.clone();
        if !batch.auto_export && !item.export_only {
            return Ok(None);
        }
        let base = batch.output_folder.as_ref().context("Thiếu nơi lưu MP4")?;
        ensure!(
            base.is_dir(),
            "Ổ đĩa/thư mục xuất không khả dụng: {}",
            base.display()
        );
        ensure_capacity(base)?;
        let outdir = base.join(format!("AIR3view-{}", p.id));
        fs::create_dir_all(&outdir)?;
        let output = outdir.join("final.mp4");
        if item.force_export {
            ensure!(ExportFile::read(&output)? == item.export_existing, "MP4 đích đã xuất hiện hoặc thay đổi sau khi kiểm tra: {}. Bấm Xuất và xác nhận xuất lại.", output.display());
        }
        let receipt = outdir.join("batch-export.json");
        let key = receipt_key(&p);
        if !item.force_export && output.is_file()
            && fs::read(&receipt)
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                .is_some_and(|v| v["key"] == key)
            && media::probe(&settings, &output, &cancel)
                .ok()
                .is_some_and(|info| {
                    info.width == p.output.dimensions().0
                        && info.height == p.output.dimensions().1
                        && info.has_audio
                        && info.duration_ms.abs_diff(p.composition_duration()) < 250
                })
        {
            p.rendered = Some(output.clone());
            p.save()?;
            return Ok(Some(output));
        }
        let pending = outdir.join("final.partial.mp4");
        let effective = workflow::effective_settings(&p, &settings);
        let tx_export = tx.clone();
        media::render(&p, &effective, &pending, &cancel, |i, n| {
            let _ = tx_export.send(pipeline::Event::Stage {
                lane: "Export".into(),
                message: format!("Xuất MP4 {i}/{n}"),
                done: i == n,
            });
        })?;
        let info = media::probe(&effective, &pending, &cancel)?;
        ensure!(
            info.width == p.output.dimensions().0
                && info.height == p.output.dimensions().1
                && info.has_audio
                && info.duration_ms.abs_diff(p.composition_duration()) < 250,
            "MP4 chưa đạt kiểm tra hình/tiếng/thời lượng"
        );
        process::check_cancel(&cancel)?;
        if item.force_export {
            ensure!(ExportFile::read(&output)? == item.export_existing, "MP4 đích đã thay đổi khi đang xuất: {}. Bản mới nằm ở final.partial.mp4; bấm Xuất để xác nhận lại.", output.display());
        }
        if output.exists() {
            let backup = outdir.join(format!("final.previous-{}-{}.mp4", now(), uuid::Uuid::new_v4()));
            fs::rename(&output, &backup).with_context(||format!("Không thay được MP4 cũ: {}. Đóng trình phát video đang mở file rồi thử lại.", output.display()))?;
            if let Err(e) = fs::rename(&pending, &output) {
                let _ = fs::rename(&backup, &output);
                return Err(e).context("Không lưu được MP4 mới; đã thử khôi phục MP4 cũ");
            }
        } else {
            fs::rename(&pending, &output)?;
        }
        write_json(&receipt, &serde_json::json!({"key":key,"created_at":now()}))?;
        p.rendered = Some(output.clone());
        p.save()?;
        Ok(Some(output))
    })();
    drop(tx);
    let _ = report.join();
    finished.store(true, Ordering::Relaxed);
    let _ = watcher.join();
    let outcome = match result {
        Ok(output) => Outcome {
            ok: true,
            cancelled: false,
            error: String::new(),
            output,
            title,
        },
        Err(e) => Outcome {
            ok: false,
            cancelled: cancel.load(Ordering::Relaxed),
            error: diagnostics::failure_text(&format!("{e:#}")),
            output: None,
            title,
        },
    };
    write_json(&folder.join("outcome.json"), &outcome)?;
    Ok(())
}
fn global_error(text: &str) -> bool {
    if diagnostics::disk_full(text) {return true;}
    let text = text.to_lowercase();
    [
        "http 401",
        "status 401",
        "unauthorized",
        "invalid api key",
        "chưa cấu hình",
        "đăng nhập",
        "no space left",
        "disk full",
        "not enough space",
        "không đủ dung lượng",
        "ổ đĩa/thư mục",
        "thư mục tải video nguồn",
        "thư mục video nguồn",
        "cuda out of memory",
        "cublas",
        "cudnn",
    ]
    .iter()
    .any(|s| text.contains(s))
}
fn transient(text: &str) -> bool {
    let text = text.to_lowercase();
    [
        "http 429",
        "http 502",
        "http 503",
        "http 504",
        "timed out",
        "timeout",
        "connection reset",
        "connection refused",
        "temporarily unavailable",
    ]
    .iter()
    .any(|s| text.contains(s))
        && !global_error(&text)
}
pub fn run(data: &Path, id: &str, cli: &Path) -> Result<()> {
    let _global = Lease::acquire(&data.join("runtime/batch.lock"))?;
    let _queue = Lease::acquire(&root(data, id)?.join("queue.lock"))?;
    let mut batch = Batch::load(&root(data, id)?.join("batch.json"))?;
    let scope = batch.run_items.clone();
    for item in &mut batch.items {
        if scope.as_ref().is_none_or(|ids| ids.contains(&item.id)) && matches!(
            item.state,
            State::Running | State::Interrupted | State::Cancelled
        ) {
            item.state = State::Pending;
        }
    }
    batch.status = "running".into();
    batch.save(data)?;
    let events = root(data, id)?.join("events.jsonl");
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(events)?;
    loop {
        commands(data, &mut batch)?;
        if batch.status == "pausing" {
            batch.status = "paused".into();
            break;
        }
        let Some(index) = batch
            .items
            .iter()
            .position(|i| batch.scheduled(i) && i.state == State::Pending && i.retry_at <= now())
        else {
            if batch.items.iter().any(|i| batch.scheduled(i) && i.state == State::Pending) {
                thread::sleep(Duration::from_millis(500));
                continue;
            }
            batch.status = if batch.items.iter().any(|i| i.state == State::Pending) {
                "paused"
            } else if batch.items.iter().any(|i| i.state.retryable()) {
                "complete_with_errors"
            } else {
                "complete"
            }
            .into();
            break;
        };
        let item = &mut batch.items[index];
        item.state = State::Running;
        item.attempt_id = uuid::Uuid::new_v4().to_string();
        item.attempts += 1;
        item.started_at = Some(now());
        item.finished_at = None;
        item.message = "Đang khởi chạy worker".into();
        let item = item.clone();
        batch.save(data)?;
        let attempt = root(data, id)?.join("attempts").join(&item.attempt_id);
        fs::create_dir_all(&attempt)?;
        let stderr = fs::File::create(attempt.join("worker.stderr.log"))?;
        let child = process::command(&cli.to_string_lossy())
            .args(["batch-item", id, &item.id])
            .env("REACTION_DATA_DIR", data)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(stderr)
            .spawn();
        let outcome = match child {
            Ok(mut child) => {
                let job = match KillJob::attach(&child) {
                    Ok(job) => job,
                    Err(e) => {
                        process::stop(&mut child);
                        return Err(e);
                    }
                };
                let stdout = child.stdout.take().context("Thiếu worker stdout")?;
                let (tx, rx) = mpsc::channel();
                let reader = thread::spawn(move || {
                    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                        if line.len() < 32768 {
                            if let Ok(e) = serde_json::from_str::<Progress>(&line) {
                                let _ = tx.send(e);
                            }
                        }
                    }
                });
                let mut stopping = None;
                loop {
                    while let Ok(event) = rx.try_recv() {
                        if event.belongs_to(id, &item) {
                            if let Some(current) = batch.items.iter_mut().find(|i| i.id == item.id)
                            {
                                current.stage = event.stage.clone();
                                current.message = event.message.clone();
                            }
                            writeln!(log, "{}", serde_json::to_string(&event)?)?;
                            batch.save(data)?;
                        }
                    }
                    if commands(data, &mut batch)? && stopping.is_none() {
                        fs::write(attempt.join("cancel"), b"cancel")?;
                        stopping = Some(std::time::Instant::now());
                    }
                    if stopping.is_some_and(|s| s.elapsed() > Duration::from_secs(15)) {
                        process::stop(&mut child);
                        break;
                    }
                    if child.try_wait()?.is_some() {
                        break;
                    }
                    thread::sleep(Duration::from_millis(250));
                }
                drop(job);
                let _ = reader.join();
                fs::read(attempt.join("outcome.json"))
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Outcome>(&b).ok())
                    .unwrap_or_else(|| Outcome {
                        ok: false,
                        cancelled: stopping.is_some(),
                        error: "Worker bị gián đoạn. Xem nhật ký và tiếp tục từ checkpoint.".into(),
                        output: None,
                        title: item.title.clone(),
                    })
            }
            Err(e) => Outcome {
                ok: false,
                cancelled: false,
                error: format!("Không mở được worker: {e}"),
                output: None,
                title: item.title.clone(),
            },
        };
        let current = batch
            .items
            .iter_mut()
            .find(|i| i.id == item.id)
            .context("Mục đang chạy bị mất")?;
        current.title = outcome.title;
        current.finished_at = Some(now());
        if outcome.ok {
            current.state = if outcome.output.is_some() {
                State::Exported
            } else {
                State::Ready
            };
            current.output = outcome.output;
            current.stage = "done".into();
            current.message = "Hoàn thành".into();
        } else {
            current.message = outcome.error.clone();
            current.state = if outcome.cancelled {
                State::Cancelled
            } else {
                State::Failed
            };
            if !outcome.cancelled && transient(&outcome.error) && current.auto_retries < 1 {
                current.state = State::Pending;
                current.auto_retries += 1;
                current.retry_at = now() + 30;
                current.message = format!("Chờ thử lại một lần: {}", outcome.error);
            }
            if global_error(&outcome.error) {
                batch.status = "blocked".into();
                batch.save(data)?;
                break;
            }
        }
        batch.save(data)?;
    }
    batch.save(data)
}
pub fn cli_path() -> Result<PathBuf> {
    let parent = std::env::current_exe()?
        .parent()
        .context("Thiếu thư mục app")?
        .to_path_buf();
    let path = parent.join("studio-cli.exe");
    ensure!(
        path.is_file(),
        "Thiếu studio-cli.exe trong thư mục AIR3view"
    );
    Ok(path)
}
pub fn start(data: &Path, id: &str) -> Result<(Child, KillJob)> {
    ensure!(!running(data), "Một hàng đợi khác đang chạy");
    let cli = cli_path()?;
    let mut child = process::command(&cli.to_string_lossy())
        .args(["batch-run", id])
        .env("REACTION_DATA_DIR", data)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Không mở được bộ điều phối hàng loạt")?;
    match KillJob::attach(&child) {
        Ok(job) => Ok((child, job)),
        Err(e) => {
            process::stop(&mut child);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const URLS:&str="https://youtu.be/dBy-8s6ly08?si=1\nhttps://www.youtube.com/watch?v=r1cvbJJ5G54\nhttps://youtu.be/dNYsRcfWeLU\nhttps://www.youtube.com/watch?v=dBy-8s6ly08&t=4";
    fn setup() -> (tempfile::TempDir, Project, Settings) {
        let temp = tempfile::tempdir().unwrap();
        let p = Project {
            production: Some(ProductionSettings {
                avatar_enabled: false,
                ..Default::default()
            }),
            ..Default::default()
        };
        let s = Settings {
            provider: "antigravity".into(),
            avatar_enabled: false,
            ..Default::default()
        };
        (temp, p, s)
    }
    #[test]
    fn batch_creation_isolates_sources_snapshots_and_does_not_inherit_timeline() {
        let (temp, mut p, mut s) = setup();
        let source_base=temp.path().join("YouTube originals");fs::create_dir(&source_base).unwrap();
        s.youtube_download_folder=source_base.to_string_lossy().into_owned();
        p.rules.min_seconds = 100;
        p.rules.max_seconds = 240;
        p.rules.language = "en".into();
        p.segments.push(Segment::default());
        p.rules.excluded_ranges.push(TimeRange {
            source_id: "old".into(),
            start_ms: 1,
            end_ms: 100,
        });
        p.rendered = Some("old.mp4".into());
        let photo = temp.path().join("original.png");
        fs::write(&photo, b"reference-photo-fixture").unwrap();
        let prod = p.production.as_mut().unwrap();
        prod.avatar_enabled = true;
        prod.avatar_provider = "cartoon2d".into();
        prod.avatar_image_path = photo.to_string_lossy().into_owned();
        let b = create(
            temp.path(),
            "Three isolated sources",
            URLS,
            &p,
            &s,
            false,
            None,
        )
        .unwrap();
        assert_eq!(b.items.len(), 3);
        let snapshot=Settings::load(&root(temp.path(),&b.id).unwrap().join("settings.json")).unwrap();
        assert_eq!(snapshot.youtube_download_folder,s.youtube_download_folder);
        assert_eq!(parse_urls(URLS).duplicates, 1);
        assert_eq!(
            p.segments.len(),
            1,
            "Creating a batch must not mutate its source project"
        );
        fs::remove_file(photo).unwrap();
        let mut ids = std::collections::HashSet::new();
        for item in &b.items {
            let mut child = Project::load(&project_path(temp.path(), item).unwrap()).unwrap();
            assert!(ids.insert(child.id.clone()));
            assert_eq!(crate::download_location::folder(&child,&snapshot).unwrap(),source_base.join(format!("AIR3view-{}",child.id)).join("sources"));
            assert_eq!(child.sources.len(), 1);
            assert_eq!(child.sources[0].url, item.url);
            assert!(
                child.segments.is_empty()
                    && child.rendered.is_none()
                    && child.composition.is_none()
            );
            assert!(child.rules.excluded_ranges.is_empty());
            assert_eq!(child.rules.max_seconds, 240);
            assert_eq!(child.rules.language, "en");
            assert!(
                Path::new(&child.production.as_ref().unwrap().avatar_image_path).is_file(),
                "Removing the original portrait must not break queued projects"
            );
            child.rules.language = "vi".into();
            child.save().unwrap();
        }
        assert_eq!(p.rules.language, "en");
        assert_eq!(
            Batch::load(&root(temp.path(), &b.id).unwrap().join("batch.json"))
                .unwrap()
                .items
                .len(),
            3
        );
        assert!(create(
            temp.path(),
            "bad",
            "https://example.com/video",
            &p,
            &s,
            false,
            None
        )
        .is_err());
    }
    #[test]
    fn queue_commands_reorder_skip_retry_and_recover_without_resetting_completed_items() {
        let (temp, p, s) = setup();
        let mut b = create(temp.path(), "controls", URLS, &p, &s, false, None).unwrap();
        let third = b.items[2].id.clone();
        action(temp.path(), &b.id, Action::Up(third.clone())).unwrap();
        b = Batch::load(&root(temp.path(), &b.id).unwrap().join("batch.json")).unwrap();
        assert_eq!(b.items[1].id, third);
        b.items[0].state = State::Exported;
        b.items[1].state = State::Running;
        b.items[2].state = State::Failed;
        b.status = "running".into();
        b.save(temp.path()).unwrap();
        recover(temp.path(), &mut b).unwrap();
        assert_eq!(b.status, "interrupted");
        assert_eq!(b.items[0].state, State::Exported);
        assert_eq!(b.items[1].state, State::Interrupted);
        apply_action(&mut b, Action::RetryAll);
        assert_eq!(b.items[0].state, State::Exported);
        assert!(b.items[1..].iter().all(|i| i.state == State::Pending));
        let id = b.items[2].id.clone();
        apply_action(&mut b, Action::Skip(id));
        assert_eq!(b.items[2].state, State::Skipped);
        #[cfg(windows)]
        {
            let _lease =
                Lease::acquire(&root(temp.path(), &b.id).unwrap().join("queue.lock")).unwrap();
            action(temp.path(), &b.id, Action::Pause).unwrap();
            assert!(
                root(temp.path(), &b.id)
                    .unwrap()
                    .join("commands")
                    .read_dir()
                    .unwrap()
                    .next()
                    .is_some(),
                "UI must send a command, not overwrite the supervisor's manifest"
            );
            commands(temp.path(), &mut b).unwrap();
            assert_eq!(b.status, "pausing");
        }
    }
    #[test]
    fn failure_policy_distinguishes_bad_links_from_global_and_transient_errors() {
        assert!(!global_error("YouTube HTTP 403 age restricted"));
        assert!(global_error("API HTTP 401 unauthorized"));
        assert!(global_error("Không đủ dung lượng trống"));
        assert!(global_error("There is not enough space on the disk. (os error 112)"));
        assert!(!transient("No space left on device; timeout"));
        assert!(transient("HTTP 429 temporarily unavailable"));
        assert!(!transient("CUDA out of memory timeout"));
        assert!(!global_error(
            "Source timestamp 401.25 seconds not available"
        ));
        assert!(!transient("Invalid timestamp 503.2"));
    }
    #[test]
    fn progress_rejects_other_projects_and_stale_attempts() {
        let (temp, p, s) = setup();
        let b = create(temp.path(), "identity", URLS, &p, &s, false, None).unwrap();
        let mut item = b.items[0].clone();
        item.attempt_id = uuid::Uuid::new_v4().to_string();
        let good = progress(&item, &b.id, "Assets", "safe");
        assert!(good.belongs_to(&b.id, &item));
        for field in ["batch", "item", "project", "attempt"] {
            let mut wrong = good.clone();
            match field {
                "batch" => wrong.batch_id = "other".into(),
                "item" => wrong.item_id = b.items[1].id.clone(),
                "project" => wrong.project_id = b.items[1].project_id.clone(),
                _ => wrong.attempt_id = "previous-attempt".into(),
            }
            assert!(!wrong.belongs_to(&b.id, &item));
        }
    }
    #[cfg(windows)]
    #[test]
    fn windows_job_closes_worker_and_descendants_after_owner_exit() {
        let temp = tempfile::tempdir().unwrap();
        let pid_path = temp.path().join("child.pid");
        let script = format!("$taskChild=Start-Process powershell.exe -ArgumentList @('-NoProfile','-Command','Start-Sleep -Seconds 120') -WindowStyle Hidden -PassThru; Set-Content -LiteralPath '{}' -Value $taskChild.Id; Start-Sleep -Seconds 120",pid_path.display());
        let mut parent = process::command("powershell.exe")
            .args(["-NoProfile", "-Command", &script])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let job = KillJob::attach(&parent).unwrap();
        let start = std::time::Instant::now();
        while !pid_path.is_file() && start.elapsed() < Duration::from_secs(10) {
            thread::sleep(Duration::from_millis(50));
        }
        let pid = fs::read_to_string(pid_path)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        drop(job);
        parent.wait().unwrap();
        let result = process::command("powershell.exe").args(["-NoProfile", "-Command", &format!("if(Get-Process -Id {pid} -ErrorAction SilentlyContinue){{exit 1}}else{{exit 0}}")]).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap();
        assert!(
            result.success(),
            "A descendant survived its owner's Job Object"
        );
    }
    #[test]
    fn export_only_resume_reuses_a_verified_file_without_calling_ai_or_tts() {
        let (temp, p, s) = setup();
        let out = temp.path().join("output");
        fs::create_dir_all(&out).unwrap();
        let mut b = create(
            temp.path(),
            "export",
            URLS.lines().next().unwrap(),
            &p,
            &s,
            false,
            Some(out),
        )
        .unwrap();
        let item = &mut b.items[0];
        item.state = State::Running;
        item.export_only = true;
        item.attempt_id = uuid::Uuid::new_v4().to_string();
        let item = item.clone();
        b.save(temp.path()).unwrap();
        let mut p = Project::load(&project_path(temp.path(), &item).unwrap()).unwrap();
        p.rules.min_seconds = 1;
        p.rules.max_seconds = 10;
        p.rules.hook_enabled = false;
        p.rules.intercuts_enabled = false;
        let input = p.root.join("source.mp4");
        let cancel = process::cancel_token();
        process::run(
            &s.ffmpeg,
            &process::args(&[
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=blue:s=320x180:r=24:d=2",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000:duration=2",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-c:a",
                "aac",
                "-shortest",
                &input.to_string_lossy(),
            ]),
            None,
            &cancel,
            Duration::from_secs(30),
            None,
        )
        .unwrap();
        p.sources[0].path = input;
        p.sources[0].duration_ms = 2000;
        p.sources[0].width = 320;
        p.sources[0].height = 180;
        p.sources[0].has_audio = true;
        p.rules.min_seconds = 2;
        p.rules.max_seconds = 2;
        p.rules.hook_enabled = false;
        p.rules.intercuts_enabled = false;
        p.evidence_settings.enabled = false;
        p.subtitles.enabled = false;
        p.output.resolution = 360;
        p.output.fps = 24;
        p.segments = vec![Segment {
            source_id: "src_1".into(),
            out_ms: 2000,
            duration_ms: 2000,
            ..Default::default()
        }];
        p.ensure_composition();
        p.save().unwrap();
        assert!(!p.root.join("workflow.json").exists());
        run_item(temp.path(), &b.id, &item.id).unwrap();
        let folder = root(temp.path(), &b.id)
            .unwrap()
            .join("attempts")
            .join(&item.attempt_id);
        let outcome: Outcome =
            serde_json::from_slice(&fs::read(folder.join("outcome.json")).unwrap()).unwrap();
        assert!(outcome.ok, "{}", outcome.error);
        let output = outcome.output.unwrap();
        let before = fs::metadata(&output).unwrap().modified().unwrap();
        run_item(temp.path(), &b.id, &item.id).unwrap();
        assert_eq!(
            before,
            fs::metadata(&output).unwrap().modified().unwrap(),
            "A resumed verified export must not be overwritten"
        );
        assert!(media::probe(&s, &output, &cancel).unwrap().has_audio);
        assert!(!p.root.join("workflow.json").exists(), "Manual export must never start analysis or TTS, even without an automatic workflow checkpoint");
        b.items[0].force_export = true;
        b.items[0].export_existing = ExportFile::read(&output).unwrap();
        b.save(temp.path()).unwrap();
        run_item(temp.path(), &b.id, &item.id).unwrap();
        let forced: Outcome = serde_json::from_slice(&fs::read(folder.join("outcome.json")).unwrap()).unwrap();
        assert!(forced.ok, "{}", forced.error);
        assert_ne!(before, fs::metadata(&output).unwrap().modified().unwrap());
        assert!(fs::read_dir(output.parent().unwrap()).unwrap().flatten().any(|e|e.file_name().to_string_lossy().starts_with("final.previous-")));
    }
    fn export_fixture(data: &Path, item: &Item) {
        let path = project_path(data, item).unwrap();
        let mut p = Project::load(&path).unwrap();
        let source = p.root.join("source.mp4");
        fs::write(&source, b"preflight fixture (not decoded)").unwrap();
        p.sources[0].path = source;
        p.sources[0].duration_ms = 2000;
        p.rules.min_seconds = 1; p.rules.max_seconds = 3;
        p.rules.hook_enabled = false; p.rules.intercuts_enabled = false;
        p.evidence_settings.enabled = false;
        p.segments = vec![Segment { source_id: "src_1".into(), out_ms: 2000, duration_ms: 2000, ..Default::default() }];
        p.ensure_composition(); p.save().unwrap();
    }
    #[test]
    fn export_preflight_skips_missing_projects_and_requires_fresh_output_confirmation() {
        let (temp, p, s) = setup();
        let mut b = create(temp.path(), "preflight", URLS, &p, &s, false, None).unwrap();
        for item in &mut b.items { item.state = State::Ready; }
        export_fixture(temp.path(), &b.items[0]);
        // Missing project.json and missing referenced media must both be explained.
        let missing = project_path(temp.path(), &b.items[1]).unwrap();
        fs::remove_dir_all(missing.parent().unwrap()).unwrap();
        export_fixture(temp.path(), &b.items[2]);
        fs::remove_file(project_path(temp.path(), &b.items[2]).unwrap().parent().unwrap().join("source.mp4")).unwrap();
        b.save(temp.path()).unwrap();
        let plan = preview_export(temp.path(), &b.id, None, temp.path()).unwrap();
        assert_eq!(plan.targets.len(), 1); assert_eq!(plan.skipped.len(), 2);
        assert!(plan.skipped[0].contains("dữ liệu dự án"));
        assert!(plan.skipped[1].contains("Thiếu video nguồn"));
        assert!(!missing.parent().unwrap().exists());
        let output = &plan.targets[0].output;
        fs::create_dir_all(output.parent().unwrap()).unwrap(); fs::write(output, b"original MP4").unwrap();
        // A file appearing after preview invalidates the review, even with overwrite permission.
        assert!(queue_export(temp.path(), &plan, true).unwrap_err().to_string().contains("đã thay đổi"));
        let plan = preview_export(temp.path(), &b.id, None, temp.path()).unwrap();
        let unchanged = fs::read(root(temp.path(), &b.id).unwrap().join("batch.json")).unwrap();
        assert!(queue_export(temp.path(), &plan, false).unwrap_err().to_string().contains("xác nhận"));
        assert_eq!(fs::read(root(temp.path(), &b.id).unwrap().join("batch.json")).unwrap(), unchanged);
        assert_eq!(fs::read(output).unwrap(), b"original MP4");
        let queued = queue_export(temp.path(), &plan, true).unwrap();
        assert_eq!(queued.run_items, Some(vec![b.items[0].id.clone()]));
        assert!(queued.items[0].force_export && queued.items[0].export_existing.is_some());
        // With every valid project gone, don't queue work or recreate its directory.
        let valid = project_path(temp.path(), &b.items[0]).unwrap();
        fs::remove_dir_all(valid.parent().unwrap()).unwrap();
        assert!(preview_export(temp.path(), &b.id, None, temp.path()).unwrap_err().to_string().contains("Không có dự án hợp lệ"));
        assert!(!valid.parent().unwrap().exists());
        assert_eq!(fs::read(output).unwrap(), b"original MP4");
        // Defensive worker path also records a readable outcome, without making a ghost project.
        let mut queued = queued; queued.items[0].state = State::Running;
        queued.items[0].attempt_id = uuid::Uuid::new_v4().to_string(); queued.save(temp.path()).unwrap();
        run_item(temp.path(), &queued.id, &queued.items[0].id).unwrap();
        let outcome: Outcome = serde_json::from_slice(&fs::read(root(temp.path(), &queued.id).unwrap().join("attempts").join(&queued.items[0].attempt_id).join("outcome.json")).unwrap()).unwrap();
        assert!(!outcome.ok && outcome.error.contains("Không tìm thấy dữ liệu dự án"));
        assert!(!valid.parent().unwrap().exists());
    }
    #[test]
    fn manual_exports_scope_single_and_all_without_running_unready_urls_or_changing_projects() {
        let (temp, p, s) = setup();
        let mut b = create(temp.path(), "exports", URLS, &p, &s, false, None).unwrap();
        b.items[0].state = State::Ready;
        b.items[1].state = State::Exported;
        b.items[1].output = Some(temp.path().join("existing.mp4"));
        export_fixture(temp.path(), &b.items[0]);
        export_fixture(temp.path(), &b.items[1]);
        b.save(temp.path()).unwrap();
        let before: Vec<_> = b.items.iter().map(|i| fs::read(project_path(temp.path(),i).unwrap()).unwrap()).collect();
        let item = b.items[1].id.clone();
        let queued = request_export(temp.path(), &b.id, Some(&item), temp.path()).unwrap();
        assert_eq!(queued.run_items, Some(vec![item.clone()]));
        assert_eq!(queued.items[0].state, State::Ready);
        assert_eq!(queued.items[2].state, State::Pending);
        assert!(!queued.items[2].export_only && !queued.scheduled(&queued.items[2]));
        assert!(queued.items[1].export_only && queued.resumable());
        assert_eq!(queued.items[1].output, b.items[1].output);
        let all = request_export(temp.path(), &b.id, None, temp.path()).unwrap();
        assert_eq!(all.run_items.as_ref().unwrap().len(),2);
        assert!(!all.scheduled(&all.items[2]));
        for (i, bytes) in all.items.iter().zip(before) {assert_eq!(fs::read(project_path(temp.path(), i).unwrap()).unwrap(),bytes);}
        assert!(request_export(temp.path(), &b.id, Some(&all.items[2].id), temp.path()).is_err());
        assert!(request_export(temp.path(), &b.id, None, Path::new("missing-output")).is_err());
        let mut restarted = Batch::load(&root(temp.path(),&b.id).unwrap().join("batch.json")).unwrap();
        restarted.status = "running".into();
        assert!(restarted.reserved(&restarted.items[0].project_id));
        assert!(!restarted.reserved(&restarted.items[2].project_id));
        restarted.items[0].state = State::Failed;
        apply_action(&mut restarted, Action::RetryAll);
        assert!(restarted.run_items.is_none());
        assert!(restarted.items[0].export_only && !restarted.items[2].export_only);
        #[cfg(windows)] {
            let _lease = Lease::acquire(&temp.path().join("runtime/batch.lock")).unwrap();
            assert!(request_export(temp.path(), &b.id, None, temp.path()).is_err());
        }
        // Older manifests must load with their original workflow behaviour.
        let mut legacy = serde_json::to_value(&b).unwrap();
        legacy.as_object_mut().unwrap().remove("run_items");
        for i in legacy["items"].as_array_mut().unwrap() {i.as_object_mut().unwrap().remove("export_only");}
        let legacy: Batch = serde_json::from_value(legacy).unwrap();
        assert!(legacy.run_items.is_none() && legacy.items.iter().all(|i|!i.export_only));
    }
}
