//! Source-space overlays detected with geometry, temporal consistency and image
//! templates. No model, OCR engine, network call or source-file modification.
use crate::{
    composition::LayerClip,
    media,
    model::*,
    process::{self, Cancel},
    timeline,
};
use anyhow::{ensure, Result};
use image::{GrayImage, RgbaImage};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const VERSION: &str = "source-masks-v5-outlined-text";
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Default for Rect {
    fn default() -> Self {
        Self {
            x: 0.05,
            y: 0.8,
            w: 0.9,
            h: 0.12,
        }
    }
}
impl Rect {
    pub fn valid(self) -> bool {
        [self.x, self.y, self.w, self.h]
            .iter()
            .all(|v| v.is_finite())
            && self.x >= 0.
            && self.y >= 0.
            && self.w >= 0.002
            && self.h >= 0.002
            && self.x + self.w <= 1.00001
            && self.y + self.h <= 1.00001
    }
    pub fn padded(self, p: f32) -> Self {
        let x = (self.x - p).max(0.);
        let y = (self.y - p).max(0.);
        Self {
            x,
            y,
            w: (self.x + self.w + p).min(1.) - x,
            h: (self.y + self.h + p).min(1.) - y,
        }
    }
    pub fn overlap(self, b: Self) -> f32 {
        let a = (self.x + self.w).min(b.x + b.w) - self.x.max(b.x);
        let h = (self.y + self.h).min(b.y + b.h) - self.y.max(b.y);
        let i = a.max(0.) * h.max(0.);
        i / (self.w * self.h + b.w * b.h - i).max(0.000001)
    }
    fn union(self, b: Self) -> Self {
        let x = self.x.min(b.x);
        let y = self.y.min(b.y);
        Self {
            x,
            y,
            w: (self.x + self.w).max(b.x + b.w) - x,
            h: (self.y + self.h).max(b.y + b.h) - y,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Region {
    pub id: String,
    pub source_id: String,
    pub rect: Rect,
    pub start_ms: u64,
    pub end_ms: u64,
    pub kind: String,
    pub score: f32,
    pub enabled: bool,
    pub manual: bool,
}
impl Default for Region {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            source_id: String::new(),
            rect: Rect::default(),
            start_ms: 0,
            end_ms: 1000,
            kind: "manual".into(),
            score: 1.,
            enabled: true,
            manual: true,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Template {
    pub name: String,
    pub path: PathBuf,
    #[serde(default)]
    pub width: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Ignored {
    pub source_id: String,
    pub rect: Rect,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    pub auto_scan: bool,
    pub subtitles: bool,
    pub logos: bool,
    pub camera: bool,
    pub sensitivity: String,
    pub strength: f32,
    pub padding: f32,
    pub evidence: bool,
    pub templates: Vec<Template>,
    pub regions: Vec<Region>,
    pub ignored: Vec<Ignored>,
    pub scan_key: String,
    pub scan_complete: bool,
    pub report: String,
    pub evidence_revision: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_scan: true,
            subtitles: true,
            logos: true,
            camera: false,
            sensitivity: "balanced".into(),
            strength: 18.,
            padding: 0.008,
            evidence: true,
            templates: vec![],
            regions: vec![],
            ignored: vec![],
            scan_key: String::new(),
            scan_complete: false,
            report: String::new(),
            evidence_revision: String::new(),
        }
    }
}
impl Settings {
    pub fn preferences(&self) -> Self {
        Self {
            regions: vec![],
            ignored: vec![],
            scan_key: String::new(),
            scan_complete: false,
            report: String::new(),
            evidence_revision: String::new(),
            ..self.clone()
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            matches!(self.sensitivity.as_str(), "careful" | "balanced" | "wide"),
            "Độ nhạy vùng che không hợp lệ"
        );
        ensure!(
            self.strength.is_finite()
                && (2. ..=60.).contains(&self.strength)
                && self.padding.is_finite()
                && (0. ..=0.05).contains(&self.padding),
            "Thông số blur / viền che không hợp lệ"
        );
        ensure!(
            self.regions.len() <= 512 && self.templates.len() <= 16 && self.ignored.len() <= 512,
            "Quá nhiều vùng che / mẫu logo"
        );
        for r in &self.regions {
            ensure!(
                r.rect.valid()
                    && r.start_ms < r.end_ms
                    && r.end_ms <= 86_400_000
                    && r.score.is_finite()
                    && (0. ..=1.).contains(&r.score)
                    && matches!(r.kind.as_str(), "manual" | "subtitle" | "logo" | "camera"),
                "Vùng che {} không hợp lệ",
                r.id
            );
        }
        for t in &self.templates {
            ensure!(
                t.width.is_finite() && (0. ..=1.).contains(&t.width),
                "Kích thước mẫu logo không hợp lệ"
            );
        }
        for r in &self.ignored {
            ensure!(r.rect.valid(), "Vùng bỏ qua không hợp lệ");
        }
        Ok(())
    }
}
pub fn identity(p: &Project) -> String {
    if !p.source_masks.enabled {
        return String::new();
    }
    media::hash(
        serde_json::to_vec(&(
            VERSION,
            p.source_masks.strength,
            p.source_masks.padding,
            p.source_masks.evidence,
            p.source_masks.subtitles,
            p.source_masks.logos,
            p.source_masks.camera,
            &p.source_masks.regions,
        ))
        .unwrap_or_default(),
    )
}
pub fn evidence_key(p:&Project)->String {
    if p.source_masks.enabled&&p.source_masks.evidence {identity(p)}else{String::new()}
}
pub fn evidence_pending(p:&Project)->bool {p.source_masks.evidence_revision!=evidence_key(p)}
fn active(cfg: &Settings, r: &Region) -> bool {
    r.enabled
        && match r.kind.as_str() {
            "subtitle" => cfg.subtitles,
            "logo" => cfg.logos,
            "camera" => cfg.camera,
            _ => true,
        }
}
pub fn region_active(cfg:&Settings,r:&Region)->bool {cfg.enabled&&active(cfg,r)}
pub fn clip_identity(p: &Project, c: &LayerClip) -> String {
    let Some(source) = source_id(p, c) else {
        return String::new();
    };
    let end = c.source_at(c.duration_ms);
    media::hash(
        serde_json::to_vec(&(
            VERSION,
            p.source_masks.strength,
            p.source_masks.padding,
            p.source_masks
                .regions
                .iter()
                .filter(|r| {
                    active(&p.source_masks, r)
                        && r.source_id == source
                        && r.start_ms < end
                        && r.end_ms > c.media_in_ms
                })
                .collect::<Vec<_>>(),
        ))
        .unwrap_or_default(),
    )
}
fn source_id<'a>(p: &'a Project, c: &LayerClip) -> Option<&'a str> {
    if !p.source_masks.enabled
        || !matches!(c.track, timeline::VIDEO | timeline::BACKGROUND)
        || c.still()
    {
        return None;
    }
    let scene = p
        .segments
        .iter()
        .find(|s| s.id == c.scene_id && s.kind != "host")?;
    let source = p.sources.iter().find(|s| s.id == scene.source_id)?;
    if c.media
        .as_ref()
        .is_none_or(|path| path != source.input_path() && path != &source.path)
    {
        return None;
    }
    Some(scene.source_id.as_str())
}
/// A single-input/single-output FFmpeg graph, valid in both -vf and filter_complex.
/// PTS has already been reset/speed-adjusted by the caller.
pub fn filters(p: &Project, c: &LayerClip) -> Result<String> {
    let Some(source) = source_id(p, c) else {
        return Ok(String::new());
    };
    p.source_masks.validate()?;
    let mut out = String::new();
    let prefix = media::hash(c.id.as_bytes());
    let prefix = &prefix[..12];
    for (i, r) in p
        .source_masks
        .regions
        .iter()
        .filter(|r| {
            active(&p.source_masks, r)
                && r.source_id == source
                && r.start_ms < c.source_at(c.duration_ms)
                && r.end_ms > c.media_in_ms
        })
        .enumerate()
    {
        let rbox = r.rect.padded(p.source_masks.padding);
        let a = (r.start_ms.saturating_sub(c.media_in_ms) as f64 / c.playback_rate) / 1000.;
        let b = (r.end_ms.saturating_sub(c.media_in_ms) as f64 / c.playback_rate) / 1000.;
        // Crop bounds use the same floor as overlay, avoiding seams on odd sizes.
        let x = rbox.x;
        let y = rbox.y;
        let w = rbox.w;
        let h = rbox.h;
        let radius = p.source_masks.strength / (960. * w);
        out.push_str(&format!(",split=2[sm{prefix}{i}][sr{prefix}{i}];[sr{prefix}{i}]crop=w='max(2,min(iw-floor(iw*{x}),floor(iw*{w})))':h='max(2,min(ih-floor(ih*{y}),floor(ih*{h})))':x='floor(iw*{x})':y='floor(ih*{y})':exact=1,boxblur=lr='max(0,min(max(1,w*{radius}),min(w,h)/2-1))':lp=2:cr='max(0,min(max(1,cw*{radius}),min(cw,ch)/2-1))':cp=2[sb{prefix}{i}];[sm{prefix}{i}][sb{prefix}{i}]overlay=x='floor(main_w*{x})':y='floor(main_h*{y})':enable='gte(t,{a:.6})*lt(t,{b:.6})':shortest=1"));
    }
    Ok(out)
}
pub fn filters_at(p: &Project, c: &LayerClip, local: u64) -> Result<String> {
    let mut part = c.clone();
    part.media_in_ms = c.source_at(local);
    part.duration_ms = c.duration_ms.saturating_sub(local);
    filters(p, &part)
}
pub fn mask_image(p: &Project, source: &str, time: u64, img: &mut RgbaImage) {
    if !p.source_masks.enabled || !p.source_masks.evidence {
        return;
    }
    for r in p.source_masks.regions.iter().filter(|r| {
        active(&p.source_masks, r) && r.source_id == source && time >= r.start_ms && time < r.end_ms
    }) {
        let b = r.rect.padded(p.source_masks.padding);
        let (w, h) = img.dimensions();
        let x = (b.x * w as f32).floor() as u32;
        let y = (b.y * h as f32).floor() as u32;
        let rw = ((b.w * w as f32).ceil() as u32).min(w - x);
        let rh = ((b.h * h as f32).ceil() as u32).min(h - y);
        if rw == 0 || rh == 0 {
            continue;
        }
        let roi = image::imageops::crop_imm(img, x, y, rw, rh).to_image();
        let blur = image::imageops::blur(&roi, p.source_masks.strength * w as f32 / 1080.);
        image::imageops::overlay(img, &blur, x as i64, y as i64);
    }
}
pub fn import_template(path: &Path, folder: &Path, width: f32) -> Result<Template> {
    ensure!(
        fs::metadata(path)?.len() <= 16 * 1024 * 1024,
        "Mẫu logo vượt 16 MB"
    );
    let reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let (w, h) = reader.into_dimensions()?;
    ensure!(
        w > 0 && h > 0 && w as u64 * h as u64 <= 16_000_000,
        "Ảnh mẫu logo quá lớn"
    );
    let img = image::open(path)?.thumbnail(640, 640).to_rgba8();
    fs::create_dir_all(folder)?;
    let dest = folder.join(format!("{}.png", media::hash(img.as_raw())));
    img.save(&dest)?;
    Ok(Template {
        name: path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .chars()
            .take(80)
            .collect(),
        path: dest,
        width,
    })
}
pub fn pin(data: &Path, cfg: &mut Settings) -> Result<()> {
    for t in &mut cfg.templates {
        t.path = crate::input_assets::shared_file(data, &t.path.to_string_lossy(), "mẫu logo")?;
    }
    Ok(())
}
fn ranges(p: &Project) -> BTreeMap<String, Vec<(u64, u64)>> {
    let mut map: BTreeMap<String, Vec<(u64, u64)>> = BTreeMap::new();
    for c in p
        .composition_snapshot()
        .clips
        .iter()
        .filter(|c| c.track == timeline::VIDEO && c.enabled(p))
    {
        if let Some(s) = p.segments.iter().find(|s| s.id == c.scene_id) {
            map.entry(s.source_id.clone())
                .or_default()
                .push((c.media_in_ms, c.source_at(c.duration_ms)));
        }
    }
    // Host photos also need the same protection, without scanning unused footage.
    if p.source_masks.evidence {
        for s in &p.segments {
            if crate::footage_first::context_path(p, s).is_some() {
                map.entry(s.source_id.clone())
                    .or_default()
                    .push((s.in_ms.saturating_sub(500), s.in_ms.saturating_add(1000)));
            }
            for e in s.evidence_items.iter().filter(|e| e.enabled) {
                map.entry(e.source_id.clone()).or_default().push((
                    e.source_time_ms.saturating_sub(500),
                    e.source_time_ms.saturating_add(1000),
                ));
                if let Some(time) = e.second_time_ms {
                    let source = if e.second_source_id.is_empty() {
                        &e.source_id
                    } else {
                        &e.second_source_id
                    };
                    map.entry(source.clone())
                        .or_default()
                        .push((time.saturating_sub(500), time.saturating_add(1000)));
                }
            }
        }
    }
    for list in map.values_mut() {
        list.sort_unstable();
        let mut merged: Vec<(u64, u64)> = vec![];
        for (a, b) in list.drain(..) {
            if let Some(last) = merged.last_mut().filter(|v| a <= v.1 + 500) {
                last.1 = last.1.max(b);
            } else {
                merged.push((a, b));
            }
        }
        *list = merged;
    }
    map
}
pub fn scan_key(p: &Project) -> String {
    let ids: Vec<_> = p
        .sources
        .iter()
        .map(|s| (&s.id, crate::visual_effects::fingerprint(s.input_path()),crate::visual_effects::fingerprint(&s.proxy)))
        .collect();
    let templates: Vec<_> = p
        .source_masks
        .templates
        .iter()
        .map(|t| (t, crate::visual_effects::fingerprint(&t.path)))
        .collect();
    media::hash(
        serde_json::to_vec(&(
            VERSION,
            ids,
            ranges(p),
            p.source_masks.subtitles,
            p.source_masks.logos,
            p.source_masks.camera,
            &p.source_masks.sensitivity,
            templates,
            &p.source_masks.ignored,
        ))
        .unwrap_or_default(),
    )
}
#[derive(Clone)]
struct Candidate {
    rect: Rect,
    kind: &'static str,
    score: f32,
    signature: [f32; 64],
}
struct Observation {
    time: u64,
    step: u64,
    window: usize,
    end: u64,
    items: Vec<Candidate>,
    scene: [f32; 64],
}
fn signature(img: &GrayImage, r: Rect) -> [f32; 64] {
    let (w, h) = img.dimensions();
    let mut v = [0.; 64];
    let x0 = (r.x * w as f32).round() as u32;
    let y0 = (r.y * h as f32).round() as u32;
    let rw = (r.w * w as f32).round();
    let rh = (r.h * h as f32).round();
    for (i, out) in v.iter_mut().enumerate() {
        let x = x0 + (((i % 8) as f32 + 0.5) * rw / 8.) as u32;
        let y = y0 + (((i / 8) as f32 + 0.5) * rh / 8.) as u32;
        *out = img.get_pixel(x.min(w - 1), y.min(h - 1))[0] as f32 / 255.;
    }
    let mean = v.iter().sum::<f32>() / 64.;
    for x in &mut v {
        *x -= mean;
    }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(0.001);
    for x in &mut v {
        *x /= norm;
    }
    v
}
fn similarity(a: &[f32; 64], b: &[f32; 64]) -> f32 {
    a.iter().zip(b).map(|(a, b)| a * b).sum::<f32>()
}
fn components(img: &GrayImage) -> Vec<Rect> {
    feature_components(img,false)
}
fn feature_components(img: &GrayImage, bright:bool) -> Vec<Rect> {
    let (w, h) = img.dimensions();
    if w < 8 || h < 8 {
        return vec![];
    }
    let mut bits = vec![false; (w * h) as usize];
    // Canny-like gradient threshold followed by a small morphological dilation.
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            if bright {bits[(y*w+x) as usize]=img.get_pixel(x,y)[0]>=195;continue;}
            let dx = img.get_pixel(x + 1, y)[0].abs_diff(img.get_pixel(x - 1, y)[0]);
            let dy = img.get_pixel(x, y + 1)[0].abs_diff(img.get_pixel(x, y - 1)[0]);
            if dx.max(dy) > 65 {
                for yy in y - 1..=y + 1 {
                    for xx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                        bits[(yy * w + xx) as usize] = true;
                    }
                }
            }
        }
    }
    let mut result = vec![];
    let mut stack = vec![];
    for start in 0..bits.len() {
        if !bits[start] {
            continue;
        }
        bits[start] = false;
        stack.push(start);
        let (mut x0, mut y0, mut x1, mut y1, mut count) = (w, h, 0, 0, 0);
        while let Some(i) = stack.pop() {
            let x = i as u32 % w;
            let y = i as u32 / w;
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
            count += 1;
            for n in [
                if x > 0 { Some(i - 1) } else { None },
                if x + 1 < w { Some(i + 1) } else { None },
                if y > 0 { Some(i - w as usize) } else { None },
                if y + 1 < h {
                    Some(i + w as usize)
                } else {
                    None
                },
            ]
            .into_iter()
            .flatten()
            {
                if bits[n] {
                    bits[n] = false;
                    stack.push(n);
                }
            }
        }
        let rw = x1 - x0 + 1;
        let rh = y1 - y0 + 1;
        let keep=if bright {count>=4&&rw>=1&&rh>=4&&rh<h/8&&rw<w/10&&rw<=rh*3}
            else {count>=8&&rw>=2&&rh>=5&&rh<h/5&&rw<w/3};
        if keep {
            result.push(Rect {
                x: x0 as f32 / w as f32,
                y: y0 as f32 / h as f32,
                w: rw as f32 / w as f32,
                h: rh as f32 / h as f32,
            });
        }
    }
    result
}
/// Separate bright letter interiors before dilation. Thick colored outlines
/// join a whole sentence in the edge map, so counting edge components misses it.
fn outlined_text(img:&GrayImage,cfg:&Settings,edges:&[Rect])->Vec<Candidate> {
    let (w,h)=img.dimensions();if w<40||h<40{return vec![];}
    let mut glyphs:Vec<_>=feature_components(img,true).into_iter().filter(|r|{
        let x=(r.x*w as f32).round() as i32;let y=(r.y*h as f32).round() as i32;
        let rw=(r.w*w as f32).round() as i32;let rh=(r.h*h as f32).round() as i32;
        let probes=[(x-2,y),(x+rw+1,y),(x-2,y+rh),(x+rw+1,y+rh),(x+rw/2,y-2),(x+rw/2,y+rh+1),(x-2,y+rh/2),(x+rw+1,y+rh/2)];
        probes.iter().filter(|&&(px,py)|px>=0&&py>=0&&px<w as i32&&py<h as i32&&img.get_pixel(px as u32,py as u32)[0]<155).count()>=5
    }).collect();
    glyphs.sort_by(|a,b|a.x.total_cmp(&b.x));let mut used=vec![false;glyphs.len()];let mut result=vec![];
    for i in 0..glyphs.len(){if used[i]{continue;}let mut line=glyphs[i];let mut n=1;used[i]=true;
        for j in i+1..glyphs.len(){let b=glyphs[j];
            if !used[j]&&(line.y+line.h/2.-b.y-b.h/2.).abs()<line.h.max(b.h)*0.45
                &&b.x<=line.x+line.w+line.h*1.8&&(0.45..=2.0).contains(&(b.h/line.h)) {
                line=line.union(b);used[j]=true;n+=1;
            }
        }
        if n<6||line.w<line.h*3.||line.w<0.12||line.h>0.1||line.w*line.h>0.12{continue;}
        let kind=if cfg.subtitles&&line.y>=0.7 {Some("subtitle")}
            else if cfg.logos&&line.y<0.20&&line.x<0.20 {Some("logo")}
            else if line.y<0.20&&line.x+line.w>0.80 {if cfg.camera{Some("camera")}else{None}}
            else{None};
        if let Some(kind)=kind {
            // Include the containing badge/pill border, not just white letters.
            let rect=if kind=="logo" {edges.iter().filter(|b|b.x<=line.x+0.005&&b.y<=line.y+0.005&&b.x+b.w>=line.x+line.w-0.005&&b.y+b.h>=line.y+line.h-0.005
                &&b.w<=line.w*1.65&&b.h<=line.h*2.7&&b.w<0.4&&b.h<0.2).min_by(|a,b|(a.w*a.h).total_cmp(&(b.w*b.h))).copied().unwrap_or(line)}else{line};
            result.push(Candidate{rect:rect.padded(0.004),kind,score:if n>=9{0.975}else{0.96},signature:signature(img,rect)});
        }
    }
    result
}
fn candidates(img: &GrayImage, cfg: &Settings) -> Vec<Candidate> {
    let mut glyphs = components(img);
    glyphs.sort_by(|a, b| a.x.total_cmp(&b.x));
    let mut result = vec![];
    let mut used = vec![false; glyphs.len()];
    for i in 0..glyphs.len() {
        if used[i] {
            continue;
        }
        let mut line = glyphs[i];
        let mut n = 1;
        used[i] = true;
        for j in i + 1..glyphs.len() {
            let b = glyphs[j];
            let center = (line.y + line.h / 2. - b.y - b.h / 2.).abs();
            if !used[j]
                && center < line.h.max(b.h) * 0.48
                && b.x <= line.x + line.w + line.h * 1.8
                && b.h / line.h > 0.45
                && b.h / line.h < 2.2
            {
                line = line.union(b);
                used[j] = true;
                n += 1;
            }
        }
        if n >= 4 && line.w > line.h * 2.2 && line.w * line.h < 0.14 && line.h < 0.11 {
            let kind = if line.y >= 0.45 && cfg.subtitles {
                Some("subtitle")
            } else if line.y < 0.3 && cfg.camera {
                Some("camera")
            } else if cfg.logos && (line.x < 0.25 || line.x + line.w > 0.75) && line.y < 0.3 {
                Some("logo")
            } else if cfg.subtitles && cfg.sensitivity == "wide" {
                Some("subtitle")
            } else {
                None
            };
            if let Some(kind) = kind {
                result.push(Candidate {
                    rect: line,
                    kind,
                    score: if n >= 7 { 0.91 } else { 0.82 },
                    signature: signature(img, line),
                });
            }
        } else if cfg.logos
            && line.w >= 0.025
            && line.h >= 0.025
            && line.w < 0.25
            && line.h < 0.2
            && (line.x < 0.2 || line.x + line.w > 0.8)
            && (line.y < 0.2 || line.y + line.h > 0.8)
        {
            result.push(Candidate {
                rect: line,
                kind: "logo",
                score: 0.68,
                signature: signature(img, line),
            });
        }
    }
    let mut strong=outlined_text(img,cfg,&glyphs);
    for c in result {if !strong.iter().any(|s|s.kind==c.kind&&s.rect.overlap(c.rect)>0.3){strong.push(c);}}
    strong.truncate(32);
    strong
}
fn template_candidates(img: &GrayImage, patterns: &[(Template, GrayImage)]) -> Vec<Candidate> {
    let (iw, ih) = img.dimensions();
    let mut result = vec![];
    for (t, pattern) in patterns {
        let sig = signature(
            pattern,
            Rect {
                x: 0.,
                y: 0.,
                w: 1.,
                h: 1.,
            },
        );
        let widths = if t.width > 0. {
            [t.width * 0.9, t.width, t.width * 1.1]
        } else {
            [0.06, 0.12, 0.24]
        };
        let mut best = (0.0, Rect::default());
        for width in widths {
            let pw = (iw as f32 * width).round() as u32;
            let ph = (pw as f32 * pattern.height() as f32 / pattern.width() as f32).round() as u32;
            if pw < 8 || ph < 8 || pw >= iw || ph >= ih {
                continue;
            }
            for y in (0..ih - ph).step_by(4) {
                for x in (0..iw - pw).step_by(4) {
                    let rect = Rect {
                        x: x as f32 / iw as f32,
                        y: y as f32 / ih as f32,
                        w: pw as f32 / iw as f32,
                        h: ph as f32 / ih as f32,
                    };
                    let score = similarity(&sig, &signature(img, rect));
                    if score > best.0 {
                        best = (score, rect);
                    }
                }
            }
        }
        if best.0 >= 0.94 {
            result.push(Candidate {
                rect: best.1,
                kind: "logo",
                score: 0.99,
                signature: signature(img, best.1),
            });
        }
    }
    result
}
fn consolidate(source: &str, obs: &[Observation], cfg: &Settings) -> Vec<Region> {
    struct Track {
        item: Candidate,
        first: u64,
        last: u64,
        step: u64,
        window: usize,
        end: u64,
        count: usize,
        stable: usize,
        changes: usize,
        text_changes: usize,
        last_scene: [f32; 64],
    }
    let mut tracks: Vec<Track> = vec![];
    for o in obs {
        for c in &o.items {
            let found = tracks.iter_mut().rev().find(|t| {
                t.window == o.window
                    && o.time > t.last
                    && o.time - t.last <= o.step.saturating_mul(3).max(1600)
                    && t.item.kind == c.kind
                    && t.item.rect.overlap(c.rect) > 0.25
            });
            if let Some(t) = found {
                let sim = similarity(&t.item.signature, &c.signature);
                if sim > 0.94 {
                    t.stable += 1;
                }
                if sim < 0.9 {
                    t.text_changes += 1;
                }
                if similarity(&t.last_scene, &o.scene) < 0.9 {
                    t.changes += 1;
                }
                t.item.rect = t.item.rect.union(c.rect);
                t.item.score = t.item.score.max(c.score);
                t.item.signature = c.signature;
                t.last = o.time;
                t.last_scene = o.scene;
                t.count += 1;
            } else {
                tracks.push(Track {
                    item: c.clone(),
                    first: o.time,
                    last: o.time,
                    step: o.step,
                    window: o.window,
                    end: o.end,
                    count: 1,
                    stable: 0,
                    changes: 0,
                    text_changes: 0,
                    last_scene: o.scene,
                });
            }
        }
    }
    let threshold = match cfg.sensitivity.as_str() {
        "careful" => 0.96,
        "wide" => 0.87,
        _ => 0.92,
    };
    let mut result = vec![];
    for t in tracks {
        if t.count < 2
            || t.item.rect.w * t.item.rect.h > 0.16
            || cfg
                .ignored
                .iter()
                .any(|i| i.source_id == source && i.rect.overlap(t.item.rect) > 0.3)
        {
            continue;
        }
        let score = if t.item.score > 0.98 && t.count >= 2 {
            0.99
        } else if t.item.score>=0.96&&t.count>=3&&(t.item.kind!="logo"||t.stable>=2) {
            0.97
        } else if t.item.kind == "logo" {
            if t.stable >= 3 && t.changes >= 2 && t.stable * 4 >= t.count * 3 {
                0.96
            } else {
                0.68
            }
        } else if t.count >= 3
            && t.item.score >= 0.9
            && (t.text_changes > 0 || (t.stable >= 2 && t.changes >= 2))
        {
            0.95
        } else {
            0.82
        };
        let start = t.first.saturating_sub(t.step / 2);
        let end = t.last.saturating_add(t.step * 3 / 2).min(t.end);
        let rect = t.item.rect;
        let id = media::hash(format!(
            "{source}:{}:{start}:{:.2}:{:.2}",
            t.item.kind, rect.x, rect.y
        ));
        result.push(Region {
            id,
            source_id: source.into(),
            rect,
            start_ms: start,
            end_ms: end,
            kind: t.item.kind.into(),
            score,
            enabled: score >= threshold,
            manual: false,
        });
    }
    // A static first line often stays unchanged while the second line updates.
    // Support it only when another accepted subtitle line shares alignment and
    // time; do not expand a single mask blindly into surrounding scene pixels.
    let confirmed:Vec<_>=result.iter().filter(|r|r.enabled&&r.kind=="subtitle").cloned().collect();
    for r in result.iter_mut().filter(|r|!r.enabled&&r.kind=="subtitle"&&r.score>=0.82) {
        if confirmed.iter().any(|other|{
            let a=r.rect;let b=other.rect;
            let gap=(a.y.max(b.y)-(a.y+a.h).min(b.y+b.h)).max(0.);
            let overlap=r.end_ms.min(other.end_ms).saturating_sub(r.start_ms.max(other.start_ms));
            let shorter=(r.end_ms-r.start_ms).min(other.end_ms-other.start_ms);
            gap<a.h.max(b.h)*0.75&&(a.x+a.w/2.-b.x-b.w/2.).abs()<0.15
                &&(0.6..=1.6).contains(&(a.h/b.h))&&a.union(b).h<0.20
                &&overlap as f64>=shorter as f64*0.6
        }) {r.score=0.93;r.enabled=r.score>=threshold;}
    }
    // Keep confirmed overlays before ambiguous scene texture when the cap is hit.
    result.sort_by(|a, b| {
        b.enabled
            .cmp(&a.enabled)
            .then_with(|| b.score.total_cmp(&a.score))
            .then_with(|| a.start_ms.cmp(&b.start_ms))
    });
    result.truncate(64);
    result.sort_by_key(|r| r.start_ms);
    result
}
/// Writes only disposable detector caches. Callers own project checkpointing.
#[derive(Serialize, Deserialize)]
struct Receipt {
    regions: Vec<Region>,
    complete: bool,
    samples: usize,
    intervals: usize,
    total_intervals: usize,
    missing_templates: usize,
}
pub fn scan(
    p: &mut Project,
    s: &crate::model::Settings,
    cancel: &Cancel,
    force: bool,
    mut progress: impl FnMut(&str),
) -> Result<()> {
    if !p.source_masks.enabled {
        return Ok(());
    }
    p.source_masks.validate()?;
    let key = scan_key(p);
    let started = Instant::now();
    let folder = p.root.join("cache/source-masks");
    fs::create_dir_all(&folder)?;
    let receipt = folder.join(format!("{key}.json"));
    let cached:Option<Receipt>=if receipt.is_file(){serde_json::from_slice(&fs::read(&receipt)?).ok()}else{None};
    if !force&&p.source_masks.scan_key==key&&p.source_masks.scan_complete&&cached.as_ref().is_some_and(|r|r.complete){return Ok(());}
    let result: Receipt = if let Some(cached)=cached.filter(|r|r.complete&&!force) {
        cached
    } else {
        let patterns: Vec<_> = p
            .source_masks
            .templates
            .iter()
            .filter_map(|t| {
                image::open(&t.path)
                    .ok()
                    .map(|im| (t.clone(), im.to_luma8()))
            })
            .collect();
        let all = ranges(p);
        let total_intervals = all.values().map(Vec::len).sum();
        let desired = all
            .values()
            .flatten()
            .map(|(a, b)| b.saturating_sub(*a).div_ceil(500).min(120) as usize)
            .sum::<usize>()
            .max(1);
        let ratio = (360_f64 / desired as f64).min(1.);
        let mut result = Receipt {
            regions: vec![],
            complete: true,
            samples: 0,
            intervals: 0,
            total_intervals,
            missing_templates: p.source_masks.templates.len() - patterns.len(),
        };
        let mut budget = 360usize;
        for (id, intervals) in all {
            let source = p.source(&id)?;
            let mut observations = vec![];
            // Detection uses source coordinates. The app proxy is much faster
            // to decode; reject any proxy with a changed aspect ratio.
            let valid_proxy=source.proxy.is_file()&&source.width>0&&source.height>0
                &&media::probe(s,&source.proxy,cancel).ok().is_some_and(|v|v.width>0&&v.height>0&&((v.width as f64/v.height as f64)/(source.width as f64/source.height as f64)-1.).abs()<0.015);
            let input = if valid_proxy{source.proxy.as_path()}else{source.input_path()};
            let stamp = crate::visual_effects::fingerprint(input);
            for (window, (a, b)) in intervals.into_iter().enumerate() {
                process::check_cancel(cancel)?;
                if budget == 0 || started.elapsed() > Duration::from_secs(60) {
                    result.complete = false;
                    break;
                }
                let b = if source.duration_ms > 0 {
                    b.min(source.duration_ms)
                } else {
                    b
                };
                if b <= a {
                    continue;
                }
                let take = ((((b - a).div_ceil(500).min(120)) as f64 * ratio).floor() as usize)
                    .max(3)
                    .min(budget);
                budget -= take;
                let part = folder.join(media::hash(format!("{stamp}:{a}:{b}:{take}")));
                fs::create_dir_all(&part)?;
                let fps = if b - a > take as u64 * 500 {
                    take as f64 * 1000. / (b - a) as f64
                } else {
                    2.
                };
                // force repeats detection; unchanged source fingerprints keep
                // the sampled images reusable when sensitivity/templates change.
                if !part.join("ready").exists() {
                    progress(&format!(
                        "Dò chữ/logo · {} · {:.1}–{:.1}s",
                        source.id,
                        a as f64 / 1000.,
                        b as f64 / 1000.
                    ));
                    let args = vec![
                        "-y".into(),
                        "-v".into(),
                        "error".into(),
                        "-threads".into(),
                        "1".into(),
                        "-ss".into(),
                        format!("{:.3}", a as f64 / 1000.),
                        "-i".into(),
                        input.to_string_lossy().into_owned(),
                        "-t".into(),
                        format!("{:.3}", (b - a) as f64 / 1000.),
                        "-an".into(),
                        "-vf".into(),
                        format!("fps={fps:.6},scale=480:480:force_original_aspect_ratio=decrease"),
                        "-frames:v".into(),
                        take.to_string(),
                        "-q:v".into(),
                        "3".into(),
                        part.join("%04d.jpg").to_string_lossy().into_owned(),
                    ];
                    process::run(
                        &s.ffmpeg,
                        &args,
                        None,
                        cancel,
                        Duration::from_secs(30),
                        None,
                    )?;
                    fs::write(part.join("ready"), b"ok")?;
                }
                let mut observed = 0;
                for i in 1..=take {
                    process::check_cancel(cancel)?;
                    if started.elapsed() > Duration::from_secs(60) {
                        result.complete = false;
                        break;
                    }
                    let path = part.join(format!("{i:04}.jpg"));
                    if !path.is_file() {
                        break;
                    }
                    let img = image::open(path)?.to_luma8();
                    let mut items = candidates(&img, &p.source_masks);
                    if p.source_masks.logos {
                        items.extend(template_candidates(&img, &patterns));
                    }
                    observations.push(Observation {
                        time: a + ((i - 1) as f64 * 1000. / fps) as u64,
                        step: (1000. / fps).ceil() as u64,
                        window,
                        end: b,
                        items,
                        scene: signature(
                            &img,
                            Rect {
                                x: 0.,
                                y: 0.,
                                w: 1.,
                                h: 1.,
                            },
                        ),
                    });
                    observed += 1;
                }
                result.samples += observed;
                if observed > 0 {
                    result.intervals += 1;
                } else {
                    result.complete = false;
                }
            }
            result
                .regions
                .extend(consolidate(&id, &observations, &p.source_masks));
        }
        write_json(&receipt, &result)?;
        result
    };
    let mut regions: Vec<_> = p
        .source_masks
        .regions
        .iter()
        .filter(|r| r.manual)
        .cloned()
        .collect();
    // User overrides are promoted to manual by the inspector. Automatic
    // proposals must be allowed to gain confidence after a new scan.
    for r in result.regions {
        if p.source_masks
            .ignored
            .iter()
            .any(|i| i.source_id == r.source_id && i.rect.overlap(r.rect) > 0.3)
            || regions.iter().any(|v| {
                v.id == r.id
                    || (v.manual
                        && v.source_id == r.source_id
                        && v.rect.overlap(r.rect) > 0.5
                        && v.start_ms < r.end_ms
                        && v.end_ms > r.start_ms)
            })
        {
            continue;
        }
        regions.push(r);
    }
    regions.truncate(512);
    p.source_masks.regions = regions;
    p.source_masks.scan_key = key;
    p.source_masks.scan_complete=result.complete;
    let enabled = p
        .source_masks
        .regions
        .iter()
        .filter(|r| active(&p.source_masks, r))
        .count();
    let proposed = p.source_masks.regions.len() - enabled;
    p.source_masks.report = format!(
        "{enabled} vùng đang che · {proposed} đề xuất / tắt · {} ảnh / {}/{} khoảng · {:.1}s{}{}",
        result.samples,
        result.intervals,
        result.total_intervals,
        started.elapsed().as_secs_f32(),
        if !result.complete {
            " · quét một phần, kiểm tra thêm bằng preview"
        } else {
            ""
        },
        if result.missing_templates > 0 {
            " · thiếu / lỗi ảnh mẫu logo"
        } else {
            ""
        }
    );
    write_json(
        &folder.join("report.json"),
        &serde_json::json!({"algorithm":VERSION,"no_models":true,"elapsed_ms":started.elapsed().as_millis(),"active":enabled,"proposed":proposed,"samples":result.samples,"intervals":result.intervals,"total_intervals":result.total_intervals,"complete":result.complete,"missing_templates":result.missing_templates,"scan_key":p.source_masks.scan_key}),
    )?;
    p.rendered = None;
    Ok(())
}
pub fn prepare(
    p: &mut Project,
    s: &crate::model::Settings,
    cancel: &Cancel,
    progress: impl FnMut(&str),
) -> Result<()> {
    if !p.source_masks.enabled || !p.source_masks.auto_scan {
        return Ok(());
    }
    if let Err(e) = scan(p, s, cancel, false, progress) {
        process::check_cancel(cancel)?;
        p.source_masks.report =
            format!("Chưa dò xong: {e:#}. Có thể quét lại hoặc vẽ vùng che thủ công.");
    }
    Ok(())
}

