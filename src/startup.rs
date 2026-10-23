//! Native launcher: all slow setup work runs off the UI thread. Only the worker
//! started by this session is stopped on exit; compatible external workers stay.
use crate::{model, process};
use anyhow::{bail, ensure, Context, Result};
use eframe::egui::{self, Color32, RichText};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Stdio},
    sync::{atomic::Ordering, mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

const MODEL: &str = "pnnbao-ump/VieNeu-TTS-v3-Turbo";

pub fn runtime_root() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let beside = exe.parent().context("Không tìm thấy thư mục AIR3view")?;
    if beside.join("workers/requirements.lock").is_file() {
        return Ok(beside.into());
    }
    // This fallback is for a source checkout, never a moved portable release.
    let source = Path::new(env!("CARGO_MANIFEST_DIR"));
    ensure!(beside.starts_with(source.join("target")) && source.join("Cargo.toml").is_file(),
        "Thiếu thư mục workers/tools. Hãy giải nén toàn bộ gói AIR3view, giữ EXE cùng các thư mục đi kèm.");
    Ok(source.into())
}

pub fn recreate_shortcuts() -> Result<()> {
    let root = runtime_root()?;
    let exe = std::env::current_exe()?;
    let (tx, _rx) = mpsc::channel();
    let log_dir = model::data_dir().join("logs"); fs::create_dir_all(&log_dir)?;
    let log = Arc::new(Mutex::new(File::create(log_dir.join("shortcuts.log"))?));
    run_script(&root, "create-shortcuts.ps1", &["-AppPath".into(), exe.to_string_lossy().into_owned()], &process::cancel_token(), &tx, &log)?;
    Ok(())
}

pub struct Session {
    pub python: PathBuf,
    pub speech_url: String,
    _worker: Option<OwnedWorker>,
    _lock: File,
}

struct OwnedWorker(Child);
impl Drop for OwnedWorker {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            process::stop(&mut self.0);
        }
    }
}

fn instance_lock(data: &Path) -> Result<File> {
    fs::create_dir_all(data.join("runtime"))?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    options.open(data.join("runtime/gui.lock"))
        .context("AIR3view đang mở trong một cửa sổ khác. Hãy dùng cửa sổ đó hoặc đóng app trước khi mở lại.")
}

#[derive(Deserialize)]
struct Health {
    status: String,
    model: String,
    #[serde(default)]
    protocol: u32,
    #[serde(default)]
    reference_audio: bool,
    #[serde(default)]
    languages: Vec<String>,
    #[serde(default)]
    worker_sha256: String,
    #[serde(default)]
    session: String,
}
impl Health {
    fn compatible(&self) -> bool {
        self.status == "ready" && self.model == MODEL && self.protocol == 3
            && self.reference_audio && ["vi", "en"].iter().all(|v| self.languages.iter().any(|s| s == v))
    }
}

enum Event {
    Progress(String),
    Finished(Result<Session, String>),
}
type Log = Arc<Mutex<File>>;
fn progress(tx: &mpsc::Sender<Event>, log: &Log, message: impl Into<String>) {
    let message = message.into();
    if let Ok(mut file) = log.lock() { let _ = writeln!(file, "{message}"); }
    let _ = tx.send(Event::Progress(message));
}

