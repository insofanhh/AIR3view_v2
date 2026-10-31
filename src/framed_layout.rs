//! A footage viewport has its own crop. Speech and source clocks never change.
use crate::{
    composition::{LayerClip, Transform},
    model::*,
    timeline,
};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Framing {
    pub trim: [f32; 4],
    pub zoom: f32,
    pub x: f32,
    pub y: f32,
}
impl Default for Framing {
    fn default() -> Self {
        Self {
            trim: [0.; 4],
            zoom: 1.,
            x: 0.5,
            y: 0.5,
        }
    }
}
impl Framing {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.trim
                .iter()
                .all(|v| v.is_finite() && (0. ..=0.8).contains(v))
                && self.trim[0] + self.trim[2] <= 0.8
                && self.trim[1] + self.trim[3] <= 0.8,
            "Crop phải giữ ít nhất 20% mỗi chiều nguồn"
        );
        ensure!(
            self.zoom.is_finite()
                && (1. ..=4.).contains(&self.zoom)
                && [self.x, self.y]
                    .iter()
                    .all(|v| v.is_finite() && (0. ..=1.).contains(v)),
            "Zoom / vị trí nội dung trong khung không hợp lệ"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    /// Explicit opt-in: old projects with an inactive framed layout keep their picture.
    #[serde(skip_serializing_if="disabled")]
    pub crop_unframed: bool,
    pub background: PathBuf,
    pub background_fit: String,
    pub background_zoom: f32,
    pub background_x: f32,
    pub background_y: f32,
    pub brightness: f32,
    pub top: f32,
    pub height: f32,
    pub left: f32,
    pub width: f32,
    pub framing: Framing,
    pub sources: BTreeMap<String, Framing>,
    pub scenes: BTreeMap<String, Framing>,
    pub captions_in_frame: bool,
    pub title_enabled: bool,
    pub title: String,
    pub part: String,
}
fn disabled(value:&bool)->bool {!*value}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            crop_unframed: false,
            background: PathBuf::new(),
            background_fit: "cover".into(),
            background_zoom: 1.,
            background_x: 0.5,
            background_y: 0.5,
            brightness: 1.,
            top: 0.10,
            height: 0.475,
            left: 0.,
            width: 1.,
            framing: Default::default(),
            sources: BTreeMap::new(),
            scenes: BTreeMap::new(),
            captions_in_frame: true,
            title_enabled: false,
            title: String::new(),
            part: String::new(),
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            [self.top, self.height, self.left, self.width]
                .iter()
                .all(|v| v.is_finite())
                && self.top >= 0.
                && self.left >= 0.
                && self.height >= 0.15
                && self.width >= 0.2
                && self.top + self.height <= 0.96
                && self.left + self.width <= 1.001,
            "Khung video vượt canvas hoặc quá nhỏ"
        );
        ensure!(
            matches!(self.background_fit.as_str(), "cover" | "contain")
                && self.background_zoom.is_finite()
                && (1. ..=4.).contains(&self.background_zoom)
                && [self.background_x, self.background_y]
                    .iter()
                    .all(|v| v.is_finite() && (0. ..=1.).contains(v))
                && self.brightness.is_finite()
                && (0. ..=1.).contains(&self.brightness),
            "Thiết lập ảnh nền không hợp lệ"
        );
        ensure!(
            self.title.chars().count() <= 300
                && self.part.chars().count() <= 40
                && self.sources.len() <= 200
                && self.scenes.len() <= 1000,
            "Thiết lập bố cục quá lớn"
        );
        self.framing.validate()?;
        for f in self.sources.values().chain(self.scenes.values()) {
            f.validate()?;
        }
        Ok(())
    }
    pub fn viewport(&self) -> Transform {
        Transform {
            x: self.left + self.width / 2.,
            y: self.top + self.height / 2.,
            width: self.width,
            height: self.height,
            ..Default::default()
        }
    }
}
pub fn footage(p: &Project, c: &LayerClip) -> bool {
    p.output.layout.enabled && c.track == timeline::VIDEO
}
pub fn cropped_footage(p: &Project, c: &LayerClip) -> bool {
    c.track == timeline::VIDEO && p.output.layout.crop_unframed
        && *framing(p, c) != Framing::default()
}
pub fn rasterized(p: &Project, c: &LayerClip) -> bool {
    footage(p, c) || cropped_footage(p, c) || background(p, c)
}
pub fn background(p: &Project, c: &LayerClip) -> bool {
    p.output.layout.enabled
        && c.track == timeline::BACKGROUND
        && c.id.ends_with(":layout-background")
}
pub fn overlay(c: &LayerClip) -> bool {
    c.id.ends_with(":layout-title")
}
pub fn framing<'a>(p: &'a Project, c: &LayerClip) -> &'a Framing {
    let cfg = &p.output.layout;
    cfg.scenes
        .get(&c.scene_id)
        .or_else(|| {
            p.segments
                .iter()
                .find(|s| s.id == c.scene_id)
                .and_then(|s| cfg.sources.get(&s.source_id))
        })
        .unwrap_or(&cfg.framing)
}
/// Broad edits leave locked scenes visually unchanged by pinning their current crop.
pub fn set_framing(p:&mut Project,c:&LayerClip,scope:u8,f:Framing)->bool {
    if c.track!=timeline::VIDEO||!c.editable(p)||f.validate().is_err(){return false;}
    let Some(source)=p.segments.iter().find(|s|s.id==c.scene_id).map(|s|s.source_id.clone()) else{return false;};
    let locked:Vec<_>=p.composition.as_ref().into_iter().flat_map(|comp|&comp.clips)
        .filter(|v|v.track==timeline::VIDEO&&!v.editable(p))
        .filter(|v|scope==0||p.segments.iter().any(|s|s.id==v.scene_id&&s.source_id==source))
        .map(|v|(v.scene_id.clone(),if !p.output.layout.enabled&&!p.output.layout.crop_unframed {Framing::default()}else{framing(p,v).clone()})).collect();
    if scope==2&&locked.iter().any(|(id,_)|id==&c.scene_id){return false;}
    let cfg=&mut p.output.layout;
    if !cfg.enabled&&!cfg.crop_unframed {
        // Previously inactive framed-layout trims must not suddenly crop other scenes.
        cfg.framing=Default::default();cfg.sources.clear();cfg.scenes.clear();
        cfg.crop_unframed=true;
    }
    for (id,framing) in locked {cfg.scenes.insert(id,framing);}
    match scope {
        0=>cfg.framing=f,
        1=>{cfg.sources.insert(source,f);},
        _=>{cfg.scenes.insert(c.scene_id.clone(),f);},
    }
    true
}
/// Translate pointer distance by the actual cropped image overflow, not the frame size.
pub fn dragged(p: &Project, c: &LayerClip, w: f32, h: f32, dx: f32, dy: f32) -> Framing {
    let mut f = framing(p, c).clone();
    if let Some(src) = p
        .segments
        .iter()
        .find(|s| s.id == c.scene_id)
        .and_then(|s| p.sources.iter().find(|v| v.id == s.source_id))
    {
        let cw = src.width as f32 * (1. - f.trim[0] - f.trim[2]);
        let ch = src.height as f32 * (1. - f.trim[1] - f.trim[3]);
        let scale = (w / cw.max(1.)).max(h / ch.max(1.)) * f.zoom;
        let ex = cw * scale - w;
        let ey = ch * scale - h;
        if ex > 0.5 {
            f.x = (f.x - dx / ex).clamp(0., 1.);
        }
        if ey > 0.5 {
            f.y = (f.y - dy / ey).clamp(0., 1.);
        }
    }
    f
}
/// Shared by preview and export, at their respective raster resolutions.
pub fn raster(p: &Project, c: &LayerClip, w: u32, h: u32, fps: u32) -> Result<Option<String>> {
    if !rasterized(p, c) {
        return Ok(None);
    }
    let effects = if c.track == timeline::VIDEO {
        crate::visual_effects::filters(p, c)?
    } else {
        String::new()
    };
    let mut filter = format!("{}{}{effects}", c.pts(), crate::source_masks::filters(p,c)?);
    let cfg = &p.output.layout;
    if c.track == timeline::VIDEO {
        let f = framing(p, c);
        f.validate()?;
        let [l, t, r, b] = f.trim;
        filter.push_str(&format!(
            ",crop=w='iw*{}':h='ih*{}':x='iw*{l}':y='ih*{t}'",
            1. - l - r,
            1. - t - b
        ));
        let zw = ((w as f32 * f.zoom).ceil() as u32).max(w);
        let zh = ((h as f32 * f.zoom).ceil() as u32).max(h);
        filter.push_str(&format!(",scale={zw}:{zh}:force_original_aspect_ratio=increase,crop={w}:{h}:x='(iw-ow)*{}':y='(ih-oh)*{}'",f.x,f.y));
    } else if cfg.background_fit == "contain" {
        filter.push_str(&format!(",scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)*{}:(oh-ih)*{}:black",cfg.background_x,cfg.background_y));
    } else {
        let zw = (w as f32 * cfg.background_zoom).ceil() as u32;
        let zh = (h as f32 * cfg.background_zoom).ceil() as u32;
        filter.push_str(&format!(",scale={zw}:{zh}:force_original_aspect_ratio=increase,crop={w}:{h}:x='(iw-ow)*{}':y='(ih-oh)*{}'",cfg.background_x,cfg.background_y));
    }
    if background(p, c) {
        filter.push_str(&format!(",eq=brightness={}", cfg.brightness - 1.));
    }
    filter.push_str(&format!(",setsar=1,fps={fps},format=rgba"));
    Ok(Some(filter))
}
pub fn apply(p: &Project, clips: &mut Vec<LayerClip>) {
    if !p.output.layout.enabled {
        return;
    }
    let cfg = &p.output.layout;
    let footage_scenes: Vec<_> = clips
        .iter()
        .filter(|c| c.track == timeline::VIDEO)
        .map(|c| c.scene_id.clone())
        .collect();
    clips.retain(|c| c.track != timeline::BACKGROUND || !footage_scenes.contains(&c.scene_id));
    let mut additions = vec![];
    for c in clips.iter_mut().filter(|c| c.track == timeline::VIDEO) {
        c.transform = cfg.viewport();
        let mut base = c.clone();
        base.id = format!("{}:layout-background", c.id);
        base.track = timeline::BACKGROUND;
        base.z = 0;
        base.media = Some(cfg.background.clone());
        base.proxy = None;
        base.media_in_ms = 0;
        base.playback_rate = 1.;
        base.cue_in_ms = 0;
        base.motion = Default::default();
        base.transform = Transform {
            y: (1. + cfg.top + cfg.height) / 2.,
            height: 1. - cfg.top - cfg.height,
            ..Default::default()
        };
        if !cfg.background.as_os_str().is_empty() {
            additions.push(base);
        }
        if cfg.title_enabled || !cfg.part.trim().is_empty() {
            let mut title = c.clone();
            title.id = format!("{}:layout-title", c.id);
            title.track = timeline::GRAPHICS;
            title.z = 31;
            title.media = None;
            title.proxy = None;
            title.media_in_ms = 0;
            title.playback_rate = 1.;
            title.cue_in_ms = 0;
            title.motion = Default::default();
            title.transform = Transform::default();
            additions.push(title);
        }
    }
    clips.extend(additions);
}
pub fn check_file(p: &Project) -> Result<()> {
    if p.output.layout.enabled && !p.output.layout.background.as_os_str().is_empty() {
        let path = &p.output.layout.background;
        ensure!(
            image::image_dimensions(path).is_ok(),
            "Không đọc được ảnh nền: {}. Chọn lại ảnh trong Nền & bố cục.",
            path.display()
        );
    }
    Ok(())
}
/// Only the layout-owned picture layers change. Audio and authored cards survive.
pub fn refresh(p: &mut Project, reset_geometry: bool) {
    if reset_geometry {
        // An explicit viewport/preset change replaces remembered footage geometry.
        // Keep host, subtitle and audio preferences independent of that action.
        for kind in ["review", "source"] {
            for track in [timeline::VIDEO, timeline::BACKGROUND] {
                p.layer_defaults.presets.remove(&format!("{kind}:{track}:"));
            }
        }
    }
    let fresh = crate::composition::Composition::from_project(p);
    if let Some(mut old) = p.composition.take() {
        let previous_backgrounds: Vec<_> = old.clips.iter()
            .filter(|c| c.track == timeline::BACKGROUND).cloned().collect();
        old.clips.retain(|c| {
            !(c.id.ends_with(":layout-background")
                || overlay(c)
                || (c.track == timeline::BACKGROUND
                    && p.segments
                        .iter()
                        .any(|s| s.id == c.scene_id && s.kind != "host")))
        });
        if reset_geometry {
            for c in old
                .clips
                .iter_mut()
                .filter(|c| c.track == timeline::VIDEO && c.editable(p))
            {
                if let Some(new) = fresh.clips.iter().find(|n| n.id == c.id) {
                    c.transform = new.transform.clone();
                }
            }
        }
        if p.output.layout.enabled {
            // Derive companions from the edited video clips, including splits and trims.
            let mut companions = old
                .clips
                .iter()
                .filter(|c| c.track == timeline::VIDEO)
                .cloned()
                .collect();
            apply(p, &mut companions);
            p.layer_defaults.apply_clips(p, &mut companions);
            for c in companions.iter_mut().filter(|c| c.track == timeline::BACKGROUND) {
                if let Some(prior) = previous_backgrounds.iter().find(|v| v.id == c.id) {
                    if !prior.editable(p) {
                        *c = prior.clone();
                    } else if !reset_geometry {
                        c.transform = prior.transform.clone();
                    }
                }
            }
            old.clips.extend(
                companions
                    .into_iter()
                    .filter(|c| c.track != timeline::VIDEO && !old.removed.contains(&c.id)),
            );
        } else {
            old.clips.extend(
                fresh
                    .clips
                    .iter()
                    .filter(|c| {
                        c.track == timeline::BACKGROUND
                            && p.segments
                                .iter()
                                .any(|s| s.id == c.scene_id && s.kind != "host")
                            && !old.removed.contains(&c.id)
                    })
                    .cloned(),
            );
        }
        old.script_key = fresh.script_key;
        p.composition = Some(old);
    } else {
        p.composition = Some(fresh);
    }
    p.rendered = None;
}
pub fn pin(data: &Path, cfg: &mut Settings) -> Result<()> {
    if !cfg.background.as_os_str().is_empty() {
        cfg.background =
            crate::input_assets::shared_file(data, &cfg.background.to_string_lossy(), "ảnh nền")?;
    }
    Ok(())
}
pub fn caption_box(p: &Project, seg: &Segment) -> Option<Transform> {
    if !p.output.layout.enabled || !p.output.layout.captions_in_frame || seg.kind == "host" {
        return None;
    }
    Some(
        p.composition
            .as_ref()
            .and_then(|c| {
                c.clips
                    .iter()
                    .find(|c| c.scene_id == seg.id && c.track == timeline::VIDEO)
            })
            .map(|c| c.transform.clone())
            .unwrap_or_else(|| p.output.layout.viewport()),
    )
}
pub fn write_overlay(p: &Project, seg: &Segment, path: &Path) -> Result<()> {
    let cfg = &p.output.layout;
    let (w, h) = p.output.dimensions();
    let scale = w.min(h) as f64 / 1080.;
    let edge = (cfg.top + cfg.height) as f64 * h as f64;
    let font = p.subtitles.font.replace([',', '\n', '\r'], " ");
    let mut ass=format!("[Script Info]\nScriptType: v4.00+\nPlayResX: {w}\nPlayResY: {h}\nWrapStyle: 0\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Layout,{font},32,&H00202020,&H00202020,&H00FFFFFF,&H00FFFFFF,0,0,0,0,100,100,0,0,1,0,0,5,0,0,0,1\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n");
    let end = crate::subtitle::ass_time(seg.duration_ms);
    let mut event = |layer: u8, text: String| {
        ass.push_str(&format!(
            "Dialogue: {layer},0:00:00.00,{end},Layout,,0,0,0,,{text}\n"
        ));
    };
    let ctx = crate::motion::metrics_context(&font);
    if cfg.title_enabled {
        let title = if cfg.title.trim().is_empty() {
            &p.title
        } else {
            &cfg.title
        };
        let cw = w as f64 * 0.90;
        let ch = 72. * scale;
        let (text, size) = crate::motion::fit_text(&ctx, title, 36. * scale, cw * 0.94, ch * 0.85);
        let x = (w as f64 - cw) / 2.;
        let y = edge;
        event(
            0,
            format!(
                "{{\\an7\\pos({x:.2},{y:.2})\\p1\\c&HFFFFFF&}}{}{{\\p0}}",
                rounded(cw, ch, 10. * scale)
            ),
        );
        event(
            1,
            format!(
                "{{\\an5\\pos({:.2},{:.2})\\fs{size:.2}}}{text}",
                w as f64 / 2.,
                y + ch / 2.
            ),
        );
    }
    if !cfg.part.trim().is_empty() {
        let cw = w as f64 * 0.36;
        let ch = 80. * scale;
        let y = edge + (h as f64 - edge) * 0.62;
        let x = (w as f64 - cw) / 2.;
        let (text, size) =
            crate::motion::fit_text(&ctx, &cfg.part, 42. * scale, cw * 0.90, ch * 0.8);
        event(
            0,
            format!(
                "{{\\an7\\pos({x:.2},{y:.2})\\p1\\c&H55FFFF&}}{}{{\\p0}}",
                rounded(cw, ch, 14. * scale)
            ),
        );
        event(
            1,
            format!(
                "{{\\an5\\pos({:.2},{:.2})\\fs{size:.2}}}{text}",
                w as f64 / 2.,
                y + ch / 2.
            ),
        );
    }
    std::fs::write(path, ass)?;
    Ok(())
}
fn rounded(w: f64, h: f64, r: f64) -> String {
    format!("m {r} 0 l {} 0 b {w} 0 {w} 0 {w} {r} l {w} {} b {w} {h} {w} {h} {} {h} l {r} {h} b 0 {h} 0 {h} 0 {} l 0 {r} b 0 0 0 0 {r} 0",w-r,h-r,w-r,h-r)
}
