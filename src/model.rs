use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Settings {
    pub ffmpeg: String,
    pub ffprobe: String,
    pub ytdlp: String,
    /// Optional user-selected Netscape cookie file; never store cookie contents here.
    pub youtube_cookie_path: String,
    /// Optional app-wide base for newly downloaded originals; existing paths stay valid.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub youtube_download_folder: String,
    /// Node must be explicitly enabled in yt-dlp; only Deno is enabled by default.
    pub youtube_node: String,
    pub python: String,
    pub provider: String,
    pub cli_path: String,
    pub cli_args: Vec<String>,
    pub cli_model: String,
    /// auto: attempt URL concurrently, fall back to local evidence. url: strict.
    pub analysis_input: String,
    pub visual_sample_seconds: u32,
    pub asr_model: String,
    pub asr_device: String,
    pub asr_compute_type: String,
    pub asr_language: String,
    pub gemini_base: String,
    pub gemini_model: String,
    pub gemini_key_env: String,
    pub vieneu_url: String,
    pub vieneu_key_env: String,
    pub voice: String,
    pub voice_reference_enabled: bool,
    pub voice_reference_path: String,
    pub voice_reference_denoise: bool,
    pub heygen_base: String,
    pub heygen_key_env: String,
    pub avatar_id: String,
    pub avatar_image_url: String,
    pub avatar_image_path: String,
    pub avatar_enabled: bool,
    pub avatar_provider: String,
    pub cartoon_character: String,
    pub rhubarb: String,
    pub poll_seconds: u64,
    pub timeout_seconds: u64,
    pub encoder: String,
    pub preview_long_edge: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            ffmpeg: default_tool("ffmpeg"),
            ffprobe: default_tool("ffprobe"),
            ytdlp: default_tool("yt-dlp"),
            youtube_cookie_path: String::new(),
            youtube_download_folder: String::new(),
            youtube_node: default_youtube_node(),
            python: default_python(),
            provider: "antigravity".into(),
            cli_path: default_cli_path(),
            cli_args: vec![
                "--input-format".into(),
                "stream-json".into(),
                "--output-format".into(),
                "stream-json".into(),
                "--model".into(),
                "{model}".into(),
                "--json-schema".into(),
                "{schema}".into(),
            ],
            cli_model: "gemini-3.8-flash-medium".into(),
            analysis_input: "auto".into(),
            visual_sample_seconds: 3,
            asr_model: "large-v3-turbo".into(),
            asr_device: "cpu".into(),
            asr_compute_type: "auto".into(),
            asr_language: String::new(),
            gemini_base: "https://generativelanguage.googleapis.com/v1beta".into(),
            gemini_model: "gemini-3.8-flash".into(),
            gemini_key_env: "GEMINI_API_KEY".into(),
            vieneu_url: "http://127.0.0.1:8765/v1/audio/speech".into(),
            vieneu_key_env: "VIENEU_API_KEY".into(),
            voice: "Minh Quân".into(),
            voice_reference_enabled: false,
            voice_reference_path: String::new(),
            voice_reference_denoise: true,
            heygen_base: "https://api.heygen.com".into(),
            heygen_key_env: "HEYGEN_API_KEY".into(),
            avatar_id: String::new(),
            avatar_image_url: String::new(),
            avatar_image_path: String::new(),
            avatar_enabled: false,
            avatar_provider: "heygen".into(),
            cartoon_character: "portrait".into(),
            rhubarb: default_tool("rhubarb/rhubarb"),
            poll_seconds: 10,
            timeout_seconds: 1800,
            encoder: "libx264".into(),
            preview_long_edge: 540,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Result<Self> {
        let mut settings: Self = serde_json::from_slice(&fs::read(path)?)?;
        if let Ok(exe) = std::env::current_exe() {
            if let Some(root) = exe.parent() { settings.relocate_runtime(root); }
        }
        if let Some(python) = std::env::var_os("AIR3VIEW_PYTHON").map(PathBuf::from).filter(|p| p.is_file()) {
            settings.python = python.to_string_lossy().into_owned();
        }
        Ok(settings)
    }

    fn relocate_runtime(&mut self, root: &Path) {
        for (stored, suffix, command) in [
            (&mut self.ffmpeg, "tools/ffmpeg.exe", "ffmpeg"),
            (&mut self.ffprobe, "tools/ffprobe.exe", "ffprobe"),
            (&mut self.ytdlp, "tools/yt-dlp.exe", "yt-dlp"),
            (&mut self.youtube_node, "tools/node.exe", "node"),
            (&mut self.rhubarb, "tools/rhubarb/rhubarb.exe", "rhubarb/rhubarb"),
            (&mut self.python, ".venv/Scripts/python.exe", "python"),
        ] {
            let candidate = root.join(suffix);
            if !candidate.is_file() { continue; }
            let normalized = stored.replace('\\', "/");
            let previous_root = normalized.to_ascii_lowercase().strip_suffix(&suffix.to_ascii_lowercase())
                .map(|_| PathBuf::from(&normalized[..normalized.len() - suffix.len()]));
            let managed = previous_root.is_some_and(|old| !Path::new(stored).is_file()
                || old.join("release.json").is_file() || old.join("Cargo.toml").is_file());
            if stored.is_empty() || normalized == command || normalized == format!("{command}.exe") || managed {
                *stored = candidate.to_string_lossy().into_owned();
            }
        }
    }
}

fn default_youtube_node() -> String {
    let bundled = default_tool("node");
    if bundled != "node" { return bundled; }
    if let Some(root) = std::env::var_os("ProgramFiles") {
        let path = PathBuf::from(root).join("nodejs/node.exe");
        if path.is_file() { return path.to_string_lossy().into_owned(); }
    }
    "node".into()
}

fn default_cli_path() -> String {
    if let Some(root) = std::env::var_os("LOCALAPPDATA") {
        let path = PathBuf::from(root).join("agy/bin/agy.exe");
        if path.is_file() {
            return path.to_string_lossy().into_owned();
        }
    }
    "agy".into()
}

