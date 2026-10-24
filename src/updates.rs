//! Public GitHub update checks run off the UI thread and never receive provider keys.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, io::Read, path::PathBuf, sync::mpsc, time::{Duration, Instant}};

pub const REPOSITORY: &str = "https://github.com/insofanhh/AIR3view_v2";
const API: &str = "https://api.github.com/repos/insofanhh/AIR3view_v2";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct BuildInfo {
    pub version: String,
    pub tag: String,
    pub commit: String,
    pub built_at: String,
}
impl BuildInfo {
    pub fn installed() -> Self {
        std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.join("release.json")))
            .and_then(|p| fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(s.trim_start_matches('\u{feff}')).ok())
            .unwrap_or_else(|| Self { version: env!("CARGO_PKG_VERSION").into(), ..Self::default() })
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Current,
    Code { commit: String, url: String },
    Release { tag: String, url: String, download: String, checksum: String },
}
#[derive(Deserialize)]
struct Asset { name: String, browser_download_url: String }
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    target_commitish: String,
    published_at: Option<String>,
    html_url: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Commit { sha: String, html_url: String, commit: CommitDetails }
#[derive(Deserialize)]
struct CommitDetails { committer: CommitDate }
#[derive(Deserialize)]
struct CommitDate { date: String }

fn official(url: &str, prefix: &str) -> bool {
    url::Url::parse(url).is_ok_and(|u| u.scheme() == "https" && u.host_str() == Some("github.com")
        && u.username().is_empty() && u.password().is_none() && u.path().starts_with(prefix))
}
fn version_triplet(version: &str) -> Option<[u64; 3]> {
    let mut parts = version.trim_start_matches('v').split(['-', '+']).next()?.split('.');
    let result = [parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?];
    parts.next().is_none().then_some(result)
}
fn get<T: serde::de::DeserializeOwned>(client: &reqwest::blocking::Client, url: &str) -> Result<Option<T>> {
    let response = client.get(url).send()?;
    if response.status() == reqwest::StatusCode::NOT_FOUND { return Ok(None); }
    if response.status() == reqwest::StatusCode::FORBIDDEN || response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        bail!("GitHub đang giới hạn lượt kiểm tra. Thử lại sau.");
    }
    let response = response.error_for_status()?;
    let mut bytes = Vec::new();
    response.take(262145).read_to_end(&mut bytes)?;
    if bytes.len() > 262144 { bail!("Phản hồi cập nhật quá lớn"); }
    Ok(Some(serde_json::from_slice(&bytes)?))
}
fn check_at(build: &BuildInfo, api: &str) -> Result<Status> {
    let client = reqwest::blocking::Client::builder().user_agent(concat!("AIR3view/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(3)).timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none()).build()?;
    let release: Option<Release> = get(&client, &format!("{api}/releases/latest"))?;
    if let Some(release) = release {
        let newer = build.built_at.is_empty() || release.published_at.as_deref().is_some_and(|d| d > build.built_at.as_str());
        let not_older_version = version_triplet(&release.tag_name).zip(version_triplet(&build.version))
            .is_none_or(|(available, installed)| available >= installed);
        if !release.draft && !release.prerelease && release.tag_name != build.tag
            && (build.commit.is_empty() || release.target_commitish != build.commit) && newer && not_older_version
            && official(&release.html_url, "/insofanhh/AIR3view_v2/releases/tag/") {
            if let Some(asset) = release.assets.iter().find(|a| a.name.starts_with("AIR3view-")
                && a.name.ends_with("-windows-x64.zip")
                && official(&a.browser_download_url, "/insofanhh/AIR3view_v2/releases/download/")) {
                let checksum = release.assets.iter().find(|a| a.name == "SHA256SUMS.txt"
                    && a.browser_download_url == format!("{REPOSITORY}/releases/download/{}/SHA256SUMS.txt", release.tag_name));
                if let Some(checksum) = checksum {
                    return Ok(Status::Release { tag: release.tag_name, url: release.html_url,
                        download: asset.browser_download_url.clone(), checksum: checksum.browser_download_url.clone() });
                }
            }
        }
    }
    let commit: Option<Commit> = get(&client, &format!("{api}/commits/main"))?;
    if let Some(c) = commit {
        let newer = build.built_at.is_empty() || c.commit.committer.date > build.built_at;
        if !build.commit.is_empty() && c.sha != build.commit && newer
            && official(&c.html_url, "/insofanhh/AIR3view_v2/commit/") {
            return Ok(Status::Code { commit: c.sha.chars().take(8).collect(), url: c.html_url });
        }
    }
    Ok(Status::Current)
}
pub fn check(build: &BuildInfo) -> Result<Status> { check_at(build, API).context("Không thể kiểm tra cập nhật") }

#[derive(Deserialize, Serialize)]
struct Preferences { automatic: bool }
pub struct Updates {
    pub installer: crate::updater::Installer,
    pub build: BuildInfo,
    pub automatic: bool,
    pub open: bool,
    pub status: Option<Status>,
    pub error: Option<String>,
    pub checking: bool,
    dismissed: Option<Status>,
    path: PathBuf,
    rx: Option<mpsc::Receiver<Result<Status, String>>>,
    last_check: Option<Instant>,
}
impl Updates {
    pub fn new(path: PathBuf) -> Self {
        let automatic = fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Preferences>(&b).ok()).is_none_or(|p| p.automatic);
        let mut installer = crate::updater::Installer::default();
        let report = path.parent().unwrap_or(std::path::Path::new(".")).join("logs/update-last.json");
        if let Some(result) = fs::read(&report).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) {
            installer.message = result["message"].as_str().unwrap_or_default().into();
            installer.error = result["status"].as_str() == Some("error");
        }
        let open = installer.error;
        Self { installer, build: BuildInfo::installed(), automatic, open, status:None, error:None, checking:false,
            dismissed:None, path, rx:None, last_check:None }
    }
    pub fn save(&self) -> Result<()> {
        if let Some(parent)=self.path.parent() { fs::create_dir_all(parent)?; }
        fs::write(&self.path, serde_json::to_vec_pretty(&Preferences{automatic:self.automatic})?)?;
        Ok(())
    }
    pub fn start(&mut self) {
        if self.checking { return; }
        self.error=None;self.checking=true;self.last_check=Some(Instant::now());
        let (tx,rx)=mpsc::channel();self.rx=Some(rx);let build=self.build.clone();
        std::thread::spawn(move||{let _=tx.send(check(&build).map_err(|e|format!("{e:#}")));});
    }
    pub fn poll(&mut self) {
        self.installer.poll();
        if let Some(result)=self.rx.as_ref().and_then(|rx|rx.try_recv().ok()) {
            self.checking=false;self.rx=None;
            match result { Ok(status)=>{self.status=Some(status);self.error=None;},Err(e)=>self.error=Some(e) }
        }
        #[cfg(not(test))]
        if self.automatic && !self.checking && !self.installer.active && self.last_check.is_none_or(|t|t.elapsed()>=Duration::from_secs(900)) { self.start(); }
    }
    pub fn banner(&self) -> Option<&Status> {
        self.status.as_ref().filter(|s|**s!=Status::Current && self.dismissed.as_ref()!=Some(*s))
    }
    pub fn dismiss(&mut self) { self.dismissed=self.status.clone(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::{Read,Write},net::TcpListener};
    fn fixture(release: serde_json::Value, commit: serde_json::Value) -> String {
        let listener=TcpListener::bind("127.0.0.1:0").unwrap();let base=format!("http://{}",listener.local_addr().unwrap());
        std::thread::spawn(move||for incoming in listener.incoming().take(2) {
            let mut stream=incoming.unwrap();stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut buf=[0;8192];let n=stream.read(&mut buf).unwrap();let request=String::from_utf8_lossy(&buf[..n]);
            assert!(!request.to_lowercase().contains("authorization:"));
            let body=if request.contains("/releases/latest") { &release } else { &commit };
            let status=if body.is_null(){"404 Not Found"}else{"200 OK"};let body=body.to_string();
            write!(stream,"HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
        });base
    }
    fn build() -> BuildInfo { BuildInfo { tag:"v0.2.0".into(),commit:"old".into(),built_at:"2026-01-02T00:00:00Z".into(),..Default::default() } }
    fn release() -> serde_json::Value { serde_json::json!({"tag_name":"v0.2.1","target_commitish":"new","published_at":"2026-01-03T00:00:00Z","html_url":format!("{REPOSITORY}/releases/tag/v0.2.1"),"draft":false,"prerelease":false,"assets":[{"name":"AIR3view-0.2.1-windows-x64.zip","browser_download_url":format!("{REPOSITORY}/releases/download/v0.2.1/AIR3view-0.2.1-windows-x64.zip")},{"name":"SHA256SUMS.txt","browser_download_url":format!("{REPOSITORY}/releases/download/v0.2.1/SHA256SUMS.txt")}]}) }
    #[test] fn ready_release_has_a_download() { assert!(matches!(check_at(&build(),&fixture(release(),serde_json::Value::Null)).unwrap(),Status::Release{..})); }
    #[test] fn missing_build_metadata_never_offers_an_older_version() {
        let build=BuildInfo { version:"0.2.3".into(), ..Default::default() };
        assert_eq!(check_at(&build,&fixture(release(),serde_json::Value::Null)).unwrap(),Status::Current);
        assert_eq!(version_triplet("v0.2.3-build.1"), Some([0,2,3]));
    }
    #[test] fn release_without_matching_checksums_cannot_be_installed() {
        let mut r=release();r["assets"].as_array_mut().unwrap().pop();
        assert_eq!(check_at(&build(),&fixture(r,serde_json::Value::Null)).unwrap(),Status::Current);
        let mut r=release();r["assets"][1]["browser_download_url"]="https://example.com/SHA256SUMS.txt".into();
        assert_eq!(check_at(&build(),&fixture(r,serde_json::Value::Null)).unwrap(),Status::Current);
    }
    #[test] fn commits_notify_while_binary_is_pending() {
        let c=serde_json::json!({"sha":"next123456","html_url":format!("{REPOSITORY}/commit/next123456"),"commit":{"committer":{"date":"2026-01-03T00:00:00Z"}}});
        assert!(matches!(check_at(&build(),&fixture(serde_json::Value::Null,c)).unwrap(),Status::Code{..}));
    }
    #[test] fn same_build_and_older_release_do_not_offer_downgrade() {
        let mut r=release();r["published_at"]="2026-01-01T00:00:00Z".into();
        assert_eq!(check_at(&build(),&fixture(r,serde_json::Value::Null)).unwrap(),Status::Current);
        let mut r=release();r["target_commitish"]="old".into();
        assert_eq!(check_at(&build(),&fixture(r,serde_json::Value::Null)).unwrap(),Status::Current);
    }
    #[test] fn external_downloads_are_rejected_and_preferences_persist() {
        let mut r=release();r["assets"][0]["browser_download_url"]="https://example.com/app.exe".into();
        assert_eq!(check_at(&build(),&fixture(r,serde_json::Value::Null)).unwrap(),Status::Current);
        let tmp=tempfile::tempdir().unwrap();let p=tmp.path().join("updates.json");let mut updates=Updates::new(p.clone());updates.automatic=false;updates.save().unwrap();assert!(!Updates::new(p).automatic);
    }
}
