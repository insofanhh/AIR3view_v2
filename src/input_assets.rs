//! User-picked files are copied into app storage so later moves do not break settings.
use crate::{media, model::Settings, process};
use anyhow::{ensure, Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

pub fn reference_audio(settings: &Settings, source: &Path, folder: &Path) -> Result<PathBuf> {
    ensure!(
        fs::metadata(source)?.len() <= 64 * 1024 * 1024,
        "File giọng mẫu vượt 64 MB"
    );
    let cancel = process::cancel_token();
    let info = media::probe(settings, source, &cancel)?;
    ensure!(
        info.has_audio && info.duration_ms >= 1000,
        "Chọn audio có giọng nói dài ít nhất 1 giây"
    );
    fs::create_dir_all(folder)?;
    let output = folder.join(format!("{}.wav", media::hash(fs::read(source)?)));
    if !output.is_file() {
        let pending = output.with_extension("pending.wav");
        process::run(
            &settings.ffmpeg,
            &process::args(&[
                "-y",
                "-v",
                "error",
                "-i",
                &source.to_string_lossy(),
                "-t",
                "8",
                "-vn",
                "-ac",
                "1",
                "-ar",
                "48000",
                "-c:a",
                "pcm_s16le",
                &pending.to_string_lossy(),
            ]),
            None,
            &cancel,
            Duration::from_secs(60),
            None,
        )?;
        fs::rename(pending, &output)?;
    }
    Ok(output)
}

pub fn avatar_image(source: &Path, folder: &Path) -> Result<PathBuf> {
    let bytes = image_bytes(source)?;
    let ext = match image::guess_format(&bytes)? {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpg",
        _ => unreachable!(),
    };
    fs::create_dir_all(folder)?;
    let output = folder.join(format!("{}.{}", media::hash(&bytes), ext));
    if !output.is_file() {
        fs::write(&output, bytes)?;
    }
    Ok(output)
}

pub fn image_bytes(path: &Path) -> Result<Vec<u8>> {
    ensure!(
        fs::metadata(path)?.len() <= 32 * 1024 * 1024,
        "Ảnh vượt 32 MB"
    );
    let bytes = fs::read(path)?;
    ensure!(
        matches!(
            image::guess_format(&bytes)?,
            image::ImageFormat::Png | image::ImageFormat::Jpeg
        ),
        "Chọn ảnh PNG hoặc JPEG"
    );
    // Decoder limits protect against images with huge declared dimensions.
    let reader = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format()?;
    let decoded = reader.decode().context("Không đọc được ảnh nhân vật")?;
    ensure!(
        decoded.width() >= 64 && decoded.height() >= 64,
        "Ảnh nhân vật cần ít nhất 64×64 pixel"
    );
    Ok(bytes)
}

pub fn file_fingerprint(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    fs::read(path)
        .map(media::hash)
        .unwrap_or_else(|_| format!("missing:{path}"))
}