fn run_script(root: &Path, name: &str, extra: &[String], cancel: &process::Cancel,
              tx: &mpsc::Sender<Event>, log: &Log) -> Result<()> {
    let script = root.join("scripts").join(name);
    ensure!(script.is_file(), "Thiếu {}. Hãy giải nén toàn bộ gói AIR3view.", script.display());
    let mut command = process::command("powershell.exe");
    // PowerShell 7 / IDEs can pass a PSModulePath that hides Windows PowerShell
    // 5.1 built-in modules (including Get-FileHash). Let 5.1 initialize its own.
    command.env_remove("PSModulePath");
    command.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(script).args(extra).current_dir(root).stdin(Stdio::null())
        .stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = OwnedWorker(command.spawn().context("Không khởi chạy được bước chuẩn bị môi trường")?);
    let pipes: [Box<dyn std::io::Read + Send>; 2] = [Box::new(child.0.stdout.take().unwrap()), Box::new(child.0.stderr.take().unwrap())];
    let readers: Vec<_> = pipes
        .into_iter().map(|reader| {
            let tx = tx.clone(); let log = log.clone();
            thread::spawn(move || {
                for line in BufReader::new(reader).lines().map_while(Result::ok) {
                    if !line.trim().is_empty() { progress(&tx, &log, line); }
                }
            })
        }).collect();
    let started = Instant::now();
    let status = loop {
        process::check_cancel(cancel)?;
        if let Some(status) = child.0.try_wait()? { break status; }
        ensure!(started.elapsed() < Duration::from_secs(1800), "Chuẩn bị môi trường quá 30 phút. Kiểm tra mạng và dung lượng ổ đĩa rồi thử lại.");
        thread::sleep(Duration::from_millis(80));
    };
    for reader in readers { let _ = reader.join(); }
    let step = if name == "setup-workers.ps1" { "Chuẩn bị môi trường Python" } else { "Tạo icon AIR3view" };
    ensure!(status.success(), "{step} chưa hoàn tất. Xem chi tiết bên dưới hoặc mở thư mục nhật ký, khắc phục lỗi rồi bấm Thử lại.");
    Ok(())
}

fn local_speech_port(endpoint: &str) -> Result<Option<u16>> {
    let url = url::Url::parse(endpoint).context("Speech endpoint không hợp lệ trong Cài đặt kết nối")?;
    if !matches!(url.host_str(), Some("127.0.0.1" | "localhost")) { return Ok(None); }
    ensure!(url.scheme() == "http", "VieNeu local cần endpoint http://127.0.0.1:<cổng>/v1/audio/speech");
    Ok(Some(url.port_or_known_default().context("Speech endpoint thiếu cổng")?))
}

