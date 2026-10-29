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

/// Pin bytes without transcoding: the shared asset survives deletion of the
/// project which originally owned it. Recovery accepts exact identity only.
pub fn shared_file(data:&Path, value:&str, label:&str)->Result<PathBuf> {
    ensure!(!value.trim().is_empty(),"Chưa chọn {label}. Chọn lại file trong Giọng & nhân vật.");
    let source=Path::new(value);
    let store=data.join("assets/references");
    let alias=store.join("aliases").join(format!("{}.json",media::hash(value.as_bytes())));
    let (bytes,ext)=if source.is_file() {
        ensure!(fs::metadata(source)?.len()<=64*1024*1024,"File {label} vượt 64 MB");
        (fs::read(source).with_context(||format!("Không đọc được {label}: {value}"))?,source.extension().and_then(|v|v.to_str()).unwrap_or("bin").to_ascii_lowercase())
    } else {
        let recorded=fs::read(&alias).ok().and_then(|b|serde_json::from_slice::<(String,String)>(&b).ok());
        let expected=recorded.as_ref().map(|r|r.0.as_str()).or_else(||source.file_stem().and_then(|v|v.to_str()))
            .filter(|v|v.len()==64&&v.bytes().all(|b|b.is_ascii_hexdigit()))
            .with_context(||format!("Không tìm thấy {label}: {value}. Chọn lại đúng file trong Giọng & nhân vật."))?;
        let mut found=None;
        for folder in [&store,&data.join("assets/voices"),&data.join("assets/avatars")] {
            for entry in fs::read_dir(folder).into_iter().flatten().flatten() {
                let path=entry.path();
                if entry.file_type().is_ok_and(|t|t.is_file())&&entry.metadata().is_ok_and(|m|m.len()<=64*1024*1024) {
                    if let Ok(b)=fs::read(&path) {if media::hash(&b)==expected {
                        found=Some((b,path.extension().and_then(|v|v.to_str()).unwrap_or("bin").to_ascii_lowercase()));break;
                    }}
                }
            }
            if found.is_some(){break;}
        }
        found.with_context(||format!("Không tìm thấy bản cùng nội dung của {label}: {value}. Chọn lại file; tool không tự đổi sang giọng/ảnh khác."))?
    };
    ensure!(!bytes.is_empty(),"File {label} rỗng");
    let digest=media::hash(&bytes);
    ensure!(ext.bytes().all(|b|b.is_ascii_alphanumeric()),"Đuôi file {label} không hợp lệ");
    fs::create_dir_all(&store)?;
    let target=store.join(format!("{digest}.{ext}"));
    if !target.is_file() {
        use std::io::Write;
        let mut pending=tempfile::NamedTempFile::new_in(&store)?;pending.write_all(&bytes)?;
        if let Err(e)=pending.persist_noclobber(&target) {if !target.is_file(){return Err(e.error.into());}}
    }
    ensure!(media::hash(fs::read(&target)?)==digest,"Bản lưu {label} bị lỗi; kiểm tra ổ đĩa");
    fs::create_dir_all(alias.parent().unwrap())?;
    let mut pending=tempfile::NamedTempFile::new_in(alias.parent().unwrap())?;
    serde_json::to_writer(pending.as_file_mut(),&(digest,ext))?;
    pending.persist(&alias).map_err(|e|e.error)?;
    Ok(target)
}

pub fn pin_production(data:&Path,p:&mut crate::model::ProductionSettings)->Result<()> {
    if p.voice_reference_enabled || (!p.voice_reference_path.is_empty()&&Path::new(&p.voice_reference_path).is_file()) {
        p.voice_reference_path=shared_file(data,&p.voice_reference_path,"giọng tham chiếu")?.to_string_lossy().into_owned();
    }
    if (!p.avatar_image_path.is_empty()&&Path::new(&p.avatar_image_path).is_file()) || (p.avatar_enabled&&p.avatar_provider=="heygen"&&!p.avatar_image_path.is_empty()) {
        p.avatar_image_path=shared_file(data,&p.avatar_image_path,"ảnh nhân vật HeyGen")?.to_string_lossy().into_owned();
    }
    Ok(())
}
