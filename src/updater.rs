//! Downloads are isolated from the UI; the external installer only mutates the
//! portable installation after this process and its owned workers have exited.
use anyhow::{ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::{fs, io::{Read, Write}, path::{Path, PathBuf}, process::Stdio, sync::mpsc, time::Duration};

const SCRIPT: &str = include_str!("../scripts/apply-update.ps1");
const MAX_ZIP: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug)]
pub enum Event {
    Progress(String, Option<f32>),
    Ready(PathBuf),
    Failed(String),
}

#[derive(Default)]
pub struct Installer {
    pub message: String,
    pub fraction: Option<f32>,
    pub active: bool,
    pub ready: Option<PathBuf>,
    pub error: bool,
    pub launching: Option<std::time::Instant>,
    pub child: Option<std::process::Child>,
    rx: Option<mpsc::Receiver<Event>>,
}
impl Installer {
    pub fn start(&mut self, tag: String, download: String, checksum: String) {
        if self.active { return; }
        self.active = true;
        self.error = false;
        self.message = "Đang chuẩn bị tải bản cập nhật…".into();
        self.fraction = None;
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let result = prepare(&tag, &download, &checksum, &tx);
            let _ = tx.send(match result {
                Ok(job) => Event::Ready(job),
                Err(e) => Event::Failed(format!("Cập nhật chưa hoàn tất: {e:#}")),
            });
        });
    }
    pub fn poll(&mut self) {
        let events: Vec<_> = self.rx.as_ref().map(|r| r.try_iter().collect()).unwrap_or_default();
        for event in events {
            match event {
                Event::Progress(message, fraction) => { self.message = message; self.fraction = fraction; }
                Event::Ready(job) => {
                    self.ready = Some(job); self.rx = None;
                    self.message = "Đã kiểm tra bản tải. Chờ tác vụ hoàn tất để cài và mở lại…".into();
                    self.fraction = Some(1.);
                }
                Event::Failed(message) => { self.fail(message); }
            }
        }
    }
    pub fn fail(&mut self, message: String) {
        if let Some(mut child) = self.child.take() { let _ = child.kill(); let _ = child.wait(); }
        self.message = message; self.error = true; self.active = false;
        self.ready = None; self.rx = None; self.launching = None;
    }
}

fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(concat!("AIR3view/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15)).timeout(Duration::from_secs(1800))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let u = attempt.url();
            let allowed = u.scheme() == "https" && u.username().is_empty() && u.password().is_none()
                && matches!(u.host_str(), Some("github.com" | "release-assets.githubusercontent.com" | "objects.githubusercontent.com"));
            if attempt.previous().len() > 5 || !allowed { attempt.error("Untrusted release redirect") }
            else { attempt.follow() }
        })).build()?)
}

fn expected_hash(text: &str, filename: &str) -> Result<String> {
    let mut hashes = text.trim_start_matches('\u{feff}').lines().filter_map(|line| {
        let (hash, name) = line.split_once(char::is_whitespace)?;
        (name.trim().trim_start_matches('*') == filename).then_some(hash)
    });
    let hash = hashes.next().context("Release thiếu SHA256 cho ZIP Windows")?;
    ensure!(hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()) && hashes.next().is_none(), "SHA256 không hợp lệ hoặc bị lặp");
    Ok(hash.to_ascii_lowercase())
}

fn transfer(mut input: impl Read, path: &Path, expected: &str, total: Option<u64>, tx: &mpsc::Sender<Event>) -> Result<()> {
    ensure!(total.is_none_or(|n| n <= MAX_ZIP), "ZIP vượt giới hạn 2 GB");
    let mut output = fs::File::create(path)?;
    let mut digest = Sha256::new();
    let mut done = 0u64;
    let mut buf = [0u8; 128 * 1024];
    let mut last = std::time::Instant::now();
    loop {
        let count = input.read(&mut buf)?;
        if count == 0 { break; }
        done += count as u64;
        ensure!(done <= MAX_ZIP, "ZIP vượt giới hạn 2 GB");
        output.write_all(&buf[..count])?; digest.update(&buf[..count]);
        if last.elapsed() > Duration::from_millis(200) {
            let _ = tx.send(Event::Progress(format!("Đang tải bản mới: {:.1} MB", done as f64 / 1048576.), total.filter(|n| *n > 0).map(|n| done as f32 / n as f32)));
            last = std::time::Instant::now();
        }
    }
    output.sync_all()?;
    ensure!(total.is_none_or(|n| n == done), "Tải ZIP chưa đủ dung lượng");
    ensure!(format!("{:x}", digest.finalize()) == expected, "SHA256 không khớp. Bản đang dùng được giữ nguyên; hãy thử tải lại.");
    Ok(())
}