fn prepare(root: &Path, data: &Path, cancel: &process::Cancel, tx: &mpsc::Sender<Event>) -> Result<Session> {
    let lock = instance_lock(data)?;
    fs::create_dir_all(data.join("logs"))?;
    let log = Arc::new(Mutex::new(File::create(data.join("logs/startup.log"))?));
    progress(tx, &log, "Đang kiểm tra Python và thư viện xử lý giọng/video…");
    run_script(root, "setup-workers.ps1", &[], cancel, tx, &log)?;
    let python = root.join(".venv/Scripts/python.exe");
    ensure!(python.is_file(), "Chưa tạo được Python trong thư mục AIR3view");
    let settings = model::Settings::load(&data.join("settings.json")).unwrap_or_default();
    let mut session = Session { python, speech_url: settings.vieneu_url.clone(), _worker: None, _lock: lock };
    if let Some(port) = local_speech_port(&session.speech_url)? {
        let client = reqwest::blocking::Client::builder().no_proxy()
            .connect_timeout(Duration::from_millis(500)).timeout(Duration::from_secs(2)).build()?;
        let health_url = format!("http://127.0.0.1:{port}/health");
        if let Ok(response) = client.get(&health_url).send() {
            let health: Health = response.json().context("Cổng VieNeu đang được một dịch vụ khác sử dụng")?;
            ensure!(health.compatible(), "Cổng {port} có worker không tương thích. Đóng phiên VieNeu cũ hoặc đổi Speech endpoint trong Cài đặt kết nối.");
            // Protocol 3 already describes reference audio + vi/en. Legacy source
            // workers lack a source hash/PID; reuse compatible APIs, never kill them.
            progress(tx, &log, "Đã kết nối VieNeu đang chạy, hỗ trợ giọng tham chiếu và Việt/Anh.");
        } else {
            process::check_cancel(cancel)?;
            let probe = TcpListener::bind(("127.0.0.1", port))
                .with_context(|| format!("Cổng {port} đang bận nhưng VieNeu chưa trả lời. Chờ worker cũ khởi động xong rồi thử lại."))?;
            drop(probe);
            progress(tx, &log, "Đang khởi động dịch vụ giọng VieNeu… Model được nạp khi tạo giọng lần đầu.");
            let worker_script = root.join("workers/vieneu_server.py");
            let expected_hash = format!("{:x}", Sha256::digest(fs::read(&worker_script)?));
            let token = uuid::Uuid::new_v4().to_string();
            let stdout = File::create(data.join("logs/vieneu-out.log"))?;
            let stderr = File::create(data.join("logs/vieneu-err.log"))?;
            let child = process::command(&session.python.to_string_lossy()).arg(&worker_script)
                .current_dir(root).env("VIENEU_PORT", port.to_string()).env("AIR3VIEW_WORKER_SESSION", &token)
                .env("AIR3VIEW_WORKER_OWNER_PID", std::process::id().to_string())
                .env("AIR3VIEW_LAZY_VOICE", "1")
                .env("PYTHONUNBUFFERED", "1").stdin(Stdio::null()).stdout(stdout).stderr(stderr).spawn()
                .context("Không khởi động được VieNeu")?;
            session._worker = Some(OwnedWorker(child));
            let started = Instant::now();
            loop {
                process::check_cancel(cancel)?;
                if let Some(status) = session._worker.as_mut().unwrap().0.try_wait()? {
                    bail!("VieNeu dừng khi khởi động ({status}). Xem vieneu-err.log trong thư mục nhật ký.");
                }
                if let Ok(response) = client.get(&health_url).send() {
                    if let Ok(health) = response.json::<Health>() {
                        ensure!(health.session == token && health.worker_sha256.eq_ignore_ascii_case(&expected_hash),
                            "Cổng {port} vừa được worker khác sử dụng. Đóng cửa sổ AIR3view khác rồi thử lại.");
                        ensure!(health.compatible(), "VieNeu chưa hỗ trợ đầy đủ giọng tham chiếu và Việt/Anh");
                        break;
                    }
                }
                ensure!(started.elapsed() < Duration::from_secs(900), "VieNeu chưa sẵn sàng sau 15 phút. Kiểm tra mạng, dung lượng và vieneu-err.log rồi thử lại.");
                thread::sleep(Duration::from_millis(350));
            }
            progress(tx, &log, "VieNeu đã sẵn sàng. Worker này sẽ tự đóng khi thoát AIR3view.");
        }
    } else {
        progress(tx, &log, "Đã giữ cấu hình dịch vụ giọng từ xa của bạn.");
    }
    process::check_cancel(cancel)?;
    let exe = std::env::current_exe()?;
    let shortcut_marker = data.join("runtime/shortcut-target.txt");
    if fs::read_to_string(&shortcut_marker).ok().as_deref() != Some(exe.to_string_lossy().as_ref()) {
        progress(tx, &log, "Đang tạo icon AIR3view trên Desktop và Start Menu…");
        if let Err(error) = run_script(root, "create-shortcuts.ps1", &["-AppPath".into(), exe.to_string_lossy().into_owned()], cancel, tx, &log) {
            process::check_cancel(cancel)?;
            progress(tx, &log, format!("Chưa tạo được shortcut: {error:#}. Bạn vẫn có thể mở trực tiếp AIR3view.exe."));
        } else {
            fs::write(shortcut_marker, exe.to_string_lossy().as_bytes())?;
        }
    }
    progress(tx, &log, "Sẵn sàng mở trình biên tập.");
    Ok(session)
}

/// CI exercises the exact native bootstrap without requiring a graphical desktop.
/// The session is dropped before returning, including the GUI-owned worker.
pub fn verify_runtime(report_path: &Path) -> Result<()> {
    let (tx, _rx) = mpsc::channel();
    let result = runtime_root().and_then(|root| prepare(&root, &model::data_dir(), &process::cancel_token(), &tx));
    let report = match &result {
        Ok(session) => serde_json::json!({"status":"ready","python":session.python,"owned_worker":session._worker.is_some()}),
        Err(error) => serde_json::json!({"status":"error","message":format!("{error:#}")}),
    };
    model::write_json(report_path, &report)?;
    result.map(|_| ())
}

