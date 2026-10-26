//! Portable settings container: bounded JSON manifest followed by streamed assets.
//! No executable paths, credentials, source video or generated timeline data.
use crate::{model::*, timeline};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAGIC: &[u8; 12] = b"AIR3PRESET\0\x01";
const MAX_JSON: u64 = 4 * 1024 * 1024;
const MAX_TOTAL: u64 = 512 * 1024 * 1024;
const PREFIX: &str = "air3asset:";

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Avatar,
    Reference,
    Music,
    Lut,
}
impl AssetKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Avatar => "Ảnh nhân vật",
            Self::Reference => "Giọng tham chiếu",
            Self::Music => "Nhạc nền",
            Self::Lut => "LUT màu",
        }
    }
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Avatar => &["png", "jpg", "jpeg"],
            Self::Reference => &["wav", "mp3", "flac", "ogg", "m4a"],
            Self::Music => &["mp3", "wav", "m4a", "flac", "ogg"],
            Self::Lut => &["cube"],
        }
    }
    fn limit(self) -> u64 {
        (match self {
            Self::Avatar => 32,
            Self::Reference => 64,
            Self::Music => 256,
            Self::Lut => 16,
        }) * 1024
            * 1024
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Asset {
    pub id: String,
    pub name: String,
    pub kind: AssetKind,
    pub bytes: u64,
    pub sha256: String,
    pub included: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    pub app_version: String,
    pub name: String,
    pub settings: ProjectDefaults,
    pub assets: Vec<Asset>,
}
pub struct Loaded {
    pub manifest: Manifest,
    staging: tempfile::TempDir,
}

/// Every file-bearing field passes through this one allowlist on export AND import.
fn paths(
    settings: &mut ProjectDefaults,
    mut map: impl FnMut(AssetKind, &str) -> Result<String>,
) -> Result<()> {
    if !settings.music.file.as_os_str().is_empty() {
        settings.music.file = map(AssetKind::Music, &settings.music.file.to_string_lossy())?.into();
    }
    if let Some(p) = &mut settings.production {
        if !p.avatar_image_path.is_empty() {
            p.avatar_image_path = map(AssetKind::Avatar, &p.avatar_image_path)?;
        }
        if !p.voice_reference_path.is_empty() {
            p.voice_reference_path = map(AssetKind::Reference, &p.voice_reference_path)?;
        }
    }
    for e in std::iter::once(&mut settings.effects.footage)
        .chain(settings.layer_defaults.effects.values_mut())
    {
        if !matches!(e.lut.as_str(), "off" | "warm" | "cool" | "documentary") {
            e.lut = map(AssetKind::Lut, &e.lut)?;
        }
    }
    Ok(())
}

fn digest(path: &Path, limit: u64) -> Result<(u64, String)> {
    let mut file = File::open(path)
        .with_context(|| format!("Không mở được tài nguyên: {}", path.display()))?;
    ensure!(
        file.metadata()?.len() <= limit,
        "Tài nguyên vượt giới hạn: {}",
        path.display()
    );
    let mut hash = Sha256::new();
    let mut total = 0;
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        ensure!(total <= limit, "Tài nguyên vượt giới hạn");
        hash.update(&buffer[..n]);
    }
    ensure!(total > 0, "Tài nguyên rỗng: {}", path.display());
    Ok((total, format!("{:x}", hash.finalize())))
}