/// Map between decoded source pixels and the visible, already cropped/flipped
/// layer. Coordinates are unbounded so partially cropped masks clip naturally.
pub fn source_to_layer(p: &Project, c: &LayerClip, point: [f32; 2], inverse: bool) -> [f32; 2] {
    let source = p
        .segments
        .iter()
        .find(|s| s.id == c.scene_id)
        .and_then(|s| p.sources.iter().find(|src| src.id == s.source_id));
    let Some(src) = source else {
        return point;
    };
    let (ow, oh) = p.output.dimensions();
    let framed = crate::framed_layout::footage(p, c) || crate::framed_layout::cropped_footage(p, c);
    let f = if framed {
        crate::framed_layout::framing(p, c).clone()
    } else {
        Default::default()
    };
    let [l, t, r, b] = f.trim;
    let sw = src.width.max(1) as f32 * (1. - l - r);
    let sh = src.height.max(1) as f32 * (1. - t - b);
    let vw = c.transform.width * ow as f32;
    let vh = c.transform.height * oh as f32;
    let scale = if framed {
        (vw / sw).max(vh / sh) * f.zoom
    } else {
        (vw / sw).min(vh / sh)
    };
    let dx = if framed {
        (sw * scale - vw) * f.x
    } else {
        (sw * scale - vw) / 2.
    };
    let dy = if framed {
        (sh * scale - vh) * f.y
    } else {
        (sh * scale - vh) / 2.
    };
    let e = crate::visual_effects::selected(p, c);
    let flipx = e.is_some_and(|e| e.mirror_x);
    let flipy = e.is_some_and(|e| e.mirror_y);
    if inverse {
        let mut x = (point[0] * vw + dx) / scale / src.width.max(1) as f32 + l;
        let mut y = (point[1] * vh + dy) / scale / src.height.max(1) as f32 + t;
        if flipx {
            x = 1. - x;
        }
        if flipy {
            y = 1. - y;
        }
        [x, y]
    } else {
        let [mut x, mut y] = point;
        if flipx {
            x = 1. - x;
        }
        if flipy {
            y = 1. - y;
        }
        [
            ((x - l) * src.width.max(1) as f32 * scale - dx) / vw,
            ((y - t) * src.height.max(1) as f32 * scale - dy) / vh,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thick_outlines_do_not_join_bright_letter_cores_or_block_caption_detection() {
        let mut img=GrayImage::from_pixel(480,270,image::Luma([90]));
        for (row,count,left) in [(245,26,15),(8,9,15),(8,6,350)] {
            for letter in 0..count {for y in 0..12 {for x in 0..7 {
                if x<=1||y<=1||(5..=6).contains(&y)||y>=10 {
                    let px=left+letter*14+x;let py=row+y;
                    for yy in py-3..=py+3{for xx in px-3..=px+3{img.put_pixel(xx,yy,image::Luma([70]));}}
                }
            }}}
            for letter in 0..count {for y in 0..12 {for x in 0..7 {if x<=1||y<=1||(5..=6).contains(&y)||y>=10{img.put_pixel(left+letter*14+x,row+y,image::Luma([245]));}}}}
        }
        let cfg=Settings::default();let items=candidates(&img,&cfg);
        assert!(items.iter().any(|c|c.kind=="subtitle"&&c.score>=0.96),"outlined caption missed");
        assert!(items.iter().any(|c|c.kind=="logo"&&c.score>=0.96),"outlined corner label missed");
        assert!(!items.iter().any(|c|c.kind=="logo"&&c.rect.x>0.55&&c.score>=0.96),"camera timestamp must not become a confirmed logo when camera masking is off");
        let camera=candidates(&img,&Settings{camera:true,..cfg.clone()});assert!(camera.iter().any(|c|c.kind=="camera"&&c.score>=0.96));
        let obs:Vec<_>=(0..4).map(|n|Observation{time:n*500,step:500,window:0,end:2000,items:items.clone(),scene:[0.;64]}).collect();
        let regions=consolidate("s",&obs,&cfg);
        assert!(regions.iter().any(|r|r.kind=="subtitle"&&r.enabled));
        assert!(regions.iter().any(|r|r.kind=="logo"&&r.enabled));
        assert!(candidates(&GrayImage::from_pixel(480,270,image::Luma([250])),&cfg).is_empty());
    }
    #[test]
    fn static_first_subtitle_line_is_supported_by_changing_second_line() {
        let mut sig=[0.;64];sig[0]=1.;let mut changed=[0.;64];changed[1]=1.;
        let first=Candidate{rect:Rect{x:0.15,y:0.7,w:0.7,h:0.04},kind:"subtitle",score:0.91,signature:sig};
        let second=Candidate{rect:Rect{x:0.2,y:0.76,w:0.6,h:0.04},..first.clone()};
        let obs:Vec<_>=(0..4).map(|i|Observation{time:i*500,step:500,window:0,end:2000,scene:sig,items:vec![first.clone(),Candidate{signature:if i%2==0{sig}else{changed},..second.clone()}]}).collect();
        let regions=consolidate("s",&obs,&Settings::default());
        assert_eq!(regions.len(),2);assert!(regions.iter().all(|r|r.enabled));
        let one=consolidate("s",&obs.iter().map(|o|Observation{items:vec![first.clone()],..Observation{time:o.time,step:o.step,window:o.window,end:o.end,items:vec![],scene:o.scene}}).collect::<Vec<_>>(),&Settings::default());
        assert!(one.iter().all(|r|!r.enabled),"An isolated static scene sign must stay a proposal");
    }
    #[test]
    fn template_match_and_sparse_tracks_do_not_bridge_unused_ranges() {
        let pattern = GrayImage::from_fn(64, 32, |x, y| {
            image::Luma([((x * 17 + y * 31 + (x / 5) * 71) % 255) as u8])
        });
        let mut frame = GrayImage::from_pixel(480, 270, image::Luma([40]));
        image::imageops::overlay(&mut frame, &pattern, 16, 16);
        let cfg = Settings::default();
        let items = template_candidates(
            &frame,
            &[(
                Template {
                    name: "test".into(),
                    path: PathBuf::new(),
                    width: 64. / 480.,
                },
                pattern,
            )],
        );
        assert!(!items.is_empty());
        assert!((items[0].rect.x - 16. / 480.).abs() < 0.01);
        let obs = |time, window| Observation {
            time,
            step: 5000,
            window,
            end: 50000,
            items: items.clone(),
            scene: [0.; 64],
        };
        let tracks = consolidate(
            "s",
            &[obs(0, 0), obs(5000, 0), obs(20000, 1), obs(25000, 1)],
            &cfg,
        );
        assert_eq!(tracks.len(), 2);
        assert!(tracks.iter().all(|r| r.enabled && r.score > 0.98));
        assert!(tracks[0].end_ms < tracks[1].start_ms);
        assert!(candidates(&GrayImage::from_pixel(480, 270, image::Luma([100])), &cfg).is_empty());
    }
}
