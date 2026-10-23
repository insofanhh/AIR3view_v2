//! Source-bound stills, cards and seek-safe motion. No provider calls here.
use crate::{
    composition::Transform,
    media,
    model::*,
    process::{self, Cancel},
};
use ab_glyph::{Font, FontArc, PxScale, ScaleFont};
use anyhow::{ensure, Context, Result};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct EvidenceSettings {
    pub enabled: bool,
    pub preset: String,
    pub background: String,
    pub density: usize,
    pub default_duration_ms: u64,
    /// Stack is a presentation policy, independent of source/proof selection.
    pub presentation: String,
    pub sound_enabled: bool,
    pub sound_gain: f32,
    /// Trim this percentage from each edge of the original frame, intersected with manual crop.
    pub crop_percent: f32,
}
impl Default for EvidenceSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            preset: "case_file".into(),
            background: "paper".into(),
            density: 3,
            default_duration_ms: 3500,
            presentation: "stack".into(),
            sound_enabled: true,
            sound_gain: 0.25,
            crop_percent: 0.,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Proof {
    pub kind: String,
    pub quote: String,
    pub formula: String,
    pub start_quote: String,
    pub end_quote: String,
    pub confirmed: bool,
}
impl Default for Proof {
    fn default() -> Self {
        Self {
            kind: "frame".into(),
            quote: String::new(),
            formula: String::new(),
            start_quote: String::new(),
            end_quote: String::new(),
            confirmed: false,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Annotation {
    pub kind: String,
    pub from: [f32; 2],
    pub to: [f32; 2],
    pub text: String,
}
impl Default for Annotation {
    fn default() -> Self {
        Self {
            kind: "circle".into(),
            from: [0.35, 0.3],
            to: [0.65, 0.7],
            text: String::new(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct EvidenceItem {
    pub id: String,
    pub kind: String,
    pub source_id: String,
    pub source_time_ms: u64,
    pub crop: [f32; 4],
    pub second_source_id: String,
    pub second_time_ms: Option<u64>,
    pub second_crop: [f32; 4],
    pub label: String,
    pub claim: String,
    pub value: String,
    pub unit: String,
    pub anchor_phrase: String,
    pub occurrence: usize,
    pub start_ms: u64,
    pub duration_ms: u64,
    pub slot: String,
    pub transition: String,
    pub zoom: bool,
    pub motion_in_ms: u64,
    pub motion_duration_ms: u64,
    pub proof: Proof,
    pub annotations: Vec<Annotation>,
    pub enabled: bool,
    pub timing_manual: bool,
    pub timing_status: String,
    pub asset: Option<PathBuf>,
    pub annotation_asset: Option<PathBuf>,
    pub asset_key: String,
}
impl Default for EvidenceItem {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "frame".into(),
            source_id: String::new(),
            source_time_ms: 0,
            crop: [0., 0., 1., 1.],
            second_source_id: String::new(),
            second_time_ms: None,
            second_crop: [0., 0., 1., 1.],
            label: "Dẫn chứng từ video".into(),
            claim: String::new(),
            value: String::new(),
            unit: String::new(),
            anchor_phrase: String::new(),
            occurrence: 0,
            start_ms: 0,
            duration_ms: 3500,
            slot: "auto".into(),
            transition: "slide_fade".into(),
            zoom: false,
            motion_in_ms: 0,
            motion_duration_ms: 0,
            proof: Proof::default(),
            annotations: vec![],
            enabled: true,
            timing_manual: false,
            timing_status: "estimated".into(),
            asset: None,
            annotation_asset: None,
            asset_key: String::new(),
        }
    }
}
pub fn time(ms: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ms / 3600000,
        ms / 60000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}
fn valid_crop(c: [f32; 4]) -> bool {
    c.iter().all(|v| v.is_finite() && *v >= 0. && *v <= 1.)
        && c[2] > 0.
        && c[3] > 0.
        && c[0] + c[2] <= 1.00001
        && c[1] + c[3] <= 1.00001
}
pub fn validate(p: &Project) -> Result<()> {
    ensure!(p.evidence_settings.crop_percent.is_finite() && (0.0..=30.0).contains(&p.evidence_settings.crop_percent),
        "Crop ảnh dẫn chứng phải từ 0 đến 30% mỗi cạnh");
    ensure!(matches!(p.evidence_settings.presentation.as_str(), "stack" | "classic")
        && p.evidence_settings.sound_gain.is_finite() && (0.0..=1.0).contains(&p.evidence_settings.sound_gain),
        "Thiết lập motion / âm thanh dẫn chứng không hợp lệ");
    ensure!(
        matches!(
            p.evidence_settings.preset.as_str(),
            "case_file" | "comparison" | "center"
        ),
        "Preset dẫn chứng không hợp lệ"
    );
    ensure!(
        matches!(
            p.evidence_settings.background.as_str(),
            "paper" | "dark" | "blur"
        ),
        "Nền dẫn chứng không hợp lệ"
    );
    ensure!(
        (1..=3).contains(&p.evidence_settings.density)
            && (500..=30000).contains(&p.evidence_settings.default_duration_ms),
        "Mật độ/thời gian dẫn chứng không hợp lệ"
    );
    if !p.evidence_settings.enabled {
        return Ok(());
    }
    for (si, seg) in p.segments.iter().enumerate() {
        ensure!(seg.evidence_items.len() <= 4, "Tối đa 4 dẫn chứng mỗi cảnh");
        let mut ids = std::collections::HashSet::new();
        for (ei, item) in seg.evidence_items.iter().enumerate().filter(|(_,e)| e.enabled) {
            let checked = (|| -> Result<()> {
            ensure!(
                !item.id.is_empty() && ids.insert(&item.id),
                "ID dẫn chứng trùng/rỗng"
            );
            ensure!(
                matches!(item.kind.as_str(), "frame" | "stat" | "fact" | "comparison"),
                "Loại dẫn chứng không hợp lệ"
            );
            ensure!(
                valid_crop(item.crop) && valid_crop(item.second_crop),
                "Vùng crop dẫn chứng ngoài khung"
            );
            effective_crop(item.crop,p.evidence_settings.crop_percent)?;
            if item.kind=="comparison" { effective_crop(item.second_crop,p.evidence_settings.crop_percent)?; }
            ensure!(
                (100..=60000).contains(&item.duration_ms),
                "Thời gian dẫn chứng không hợp lệ"
            );
            ensure!(
                matches!(item.transition.as_str(), "none" | "fade" | "slide_fade"),
                "Hiệu ứng dẫn chứng không hợp lệ"
            );
            ensure!(
                matches!(
                    item.slot.as_str(),
                    "auto" | "upper_left" | "upper_right" | "lower_left" | "lower_right"
                ),
                "Vị trí dẫn chứng không hợp lệ"
            );
            ensure!(
                item.label.chars().count() <= 180
                    && item.claim.chars().count() <= 800
                    && item.value.chars().count() <= 100
                    && item.proof.quote.chars().count() <= 800,
                "Nội dung thẻ quá dài"
            );
            let source = p.source(&item.source_id)?;
            validate_time(p, source, item.source_time_ms)?;
            if item.kind == "comparison" || item.proof.formula == "delta_ms" {
                let second = p.source(if item.second_source_id.is_empty() {
                    &item.source_id
                } else {
                    &item.second_source_id
                })?;
                validate_time(
                    p,
                    second,
                    item.second_time_ms.context("Thiếu timestamp thứ hai")?,
                )?;
                if item.proof.formula == "delta_ms" {
                    ensure!(
                        second.id == source.id,
                        "Chênh lệch thời gian phải cùng nguồn"
                    );
                }
            }
            if matches!(item.kind.as_str(), "stat" | "fact") {
                ensure!(
                    !item.value.trim().is_empty() || item.proof.formula == "delta_ms",
                    "Số liệu thiếu giá trị"
                );
                ensure!(
                    !item.proof.quote.trim().is_empty() || item.proof.formula == "delta_ms",
                    "Số liệu cần trích dẫn nguồn hoặc công thức"
                );
            }
            if item.proof.formula == "delta_ms" {
                ensure!(item.second_time_ms.is_some_and(|t|t>item.source_time_ms), "Mốc kết thúc phải sau mốc bắt đầu");
            }
            ensure!(
                matches!(item.proof.formula.as_str(), "" | "delta_ms"),
                "Công thức dẫn chứng chưa hỗ trợ"
            );
            ensure!(
                item.annotations.len() <= 4,
                "Tối đa 4 chú thích trên một ảnh"
            );
            for a in &item.annotations {
                ensure!(
                    matches!(a.kind.as_str(), "circle" | "arrow" | "underline" | "label")
                        && a.from
                            .iter()
                            .chain(a.to.iter())
                            .all(|v| v.is_finite() && (0.0..=1.).contains(v))
                        && a.text.chars().count() <= 80,
                    "Chú thích không hợp lệ"
                );
            }
            for path in [&item.asset, &item.annotation_asset].into_iter().flatten() {
                ensure!(
                    !path.is_absolute()
                        && path
                            .components()
                            .all(|c| matches!(c, std::path::Component::Normal(_))),
                    "Asset dẫn chứng phải nằm trong dự án"
                );
            }
            Ok(())
            })();
            checked.with_context(||format!("Cảnh {} / thẻ {} ({}), segments[{}].evidence_items[{}]",si+1,ei+1,item.label,si,ei))?;
        }
    }
    Ok(())
}
fn validate_time(p: &Project, s: &Source, t: u64) -> Result<()> {
    ensure!(
        s.duration_ms == 0 || t < s.duration_ms,
        "Dẫn chứng vượt thời lượng {}",
        s.id
    );
    ensure!(
        !p.rules
            .excluded_ranges
            .iter()
            .any(|r| r.source_id == s.id && r.start_ms <= t && t < r.end_ms),
        "Dẫn chứng nằm trong vùng đã loại"
    );
    Ok(())
}
fn token(t: &str) -> String {
    t.trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}
pub fn align(p: &mut Project) {
    for seg in &mut p.segments {
        for item in &mut seg.evidence_items {
            if !item.timing_manual && !item.anchor_phrase.trim().is_empty() {
                let needle: Vec<_> = item.anchor_phrase.split_whitespace().map(token).collect();
                let hay: Vec<_> = seg.words.iter().map(|w| token(&w.text)).collect();
                let hits: Vec<_> = if needle.is_empty() {
                    vec![]
                } else {
                    hay.windows(needle.len())
                        .enumerate()
                        .filter(|(_, w)| *w == needle)
                        .map(|(i, _)| i)
                        .collect()
                };
                if !seg.timing_estimated && (hits.len() == 1 || item.occurrence > 0) {
                    if let Some(i) = hits.get(item.occurrence.saturating_sub(1)) {
                        item.start_ms = seg.words[*i].start_ms;
                        item.timing_status = "aligned".into();
                    } else {
                        item.timing_status = "not_found".into();
                    }
                } else {
                    item.timing_status = if seg.timing_estimated {
                        "estimated"
                    } else if hits.len() > 1 {
                        "ambiguous"
                    } else {
                        "not_found"
                    }
                    .into();
                }
            } else if item.timing_manual {
                item.timing_status = "manual".into();
            }
            item.start_ms = item.start_ms.min(seg.duration_ms.saturating_sub(100));
        }
    }
}
pub fn window(seg: &Segment, item: &EvidenceItem) -> (u64, u64) {
    let start = item.start_ms.min(seg.duration_ms.saturating_sub(100));
    (start, item.duration_ms.min(seg.duration_ms - start).max(1))
}
pub fn content_identity(item: &EvidenceItem) -> Vec<u8> {
    let mut v = item.clone();
    v.asset = None;
    v.annotation_asset = None;
    v.asset_key.clear();
    v.start_ms = 0;
    v.duration_ms = 0;
    v.anchor_phrase.clear();
    v.occurrence = 0;
    v.timing_status.clear();
    v.slot.clear();
    v.transition.clear();
    v.timing_manual = false;
    v.zoom = false;
    v.motion_in_ms = 0;
    v.motion_duration_ms = 0;
    serde_json::to_vec(&v).unwrap_or_default()
}
pub fn layout(p: &Project, index: usize, count: usize, slot: &str) -> Transform {
    let portrait = p.output.portrait;
    let preset = p.evidence_settings.preset.as_str();
    if slot != "auto" {
        let right = slot.ends_with("right");
        let lower = slot.starts_with("lower");
        let (x, y, width) = if portrait {
            (
                if right { 0.73 } else { 0.27 },
                if lower { 0.48 } else { 0.24 },
                0.44,
            )
        } else {
            (
                if right { 0.80 } else { 0.20 },
                if lower { 0.57 } else { 0.23 },
                0.24,
            )
        };
        let (w, h) = p.output.dimensions();
        return Transform {
            x,
            y,
            width,
            height: width * w as f32 / h as f32 * 0.75,
            ..Default::default()
        };
    }
    let i = index;
    let (x, y, width) = if portrait {
        if count == 1 {
            (0.5, 0.25, 0.84)
        } else if preset == "case_file" && i == 0 {
            (0.5, 0.235, 0.84)
        } else if preset == "case_file" {
            (if i % 2 == 1 { 0.27 } else { 0.73 }, 0.515, 0.43)
        } else {
            (
                if i % 2 == 0 { 0.27 } else { 0.73 },
                if i < 2 { 0.22 } else { 0.44 },
                0.44,
            )
        }
    } else if preset == "center" {
        (
            if i % 2 == 0 { 0.20 } else { 0.80 },
            if i < 2 { 0.23 } else { 0.57 },
            0.24,
        )
    } else {
        (
            if i % 2 == 0 { 0.20 } else { 0.80 },
            if i < 2 { 0.24 } else { 0.59 },
            if i < 2 { 0.28 } else { 0.22 },
        )
    };
    let (w, h) = p.output.dimensions();
    Transform {
        x,
        y,
        width,
        height: width * w as f32 / h as f32 * 0.75,
        ..Default::default()
    }
}
/// A large readable focal card; later cards overlap it instead of shrinking into corners.
pub fn stack_layout(p: &Project, index: usize) -> Transform {
    let (w,h)=p.output.dimensions();
    let (x,y,width)=if p.output.portrait {(0.5,0.39,0.86)}else{(0.415,0.505,0.46)};
    let offset=if index==0 {0.}else if index%2==1 {1.}else{-1.};
    Transform{x:x+offset*0.035,y:y+offset.abs()*if p.output.portrait{0.11}else{0.028},width,
        height:width*w as f32/h as f32*0.75,rotation:offset*1.2,..Default::default()}
}
/// Early reveals use the beginning of a host scene, retaining explicit manual cues.
/// Each result is (cue, duration, next-card handoff relative to cue).
pub fn stack_windows(seg:&Segment,items:&[&EvidenceItem],density:usize)->Vec<(u64,u64,Option<u64>)> {
    let count=items.len();if count==0{return vec![];}
    let step=(seg.duration_ms.saturating_sub(500)/count as u64).clamp(100,2300);
    let first=120.min(seg.duration_ms/10);let mut previous=0;
    let starts:Vec<_>=items.iter().enumerate().map(|(i,item)|{
        let cue=if item.timing_manual {window(seg,item).0}else {
            let early=first+i as u64*step;
            let lead=item.start_ms.saturating_sub(650);
            early.min(lead.max(first)).max(if i==0 {first}else{previous+step.min(1600)})
                .min(seg.duration_ms.saturating_sub(100))
        };previous=cue;cue
    }).collect();
    items.iter().enumerate().map(|(i,item)|{
        let cue=starts[i];if item.timing_manual{return (cue,window(seg,item).1,None);}
        let stop=starts.get(i+density.max(1)).copied().unwrap_or(seg.duration_ms);
        let duration=stop.saturating_sub(cue).max(1);
        let handoff=starts.iter().enumerate().filter(|(j,t)|*j>i&&**t>cue&&**t<cue+duration)
            .map(|(_,t)|*t-cue).min();
        (cue,duration,handoff)
    }).collect()
}
pub fn host_layout(p: &Project, aspect: f32) -> Transform {
    let mut width = if p.output.portrait {
        0.54
    } else if p.evidence_settings.preset == "center" {
        0.30
    } else {
        0.24
    };
    let (w, h) = p.output.dimensions();
    let mut height = width * w as f32 / h as f32 * aspect;
    let max_height = if p.output.portrait { 0.36 } else { 0.60 };
    if height > max_height {
        width *= max_height / height;
        height = max_height;
    }
    Transform {
        x: if !p.output.portrait && p.evidence_settings.presentation=="stack" {0.855}else{0.5},
        y: if p.output.portrait {
            0.77
        } else if p.evidence_settings.presentation=="stack" {0.56}else{0.73 - height.min(0.6) / 2.},
        width,
        height,
        ..Default::default()
    }
}
/// Explicit reset only: preserve hand edits until the user requests a reflow.
pub fn reflow(p: &mut Project) {
    p.ensure_composition();
    let defaults = crate::composition::Composition::from_project(p);
    let editable: Vec<_> = p
        .composition
        .as_ref()
        .unwrap()
        .clips
        .iter()
        .filter(|c| c.editable(p) && c.group.as_ref().is_none_or(|g|p.composition.as_ref().unwrap().clips.iter().filter(|v|v.group.as_ref()==Some(g)).all(|v|v.editable(p))))
        .map(|c| c.id.clone())
        .collect();
    for c in &mut p.composition.as_mut().unwrap().clips {
        if editable.contains(&c.id)
            && matches!(
                c.track,
                crate::timeline::AVATAR
                    | crate::timeline::BACKGROUND
                    | crate::timeline::EVIDENCE
                    | crate::timeline::ANNOTATION
                    | crate::timeline::SFX
            )
        {
            if let Some(base) = defaults.clips.iter().find(|b| b.id == c.id) {
                c.transform = base.transform.clone();
                c.z = base.z;
                if c.track==crate::timeline::SFX {c.gain=base.gain;}
                if c.evidence_id.is_some()&&!c.timing_manual {
                    c.start_ms=base.start_ms;c.duration_ms=base.duration_ms;c.cue_in_ms=base.cue_in_ms;
                    c.motion=base.motion.clone();
                }
            }
        }
    }
    p.rendered = None;
}
pub fn background_path(p: &Project, seg: &Segment) -> PathBuf {
    p.root.join("assets/evidence").join(format!(
        "background-{}.png",
        media::hash(
            serde_json::to_vec(&(
                &p.evidence_settings.background,
                p.output.portrait,
                seg.evidence_items.first().map(|e| &e.asset_key)
            ))
            .unwrap_or_default()
        )
    ))
}
fn prepare_backgrounds(p: &Project, s: &Settings, cancel: &Cancel) -> Result<()> {
    for seg in p.segments.iter().filter(|s| {
        s.kind == "host"
            && s.evidence_items
                .iter()
                .any(|e| e.enabled && e.asset.is_some())
    }) {
        let path = background_path(p, seg);
        if path.is_file() {
            continue;
        }
        let (w, h) = if p.output.portrait {
            (720, 1280)
        } else {
            (1280, 720)
        };
        let light = p.evidence_settings.background == "paper";
        let mut img = RgbaImage::from_pixel(
            w,
            h,
            Rgba(if light {
                [246, 242, 233, 255]
            } else {
                [10, 16, 29, 255]
            }),
        );
        if light {
            for y in (12..h).step_by(24) {
                for x in (12..w).step_by(24) {
                    img.put_pixel(x, y, Rgba([218, 214, 205, 255]));
                }
            }
        }
        if p.evidence_settings.background == "blur" {
            if let Some(item) = seg.evidence_items.iter().find(|e| e.enabled) {
                if item.asset.is_some() {
                    let path = source_frame(
                        p,
                        s,
                        p.source(&item.source_id)?,
                        item.source_time_ms,
                        cancel,
                    )?;
                    let source = image::open(path)?.to_rgba8();
                    img = image::imageops::resize(
                        &source,
                        w,
                        h,
                        image::imageops::FilterType::Triangle,
                    );
                    img = image::imageops::blur(&img, 30.);
                    for pixel in img.pixels_mut() {
                        for i in 0..3 {
                            pixel[i] = (pixel[i] as f32 * 0.65) as u8;
                        }
                    }
                }
            }
        }
        atomic_png(&path, &img)?;
    }
    Ok(())
}
pub fn prepare(
    p: &mut Project,
    s: &Settings,
    cancel: &Cancel,
    mut progress: impl FnMut(&str),
) -> Result<()> {
    validate(p)?;
    if !p.evidence_settings.enabled {
        return Ok(());
    }
    let mut records = vec![];
    for si in 0..p.segments.len() {
        for ei in 0..p.segments[si].evidence_items.len() {
            process::check_cancel(cancel)?;
            let mut item = p.segments[si].evidence_items[ei].clone();
            if item.label == "Dẫn chứng từ video" && crate::content_language::english(&p.rules.language) {
                item.label = "Evidence from source video".into();
            }
            if !item.enabled {
                continue;
            }
            progress(&format!("Trích dẫn chứng {} / {}", si + 1, ei + 1));
            let source = p.source(&item.source_id)?;
            let raw = source_frame(p, s, source, item.source_time_ms, cancel)?;
            let effective=effective_crop(item.crop,p.evidence_settings.crop_percent)?;
            let first = crop_image(&image::open(&raw)?.to_rgba8(), effective);
            let second = if item.kind == "comparison" {
                let source = p.source(if item.second_source_id.is_empty() {
                    &item.source_id
                } else {
                    &item.second_source_id
                })?;
                let raw = source_frame(p, s, source, item.second_time_ms.unwrap(), cancel)?;
                Some(crop_image(&image::open(raw)?.to_rgba8(), effective_crop(item.second_crop,p.evidence_settings.crop_percent)?))
            } else {
                None
            };
            if item.proof.formula == "delta_ms" {
                item.value = format!(
                    "{:.3}",
                    item.second_time_ms.unwrap().abs_diff(item.source_time_ms) as f64 / 1000.
                );
                item.unit = crate::content_language::text(&p.rules.language,"giây trên video","seconds in source video").into();
            }
            let mut identity = item.clone();
            // IDs identify timeline instances, not rendered pixels. A script
            // repair may assign new IDs to an otherwise identical card.
            identity.id.clear();
            identity.asset = None;
            identity.annotation_asset = None;
            identity.asset_key.clear();
            identity.start_ms = 0;
            identity.duration_ms = 0;
            identity.anchor_phrase.clear();
            identity.occurrence = 0;
            identity.timing_status.clear();
            identity.slot.clear();
            identity.transition.clear();
            identity.timing_manual = false;
            identity.zoom = false;
            identity.motion_in_ms = 0;
            identity.motion_duration_ms = 0;
            let key = media::hash(
                serde_json::to_vec(&(
                    &identity,
                    crate::input_assets::file_fingerprint(&raw.to_string_lossy()),
                    if item.kind == "comparison" {
                        crate::input_assets::file_fingerprint(
                            &p.source(if item.second_source_id.is_empty() {
                                &item.source_id
                            } else {
                                &item.second_source_id
                            })?
                            .path
                            .to_string_lossy(),
                        )
                    } else {
                        String::new()
                    },
                    p.evidence_settings.crop_percent,
                    "evidence-card-v6-content",
                    &p.rules.language,
                ))
                .unwrap_or_default(),
            );
            let relative = PathBuf::from("assets/evidence").join(format!("{key}.png"));
            let ann_relative =
                PathBuf::from("assets/evidence").join(format!("{key}.annotations.png"));
            let dest = p.root.join(&relative);
            if !dest.is_file() || !p.root.join(&ann_relative).is_file() {
                let mut drawn=item.clone();
                // Annotation coordinates belong to the manual crop. Remap to the
                // intersected frame; omit out-of-view marks rather than moving them onto unrelated pixels.
                drawn.annotations=remap_annotations(&item.annotations,item.crop,effective);
                let (card, ann) = card(&drawn, &first, second.as_ref(), &p.rules.language)?;
                fs::create_dir_all(dest.parent().unwrap())?;
                atomic_png(&dest, &card)?;
                atomic_png(&p.root.join(&ann_relative), &ann)?;
            }
            item.asset = Some(relative);
            item.annotation_asset = if item.annotations.is_empty() {
                None
            } else {
                Some(ann_relative)
            };
            item.asset_key = key;
            records.push(serde_json::json!({"id":item.id,"source_id":item.source_id,"timestamp_ms":item.source_time_ms,"crop":item.crop,"effective_crop":effective,"crop_percent":p.evidence_settings.crop_percent,"second_timestamp_ms":item.second_time_ms,"proof":item.proof,"asset":item.asset}));
            p.segments[si].evidence_items[ei] = item;
        }
    }
    write_json(&p.root.join("assets/evidence/sources.json"), &records)?;
    prepare_backgrounds(p, s, cancel)?;
    align(p);
    Ok(())
}
/// Gate completion on the final script, not the worker's earlier snapshot.
/// Explicitly hidden/deleted editor layers remain valid user edits.
pub fn validate_prepared(p: &Project) -> Result<()> {
    if !p.evidence_settings.enabled { return Ok(()); }
    validate(p)?;
    for (si,seg) in p.segments.iter().enumerate() {
        for item in seg.evidence_items.iter().filter(|e|e.enabled) {
            let check=(|| -> Result<()> {
                let asset=item.asset.as_ref().context("Chưa gắn file ảnh vào dẫn chứng")?;
                image::image_dimensions(p.root.join(asset)).context("File ảnh dẫn chứng thiếu hoặc không đọc được")?;
                if !item.annotations.is_empty() {
                    let asset=item.annotation_asset.as_ref().context("Chưa gắn ảnh chú thích")?;
                    image::image_dimensions(p.root.join(asset)).context("File ảnh chú thích thiếu hoặc không đọc được")?;
                }
                Ok(())
            })();
            check.with_context(||format!("Cảnh {} / dẫn chứng {}",si+1,item.label))?;
        }
        if seg.kind=="host" && seg.evidence_items.iter().any(|e|e.enabled) {
            image::image_dimensions(background_path(p,seg)).with_context(||format!("Cảnh {} thiếu nền dẫn chứng đúng khung hình",si+1))?;
        }
    }
    let expected=crate::composition::Composition::from_project(p);
    let actual=p.composition_snapshot();
    for layer in expected.clips.iter().filter(|c|matches!(c.track,crate::timeline::EVIDENCE|crate::timeline::ANNOTATION)) {
        ensure!(actual.removed.contains(&layer.id) || actual.clips.iter().any(|c|c.id==layer.id&&c.media==layer.media),
            "Dẫn chứng chưa được gắn vào timeline của cảnh {}",layer.scene_id);
    }
    Ok(())
}
fn atomic_png(path: &Path, img: &RgbaImage) -> Result<()> {
    let pending = path.with_extension("pending.png");
    img.save(&pending)?;
    fs::rename(pending, path)?;
    Ok(())
}
pub fn source_frame(
    p: &Project,
    s: &Settings,
    source: &Source,
    ms: u64,
    cancel: &Cancel,
) -> Result<PathBuf> {
    let key = media::hash(format!(
        "frame-v1:{}:{ms}",
        crate::input_assets::file_fingerprint(&source.path.to_string_lossy())
    ));
    let path = p
        .root
        .join("assets/evidence/frames")
        .join(format!("{key}.png"));
    if path.is_file() {
        return Ok(path);
    }
    fs::create_dir_all(path.parent().unwrap())?;
    let pending = path.with_extension("pending.png");
    process::run(
        &s.ffmpeg,
        &process::args(&[
            "-y",
            "-v",
            "error",
            "-ss",
            &format!("{:.6}", ms as f64 / 1000.),
            "-i",
            &source.path.to_string_lossy(),
            "-vf",
            "scale=1920:1080:force_original_aspect_ratio=decrease",
            "-frames:v",
            "1",
            &pending.to_string_lossy(),
        ]),
        None,
        cancel,
        Duration::from_secs(s.timeout_seconds),
        None,
    )?;
    ensure!(
        pending.is_file(),
        "Không lấy được khung hình tại {}",
        time(ms)
    );
    fs::rename(pending, &path)?;
    Ok(path)
}
pub fn effective_crop(c:[f32;4],percent:f32)->Result<[f32;4]> {
    ensure!(valid_crop(c) && percent.is_finite() && (0.0..=30.0).contains(&percent),"Crop ảnh dẫn chứng không hợp lệ");
    let edge=percent/100.;let x=c[0].max(edge);let y=c[1].max(edge);
    let right=(c[0]+c[2]).min(1.-edge);let bottom=(c[1]+c[3]).min(1.-edge);
    ensure!(right-x>=0.005 && bottom-y>=0.005,"Crop % đã loại hết vùng dẫn chứng đang chọn; giảm % hoặc mở rộng vùng crop của thẻ");
    Ok([x,y,right-x,bottom-y])
}
fn remap_annotations(items:&[Annotation],old:[f32;4],crop:[f32;4])->Vec<Annotation> {
    items.iter().filter_map(|a|{
        let point=|p:[f32;2]|[(old[0]+p[0]*old[2]-crop[0])/crop[2],(old[1]+p[1]*old[3]-crop[1])/crop[3]];
        let mut a=a.clone();a.from=point(a.from);a.to=point(a.to);
        a.from.iter().chain(a.to.iter()).all(|v|(0.0..=1.0).contains(v)).then_some(a)
    }).collect()
}
#[cfg(test)]
mod crop_tests {
    use super::*;
    #[test]
    fn global_crop_preserves_source_positions_of_annotations() {
        let visible=Annotation{from:[0.25,0.3],to:[0.75,0.7],..Default::default()};
        let hidden=Annotation{from:[0.,0.],to:[0.1,0.1],..Default::default()};
        let crop=effective_crop([0.,0.,1.,1.],10.).unwrap();
        let remapped=remap_annotations(&[visible.clone(),hidden],[0.,0.,1.,1.],crop);
        assert_eq!(remapped.len(),1);
        for i in 0..2 {
            assert!((crop[i]+remapped[0].from[i]*crop[i+2]-visible.from[i]).abs()<0.00001);
            assert!((crop[i]+remapped[0].to[i]*crop[i+2]-visible.to[i]).abs()<0.00001);
        }
    }
}
fn crop_image(img: &RgbaImage, c: [f32; 4]) -> RgbaImage {
    let (w, h) = img.dimensions();
    let x = (c[0] * w as f32).floor().min((w - 1) as f32) as u32;
    let y = (c[1] * h as f32).floor().min((h - 1) as f32) as u32;
    image::imageops::crop_imm(
        img,
        x,
        y,
        ((c[2] * w as f32).round() as u32).max(1).min(w - x),
        ((c[3] * h as f32).round() as u32).max(1).min(h - y),
    )
    .to_image()
}
fn font() -> Result<FontArc> {
    static FONT: std::sync::OnceLock<FontArc> = std::sync::OnceLock::new();
    if let Some(f) = FONT.get() {
        return Ok(f.clone());
    }
    let candidates = [
        PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:/Windows".into()))
            .join("Fonts/segoeui.ttf"),
        PathBuf::from("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
    ];
    let data = candidates
        .iter()
        .find_map(|p| fs::read(p).ok())
        .unwrap_or_else(|| {
            eframe::egui::FontDefinitions::default()
                .font_data
                .values()
                .next()
                .unwrap()
                .font
                .to_vec()
        });
    let f = FontArc::try_from_vec(data)
        .map_err(|_| anyhow::anyhow!("Font thẻ dẫn chứng không hợp lệ"))?;
    let _ = FONT.set(f.clone());
    Ok(f)
}
fn blend(img: &mut RgbaImage, x: i32, y: i32, color: [u8; 4]) {
    if x < 0 || y < 0 || x >= img.width() as i32 || y >= img.height() as i32 {
        return;
    }
    let old = img.get_pixel_mut(x as u32, y as u32);
    let a = color[3] as f32 / 255.;
    let b = old[3] as f32 / 255.;
    let out = a + b * (1. - a);
    if out > 0. {
        for i in 0..3 {
            old[i] = ((color[i] as f32 * a + old[i] as f32 * b * (1. - a)) / out).round() as u8;
        }
    }
    old[3] = (out * 255.).round() as u8;
}
#[allow(clippy::too_many_arguments)]
fn text(
    img: &mut RgbaImage,
    f: &FontArc,
    value: &str,
    x: f32,
    y: f32,
    width: f32,
    size: f32,
    lines: usize,
    color: [u8; 4],
) {
    let scaled = f.as_scaled(PxScale::from(size));
    let mut px = x;
    let mut py = y + scaled.ascent();
    let mut line = 0;
    for word in value.split_whitespace() {
        let word_w: f32 = word.chars().map(|c| scaled.h_advance(f.glyph_id(c))).sum();
        if px > x && px + word_w > x + width {
            line += 1;
            px = x;
            py += size * 1.25;
        }
        if line >= lines {
            break;
        }
        for c in word.chars() {
            let glyph = f
                .glyph_id(c)
                .with_scale_and_position(size, ab_glyph::point(px, py));
            if let Some(outline) = f.outline_glyph(glyph) {
                let b = outline.px_bounds();
                outline.draw(|gx, gy, a| {
                    let mut col = color;
                    col[3] = (a * color[3] as f32) as u8;
                    blend(
                        img,
                        b.min.x as i32 + gx as i32,
                        b.min.y as i32 + gy as i32,
                        col,
                    );
                });
            }
            px += scaled.h_advance(f.glyph_id(c));
        }
        px += scaled.h_advance(f.glyph_id(' '));
    }
}
fn paste(img: &mut RgbaImage, src: &RgbaImage, rect: [u32; 4]) -> [f32; 4] {
    let [x, y, w, h] = rect;
    let scale = (w as f32 / src.width() as f32).min(h as f32 / src.height() as f32);
    let (nw, nh) = (
        (src.width() as f32 * scale).round().max(1.) as u32,
        (src.height() as f32 * scale).round().max(1.) as u32,
    );
    let x = x + (w - nw) / 2;
    let y = y + (h - nh) / 2;
    let resized = image::imageops::resize(src, nw, nh, image::imageops::FilterType::Triangle);
    image::imageops::overlay(img, &resized, x as i64, y as i64);
    [x as f32, y as f32, nw as f32, nh as f32]
}
fn line(img: &mut RgbaImage, a: [f32; 2], b: [f32; 2], color: [u8; 4], thick: i32) {
    let n = ((b[0] - a[0]).abs().max((b[1] - a[1]).abs()).ceil() as usize).max(1);
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let x = (a[0] + (b[0] - a[0]) * t) as i32;
        let y = (a[1] + (b[1] - a[1]) * t) as i32;
        for dy in -thick..=thick {
            for dx in -thick..=thick {
                if dx * dx + dy * dy <= thick * thick {
                    blend(img, x + dx, y + dy, color);
                }
            }
        }
    }
}
#[cfg(test)]
mod language_tests {
    use super::*;
    #[test]
    fn english_card_displays_translated_claim_and_keeps_raw_proof_internal() {
        let mut item=EvidenceItem{kind:"fact".into(),label:"The driver speaks".into(),claim:"The driver asks what happened.".into(),
            value:"One statement".into(),source_id:"s".into(),proof:Proof{quote:"Tôi không biết chuyện gì vừa xảy ra".into(),..Default::default()},..Default::default()};
        let frame=RgbaImage::from_pixel(160,90,Rgba([20,40,60,255]));
        let english=card(&item,&frame,None,"en").unwrap().0;
        let vietnamese=card(&item,&frame,None,"vi").unwrap().0;
        assert_ne!(english.as_raw(),vietnamese.as_raw());
        item.proof.quote="Một trích dẫn gốc khác không phải để hiển thị".into();
        assert_eq!(english.as_raw(),card(&item,&frame,None,"en").unwrap().0.as_raw());
        assert!(item.proof.quote.contains("trích dẫn gốc"));
    }
}
fn card(
    item: &EvidenceItem,
    first: &RgbaImage,
    second: Option<&RgbaImage>,
    language: &str,
) -> Result<(RgbaImage, RgbaImage)> {
    let mut img = RgbaImage::new(800, 600);
    let mut ann = RgbaImage::new(800, 600);
    let f = font()?;
    for y in 0u32..600 {
        for x in 0u32..800 {
            let dx = if x < 24 {
                24 - x
            } else {
                x.saturating_sub(775)
            } as f32;
            let dy = if y < 24 {
                24 - y
            } else {
                y.saturating_sub(575)
            } as f32;
            if dx * dx + dy * dy <= 24. * 24. {
                img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
    }
    text(
        &mut img,
        &f,
        &item.label,
        24.,
        18.,
        750.,
        34.,
        2,
        [24, 30, 36, 255],
    );
    let rect = if matches!(item.kind.as_str(), "stat" | "fact") {
        text(
            &mut img,
            &f,
            &format!("{} {}", item.value, item.unit),
            28.,
            90.,
            740.,
            46.,
            2,
            [227, 81, 50, 255],
        );
        text(
            &mut img,
            &f,
            if crate::content_language::english(language) { &item.claim } else { &item.proof.quote },
            28.,
            212.,
            740.,
            28.,
            2,
            [50, 60, 70, 255],
        );
        paste(&mut img, first, [24, 292, 752, 206])
    } else if let Some(second) = second {
        let rect = paste(&mut img, first, [24, 95, 365, 410]);
        paste(&mut img, second, [411, 95, 365, 410]);
        text(
            &mut img,
            &f,
            &format!("A · {}", time(item.source_time_ms)),
            28.,
            512.,
            360.,
            20.,
            1,
            [60, 70, 80, 255],
        );
        text(
            &mut img,
            &f,
            &format!("B · {}", time(item.second_time_ms.unwrap())),
            412.,
            512.,
            360.,
            20.,
            1,
            [60, 70, 80, 255],
        );
        rect
    } else {
        paste(&mut img, first, [24, 90, 752, 445])
    };
    text(
        &mut img,
        &f,
        &format!("{} · {}", item.source_id, time(item.source_time_ms)),
        24.,
        560.,
        750.,
        20.,
        1,
        [80, 90, 100, 255],
    );
    if matches!(item.kind.as_str(), "stat" | "fact") && !item.proof.confirmed {
        text(
            &mut img,
            &f,
            crate::content_language::text(language,"CẦN ĐỐI CHIẾU","VERIFY WITH SOURCE"),
            28.,
            510.,
            540.,
            22.,
            1,
            [180, 70, 25, 255],
        );
    }
    for a in &item.annotations {
        let point = |v: [f32; 2]| [rect[0] + v[0] * rect[2], rect[1] + v[1] * rect[3]];
        let from = point(a.from);
        let to = point(a.to);
        let c = [232, 86, 51, 255];
        match a.kind.as_str() {
            "circle" => {
                let cx = (from[0] + to[0]) / 2.;
                let cy = (from[1] + to[1]) / 2.;
                let rx = (to[0] - from[0]).abs() / 2.;
                let ry = (to[1] - from[1]).abs() / 2.;
                let mut last = [cx + rx, cy];
                for i in 1..=180 {
                    let angle = i as f32 * std::f32::consts::TAU / 180.;
                    let next = [cx + rx * angle.cos(), cy + ry * angle.sin()];
                    line(&mut ann, last, next, c, 3);
                    last = next;
                }
            }
            "arrow" => {
                line(&mut ann, from, to, c, 3);
                let angle = (to[1] - from[1]).atan2(to[0] - from[0]);
                for d in [-0.5f32, 0.5] {
                    line(
                        &mut ann,
                        to,
                        [
                            to[0] - 24. * (angle + d).cos(),
                            to[1] - 24. * (angle + d).sin(),
                        ],
                        c,
                        3,
                    );
                }
            }
            "underline" => line(&mut ann, from, to, c, 3),
            _ => text(
                &mut ann,
                &f,
                &a.text,
                from[0],
                from[1],
                (776. - from[0]).max(40.),
                26.,
                2,
                c,
            ),
        }
    }
    Ok((img, ann))
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Motion {
    pub kind: String,
    pub duration_ms: u64,
    pub zoom: bool,
    pub delay_ms: u64,
    pub backdrop_ms: Option<u64>,
}
impl Motion {
    pub fn apply(&self, t: &mut Transform, local: u64, output_h: u32) {
        let (alpha, shift, zoom) = self.values(local);
        t.opacity *= alpha;
        t.y += shift / output_h as f32;
        t.width *= zoom;
        t.height *= zoom;
        let f=self.retreat(local);
        t.x-=0.065*f;t.y-=0.018*f;
        if self.blurred(local) {t.opacity*=0.55;}
    }
    pub fn blurred(&self,local:u64)->bool {self.backdrop_ms.is_some_and(|at|local>=at)}
    pub fn retreat(&self,local:u64)->f32 {
        self.backdrop_ms.map(|at|{let x=(local.saturating_sub(at) as f32/300.).clamp(0.,1.);x*x*(3.-2.*x)}).unwrap_or(0.)
    }
    pub fn values(&self, local: u64) -> (f32, f32, f32) {
        let d = self.duration_ms.max(1) as f32;
        let enter = 220f32.min(d / 3.);
        let exit = 180f32.min(d / 3.);
        let a = if self.kind.is_empty() || self.kind == "none" {
            1.
        } else {
            (local.saturating_sub(self.delay_ms) as f32 / enter)
                .clamp(0., 1.)
                .min(((d - local as f32) / exit).clamp(0., 1.))
        };
        let shift = if self.kind == "slide_fade" || self.kind=="stack_pop" {
            32. * (1. - (local as f32 / enter).clamp(0., 1.)).powi(3)
        } else {
            0.
        };
        let mut zoom = if self.zoom {
            1. + 0.04 * (local as f32 / d).clamp(0., 1.)
        } else {
            1.
        };
        if self.kind=="stack_pop" {let x=(local.saturating_sub(self.delay_ms) as f32/320.).clamp(0.,1.)-1.;zoom*=1.+0.1*(2.70158*x*x*x+1.70158*x*x);}
        zoom*=1.-0.08*self.retreat(local);
        (a, shift, zoom)
    }
    pub fn filters(&self, cue: u64) -> String {
        if self.kind.is_empty() || self.kind == "none" {
            return String::new();
        }
        let d = self.duration_ms.max(1) as f64 / 1000.;
        let cue = cue as f64 / 1000.;
        let enter = 0.220f64.min(d / 3.);
        let delay = self.delay_ms as f64 / 1000.;
        let exit = 0.180f64.min(d / 3.);
        format!(",setpts=PTS+{cue}/TB,fade=t=in:st={delay}:d={enter}:alpha=1,fade=t=out:st={}:d={exit}:alpha=1,setpts=PTS-{cue}/TB",d-exit)
    }
    pub fn shift_expression(&self, cue: u64) -> String {
        if self.kind != "slide_fade" && self.kind!="stack_pop" {
            return String::new();
        }
        let enter = 0.220f64.min(self.duration_ms.max(1) as f64 / 3000.);
        format!(
            "+32*pow(1-min(max((t+{})/{enter},0),1),3)",
            cue as f64 / 1000.
        )
    }
    pub fn retreat_expression(&self,cue:u64,clock:&str)->String {
        self.backdrop_ms.map(|at|{let x=format!("min(max(({clock}+{}-{})/0.3,0),1)",cue as f64/1000.,at as f64/1000.);format!("({x}*{x}*(3-2*{x}))")}).unwrap_or_else(||"0".into())
    }
    pub fn scale_expression(&self,cue:u64)->String {
        let clock=format!("(t+{})",cue as f64/1000.);
        let pop=if self.kind=="stack_pop" {let x=format!("(min(max(({clock}-{})/0.32,0),1)-1)",self.delay_ms as f64/1000.);format!("(1+0.1*(2.70158*{x}*{x}*{x}+1.70158*{x}*{x}))")}else{"1".into()};
        let zoom=if self.zoom {format!("(1+0.04*min(max({clock}/{},0),1))",self.duration_ms.max(1) as f64/1000.)}else{"1".into()};
        format!("{pop}*{zoom}*(1-0.08*{})",self.retreat_expression(cue,"t"))
    }
}