pub fn export(
    project: &Project,
    runtime: &Settings,
    destination: &Path,
    include_assets: bool,
    progress: &dyn Fn(&str),
) -> Result<()> {
    let mut settings = ProjectDefaults::from_project(project);
    settings.production = Some(
        project
            .production
            .clone()
            .unwrap_or_else(|| ProductionSettings::from_settings(runtime)),
    );
    // Old projects retain choices from providers that are no longer selected.
    // Built-in 2D rigs never read the HeyGen photo; disabled inputs likewise
    // must not block a portable preset or leak an obsolete machine path.
    if let Some(p) = &mut settings.production {
        if !p.avatar_enabled || p.avatar_provider == "cartoon2d" || !p.avatar_id.is_empty() {
            p.avatar_image_path.clear();
        }
        if !p.voice_reference_enabled {p.voice_reference_path.clear();}
    }
    if !settings.music.enabled {settings.music.file.clear();settings.music.name.clear();}
    // Unlike the automatic defaults, an explicit package carries all metadata choices.
    settings.export_metadata = project.export_metadata.clone();
    let mut assets = Vec::<Asset>::new();
    let mut files = BTreeMap::new();
    let mut total = 0;
    progress("Kiểm tra thiết lập và tài nguyên…");
    paths(&mut settings, |kind, value| {
        let source = Path::new(value);
        let ext = source
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        ensure!(
            kind.extensions().contains(&ext.as_str()),
            "{}: định dạng file chưa hỗ trợ ({})",
            kind.label(),
            source.display()
        );
        let (bytes, sha256) = digest(source, kind.limit()).with_context(|| {
            format!(
                "{} bị thiếu hoặc không đọc được; chọn lại hoặc bỏ tài nguyên trong thiết lập",
                kind.label()
            )
        })?;
        let id = format!("{sha256}.{ext}");
        if !files.contains_key(&id) {
            total += bytes;
            ensure!(total <= MAX_TOTAL, "Tổng tài nguyên vượt 512 MB");
            assets.push(Asset {
                id: id.clone(),
                name: source
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                kind,
                bytes,
                sha256,
                included: include_assets,
            });
            files.insert(id.clone(), source.to_owned());
        }
        Ok(format!("{PREFIX}{id}"))
    })?;
    let manifest = Manifest {
        format: "AIR3view settings".into(),
        version: 1,
        app_version: env!("CARGO_PKG_VERSION").into(),
        name: project.title.clone(),
        settings,
        assets,
    };
    validate_manifest(&manifest)?;
    validate_settings(&manifest.settings, false)?;
    let json = serde_json::to_vec_pretty(&manifest)?;
    ensure!(json.len() as u64 <= MAX_JSON, "Thiết lập vượt 4 MB");
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut pending =
        tempfile::NamedTempFile::new_in(parent).context("Không ghi được thư mục xuất thiết lập")?;
    pending.write_all(MAGIC)?;
    pending.write_all(&(json.len() as u64).to_le_bytes())?;
    pending.write_all(&json)?;
    for asset in &manifest.assets {
        if asset.included {
            progress(&format!("Đóng gói {}: {}", asset.kind.label(), asset.name));
            let mut file = File::open(&files[&asset.id])?;
            let mut hash = Sha256::new();
            let mut count = 0;
            let mut buf = [0; 65536];
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                count += n as u64;
                ensure!(
                    count <= asset.bytes,
                    "Tài nguyên đã thay đổi khi xuất; thử lại"
                );
                hash.update(&buf[..n]);
                pending.write_all(&buf[..n])?;
            }
            ensure!(
                count == asset.bytes && format!("{:x}", hash.finalize()) == asset.sha256,
                "Tài nguyên đã thay đổi khi xuất; thử lại"
            );
        }
    }
    pending.as_file().sync_all()?;
    pending
        .persist(destination)
        .map_err(|e| e.error)
        .context("Không lưu được file thiết lập; đóng file đang mở rồi thử lại")?;
    Ok(())
}

fn validate_manifest(m: &Manifest) -> Result<()> {
    ensure!(
        m.format == "AIR3view settings" && m.version == 1,
        "Phiên bản file thiết lập chưa hỗ trợ; hãy cập nhật AIR3view"
    );
    ensure!(
        m.name.len() <= 1024 && m.assets.len() <= 64,
        "File thiết lập có quá nhiều tài nguyên hoặc tên quá dài"
    );
    ensure!(
        m.settings.effects.layers.is_empty()
            && m.settings.rules.required_ranges.is_empty()
            && m.settings.rules.excluded_ranges.is_empty(),
        "File chứa thiết lập gắn với cảnh nguồn, không phải preset dùng lại"
    );
    let mut ids = BTreeSet::new();
    let mut total = 0;
    for a in &m.assets {
        ensure!(
            a.sha256.len() == 64
                && a.sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "Mã kiểm tra tài nguyên không hợp lệ"
        );
        let ext =
            a.id.strip_prefix(&format!("{}.", a.sha256))
                .context("Tên tài nguyên không hợp lệ")?;
        ensure!(
            a.kind.extensions().contains(&ext) && ids.insert(a.id.clone()),
            "Tài nguyên trùng hoặc định dạng không hợp lệ"
        );
        ensure!(
            a.bytes > 0 && a.bytes <= a.kind.limit() && a.name.len() <= 255,
            "Kích thước/tên tài nguyên không hợp lệ"
        );
        total += a.bytes;
        ensure!(total <= MAX_TOTAL, "Tổng tài nguyên vượt 512 MB");
    }
    let mut used = BTreeSet::new();
    paths(&mut m.settings.clone(), |kind, value| {
        let id = value
            .strip_prefix(PREFIX)
            .context("File chứa đường dẫn máy cũ; cần xuất lại bằng Xuất thiết lập")?;
        let a = m
            .assets
            .iter()
            .find(|a| a.id == id)
            .context("File thiếu mô tả tài nguyên")?;
        ensure!(
            kind.extensions()
                .contains(&a.id.rsplit('.').next().unwrap_or("")),
            "Loại tài nguyên không khớp"
        );
        ensure!(
            a.bytes <= kind.limit(),
            "{} vượt giới hạn kích thước",
            kind.label()
        );
        used.insert(id.to_owned());
        Ok(value.to_owned())
    })?;
    ensure!(used == ids, "File chứa tài nguyên ngoài thiết lập");
    Ok(())
}

