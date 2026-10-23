//! Only the downloader receives cookies. Keep the selected file unchanged:
//! yt-dlp saves its cookie jar on exit, so give each invocation a temporary copy.
use anyhow::{ensure, Context, Result};
use std::{fs, io::Write, path::Path};

fn contents(path: &Path) -> Result<String> {
    let metadata = fs::metadata(path).context("Không đọc được file cookie. Chọn lại file trong Cài đặt kết nối.")?;
    ensure!(metadata.is_file() && metadata.len() <= 10 * 1024 * 1024,
        "File cookie phải là file văn bản, tối đa 10 MB.");
    let text = fs::read_to_string(path).context("File cookie phải là văn bản UTF-8 định dạng Netscape cookies.txt.")?;
    let text = text.trim_start_matches('\u{feff}');
    ensure!(matches!(text.lines().next(), Some("# Netscape HTTP Cookie File" | "# HTTP Cookie File")),
        "File cookie chưa đúng định dạng Netscape cookies.txt. Xuất lại dạng Netscape, không dùng JSON.");
    let mut youtube = false;
    for (index, line) in text.lines().enumerate().skip(1) {
        let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
        if line.is_empty() || line.starts_with('#') { continue; }
        let fields: Vec<_> = line.split('\t').collect();
        ensure!(fields.len() == 7, "File cookie sai định dạng tại dòng {} (cần 7 cột phân cách bằng tab).", index + 1);
        ensure!(!fields[0].is_empty() && matches!(fields[1], "TRUE" | "FALSE")
            && fields[2].starts_with('/') && matches!(fields[3], "TRUE" | "FALSE")
            && (fields[4].is_empty() || fields[4].parse::<i64>().is_ok()) && !fields[5].is_empty(),
            "File cookie có trường không hợp lệ tại dòng {}. Hãy xuất lại file Netscape.", index + 1);
        let domain = fields[0].trim_start_matches('.').to_ascii_lowercase();
        youtube |= ["youtube.com", "google.com"].iter().any(|base| domain == *base || domain.ends_with(&format!(".{base}")));
    }
    ensure!(youtube, "File chưa có cookie YouTube/Google. Xuất cookie sau khi đăng nhập YouTube bằng tài khoản xem được video.");
    Ok(text.to_owned())
}

/// Validates syntax only, not whether the account/session can access a video.
pub fn validate(path: &Path) -> Result<()> {
    contents(path).map(|_| ())
}

pub struct DownloadCookies(tempfile::TempPath);
impl DownloadCookies {
    pub fn from_setting(path: &str) -> Result<Option<Self>> {
        if path.trim().is_empty() { return Ok(None); }
        let text = contents(Path::new(path))?;
        let mut file = tempfile::Builder::new().prefix("air3view-cookie-").suffix(".txt").tempfile()
            .context("Không tạo được file cookie tạm cho tác vụ tải.")?;
        file.write_all(text.as_bytes())?;
        // Close the handle before yt-dlp opens/rewrites it on Windows.
        Ok(Some(Self(file.into_temp_path())))
    }
    pub fn path(&self) -> &Path { &self.0 }
}