pub struct Launcher {
    root: Result<PathBuf, String>,
    data: PathBuf,
    rx: Option<mpsc::Receiver<Event>>,
    task: Option<thread::JoinHandle<()>>,
    cancel: process::Cancel,
    session: Option<Session>,
    studio: Option<crate::app::StudioApp>,
    lines: Vec<String>,
    error: Option<String>,
    started: Instant,
    icon: egui::TextureHandle,
    capture: Option<PathBuf>,
    captured: bool,
}
impl Launcher {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::app::configure_context(&cc.egui_ctx);
        let mut app = Self { root: runtime_root().map_err(|e| format!("{e:#}")), data: model::data_dir(),
            rx: None, task: None, cancel: process::cancel_token(), session: None, studio: None,
            lines: vec![], error: None, started: Instant::now(), icon: crate::branding::logo(&cc.egui_ctx),
            capture: std::env::args().find_map(|s| s.strip_prefix("--capture-startup=").map(PathBuf::from)), captured: false };
        app.start();
        app
    }
    fn start(&mut self) {
        if let Some(task) = self.task.take() { let _ = task.join(); }
        self.lines.clear(); self.error = None; self.started = Instant::now();
        self.cancel = process::cancel_token();
        let root = match &self.root { Ok(root) => root.clone(), Err(error) => { self.error = Some(error.clone()); return; } };
        let data = self.data.clone(); let cancel = self.cancel.clone();
        let (tx, rx) = mpsc::channel(); self.rx = Some(rx);
        self.task = Some(thread::spawn(move || {
            let result = prepare(&root, &data, &cancel, &tx).map_err(|e| format!("{e:#}"));
            let report = match &result {
                Ok(_) => serde_json::json!({"status":"ready","pid":std::process::id()}),
                Err(error) => serde_json::json!({"status":"error","pid":std::process::id(),"message":error}),
            };
            let _ = model::write_json(&data.join(format!("logs/startup-{}.json", std::process::id())), &report);
            let _ = tx.send(Event::Finished(result));
        }));
    }
    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.rx {
            while let Ok(event) = rx.try_recv() {
                match event {
                    Event::Progress(line) => { self.lines.push(line); if self.lines.len() > 100 { self.lines.remove(0); } }
                    Event::Finished(Ok(session)) => {
                        self.studio = Some(crate::app::StudioApp::with_runtime(ctx, &session.python, &session.speech_url));
                        self.session = Some(session);
                    }
                    Event::Finished(Err(error)) => self.error = Some(error),
                }
            }
        }
    }
    fn startup_ui(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(45.);
                ui.vertical_centered(|ui| {
                    ui.image((self.icon.id(), egui::vec2(72., 72.)));
                    ui.heading("AIR3view");
                    ui.label("Chuẩn bị môi trường làm việc");
                });
                ui.add_space(24.);
                let width = (ui.available_width() - 48.).clamp(300., 800.);
                ui.horizontal(|ui| {
                    ui.add_space(((ui.available_width() - width) / 2.).max(0.));
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        if let Some(error) = &self.error {
                            ui.colored_label(Color32::from_rgb(255, 140, 140), error);
                        } else {
                            ui.horizontal(|ui| { ui.spinner(); ui.label(format!("Đang chuẩn bị · {} giây", self.started.elapsed().as_secs())); });
                            ui.label(self.lines.last().map(String::as_str).unwrap_or("Đang kiểm tra thư mục ứng dụng…"));
                        }
                        ui.add_space(12.);
                        ui.label("Lần đầu cần Internet và dung lượng trống để tải Python, thư viện và model. Những lần sau dùng lại dữ liệu đã cài.");
                        ui.add_space(12.);
                        let details = |ui: &mut egui::Ui| {
                            egui::ScrollArea::vertical().id_salt("startup_details").max_height(190.).stick_to_bottom(true).show(ui, |ui| {
                                for line in &self.lines { ui.label(RichText::new(line).monospace().size(12.)); }
                            });
                        };
                        if self.error.is_some() && !self.lines.is_empty() {
                            ui.label("Chi tiết lỗi"); details(ui);
                        } else if !self.lines.is_empty() {
                            egui::CollapsingHeader::new("Chi tiết khởi động").show(ui, details);
                        }
                        ui.add_space(14.);
                        ui.horizontal_wrapped(|ui| {
                            if self.error.is_some() && ui.button("Thử lại").clicked() { self.start(); }
                            if ui.button("Mở thư mục nhật ký").clicked() {
                                let folder = self.data.join("logs"); let _ = fs::create_dir_all(&folder);
                                let _ = process::command("explorer.exe").arg(folder).spawn();
                            }
                            if self.error.as_ref().is_some_and(|error| !error.starts_with("AIR3view đang mở")) && ui.button("Mở editor để kiểm tra cài đặt").clicked() {
                                self.studio = Some(crate::app::StudioApp::from_context(ctx));
                            }
                            if ui.button("Đóng AIR3view").clicked() { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
                        });
                        ui.label(RichText::new("Nhật ký: AppData / Local / ReactionStudio / logs").size(12.).color(Color32::GRAY));
                    });
                });
            });
        });
    }
}
impl eframe::App for Launcher {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.poll(ctx);
        if let Some(path) = &self.capture {
            if let Some(image) = ctx.input(|i| i.events.iter().find_map(|e| match e { egui::Event::Screenshot { image, .. } => Some(image.clone()), _ => None })) {
                let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                if let Some(image) = image::RgbaImage::from_raw(image.width() as u32, image.height() as u32, bytes) { let _ = image.save(path); }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if !self.captured && self.started.elapsed() > Duration::from_secs(2) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default())); self.captured = true;
            }
            self.startup_ui(ctx);
        } else if let Some(studio) = &mut self.studio { studio.update(ctx, frame); }
        else { self.startup_ui(ctx); }
        ctx.request_repaint_after(Duration::from_millis(100));
    }
    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        if let Some(studio) = &mut self.studio { studio.on_exit(gl); }
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(task) = self.task.take() { let _ = task.join(); }
        // Drop queued successful sessions too if the window closed before poll().
        self.rx = None;
        self.session = None;
    }
}