pub fn load(path: &Path, progress: &dyn Fn(&str)) -> Result<Loaded> {
    let mut file = File::open(path).context("Không mở được file thiết lập")?;
    ensure!(
        file.metadata()?.len() <= MAX_TOTAL + MAX_JSON + 20,
        "File thiết lập vượt giới hạn 516 MB"
    );
    let mut magic = [0; 12];
    file.read_exact(&mut magic)
        .context("File thiết lập bị thiếu dữ liệu")?;
    ensure!(
        &magic == MAGIC,
        "Đây không phải file .air3preset của AIR3view"
    );
    let mut size = [0; 8];
    file.read_exact(&mut size)?;
    let size = u64::from_le_bytes(size);
    ensure!(size <= MAX_JSON, "Phần cấu hình vượt 4 MB");
    let mut json = vec![0; size as usize];
    file.read_exact(&mut json)
        .context("File thiết lập bị cắt ngang")?;
    let manifest: Manifest =
        serde_json::from_slice(&json).context("Cấu trúc thiết lập không hợp lệ")?;
    validate_manifest(&manifest)?;
    validate_settings(&manifest.settings, false)?;
    let staging = tempfile::tempdir()?;
    for asset in &manifest.assets {
        if asset.included {
            progress(&format!("Kiểm tra {}: {}", asset.kind.label(), asset.name));
            let path = staging.path().join(&asset.id);
            let mut output = File::create(&path)?;
            let n = std::io::copy(&mut (&mut file).take(asset.bytes), &mut output)?;
            ensure!(n == asset.bytes, "File bị thiếu tài nguyên: {}", asset.name);
            ensure!(
                digest(&path, asset.kind.limit())?.1 == asset.sha256,
                "Tài nguyên bị hỏng: {}",
                asset.name
            );
        }
    }
    ensure!(file.read(&mut [0])? == 0, "File có dữ liệu dư không hợp lệ");
    let mut settings = manifest.settings.clone();
    paths(&mut settings, |_, value| {
        let id = value.strip_prefix(PREFIX).unwrap();
        Ok(staging.path().join(id).to_string_lossy().into_owned())
    })?;
    if manifest.assets.iter().all(|a| a.included) {
        validate_settings(&settings, true)?;
    }
    Ok(Loaded { manifest, staging })
}

fn validate_settings(settings: &ProjectDefaults, files: bool) -> Result<()> {
    let mut cfg = settings.clone();
    if !files {
        cfg.music.enabled = false;
        paths(&mut cfg, |kind, value| {
            Ok(if matches!(kind, AssetKind::Lut) {
                "off".into()
            } else {
                value.into()
            })
        })?;
    }
    let mut p = Project::default();
    cfg.apply(&mut p);
    p.validate_configuration()?;
    let _ = crate::export_metadata::prepare(&p, &Settings::default())?;
    ensure!(
        cfg.layer_defaults.presets.len() <= 256 && cfg.layer_defaults.effects.len() <= 256,
        "Quá nhiều mẫu layer"
    );
    for (key, params) in &cfg.layer_defaults.presets {
        validate_role(key)?;
        let t = &params.transform;
        ensure!(
            [
                t.x,
                t.y,
                t.width,
                t.height,
                t.rotation,
                t.opacity,
                params.gain
            ]
            .iter()
            .all(|v| v.is_finite())
                && t.x.abs() <= 100.
                && t.y.abs() <= 100.
                && t.width > 0.
                && t.width <= 20.
                && t.height > 0.
                && t.height <= 20.
                && (0.0..=1.).contains(&t.opacity)
                && (0.0..=2.).contains(&params.gain),
            "Thông số layer không hợp lệ"
        );
    }
    for (key, e) in &cfg.layer_defaults.effects {
        validate_role(key)?;
        e.validate()?;
    }
    if let Some(prod) = &cfg.production {
        ensure!(
            matches!(prod.avatar_provider.as_str(), "heygen" | "cartoon2d"),
            "Nhà cung cấp nhân vật chưa hỗ trợ"
        );
        ensure!(
            matches!(
                prod.cartoon_character.as_str(),
                "portrait" | "studio" | "analyst" | "panda"
            ),
            "Nhân vật 2D chưa có trên phiên bản này"
        );
        if files {
            if !prod.avatar_image_path.is_empty() {
                crate::input_assets::image_bytes(Path::new(&prod.avatar_image_path))?;
            }
            if !prod.voice_reference_path.is_empty() {
                ensure!(
                    Path::new(&prod.voice_reference_path).is_file(),
                    "Thiếu giọng tham chiếu"
                );
            }
        }
    }
    Ok(())
}
fn validate_role(key: &str) -> Result<()> {
    let fields: Vec<_> = key.split(':').collect();
    ensure!(
        fields.len() == 3
            && matches!(fields[0], "host" | "review" | "source" | "global")
            && fields[1]
                .parse::<usize>()
                .is_ok_and(|t| t < timeline::TRACK_COUNT)
            && (fields[2].is_empty() || fields[2].parse::<usize>().is_ok_and(|n| n < 64)),
        "Loại mẫu layer không hợp lệ"
    );
    Ok(())
}