fn powershell(script: &Path, job: &Path, mode: &str) -> std::process::Command {
    let mut cmd = crate::process::command("powershell.exe");
    cmd.env_remove("PSModulePath").args(["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(script).arg("-JobFile").arg(job).arg("-Mode").arg(mode).stdin(Stdio::null());
    cmd
}

fn prepare(tag: &str, download: &str, checksum: &str, tx: &mpsc::Sender<Event>) -> Result<PathBuf> {
    let base = format!("{}/releases/download/{tag}/", crate::updates::REPOSITORY);
    ensure!(tag.starts_with('v') && tag.bytes().all(|b| b.is_ascii_alphanumeric() || b".+-".contains(&b)), "Tag release không hợp lệ");
    let filename = download.strip_prefix(&base).context("ZIP không thuộc release chính thức")?;
    ensure!(filename.starts_with("AIR3view-") && filename.ends_with("-windows-x64.zip") && !filename.contains(['/', '\\', '?', '#']), "Tên ZIP không hợp lệ");
    ensure!(checksum == format!("{base}SHA256SUMS.txt"), "Checksum không thuộc release chính thức");
    let exe = std::env::current_exe()?;
    let root = exe.parent().context("Thiếu thư mục app")?;
    prepare_at(root, &exe, tag, download, checksum, tx)
}

fn prepare_at(root: &Path, exe: &Path, tag: &str, download: &str, checksum: &str, tx: &mpsc::Sender<Event>) -> Result<PathBuf> {
    let filename = url::Url::parse(download)?.path_segments().and_then(|mut s| s.next_back()).context("Thiếu tên ZIP")?.to_owned();
    ensure!(root.join("release.json").is_file() && root.join("package-manifest.json").is_file(),
        "Chỉ cập nhật tự động bản đã đóng gói. Hãy mở AIR3view.exe trong thư mục bản Windows.");
    let stage = root.join(format!(".air3view-update-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&stage).context("Không ghi được thư mục app. Chuyển app sang thư mục có quyền ghi rồi thử lại")?;
    let result = (|| -> Result<PathBuf> {
        let client = client()?;
        let mut sums = String::new();
        client.get(checksum).timeout(Duration::from_secs(30)).send()?.error_for_status()?.take(65537).read_to_string(&mut sums)?;
        ensure!(sums.len() <= 65536, "Tệp checksum quá lớn");
        let hash = expected_hash(&sums, &filename)?;
        let response = client.get(download).send()?.error_for_status()?;
        let length = response.content_length();
        transfer(response, &stage.join("release.zip"), &hash, length, tx)?;
        let _ = tx.send(Event::Progress("Đang giải nén và kiểm tra từng file ứng dụng…".into(), None));
        // A UTF-8 BOM keeps Vietnamese installation paths intact under Windows PowerShell 5.1.
        fs::write(stage.join("apply-update.ps1"), format!("\u{feff}{}", SCRIPT.trim_start_matches('\u{feff}')))?;
        let job = stage.join("job.json");
        let log_dir = crate::model::data_dir().join("logs"); fs::create_dir_all(&log_dir)?;
        crate::model::write_json(&job, &serde_json::json!({
            "root":root,"stage":stage,"tag":tag,"parent_pid":std::process::id(),
            "old_exe":exe,"result":log_dir.join("update-last.json")
        }))?;
        let log_path = stage.join("prepare.log");
        let log = fs::File::create(&log_path)?;
        let err = log.try_clone()?;
        let mut child = powershell(&stage.join("apply-update.ps1"), &job, "Prepare")
            .current_dir(root).stdout(log).stderr(err).spawn()?;
        let started = std::time::Instant::now();
        loop {
            if let Some(status) = child.try_wait()? {
                ensure!(status.success(), "Gói cập nhật không hợp lệ: {}", fs::read_to_string(&log_path).unwrap_or_default());
                break;
            }
            if started.elapsed() > Duration::from_secs(600) {
                crate::process::stop(&mut child);
                anyhow::bail!("Kiểm tra gói quá 10 phút");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(job)
    })();
    if result.is_err() {
        // Only this uniquely created staging directory is eligible for cleanup.
        let _ = fs::remove_dir_all(&stage);
    }
    result
}

pub fn launch(job: &Path) -> Result<std::process::Child> {
    let stage = job.parent().context("Thiếu thư mục cập nhật")?;
    let log = fs::File::create(stage.join("installer.log"))?;
    let err = log.try_clone()?;
    let mut child = powershell(&stage.join("apply-update.ps1"), job, "Install")
        .stdout(log).stderr(err).spawn().context("Không mở được trình cập nhật")?;
    // UI polls this handshake on subsequent frames; it never blocks waiting here.
    if let Some(status) = child.try_wait()? { ensure!(status.success(), "Trình cập nhật không khởi động. Xem installer.log"); }
    Ok(child)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn checksum_requires_exact_asset_and_unique_hash() {
        let h = "a".repeat(64);
        assert_eq!(expected_hash(&format!("{h}  a.zip\n{}  b.zip", "b".repeat(64)), "a.zip").unwrap(), h);
        assert!(expected_hash(&format!("{h}  other.zip"), "a.zip").is_err());
        assert!(expected_hash(&format!("{h}  a.zip\n{h}  a.zip"), "a.zip").is_err());
    }
    #[test] fn partial_or_corrupt_download_never_passes() {
        let dir = tempfile::tempdir().unwrap(); let path = dir.path().join("release.zip");
        let (tx, _rx) = mpsc::channel(); let bytes = b"release fixture";
        let hash = format!("{:x}", Sha256::digest(bytes));
        transfer(&bytes[..], &path, &hash, Some(bytes.len() as u64), &tx).unwrap();
        assert!(transfer(&bytes[..3], &path, &hash, Some(bytes.len() as u64), &tx).is_err());
        assert!(transfer(&bytes[..], &path, &"0".repeat(64), None, &tx).is_err());
    }
    #[test]
    #[ignore = "Downloads the public 242 MB release; run explicitly for release verification"]
    fn official_release_download_and_manifest_verification() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("release.json"), "{}").unwrap();
        fs::write(root.path().join("package-manifest.json"), "[]").unwrap();
        let (tx, _rx) = mpsc::channel();
        let base = format!("{}/releases/download/v0.2.2", crate::updates::REPOSITORY);
        let job = prepare_at(root.path(), &root.path().join("AIR3view.exe"), "v0.2.2",
            &format!("{base}/AIR3view-0.2.2-windows-x64.zip"), &format!("{base}/SHA256SUMS.txt"), &tx).unwrap();
        assert!(job.parent().unwrap().join("payload/AIR3view.exe").is_file());
        assert_eq!(fs::read_to_string(root.path().join("release.json")).unwrap(), "{}");
    }
}