fn default_python() -> String {
    if let Some(path) = std::env::var_os("AIR3VIEW_PYTHON").map(PathBuf::from) {
        if path.is_file() { return path.to_string_lossy().into_owned(); }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let path = parent.join(".venv/Scripts/python.exe");
            if path.is_file() {
                return path.to_string_lossy().into_owned();
            }
        }
    }
    let local = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".venv/Scripts/python.exe");
    if local.is_file() {
        local.to_string_lossy().into_owned()
    } else {
        "python".into()
    }
}

fn default_tool(name: &str) -> String {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let path = parent.join("tools").join(format!("{name}.exe"));
            if path.is_file() {
                return path.to_string_lossy().into_owned();
            }
        }
    }
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tools")
        .join(format!("{name}.exe"));
    if path.is_file() {
        path.to_string_lossy().into_owned()
    } else {
        name.into()
    }
}

pub fn worker_path(name: &str) -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let path = parent.join("workers").join(name);
            if path.is_file() {
                return path;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("workers")
        .join(name)
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct OutputSettings {
    pub footage_speed: crate::footage_speed::Settings,
    pub editorial: bool,
    pub blur_background: bool,
    /// Original footage audio under narrated review scenes; source intercuts are independent.
    pub original_audio_enabled: bool,
    pub original_audio_gain: f32,
    pub portrait: bool,
    pub resolution: u32,
    pub fps: u32,
    pub fit: String,
    pub scale: f32,
    pub x: f32,
    pub y: f32,
    pub avatar_scale: f32,
    pub avatar_x: f32,
    pub avatar_y: f32,
}
impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            footage_speed: Default::default(),
            editorial: false,
            blur_background: false,
            original_audio_enabled: false,
            original_audio_gain: 0.15,
            portrait: true,
            resolution: 1080,
            fps: 30,
            fit: "contain".into(),
            scale: 1.0,
            x: 0.5,
            y: 0.5,
            avatar_scale: 0.3,
            avatar_x: 0.95,
            avatar_y: 0.92,
        }
    }
}
impl OutputSettings {
    pub fn dimensions(&self) -> (u32, u32) {
        let short = self.resolution / 2 * 2;
        let long = ((short as f64 * 16.0 / 9.0).round() as u32) / 2 * 2;
        if self.portrait {
            (short, long)
        } else {
            (long, short)
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct SubtitleStyle {
    pub enabled: bool,
    pub font: String,
    pub size: f32,
    pub color: [u8; 3],
    pub highlight: [u8; 3],
    pub outline: [u8; 3],
    pub outline_width: f32,
    pub bottom_margin: u32,
    /// Pixel offsets from the bottom-centered caption anchor; positive Y moves down.
    pub offset_x: i32,
    pub offset_y: i32,
    pub words_per_line: usize,
    pub karaoke: bool,
}
impl Default for SubtitleStyle {
    fn default() -> Self {
        Self {
            enabled: true,
            font: "Segoe UI".into(),
            size: 52.0,
            color: [255, 255, 255],
            highlight: [250, 204, 21],
            outline: [9, 15, 28],
            outline_width: 3.0,
            bottom_margin: 130,
            offset_x: 0,
            offset_y: 0,
            words_per_line: 7,
            karaoke: true,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TimeRange {
    pub source_id: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct ScriptRules {
    pub hook_enabled: bool,
    /// Applied when generating a new script, never inserts guessed footage in an existing edit.
    #[serde(default)]
    pub intercuts_enabled: bool,
    pub intercut_count: usize,
    pub intercut_min_seconds: u64,
    pub intercut_max_seconds: u64,
    pub min_seconds: u64,
    pub max_seconds: u64,
    #[serde(deserialize_with = "crate::content_language::deserialize")]
    pub language: String,
    pub prompt: String,
    pub include: String,
    pub exclude: String,
    pub required_ranges: Vec<TimeRange>,
    pub excluded_ranges: Vec<TimeRange>,
}
impl Default for ScriptRules {
    fn default() -> Self {
        Self {
            hook_enabled: true,
            intercuts_enabled: true,
            intercut_count: 3,
            intercut_min_seconds: 2,
            intercut_max_seconds: 5,
            min_seconds: 180,
            max_seconds: 240,
            language: "vi".into(),
            prompt: DEFAULT_REACTION_PROMPT.into(),
            include: String::new(),
            exclude: "Quảng cáo, intro dài, đoạn lặp".into(),
            required_ranges: vec![],
            excluded_ranges: vec![],
        }
    }
}

pub const DEFAULT_REACTION_PROMPT: &str = "đọc và hiểu toàn bộ nội dung, tình huống trong video. Sau đó viết lại thành 1 kịch bản mới với nội dung dạng review, reaction ngắn khoảng {duration}. Phong cách kịch bản dạng bất ngờ cho nhân vật review, reaction thoại";

impl ScriptRules {
    pub fn duration_text(&self) -> String {
        if self.min_seconds == self.max_seconds {
            format!("{} giây", self.min_seconds)
        } else {
            format!("{}–{} giây", self.min_seconds, self.max_seconds)
        }
    }

    /// Only migrate the exact former preset; preserve custom user instructions.
    pub fn migrate_prompt(&mut self) {
        self.language = crate::content_language::canonical(&self.language);
        let legacy = DEFAULT_REACTION_PROMPT.replace("{duration}", "3-4p");
        if self.prompt == legacy {
            self.prompt = DEFAULT_REACTION_PROMPT.into();
        }
    }

    pub fn resolved_prompt(&self) -> String {
        let mut rules = self.clone();
        rules.migrate_prompt();
        if crate::content_language::english(&rules.language) && rules.prompt == DEFAULT_REACTION_PROMPT {
            format!("Read and understand the entire video and its situations. Write an original review/reaction script lasting {}–{} seconds. Use a curious, surprised reaction style, with evidence-based analysis and dialogue for the host.", self.min_seconds, self.max_seconds)
        } else { rules.prompt.replace("{duration}", &self.duration_text()) }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct GraphicCue {
    pub kind: String,
    pub label: String,
    pub value: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub evidence: String,
    /// A split inside a visible card must not replay its entrance sound.
    pub sound_suppressed: bool,
}
impl Default for GraphicCue {
    fn default() -> Self {
        Self {
            kind: "title".into(),
            label: "Điểm đáng chú ý".into(),
            value: String::new(),
            start_ms: 0,
            end_ms: 2000,
            evidence: String::new(),
            sound_suppressed: false,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Source {
    pub normalized: PathBuf,
    pub id: String,
    pub url: String,
    pub path: PathBuf,
    pub proxy: PathBuf,
    pub title: String,
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    pub has_audio: bool,
}
impl Default for Source {
    fn default() -> Self {
        Self {
            normalized:PathBuf::new(),
            id: String::new(),
            url: String::new(),
            path: PathBuf::new(),
            proxy: PathBuf::new(),
            title: String::new(),
            duration_ms: 0,
            width: 0,
            height: 0,
            has_audio: false,
        }
    }
}
impl Source {pub fn input_path(&self)->&Path {if self.normalized.is_file(){&self.normalized}else{&self.path}}}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Word {
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Segment {
    pub id: String,
    /// source = original audio; review = narration over source; host = avatar only.
    pub kind: String,
    pub source_id: String,
    pub in_ms: u64,
    pub out_ms: u64,
    pub duration_ms: u64,
    pub source_rate: f64,
    pub footage_rate_override: Option<f64>,
    pub footage_spans: Vec<crate::footage_speed::Span>,
    pub footage_anchors: Vec<crate::footage_speed::Anchor>,
    pub footage_status: String,
    pub narration: String,
    pub caption: String,
    pub emotion: String,
    pub gesture: String,
    pub motion: String,
    pub avatar_size: String,
    pub graphics: Vec<GraphicCue>,
    pub evidence_items: Vec<crate::evidence::EvidenceItem>,
    pub evidence: String,
    pub locked: bool,
    pub audio: Option<PathBuf>,
    pub avatar: Option<PathBuf>,
    pub words: Vec<Word>,
    pub timing_estimated: bool,
    pub asset_key: String,
    pub avatar_key: String,
    /// In-point within already generated WAV/avatar, independent of source footage.
    pub asset_start_ms: u64,
    pub audio_gain: f32,
    pub suppress_captions: bool,
    pub avatar_start_ms: u64,
    pub asset_window: bool,
    pub asset_narration: String,
    /// Language of the saved take, independent of the next script request.
    pub asset_language: String,
}
impl Default for Segment {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            kind: "source".into(),
            source_id: String::new(),
            in_ms: 0,
            out_ms: 5000,
            duration_ms: 5000,
            source_rate: 1.,
            footage_rate_override: None,
            footage_spans: vec![],
            footage_anchors: vec![],
            footage_status: String::new(),
            narration: String::new(),
            caption: String::new(),
            emotion: "Tự nhiên".into(),
            gesture: "auto".into(),
            motion: "none".into(),
            avatar_size: "small".into(),
            graphics: vec![],
            evidence_items: vec![],
            evidence: String::new(),
            locked: false,
            audio: None,
            avatar: None,
            words: vec![],
            timing_estimated: true,
            asset_key: String::new(),
            avatar_key: String::new(),
            asset_start_ms: 0,
            audio_gain: 1.0,
            suppress_captions: false,
            avatar_start_ms: 0,
            asset_window: false,
            asset_narration: String::new(),
            asset_language: "vi".into(),
        }
    }
}
impl Segment {
    pub fn invalidate_voice(&mut self) {
        self.footage_spans.clear();
        self.footage_status.clear();
        self.asset_start_ms = 0;
        self.avatar_start_ms = 0;
        self.asset_window = false;
        self.asset_narration.clear();
        self.audio = None;
        self.avatar = None;
        self.words.clear();
        self.asset_key.clear();
        self.avatar_key.clear();
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ScriptPlan {
    pub title: String,
    #[serde(default)]
    pub analysis_summary: String,
    pub segments: Vec<Segment>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Project {
    pub music: crate::background_music::Settings,
    pub effects: crate::visual_effects::Settings,
    pub normalization: crate::source_normalization::Settings,
    pub version: u32,
    pub id: String,
    pub title: String,
    pub analysis_summary: String,
    pub script_warnings: Vec<String>,
    pub root: PathBuf,
    pub rules: ScriptRules,
    pub output: OutputSettings,
    pub subtitles: SubtitleStyle,
    pub sources: Vec<Source>,
    pub segments: Vec<Segment>,
    pub status: String,
    pub rendered: Option<PathBuf>,
    pub timeline: crate::timeline::TimelineSettings,
    pub input_urls: String,
    pub production: Option<ProductionSettings>,
    pub composition: Option<crate::composition::Composition>,
    pub layer_defaults: crate::layer_defaults::LayerDefaults,
    pub evidence_settings: crate::evidence::EvidenceSettings,
    pub card_sound: crate::evidence_sfx::CardSoundSettings,
    #[serde(default="crate::export_metadata::legacy_settings")]
    pub export_metadata: crate::export_metadata::MetadataSettings,
}

/// Project choices only. Credentials, service URLs and executable paths stay global.
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct ProductionSettings {
    pub voice: String,
    pub voice_reference_enabled: bool,
    pub voice_reference_path: String,
    pub voice_reference_denoise: bool,
    pub avatar_enabled: bool,
    pub avatar_provider: String,
    pub cartoon_character: String,
    pub avatar_id: String,
    pub avatar_image_url: String,
    pub avatar_image_path: String,
    pub align_subtitles: bool,
}
impl Default for ProductionSettings {
    fn default() -> Self { Self::from_settings(&Settings::default()) }
}
impl ProductionSettings {
    pub fn from_settings(s: &Settings) -> Self {
        Self { voice:s.voice.clone(), voice_reference_enabled:s.voice_reference_enabled,
            voice_reference_path:s.voice_reference_path.clone(), voice_reference_denoise:s.voice_reference_denoise,
            avatar_enabled:s.avatar_enabled, avatar_provider:s.avatar_provider.clone(), cartoon_character:s.cartoon_character.clone(),
            avatar_id:s.avatar_id.clone(), avatar_image_url:s.avatar_image_url.clone(), avatar_image_path:s.avatar_image_path.clone(),
            align_subtitles:true }
    }
    pub fn apply(&self, s: &Settings) -> Settings {
        Settings { voice:self.voice.clone(), voice_reference_enabled:self.voice_reference_enabled,
            voice_reference_path:self.voice_reference_path.clone(), voice_reference_denoise:self.voice_reference_denoise,
            avatar_enabled:self.avatar_enabled, avatar_provider:self.avatar_provider.clone(), cartoon_character:self.cartoon_character.clone(),
            avatar_id:self.avatar_id.clone(), avatar_image_url:self.avatar_image_url.clone(), avatar_image_path:self.avatar_image_path.clone(),
            ..s.clone() }
    }
}

/// Reusable editing preferences. Source-specific selections and generated assets
/// deliberately never cross the boundary into another project.
#[derive(Clone, Default, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct ProjectDefaults {
    pub timeline: Option<crate::timeline::TimelineSettings>,
    pub music: crate::background_music::Settings,
    pub effects: crate::visual_effects::Settings,
    pub normalization: crate::source_normalization::Settings,
    pub layer_defaults: crate::layer_defaults::LayerDefaults,
    pub rules: ScriptRules,
    pub output: OutputSettings,
    pub subtitles: SubtitleStyle,
    pub production: Option<ProductionSettings>,
    pub evidence_settings: crate::evidence::EvidenceSettings,
    pub card_sound: crate::evidence_sfx::CardSoundSettings,
    #[serde(default="crate::export_metadata::legacy_settings")]
    pub export_metadata: crate::export_metadata::MetadataSettings,
}
impl ProjectDefaults {
    pub fn from_project(p: &Project) -> Self {
        let mut rules = p.rules.clone();
        rules.required_ranges.clear();
        rules.excluded_ranges.clear();
        let export_metadata=crate::export_metadata::MetadataSettings{title:String::new(),creation_time:String::new(),..p.export_metadata.clone()};
        let mut effects=p.effects.clone();effects.layers.clear();
        let mut timeline=p.timeline.clone();for track in &mut timeline.tracks{track.locked=false;}
        Self { timeline:Some(timeline),music:p.music.clone(),effects,normalization:p.normalization.clone(),layer_defaults: crate::layer_defaults::LayerDefaults::from_project(p), rules, output: p.output.clone(), subtitles: p.subtitles.clone(), production:p.production.clone(), evidence_settings:p.evidence_settings.clone(),card_sound:p.card_sound.clone(),export_metadata }
    }
    pub fn apply(&self, p: &mut Project) {
        if let Some(timeline)=&self.timeline {
            let locks:Vec<_>=p.timeline.tracks.iter().map(|t|t.locked).collect();
            p.timeline=timeline.clone();
            for (t,locked) in p.timeline.tracks.iter_mut().zip(locks){t.locked=locked;}
        }
        p.music=self.music.clone();p.effects=self.effects.clone();p.normalization=self.normalization.clone();
        p.layer_defaults = self.layer_defaults.clone();
        for (track, sync) in p.timeline.tracks.iter_mut().zip(&self.layer_defaults.sync_parameters) {
            track.sync_parameters = *sync;
        }
        p.rules = self.rules.clone();
        p.rules.migrate_prompt();
        p.rules.required_ranges.clear();
        p.rules.excluded_ranges.clear();
        p.output = self.output.clone();
        p.subtitles = self.subtitles.clone();
        p.production = self.production.clone();
        p.evidence_settings = self.evidence_settings.clone();
        p.card_sound = self.card_sound.clone();
        p.export_metadata = self.export_metadata.clone();
    }
    pub fn load(path: &Path) -> Option<Self> {
        serde_json::from_slice(&fs::read(path).ok()?).ok()
    }
    pub fn load_with_layer_fallback(path: &Path, recent_project: Option<&Path>) -> Option<Self> {
        let mut saved = Self::load(path);
        if saved.as_ref().is_none_or(|d| !d.layer_defaults.initialized) {
            if let Some(previous) = recent_project.and_then(|path| Project::load(path).ok()) {
                let previous = Self::from_project(&previous);
                if let Some(saved) = &mut saved { saved.layer_defaults = previous.layer_defaults; }
                else { saved = Some(previous); }
            }
        }
        saved
    }
}
impl Default for Project {
    fn default() -> Self {
        Self {
            version: 1,
            music:Default::default(),effects:Default::default(),normalization:Default::default(),
            id: Uuid::new_v4().to_string(),
            title: "Dự án mới".into(),
            analysis_summary: String::new(),
            script_warnings: vec![],
            root: PathBuf::new(),
            rules: ScriptRules::default(),
            output: OutputSettings::default(),
            subtitles: SubtitleStyle::default(),
            sources: vec![],
            segments: vec![],
            status: "Bản nháp".into(),
            rendered: None,
            timeline: Default::default(),
            input_urls: String::new(),
            production: None,
            composition: None,
            layer_defaults: Default::default(),
            evidence_settings: Default::default(),
            card_sound: Default::default(),
            export_metadata: Default::default(),
        }
    }
}
impl Project {
    /// Check AI editorial decisions separately from manually edited timelines.
    pub fn validate_intercuts(&self) -> Result<()> {
        if !self.rules.intercuts_enabled || !self.segments.iter().any(|s| s.kind != "source") {
            return Ok(());
        }
        // URL-first analysis does not know audio metadata yet. Recheck downloaded sources later.
        if self.sources.iter().all(|s| s.duration_ms > 0 && !s.has_audio) { return Ok(()); }
        let clips: Vec<_> = self.segments.iter().enumerate().filter(|(i,s)| s.kind=="source"
            && !(self.rules.hook_enabled && *i==0)).collect();
        ensure!(clips.len()==self.rules.intercut_count,
            "Cần {} đoạn hình/tiếng gốc xen giữa các cảnh review (không tính hook); hiện có {}",self.rules.intercut_count,clips.len());
        for (i,clip) in clips {
            ensure!(i>0 && i+1<self.segments.len() && self.segments[i-1].kind!="source" && self.segments[i+1].kind!="source",
                "Đoạn gốc xen kẽ phải có cảnh review/host ở cả trước và sau");
            ensure!((self.rules.intercut_min_seconds*1000..=self.rules.intercut_max_seconds*1000).contains(&clip.duration_ms)
                && clip.narration.trim().is_empty() && clip.audio.is_none() && clip.avatar.is_none()
                && clip.audio_gain>0. && !clip.evidence.trim().is_empty(),
                "Đoạn gốc xen kẽ phải dài {}–{} giây, giữ tiếng gốc, không giọng/nhân vật review và có dẫn chứng timestamp",self.rules.intercut_min_seconds,self.rules.intercut_max_seconds);
            let src=self.source(&clip.source_id)?;
            ensure!(src.duration_ms==0 || src.has_audio,"Đoạn xen kẽ chọn nguồn không có tiếng gốc");
        }
        Ok(())
    }
    /// Applied to new AI plans; existing manually edited timelines remain usable.
    pub fn validate_opening_hook(&self) -> Result<()> {
        if !self.rules.hook_enabled { return Ok(()); }
        let first = self.segments.first().context("Kịch bản thiếu hook mở đầu")?;
        ensure!(first.kind == "source" && (3000..=7000).contains(&first.duration_ms)
            && first.narration.trim().is_empty() && first.avatar.is_none()
            && !first.evidence.trim().is_empty(),
            "AI cần chọn cảnh hook gốc 3–7 giây ở đầu, không thoại reaction/avatar, có dẫn chứng timestamp. Hãy phân tích lại hoặc sửa JSON.");
        ensure!(self.segments.iter().skip(1).any(|s| s.kind != "source"),
            "Sau hook cần ít nhất một cảnh reaction có lời bình");
        Ok(())
    }
    pub fn duration_ms(&self) -> u64 {
        self.segments.iter().map(|s| s.duration_ms).sum()
    }
    pub fn source(&self, id: &str) -> Result<&Source> {
        self.sources
            .iter()
            .find(|s| s.id == id)
            .context("Không tìm thấy source_id")
    }
    pub fn save(&self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        write_json(&self.root.join("project.json"), self)
    }
    pub fn load(path: &Path) -> Result<Self> {
        let mut p: Self = serde_json::from_slice(&fs::read(path)?)?;
        p.rules.migrate_prompt();
        ensure!(p.version == 1, "Phiên bản project chưa hỗ trợ");
        p.root = std::path::absolute(path)?
            .parent()
            .context("Project không có thư mục")?
            .to_owned();
        if p.composition.is_none() && !p.segments.is_empty() {
            let backup=path.with_extension("pre-layers.json");
            if !backup.exists() { fs::copy(path,&backup)?; }
            p.ensure_composition();
        }
        if p.composition.as_ref().is_some_and(|c|c.version==1) {
            let backup=path.with_extension("pre-host-aspect.json");
            if !backup.exists() {fs::copy(path,&backup)?;}
        }
        if p.composition.as_ref().is_some_and(|c|c.version<3) {
            let backup=path.with_extension("pre-evidence.json");if !backup.exists(){fs::copy(path,backup)?;}
        }
        if p.composition.as_ref().is_some_and(|c|c.version<4) {
            let backup=path.with_extension("pre-evidence-motion.json");if !backup.exists(){fs::copy(path,backup)?;}
            p.rendered=None;
        }
        if !p.segments.is_empty() {p.ensure_composition();}
        Ok(p)
    }
    pub fn validate_configuration(&self) -> Result<()> {
        self.music.validate()?;self.effects.footage.validate()?;
        for e in self.effects.layers.values(){e.validate()?;}
        self.normalization.validate()?;
        self.output.footage_speed.validate()?;
        self.card_sound.validate()?;
        crate::evidence::validate(self)?;
        ensure!((1..=8).contains(&self.rules.intercut_count)
            && (1..=10).contains(&self.rules.intercut_min_seconds)
            && (self.rules.intercut_min_seconds..=10).contains(&self.rules.intercut_max_seconds),
            "Thiết lập đoạn gốc xen kẽ không hợp lệ (1–8 đoạn, mỗi đoạn 1–10 giây)");
        if let Some(c)=&self.composition {c.validate()?;}
        ensure!(
            self.rules.min_seconds > 0
                && self.rules.max_seconds >= self.rules.min_seconds
                && self.rules.max_seconds <= 7200,
            "Khoảng thời lượng không hợp lệ"
        );
        ensure!(
            self.output.resolution >= 240 && self.output.resolution <= 2160,
            "Độ phân giải không hợp lệ"
        );
        ensure!((1..=60).contains(&self.output.fps), "FPS không hợp lệ");
        ensure!(
            (0.2..=3.0).contains(&self.output.scale),
            "Scale phải từ 0.2 đến 3.0"
        );
        ensure!(
            [
                self.output.x,
                self.output.y,
                self.output.avatar_x,
                self.output.avatar_y
            ]
            .iter()
            .all(|x| x.is_finite() && (-100.0..=100.0).contains(x)),
            "Vị trí khung hình không hợp lệ"
        );
        ensure!(self.output.original_audio_gain.is_finite() && (0.0..=2.0).contains(&self.output.original_audio_gain),
            "Âm lượng nền video gốc phải từ 0 đến 200%");
        ensure!((-2000..=2000).contains(&self.subtitles.offset_x) && (-2000..=2000).contains(&self.subtitles.offset_y),
            "Dịch chuyển phụ đề X/Y phải từ -2000 đến 2000 px");
        ensure!(
            self.output.avatar_scale.is_finite() && (0.1..=0.8).contains(&self.output.avatar_scale),
            "Kích thước avatar không hợp lệ"
        );
        ensure!(
            matches!(self.output.fit.as_str(), "contain" | "cover"),
            "Fit mode không hợp lệ"
        );
        ensure!(
            self.subtitles.size.is_finite()
                && (8.0..=200.0).contains(&self.subtitles.size)
                && self.subtitles.outline_width.is_finite()
                && (0.0..=12.0).contains(&self.subtitles.outline_width),
            "Cỡ hoặc viền phụ đề không hợp lệ"
        );
        ensure!(
            (1..=20).contains(&self.subtitles.words_per_line),
            "Số từ phụ đề không hợp lệ"
        );
        for range in self
            .rules
            .required_ranges
            .iter()
            .chain(&self.rules.excluded_ranges)
        {
            let source = self.source(&range.source_id)?;
            ensure!(
                range.start_ms < range.end_ms
                    && (source.duration_ms == 0 || range.end_ms <= source.duration_ms),
                "Vùng thời gian nằm ngoài nguồn hoặc không hợp lệ"
            );
        }
        for required in &self.rules.required_ranges {
            ensure!(
                !self
                    .rules
                    .excluded_ranges
                    .iter()
                    .any(|excluded| excluded.source_id == required.source_id
                        && required.start_ms < excluded.end_ms
                        && excluded.start_ms < required.end_ms),
                "Vùng bắt buộc và loại bỏ bị chồng lên nhau"
            );
        }
        Ok(())
    }
    pub fn validate(&self, require_files: bool) -> Result<()> {
        for seg in &self.segments {crate::footage_speed::validate_scene(seg)?;}
        self.validate_configuration()?;
        ensure!(!self.segments.is_empty(), "Timeline chưa có cảnh");
        let mut ids = std::collections::HashSet::new();
        for s in &self.segments {
            ensure!(s.audio_gain.is_finite() && (0.0..=2.0).contains(&s.audio_gain), "Âm lượng cảnh phải từ 0 đến 200%");
            ensure!(matches!(s.gesture.as_str(), "auto" | "none" | "open_palms" | "point" | "beat" | "surprised"), "Cử chỉ tay không hợp lệ");
            ensure!(
                matches!(s.motion.as_str(), "none" | "push_in" | "reaction_shake"),
                "Chuyển động cảnh không hợp lệ"
            );
            ensure!(
                matches!(s.avatar_size.as_str(), "small" | "large"),
                "Kích thước nhân vật không hợp lệ"
            );
            ensure!(s.graphics.len() <= 8, "Mỗi cảnh tối đa 8 thẻ thông tin");
            for g in &s.graphics {
                ensure!(
                    matches!(g.kind.as_str(), "title" | "fact" | "number" | "date"),
                    "Loại thẻ không hợp lệ"
                );
                ensure!(
                    g.start_ms < g.end_ms && g.end_ms <= s.duration_ms,
                    "Thẻ thông tin vượt thời lượng cảnh {}",
                    s.id
                );
                ensure!(
                    g.label.chars().count() <= 160 && g.value.chars().count() <= 64,
                    "Nội dung thẻ quá dài"
                );
                ensure!(
                    !matches!(g.kind.as_str(), "number" | "date")
                        || (!g.value.trim().is_empty() && !g.evidence.trim().is_empty()),
                    "Số liệu/ngày tháng cần giá trị và dẫn chứng nguồn"
                );
            }
            for (index, a) in s.graphics.iter().enumerate() {
                ensure!(
                    !s.graphics[index + 1..]
                        .iter()
                        .any(|b| a.start_ms < b.end_ms && b.start_ms < a.end_ms),
                    "Thẻ thông tin chồng thời gian ở cảnh {}",
                    s.id
                );
            }
            ensure!(
                !s.id.is_empty() && ids.insert(&s.id),
                "ID cảnh trùng hoặc rỗng"
            );
            ensure!(
                (100..=3_600_000).contains(&s.duration_ms),
                "Thời lượng cảnh không hợp lệ"
            );
            ensure!(
                matches!(s.kind.as_str(), "source" | "review" | "host"),
                "Loại cảnh không hợp lệ: {}",
                s.kind
            );
            if s.kind != "host" {
                let src = self.source(&s.source_id)?;
                ensure!(s.in_ms < s.out_ms, "Điểm đầu cảnh phải nhỏ hơn điểm cuối");
                if src.duration_ms > 0 {
                    ensure!(
                        s.out_ms <= src.duration_ms,
                        "Cảnh {} vượt thời lượng video gốc",
                        s.id
                    );
                }
                if require_files {
                    ensure!(
                        src.path.is_file(),
                        "Thiếu video nguồn: {}",
                        src.path.display()
                    );
                }
                for r in &self.rules.excluded_ranges {
                    ensure!(
                        r.source_id != s.source_id || s.out_ms <= r.start_ms || s.in_ms >= r.end_ms,
                        "Cảnh {} chạm vùng đã loại",
                        s.id
                    );
                }
                if s.kind == "source" {
                    ensure!(
                        crate::footage_speed::scaled(s.duration_ms,s.source_rate).abs_diff(s.out_ms-s.in_ms)<=2,
                        "Thời lượng cảnh gốc phải khớp điểm cắt"
                    );
                }
            }
            if s.kind != "source" {
                ensure!(
                    !s.narration.trim().is_empty(),
                    "Cảnh {} thiếu lời thoại",
                    s.id
                );
            }
        }
        for r in &self.rules.required_ranges {
            ensure!(r.start_ms < r.end_ms, "Vùng bắt buộc không hợp lệ");
            let mut covered: Vec<(u64, u64)> = self
                .segments
                .iter()
                .filter(|s| s.kind != "host" && s.source_id == r.source_id)
                .flat_map(|s| if !s.footage_spans.is_empty() {s.footage_spans.iter().map(|v|(v.source_in_ms,v.source_out_ms)).collect::<Vec<_>>()}else{vec![(s.in_ms,s.out_ms.min(s.in_ms.saturating_add(crate::footage_speed::scaled(s.duration_ms,s.source_rate))))]})
                .collect();
            covered.sort_unstable();
            let mut end = r.start_ms;
            for (a, b) in covered {
                if a <= end {
                    end = end.max(b);
                }
            }
            ensure!(
                end >= r.end_ms,
                "Timeline thiếu vùng bắt buộc {}–{} ms",
                r.start_ms,
                r.end_ms
            );
        }
        Ok(())
    }
    pub fn check_duration(&self) -> Result<()> {
        let t = if self.composition.is_some() {self.composition_duration()}else{self.duration_ms()};
        ensure!(
            t >= self.rules.min_seconds * 1000 && t <= self.rules.max_seconds * 1000,
            "Timeline dài {:.1}s, mục tiêu {}–{}s. Điều chỉnh cảnh trước khi xuất.",
            t as f64 / 1000.0,
            self.rules.min_seconds,
            self.rules.max_seconds
        );
        Ok(())
    }
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("REACTION_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join("ReactionStudio")
        })
}
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, serde_json::to_vec_pretty(value)?)?;
    // Windows rename cannot replace an existing file. Preserve a recovery copy until commit.
    let backup = path.with_extension("json.bak");
    if path.exists() {
        fs::copy(path, &backup)?;
        fs::remove_file(path)?;
    }
    if let Err(e) = fs::rename(&temp, path) {
        if backup.exists() {
            let _ = fs::copy(&backup, path);
        }
        return Err(e.into());
    }
    Ok(())
}
pub fn validate_youtube_url(text: &str) -> Result<String> {
    let u = url::Url::parse(text.trim()).context("URL không hợp lệ")?;
    ensure!(u.scheme() == "https", "Chỉ nhận URL HTTPS");
    let host = u.host_str().unwrap_or("").to_lowercase();
    ensure!(
        matches!(
            host.as_str(),
            "youtube.com" | "www.youtube.com" | "m.youtube.com" | "youtu.be"
        ),
        "Chỉ nhận URL YouTube"
    );
    let id = if host == "youtu.be" {
        u.path_segments()
            .and_then(|mut p| p.next())
            .unwrap_or("")
            .to_string()
    } else if u.path() == "/watch" {
        u.query_pairs()
            .find(|(k, _)| k == "v")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default()
    } else {
        let parts: Vec<_> = u.path().trim_matches('/').split('/').collect();
        if parts.len() == 2 && ["shorts", "live", "embed"].contains(&parts[0]) {
            parts[1].into()
        } else {
            String::new()
        }
    };
    ensure!(
        id.len() == 11
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        "URL phải trỏ tới một video, không phải playlist/kênh"
    );
    Ok(format!("https://www.youtube.com/watch?v={id}"))
}
#[derive(Debug, Serialize)]
pub struct FootageAdjustment {
    pub scene_id: String,
    pub old_in_ms: u64,
    pub old_out_ms: u64,
    pub in_ms: u64,
    pub out_ms: u64,
}

/// Extend an unlocked review's original interval to cover its measured voice.
/// Keep all previously selected footage; never cross an excluded range or EOF.
pub fn fit_review_footage(p: &mut Project) -> Result<Vec<FootageAdjustment>> {
    if crate::footage_speed::enabled(p) {crate::footage_speed::fit(p)?;return Ok(vec![]);}
    let mut adjustments = Vec::new();
    // Compute every change before committing, so a later impossible scene
    // cannot leave a partially changed timeline.
    for seg in &p.segments {
        if seg.kind != "review" || seg.duration_ms <= seg.out_ms.saturating_sub(seg.in_ms) {
            continue;
        }
        let source = p.source(&seg.source_id)?;
        let mut left = 0;
        let mut right = source.duration_ms;
        for excluded in &p.rules.excluded_ranges {
            if excluded.source_id != seg.source_id {
                continue;
            }
            ensure!(
                excluded.end_ms <= seg.in_ms || excluded.start_ms >= seg.out_ms,
                "Cảnh {} chạm vùng đã loại",
                seg.id
            );
            if excluded.end_ms <= seg.in_ms {
                left = left.max(excluded.end_ms);
            } else {
                right = right.min(excluded.start_ms);
            }
        }
        ensure!(!seg.locked && right >= seg.out_ms && right.saturating_sub(left) >= seg.duration_ms,
            "Cảnh {}: thoại dài {:.2}s nhưng footage chỉ {:.2}s; {}. Mở rộng điểm cắt, rút ngắn lời thoại hoặc đổi sang cảnh host",
            seg.id, seg.duration_ms as f64 / 1000.0, (seg.out_ms-seg.in_ms) as f64 / 1000.0,
            if seg.locked { "cảnh đang khóa nên không tự đổi điểm cắt" } else { "không đủ hình liên tục trong vùng nguồn được phép" });
        let out_ms = seg.in_ms.saturating_add(seg.duration_ms).min(right);
        let in_ms = seg.in_ms.min(out_ms - seg.duration_ms);
        ensure!(
            in_ms >= left && in_ms <= seg.in_ms && out_ms >= seg.out_ms,
            "Không thể mở rộng an toàn cảnh {}",
            seg.id
        );
        adjustments.push(FootageAdjustment {
            scene_id: seg.id.clone(),
            old_in_ms: seg.in_ms,
            old_out_ms: seg.out_ms,
            in_ms,
            out_ms,
        });
    }
    for change in &adjustments {
        let seg = p
            .segments
            .iter_mut()
            .find(|s| s.id == change.scene_id)
            .expect("existing scene");
        seg.in_ms = change.in_ms;
        seg.out_ms = change.out_ms;
    }
    Ok(adjustments)
}

pub fn fit_budget(p: &mut Project) -> Result<()> {
    let target = p.rules.max_seconds * 1000;
    let mut excess = p.duration_ms().saturating_sub(target);
    let preserve_hook = p.rules.hook_enabled && p.validate_opening_hook().is_ok();
    for (index, s) in p.segments.iter_mut().enumerate().rev() {
        if excess == 0 {
            break;
        }
        if s.kind == "source" && !s.locked && !(preserve_hook && index == 0) {
            // Required footage must never be trimmed by automatic duration fitting.
            let required_end = p
                .rules
                .required_ranges
                .iter()
                .filter(|r| {
                    r.source_id == s.source_id && r.end_ms > s.in_ms && r.start_ms < s.out_ms
                })
                .map(|r| r.end_ms)
                .max()
                .unwrap_or(s.in_ms + 1000);
            let floor = required_end.max(s.in_ms + crate::footage_speed::scaled(if p.rules.intercuts_enabled {p.rules.intercut_min_seconds*1000}else{1000},s.source_rate));
            let cut = excess.min((s.out_ms.saturating_sub(floor) as f64/s.source_rate).floor() as u64);
            s.out_ms -= crate::footage_speed::scaled(cut,s.source_rate);
            s.duration_ms -= cut;
            excess -= cut;
        }
    }
    if excess > 0 {
        bail!(
            "Lời thoại và cảnh khóa vượt ngân sách {}s; rút ngắn script rồi tạo lại audio",
            p.rules.max_seconds
        );
    }
    p.check_duration()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_update_rebinds_bundled_runtime_and_keeps_custom_tools() {
        let temp = tempfile::tempdir().unwrap();
        let previous = temp.path().join("previous release");
        let current = temp.path().join("new release");
        let custom = temp.path().join("custom");
        for root in [&previous, &current, &custom] {
            for suffix in ["tools/ffmpeg.exe", "tools/ffprobe.exe", ".venv/Scripts/python.exe"] {
                let path = root.join(suffix);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, b"fixture").unwrap();
            }
        }
        fs::write(previous.join("release.json"), b"{}").unwrap();
        let mut settings = Settings { ffmpeg:previous.join("tools/ffmpeg.exe").to_string_lossy().into_owned(),
            ffprobe:custom.join("tools/ffprobe.exe").to_string_lossy().into_owned(),
            python:previous.join(".venv/Scripts/python.exe").to_string_lossy().into_owned(),
            voice:"custom voice".into(), cli_path:"custom-cli".into(), ..Default::default() };
        settings.relocate_runtime(&current);
        assert_eq!(Path::new(&settings.ffmpeg), current.join("tools/ffmpeg.exe"));
        assert_eq!(Path::new(&settings.python), current.join(".venv/Scripts/python.exe"));
        assert_eq!(Path::new(&settings.ffprobe), custom.join("tools/ffprobe.exe"));
        settings.python = temp.path().join("deleted release/.venv/Scripts/python.exe").to_string_lossy().into_owned();
        settings.relocate_runtime(&current);
        assert_eq!(Path::new(&settings.python), current.join(".venv/Scripts/python.exe"));
        assert_eq!(settings.voice, "custom voice");
        assert_eq!(settings.cli_path, "custom-cli");
    }
    fn project() -> Project {
        let mut p = Project::default();
        p.rules.min_seconds = 1;
        p.rules.max_seconds = 10;
        p.sources.push(Source {
            id: "s".into(),
            duration_ms: 20000,
            ..Default::default()
        });
        p.segments.push(Segment {
            source_id: "s".into(),
            out_ms: 15000,
            duration_ms: 15000,
            ..Default::default()
        });
        p
    }
    #[test]
    fn opening_hook_is_original_three_to_seven_seconds_and_has_evidence() {
        let mut p = project();
        p.segments = vec![Segment { source_id: "s".into(), in_ms: 12000, out_ms: 17000,
            duration_ms: 5000, evidence: "00:12–00:17: tình huống bất ngờ trong nguồn".into(), ..Default::default() },
            Segment { kind: "host".into(), narration: "Phân tích sau hook".into(), ..Default::default() }];
        p.validate(false).unwrap();
        p.validate_opening_hook().unwrap();
        let mut tight = p.clone();
        tight.rules.max_seconds = 8;
        assert!(fit_budget(&mut tight).is_err());
        assert_eq!(tight.segments[0].duration_ms, 5000, "Do not trim away the chosen hook to fit narration");
        for bad in [2999, 7001] {
            let mut invalid = p.clone(); invalid.segments[0].duration_ms = bad;
            assert!(invalid.validate_opening_hook().is_err());
        }
        let mut invalid = p.clone(); invalid.segments.swap(0, 1);
        assert!(invalid.validate_opening_hook().is_err());
        invalid.rules.hook_enabled = false;
        invalid.validate_opening_hook().unwrap();
        p.segments[0].evidence.clear();
        assert!(p.validate_opening_hook().is_err());
    }
    #[test]
    fn project_defaults_survive_restart_without_source_ranges_or_generated_data() {
        let temp = tempfile::tempdir().unwrap();
        let mut old = project();
        old.rules.prompt = "Phong cách của tôi {duration}".into();
        old.rules.min_seconds = 210; old.rules.max_seconds = 260;
        old.rules.required_ranges.push(TimeRange { source_id: "s".into(), start_ms: 1, end_ms: 2 });
        old.rules.excluded_ranges = old.rules.required_ranges.clone();
        old.output.blur_background = true;
        old.output.avatar_scale = 0.42;
        old.subtitles.highlight = [12, 34, 56];
        old.card_sound=crate::evidence_sfx::CardSoundSettings{enabled:false,style:"pop".into(),gain:0.6};
        let path = temp.path().join("project-defaults.json");
        write_json(&path, &ProjectDefaults::from_project(&old)).unwrap();
        let mut fresh = Project::default();
        ProjectDefaults::load(&path).unwrap().apply(&mut fresh);
        assert_ne!(fresh.id, old.id);
        assert!(fresh.sources.is_empty() && fresh.segments.is_empty() && fresh.rendered.is_none());
        assert!(fresh.rules.required_ranges.is_empty() && fresh.rules.excluded_ranges.is_empty());
        assert_eq!(fresh.rules.prompt, old.rules.prompt);
        assert_eq!(fresh.rules.max_seconds, 260);
        assert!(fresh.output.blur_background);
        assert_eq!(fresh.output.avatar_scale, 0.42);
        assert_eq!(fresh.subtitles.highlight, [12, 34, 56]);
        assert_eq!(fresh.card_sound,old.card_sound);
    }
    #[test]
    fn rejects_non_youtube_and_playlist() {
        assert!(validate_youtube_url("https://evil.test/watch?v=abcdefghijk").is_err());
        assert!(validate_youtube_url("https://youtube.com/playlist?list=abcdefghijk").is_err());
        assert_eq!(
            validate_youtube_url("https://youtu.be/abcdefghijk?t=9").unwrap(),
            "https://www.youtube.com/watch?v=abcdefghijk"
        );
    }
    #[test]
    fn fit_does_not_trim_required_or_locked() {
        let mut p = project();
        fit_budget(&mut p).unwrap();
        assert_eq!(p.duration_ms(), 10000);
        let mut p = project();
        p.segments[0].locked = true;
        assert!(fit_budget(&mut p).is_err());
        let mut p = project();
        p.rules.required_ranges.push(TimeRange {
            source_id: "s".into(),
            start_ms: 12000,
            end_ms: 14000,
        });
        assert!(fit_budget(&mut p).is_err());
    }
    #[test]
    fn rejects_exclusion_and_out_of_bounds() {
        let mut p = project();
        p.rules.excluded_ranges.push(TimeRange {
            source_id: "s".into(),
            start_ms: 4000,
            end_ms: 5000,
        });
        assert!(p.validate(false).is_err());
        p.rules.excluded_ranges.clear();
        p.segments[0].out_ms = 30000;
        p.segments[0].duration_ms = 30000;
        assert!(p.validate(false).is_err());
    }
    #[test]
    fn project_roundtrip() {
        let t = tempfile::tempdir().unwrap();
        let mut p = project();
        p.root = t.path().to_owned();
        p.save().unwrap();
        p.title = "Kiểm thử".into();
        p.save().unwrap();
        assert_eq!(
            Project::load(&p.root.join("project.json")).unwrap().title,
            p.title
        );
    }
}