impl Loaded {
    pub fn missing(&self) -> impl Iterator<Item = &Asset> {
        self.manifest.assets.iter().filter(|a| !a.included)
    }
    pub fn resolve(
        &self,
        data: &Path,
        replacements: &BTreeMap<String, PathBuf>,
        progress: &dyn Fn(&str),
    ) -> Result<ProjectDefaults> {
        let folder = data.join("assets/presets");
        fs::create_dir_all(&folder)?;
        let mut resolved = BTreeMap::new();
        for a in &self.manifest.assets {
            progress(&format!("Nhập {}: {}", a.kind.label(), a.name));
            let source = if a.included {
                self.staging.path().join(&a.id)
            } else {
                replacements
                    .get(&a.id)
                    .with_context(|| format!("Chọn lại {}: {}", a.kind.label(), a.name))?
                    .clone()
            };
            let (bytes, hash) = digest(&source, a.kind.limit())?;
            ensure!(
                bytes == a.bytes && hash == a.sha256,
                "{} không khớp bản gốc; hãy chọn đúng file: {}",
                a.kind.label(),
                a.name
            );
            let destination = folder.join(&a.id);
            if !destination.is_file() || digest(&destination, a.kind.limit())?.1 != a.sha256 {
                let mut pending = tempfile::NamedTempFile::new_in(&folder)?;
                std::io::copy(&mut File::open(&source)?, &mut pending)?;
                pending.as_file().sync_all()?;
                ensure!(
                    digest(pending.path(), a.kind.limit())?.1 == a.sha256,
                    "Tài nguyên thay đổi khi nhập; thử lại"
                );
                pending.persist(&destination).map_err(|e| e.error)?;
            }
            resolved.insert(a.id.clone(), destination.to_string_lossy().into_owned());
        }
        let mut cfg = self.manifest.settings.clone();
        paths(&mut cfg, |_, value| {
            resolved
                .get(
                    value
                        .strip_prefix(PREFIX)
                        .context("Đường dẫn tài nguyên không hợp lệ")?,
                )
                .cloned()
                .context("Thiếu tài nguyên")
        })?;
        validate_settings(&cfg, true)?;
        Ok(cfg)
    }
}

/// Build a candidate first. Preserve identity, source ranges and edited clip timing.
pub fn apply(settings: &ProjectDefaults, original: &Project) -> Result<Project> {
    validate_settings(settings, true)?;
    let mut next = original.clone();
    let ranges = (
        next.rules.required_ranges.clone(),
        next.rules.excluded_ranges.clone(),
    );
    settings.apply(&mut next);
    next.rules.required_ranges = ranges.0;
    next.rules.excluded_ranges = ranges.1;
    if serde_json::to_vec(&original.output.footage_speed)?
        != serde_json::to_vec(&next.output.footage_speed)?
    {
        crate::footage_speed::fit(&mut next)?;
    }
    next.ensure_composition();
    let automatic = crate::composition::Composition::from_project(&next);
    if let Some(mut comp) = next.composition.take() {
        for c in &mut comp.clips {
            let role = crate::layer_defaults::LayerDefaults::key(&next, c);
            if let Some(base) = automatic.clips.iter().find(|b| b.id == c.id).or_else(|| {
                role.as_ref().and_then(|key| {
                    automatic.clips.iter().find(|b| {
                        crate::layer_defaults::LayerDefaults::key(&next, b).as_ref() == Some(key)
                    })
                })
            }) {
                c.transform = base.transform.clone();
                c.gain = base.gain;
                c.z = base.z;
                c.fade_in_ms = base.fade_in_ms;
                c.fade_out_ms = base.fade_out_ms;
            }
        }
        next.composition = Some(comp);
    }
    next.effects.layers.clear();
    next.rendered = None;
    next.validate_configuration()?;
    Ok(next)
}