impl Drop for Launcher {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(task) = self.task.take() { let _ = task.join(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_worker_is_reused_only_with_complete_api_contract() {
        let mut h: Health = serde_json::from_value(serde_json::json!({"status":"ready","model":MODEL,"protocol":3,"reference_audio":true,"languages":["vi","en"]})).unwrap();
        assert!(h.compatible());
        h.languages = vec!["vi".into()]; assert!(!h.compatible());
        h.languages.push("en".into()); h.protocol = 2; assert!(!h.compatible());
    }
    #[test]
    fn remote_and_local_endpoints_are_not_confused() {
        assert_eq!(local_speech_port("http://127.0.0.1:8765/v1/audio/speech").unwrap(), Some(8765));
        assert_eq!(local_speech_port("http://localhost:19001/v1/audio/speech").unwrap(), Some(19001));
        assert_eq!(local_speech_port("https://voice.example/v1/audio/speech").unwrap(), None);
        assert!(local_speech_port("not a URL").is_err());
    }
    #[cfg(windows)]
    #[test]
    fn exclusive_instance_lock_releases_after_exit_without_stale_lock() {
        let dir = tempfile::tempdir().unwrap();
        let first = instance_lock(dir.path()).unwrap();
        assert!(instance_lock(dir.path()).is_err());
        drop(first);
        assert!(instance_lock(dir.path()).is_ok());
    }
}