/// Persist both destinations as a transaction; failures restore previous JSON bytes.
pub fn commit(
    data: &Path,
    settings: &ProjectDefaults,
    project: Option<&Project>,
    remember: bool,
) -> Result<()> {
    let mut writes = Vec::new();
    if let Some(p) = project {
        writes.push((p.root.join("project.json"), serde_json::to_vec_pretty(p)?));
    }
    if remember {
        writes.push((
            data.join("project-defaults.json"),
            serde_json::to_vec_pretty(settings)?,
        ));
    }
    let mut before = Vec::new();
    for (path, _) in &writes {
        let original = match fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        before.push((path, original));
    }
    if before.iter().any(|(_, old)| old.is_some()) {
        let backup = data
            .join("preset-backups")
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&backup).context("Không tạo được bản sao lưu; chưa nhập thiết lập")?;
        for (path, old) in &before {
            if let Some(bytes) = old {
                fs::write(
                    backup.join(path.file_name().context("Thiếu tên file cấu hình")?),
                    bytes,
                )
                .context("Không ghi được bản sao lưu; chưa nhập thiết lập")?;
            }
        }
    }
    for (attempted, (path, bytes)) in writes.iter().enumerate() {
        let result = (|| -> Result<()> {
            fs::create_dir_all(path.parent().context("Thiếu thư mục cấu hình")?)?;
            let mut pending = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
            pending.write_all(bytes)?;
            pending.as_file().sync_all()?;
            pending.persist(path).map_err(|e| e.error)?;
            Ok(())
        })();
        if let Err(error) = result {
            let mut failures = Vec::new();
            for (target, old) in before[..attempted].iter().rev() {
                let restore = match old {
                    Some(b) => fs::write(target, b),
                    None => {
                        if target.exists() {
                            fs::remove_file(target)
                        } else {
                            Ok(())
                        }
                    }
                };
                if let Err(e) = restore {
                    failures.push(e.to_string());
                }
            }
            if !failures.is_empty() {
                return Err(error.context(format!(
                    "Không khôi phục đủ cấu hình: {}",
                    failures.join("; ")
                )));
            }
            return Err(error.context("Không lưu được thiết lập; đã giữ cấu hình trước khi nhập"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        composition::{Composition, Transform},
        layer_defaults::LayerParameters,
        visual_effects::Effect,
    };

    fn fixture(folder: &Path) -> Project {
        fs::create_dir_all(folder).unwrap();
        let image = folder.join("nhân vật.png");
        image::RgbaImage::from_pixel(64, 80, image::Rgba([90, 100, 150, 255]))
            .save(&image)
            .unwrap();
        let audio = folder.join("giọng mẫu.wav");
        fs::write(&audio, include_bytes!("../resources/sfx/whoosh.wav")).unwrap();
        let lut = folder.join("màu.cube");
        fs::write(
            &lut,
            "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n",
        )
        .unwrap();
        let mut p = Project {
            title: "Bộ thiết lập tiếng Anh".into(),
            root: folder.join("project"),
            ..Default::default()
        };
        p.rules.prompt = "Review in English; cite verified evidence.".into();
        p.rules.language = "en".into();
        p.rules.min_seconds = 180;
        p.rules.max_seconds = 240;
        p.output.portrait = false;
        p.output.fps = 60;
        p.output.blur_background = true;
        p.output.original_audio_enabled = true;
        p.output.original_audio_gain = 0.28;
        p.output.footage_speed.mode = "auto".into();
        p.output.footage_speed.preferred = 1.4;
        p.subtitles.offset_x = 125;
        p.subtitles.offset_y = -240;
        p.subtitles.highlight = [45, 90, 255];
        p.production = Some(ProductionSettings {
            avatar_enabled: true,
            avatar_provider: "heygen".into(),
            cartoon_character: "panda".into(),
            avatar_image_path: image.to_string_lossy().into_owned(),
            voice_reference_enabled: true,
            voice_reference_path: audio.to_string_lossy().into_owned(),
            ..Default::default()
        });
        p.music.enabled = true;
        p.music.file = audio;
        p.music.gain = 0.3;
        p.effects.footage = Effect {
            lut: lut.to_string_lossy().into_owned(),
            mirror_x: true,
            strength: 0.7,
            ..Default::default()
        };
        p.layer_defaults.initialized = true;
        p.layer_defaults.presets.insert(
            "host:1:".into(),
            LayerParameters {
                transform: Transform {
                    x: 0.7,
                    y: 0.6,
                    width: 0.5,
                    height: 0.8,
                    rotation: 4.,
                    opacity: 0.8,
                },
                z: 25,
                gain: 0.6,
                fade_in_ms: 120,
                fade_out_ms: 180,
            },
        );
        p.layer_defaults.effects.insert(
            "review:0:".into(),
            Effect {
                mirror_y: true,
                ..Default::default()
            },
        );
        p.timeline.tracks[timeline::ORIGINAL].muted = true;
        p.timeline.tracks[timeline::AVATAR].sync_parameters = true;
        p.evidence_settings.crop_percent = 8.;
        p.evidence_settings.sound_gain = 0.6;
        p.card_sound.gain = 0.42;
        p.export_metadata.title = "Portable title".into();
        p.export_metadata.author = "Test author".into();
        p
    }
    fn rewrite(source: &Path, destination: &Path, edit: impl FnOnce(&mut Manifest)) {
        let bytes = fs::read(source).unwrap();
        let size = u64::from_le_bytes(bytes[12..20].try_into().unwrap()) as usize;
        let mut m: Manifest = serde_json::from_slice(&bytes[20..20 + size]).unwrap();
        edit(&mut m);
        let json = serde_json::to_vec(&m).unwrap();
        let mut out = File::create(destination).unwrap();
        out.write_all(MAGIC).unwrap();
        out.write_all(&(json.len() as u64).to_le_bytes()).unwrap();
        out.write_all(&json).unwrap();
        out.write_all(&bytes[20 + size..]).unwrap();
    }
    #[test]
    fn portable_bundle_survives_original_assets_disappearing_and_new_machine_paths() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let files = tempfile::tempdir().unwrap();
        let p = fixture(source.path());
        let runtime = Settings {
            youtube_cookie_path: "PRIVATE_COOKIE".into(),
            gemini_key_env: "PRIVATE_ENV".into(),
            cli_path: "PRIVATE_EXE".into(),
            youtube_download_folder: "PRIVATE_DOWNLOAD".into(),
            ..Default::default()
        };
        let package = files.path().join("all.air3preset");
        export(&p, &runtime, &package, true, &|_| {}).unwrap();
        let raw = fs::read(&package).unwrap();
        let text = String::from_utf8_lossy(&raw);
        for secret in [
            "PRIVATE_COOKIE",
            "PRIVATE_ENV",
            "PRIVATE_EXE",
            "PRIVATE_DOWNLOAD",
            &p.id,
            &source.path().to_string_lossy(),
        ] {
            assert!(!text.contains(secret), "Leaked {secret}");
        }
        let loaded = load(&package, &|_| {}).unwrap();
        assert_eq!(
            loaded.manifest.assets.len(),
            3,
            "Deduplicate reference/music bytes"
        );
        drop(source);
        let cfg = loaded
            .resolve(target.path(), &BTreeMap::new(), &|_| {})
            .unwrap();
        let mut expected = ProjectDefaults::from_project(&p);
        expected.export_metadata = p.export_metadata.clone();
        paths(&mut expected, |kind, _| Ok(kind.label().into())).unwrap();
        let mut comparable = cfg.clone();
        paths(&mut comparable, |kind, value| {
            assert!(Path::new(value).is_file());
            assert!(Path::new(value).starts_with(target.path()));
            Ok(kind.label().into())
        })
        .unwrap();
        assert_eq!(
            serde_json::to_value(expected).unwrap(),
            serde_json::to_value(comparable).unwrap()
        );
        let mut fresh = apply(&cfg, &Project::default()).unwrap();
        fresh.root = target.path().join("projects").join(&fresh.id);
        fresh.validate_configuration().unwrap();
        assert!(!fresh.output.portrait && fresh.timeline.tracks[timeline::ORIGINAL].muted);
        assert_eq!(
            fresh.production.as_ref().unwrap().cartoon_character,
            "panda"
        );
        commit(target.path(), &cfg, Some(&fresh), true).unwrap();
        let mut restarted = Project::default();
        ProjectDefaults::load(&target.path().join("project-defaults.json"))
            .unwrap()
            .apply(&mut restarted);
        assert_eq!(restarted.subtitles.offset_x, 125);
        assert!(restarted.music.file.is_file());
        let defaults = ProjectDefaults::from_project(&restarted);
        let mut later = Project::default();
        defaults.apply(&mut later);
        assert!(later.timeline.tracks[timeline::AVATAR].sync_parameters);
        assert!(later.layer_defaults.effects["review:0:"].mirror_y);
    }
    #[test]
    fn settings_only_package_requires_matching_relinked_assets() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let p = fixture(source.path());
        let path = source.path().join("without.air3preset");
        export(&p, &Settings::default(), &path, false, &|_| {}).unwrap();
        let loaded = load(&path, &|_| {}).unwrap();
        assert_eq!(loaded.missing().count(), 3);
        assert!(loaded
            .resolve(target.path(), &BTreeMap::new(), &|_| {})
            .is_err());
        let mut replacements = BTreeMap::new();
        for asset in loaded.missing() {
            replacements.insert(asset.id.clone(), source.path().join(&asset.name));
        }
        let wrong = source.path().join("wrong.wav");
        fs::write(&wrong, b"wrong bytes").unwrap();
        let first = loaded.missing().next().unwrap();
        let original = replacements.insert(first.id.clone(), wrong).unwrap();
        assert!(loaded
            .resolve(target.path(), &replacements, &|_| {})
            .is_err());
        replacements.insert(first.id.clone(), original);
        assert!(loaded
            .resolve(target.path(), &replacements, &|_| {})
            .is_ok());
    }
    #[test]
    fn damaged_unsupported_and_path_injection_packages_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let p = fixture(temp.path());
        let path = temp.path().join("valid.air3preset");
        export(&p, &Settings::default(), &path, true, &|_| {}).unwrap();
        let bad = temp.path().join("bad.air3preset");
        rewrite(&path, &bad, |m| m.version = 99);
        assert!(load(&bad, &|_| {}).is_err());
        rewrite(&path, &bad, |m| m.assets[0].id = "../../outside.wav".into());
        assert!(load(&bad, &|_| {}).is_err());
        rewrite(&path, &bad, |m| {
            m.settings.production.as_mut().unwrap().avatar_image_path = "C:/private.png".into()
        });
        assert!(load(&bad, &|_| {}).is_err());
        rewrite(&path, &bad, |m| m.settings.output.fps = 0);
        assert!(load(&bad, &|_| {}).is_err());
        let mut bytes = fs::read(&path).unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        fs::write(&bad, &bytes).unwrap();
        assert!(load(&bad, &|_| {}).is_err());
        bytes.truncate(22);
        fs::write(&bad, &bytes).unwrap();
        assert!(load(&bad, &|_| {}).is_err());
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&(MAX_JSON + 1).to_le_bytes());
        fs::write(&bad, bytes).unwrap();
        assert!(load(&bad, &|_| {}).is_err());
    }
    #[test]
    fn export_failure_keeps_existing_destination_and_reports_missing_asset() {
        let temp = tempfile::tempdir().unwrap();
        let mut p = fixture(temp.path());
        let path = temp.path().join("keep.air3preset");
        fs::write(&path, b"previous preset").unwrap();
        p.music.file = temp.path().join("missing.wav");
        assert!(export(&p, &Settings::default(), &path, true, &|_| {}).is_err());
        assert_eq!(fs::read(path).unwrap(), b"previous preset");
    }

    #[test]
    fn built_in_2d_exports_without_the_stale_heygen_photo_for_bundled_and_settings_only_presets() {
        let temp=tempfile::tempdir().unwrap();let mut p=fixture(temp.path());
        let stale=temp.path().join("deleted-project/inputs/old-photo.jpg");
        let prod=p.production.as_mut().unwrap();prod.avatar_provider="cartoon2d".into();prod.cartoon_character="portrait".into();prod.avatar_image_path=stale.to_string_lossy().into_owned();
        let before=serde_json::to_vec(&p).unwrap();
        for include in [true,false] {
            let path=temp.path().join(format!("two-d-{include}.air3preset"));
            export(&p,&Settings::default(),&path,include,&|_|{}).unwrap();
            let loaded=load(&path,&|_|{}).unwrap();let prod=loaded.manifest.settings.production.as_ref().unwrap();
            assert_eq!(prod.avatar_provider,"cartoon2d");assert_eq!(prod.cartoon_character,"portrait");assert!(prod.avatar_image_path.is_empty());
            assert!(loaded.manifest.assets.iter().all(|a|!matches!(a.kind,AssetKind::Avatar)));
            assert_eq!(loaded.manifest.assets.len(),2,"Keep active reference/music and LUT");
            assert!(!String::from_utf8_lossy(&fs::read(&path).unwrap()).contains("old-photo.jpg"));
            if include {
                let data=tempfile::tempdir().unwrap();let cfg=loaded.resolve(data.path(),&BTreeMap::new(),&|_|{}).unwrap();
                assert!(cfg.production.as_ref().unwrap().voice_reference_enabled);assert!(cfg.music.file.is_file());
                assert!(apply(&cfg,&Project::default()).is_ok());
            }
        }
        assert_eq!(before,serde_json::to_vec(&p).unwrap(),"Export must not change the original project");
    }

    #[test]
    fn disabled_or_id_selected_inputs_do_not_require_unused_local_files() {
        let temp=tempfile::tempdir().unwrap();let mut p=fixture(temp.path());
        let prod=p.production.as_mut().unwrap();prod.avatar_id="account-avatar".into();prod.avatar_image_path="missing/photo.jpg".into();
        prod.voice_reference_enabled=false;prod.voice_reference_path="missing/reference.wav".into();
        p.music.enabled=false;p.music.file="missing/song.mp3".into();
        let path=temp.path().join("selected-id.air3preset");export(&p,&Settings::default(),&path,true,&|_|{}).unwrap();
        let loaded=load(&path,&|_|{}).unwrap();let cfg=&loaded.manifest.settings;let prod=cfg.production.as_ref().unwrap();
        assert_eq!(prod.avatar_id,"account-avatar");assert!(prod.avatar_image_path.is_empty()&&prod.voice_reference_path.is_empty());assert!(cfg.music.file.as_os_str().is_empty());
        assert_eq!(loaded.manifest.assets.len(),1,"Only the active LUT is needed");
        p.production.as_mut().unwrap().avatar_id.clear();
        assert!(export(&p,&Settings::default(),&path,true,&|_|{}).is_err(),"An active HeyGen photo must still be required");
        p.production.as_mut().unwrap().avatar_enabled=false;
        assert!(export(&p,&Settings::default(),&path,true,&|_|{}).is_ok());
        p.production.as_mut().unwrap().voice_reference_enabled=true;
        assert!(export(&p,&Settings::default(),&path,true,&|_|{}).is_err(),"Active reference audio must still be required");
    }
    #[test]
    fn import_preserves_scene_content_and_timing_but_maps_layer_styles_to_new_ids() {
        let mut p = Project {sources: vec![Source {
            id: "source-new".into(),
            width: 1920,
            height: 1080,
            ..Default::default()
        }],
        segments: vec![Segment {
            id: "new-scene".into(),
            kind: "host".into(),
            source_id: "source-new".into(),
            duration_ms: 6000,
            narration: "Keep my words".into(),
            audio: Some("generated.wav".into()),
            ..Default::default()
        }], ..Default::default()};
        p.rules.required_ranges = vec![TimeRange {
            source_id: "source-new".into(),
            start_ms: 1,
            end_ms: 5,
        }];
        p.ensure_composition();
        let clip = p
            .composition
            .as_mut()
            .unwrap()
            .clips
            .iter_mut()
            .find(|c| c.track == timeline::AVATAR)
            .unwrap();
        clip.start_ms = 123;
        clip.duration_ms = 4000;
        clip.timing_manual = true;
        let before = serde_json::to_vec(&p.segments).unwrap();
        let mut s = ProjectDefaults::from_project(&p);
        s.rules.prompt = "New instructions".into();
        s.layer_defaults.presets.insert(
            "host:1:".into(),
            LayerParameters {
                transform: Transform {
                    x: 0.72,
                    opacity: 0.61,
                    ..Default::default()
                },
                z: 24,
                gain: 0.8,
                fade_in_ms: 10,
                fade_out_ms: 20,
            },
        );
        s.layer_defaults.free_aspect = true;
        s.layer_defaults.effects.insert(
            "host:1:".into(),
            Effect {
                mirror_x: true,
                ..Default::default()
            },
        );
        let next = apply(&s, &p).unwrap();
        assert_eq!(before, serde_json::to_vec(&next.segments).unwrap());
        assert_eq!(next.id, p.id);
        assert_eq!(next.rules.required_ranges.len(), 1);
        let c = next
            .composition
            .as_ref()
            .unwrap()
            .clips
            .iter()
            .find(|c| c.track == timeline::AVATAR)
            .unwrap();
        assert_eq!((c.start_ms, c.duration_ms), (123, 4000));
        assert_eq!(c.transform.x, 0.72);
        assert!(crate::visual_effects::selected(&next, c).unwrap().mirror_x);
        let mut later = Project::default();
        ProjectDefaults::from_project(&next).apply(&mut later);
        later.segments = p.segments.clone();
        later.segments[0].id = "another-id".into();
        let comp = Composition::from_project(&later);
        let c = comp
            .clips
            .iter()
            .find(|c| c.track == timeline::AVATAR)
            .unwrap();
        assert_eq!(c.transform.x, 0.72);
        assert!(crate::visual_effects::selected(&later, c).unwrap().mirror_x);
    }
    #[cfg(windows)]
    #[test]
    fn failed_second_write_restores_project_and_does_not_change_defaults() {
        use std::os::windows::fs::OpenOptionsExt;
        let temp = tempfile::tempdir().unwrap();
        let p = Project {
            root: temp.path().join("projects/id"),
            ..Default::default()
        };
        fs::create_dir_all(&p.root).unwrap();
        let path = p.root.join("project.json");
        fs::write(&path, b"original project bytes").unwrap();
        let defaults = temp.path().join("project-defaults.json");
        fs::write(&defaults, b"original defaults bytes").unwrap();
        let _lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&defaults)
            .unwrap();
        assert!(commit(temp.path(), &ProjectDefaults::default(), Some(&p), true).is_err());
        assert_eq!(fs::read(path).unwrap(), b"original project bytes");
        assert_eq!(fs::read(defaults).unwrap(), b"original defaults bytes");
        assert!(fs::read_dir(temp.path().join("preset-backups"))
            .unwrap()
            .next()
            .is_some());
    }
}
