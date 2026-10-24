//! Portable layer preferences. No scene IDs, media paths or timeline positions.
use crate::{composition::{Composition, LayerClip, Transform}, model::Project};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LayerDefaults {
    pub initialized: bool,
    pub free_aspect: bool,
    pub sync_parameters: Vec<bool>,
    pub presets: BTreeMap<String, LayerParameters>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerParameters {
    pub transform: Transform,
    pub z: i32,
    pub gain: f32,
    pub fade_in_ms: u64,
    pub fade_out_ms: u64,
}

impl LayerParameters {
    fn from_clip(c: &LayerClip) -> Self {
        Self { transform: c.transform.clone(), z: c.z, gain: c.gain,
            fade_in_ms: c.fade_in_ms, fade_out_ms: c.fade_out_ms }
    }
    fn apply(&self, c: &mut LayerClip) {
        c.transform = self.transform.clone();
        c.z = self.z;
        c.gain = self.gain;
        c.fade_in_ms = self.fade_in_ms.min(c.duration_ms);
        c.fade_out_ms = self.fade_out_ms.min(c.duration_ms);
    }
}

impl LayerDefaults {
    fn key(p: &Project, c: &LayerClip) -> Option<String> {
        let scene = p.segments.iter().find(|s| s.id == c.scene_id)?;
        // Preserve the distinct full-host and corner-review layouts. Evidence
        // slots stay separate so remembering card 1 never stacks every card on it.
        let slot = match &c.evidence_id {
            Some(id) => scene.evidence_items.iter().filter(|e| e.enabled).position(|e| &e.id == id)?.to_string(),
            None => String::new(),
        };
        Some(format!("{}:{}:{slot}", scene.kind, c.track))
    }

    pub fn from_project(p: &Project) -> Self {
        let mut result = p.layer_defaults.clone();
        result.sync_parameters = p.timeline.tracks.iter().map(|t| t.sync_parameters).collect();
        // Import existing edits from projects created before portable preferences.
        // Only differences are remembered; automatic layouts remain adaptive.
        if let Some(comp) = p.composition.as_ref().filter(|_| !result.initialized) {
            let generated = Composition::from_project(p);
            for c in &comp.clips {
                let Some(base) = generated.clips.iter().find(|b| b.id == c.id) else { continue; };
                let Some(key) = Self::key(p, c) else { continue; };
                if !result.presets.contains_key(&key) && LayerParameters::from_clip(c) != LayerParameters::from_clip(base) {
                    result.presets.insert(key, LayerParameters::from_clip(c));
                }
            }
        }
        result.initialized = true;
        result
    }

    pub fn apply(&self, p: &Project, comp: &mut Composition) {
        for c in &mut comp.clips {
            if let Some(pref) = Self::key(p, c).and_then(|key| self.presets.get(&key)) {
                pref.apply(c);
            }
        }
    }
}

impl Project {
    /// The selected edit wins when several existing scenes have different values.
    pub fn remember_layer_parameters(&mut self, id: &str) {
        let targets = self.layer_parameter_targets(id, true);
        let values: Vec<_> = self.composition.as_ref().into_iter().flat_map(|c| &c.clips)
            .filter(|c| targets.contains(&c.id))
            .filter_map(|c| LayerDefaults::key(self, c).map(|key| (key, LayerParameters::from_clip(c))))
            .collect();
        self.layer_defaults.presets.extend(values);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::{ProjectDefaults, Segment, Source, write_json}, timeline};

    fn sample() -> Project {
        Project { sources: vec![Source { id: "src".into(), width: 1920, height: 1080, ..Default::default() }],
            segments: ["host", "review", "source"].into_iter().enumerate().map(|(i, kind)| Segment {
                id: format!("old-{i}"), source_id: "src".into(), kind: kind.into(),
                duration_ms: 5000, out_ms: 5000, ..Default::default()
            }).collect(), ..Default::default() }
    }

    #[test]
    fn layer_preferences_survive_restart_and_apply_when_new_script_arrives() {
        let temp = tempfile::tempdir().unwrap();
        let mut old = sample(); old.ensure_composition();
        let host = "old-0-1"; let corner = "old-1-1";
        let host_t = Transform { x: 0.6, y: 0.8, width: 0.8, height: 0.45, rotation: 7., opacity: 0.9 };
        let corner_t = Transform { x: 0.85, y: 0.75, width: 0.2, height: 0.1125, ..Default::default() };
        assert!(old.apply_layer_transform(host, &host_t)); old.remember_layer_parameters(host);
        assert!(old.apply_layer_transform(corner, &corner_t)); old.remember_layer_parameters(corner);
        old.layer_defaults.free_aspect = true;
        old.timeline.tracks[timeline::AVATAR].sync_parameters = true;
        old.timeline.tracks[timeline::AVATAR].locked = true;
        let path = temp.path().join("defaults.json");
        write_json(&path, &ProjectDefaults::from_project(&old)).unwrap();
        let stored = std::fs::read_to_string(&path).unwrap();
        assert!(!stored.contains("old-0") && !stored.contains("media_in_ms"));
        let mut next = Project::default(); ProjectDefaults::load(&path).unwrap().apply(&mut next);
        assert!(next.sources.is_empty() && next.segments.is_empty() && next.composition.is_none());
        assert!(next.layer_defaults.free_aspect && next.timeline.tracks[timeline::AVATAR].sync_parameters);
        assert!(!next.timeline.tracks[timeline::AVATAR].locked);
        let content = sample(); next.sources = content.sources; next.segments = content.segments;
        for s in &mut next.segments { s.id = s.id.replace("old", "new"); }
        next.ensure_composition();
        let comp = next.composition.as_ref().unwrap();
        assert_eq!(comp.clips.iter().find(|c| c.id == "new-0-1").unwrap().transform, host_t);
        assert_eq!(comp.clips.iter().find(|c| c.id == "new-1-1").unwrap().transform, corner_t);
        next.segments[0].duration_ms = 6000; next.ensure_composition();
        assert_eq!(next.composition.as_ref().unwrap().clips.iter().find(|c| c.id == "new-0-1").unwrap().transform, host_t);
    }

    #[test]
    fn old_defaults_load_and_existing_project_edits_are_imported_without_changing_it() {
        let defaults: ProjectDefaults = serde_json::from_str("{}").unwrap();
        assert!(defaults.layer_defaults.presets.is_empty());
        let mut p = sample(); p.ensure_composition();
        let c = p.composition.as_mut().unwrap().clips.iter_mut().find(|c| c.id == "old-0-1").unwrap();
        c.transform.x = 0.82;
        let before = serde_json::to_vec(&p).unwrap();
        let defaults = ProjectDefaults::from_project(&p);
        assert_eq!(defaults.layer_defaults.presets["host:1:"].transform.x, 0.82);
        assert_eq!(serde_json::to_vec(&p).unwrap(), before);
    }

    #[test]
    fn upgrade_imports_recent_layer_edits_without_overwriting_general_preferences() {
        let temp = tempfile::tempdir().unwrap();
        let mut old = sample(); old.root = temp.path().join("recent"); old.ensure_composition();
        old.composition.as_mut().unwrap().clips.iter_mut().find(|c| c.id == "old-0-1").unwrap().transform.x = 0.72;
        old.save().unwrap();
        let project_path = old.root.join("project.json");
        let previous_bytes = std::fs::read(&project_path).unwrap();
        let settings_path = temp.path().join("project-defaults.json");
        std::fs::write(&settings_path, r#"{"output":{"portrait":false,"avatar_scale":0.35},"rules":{"min_seconds":100,"max_seconds":120}}"#).unwrap();
        let loaded = ProjectDefaults::load_with_layer_fallback(&settings_path, Some(&project_path)).unwrap();
        assert!(!loaded.output.portrait); assert_eq!(loaded.output.avatar_scale, 0.35);
        assert_eq!(loaded.rules.max_seconds, 120);
        assert_eq!(loaded.layer_defaults.presets["host:1:"].transform.x, 0.72);
        assert_eq!(std::fs::read(&project_path).unwrap(), previous_bytes);
        write_json(&settings_path, &loaded).unwrap();
        let next = ProjectDefaults::load_with_layer_fallback(&settings_path, Some(std::path::Path::new("missing-project.json"))).unwrap();
        assert_eq!(next.layer_defaults.presets["host:1:"].transform.x, 0.72);
    }

    #[test]
    fn audio_preferences_clamp_fades_and_do_not_copy_timing_or_media() {
        let mut p = sample(); p.segments[0].audio = Some("old-voice.wav".into()); p.ensure_composition();
        let c = p.composition.as_mut().unwrap().clips.iter_mut().find(|c| c.id == "old-0-2").unwrap();
        c.gain = 0.4; c.fade_in_ms = 1800; c.fade_out_ms = 2400;
        c.start_ms = 800; c.media_in_ms = 600; c.linked = false;
        p.remember_layer_parameters("old-0-2");
        let defaults = ProjectDefaults::from_project(&p);
        let mut next = sample(); defaults.apply(&mut next);
        next.segments[0].duration_ms = 1000;
        next.segments[0].audio = Some("new-voice.wav".into()); next.ensure_composition();
        let c = next.composition.as_ref().unwrap().clips.iter().find(|c| c.id == "old-0-2").unwrap();
        assert_eq!(c.gain, 0.4); assert_eq!((c.fade_in_ms, c.fade_out_ms), (1000, 1000));
        assert_eq!((c.start_ms, c.media_in_ms), (0, 0));
        assert_eq!(c.media.as_deref(), Some(std::path::Path::new("new-voice.wav")));
        assert!(c.linked);
    }

    #[test]
    fn evidence_preferences_keep_card_slots_and_annotations_separate() {
        use crate::evidence::EvidenceItem;
        let mut p = sample();
        p.segments[0].evidence_items = (0..2).map(|i| EvidenceItem {
            id: format!("e{i}"), asset: Some(format!("image{i}.png").into()),
            annotation_asset: Some(format!("annotation{i}.png").into()), ..Default::default()
        }).collect();
        p.ensure_composition();
        let id = p.composition.as_ref().unwrap().clips.iter()
            .find(|c| c.track == timeline::EVIDENCE && c.evidence_id.as_deref() == Some("e0")).unwrap().id.clone();
        let moved = Transform { x: 0.22, width: 0.35, ..Default::default() };
        p.apply_layer_transform(&id, &moved); p.remember_layer_parameters(&id);
        let untouched = p.composition.as_ref().unwrap().clips.iter()
            .find(|c| c.track == timeline::EVIDENCE && c.evidence_id.as_deref() == Some("e1")).unwrap().transform.clone();
        let mut next = sample(); ProjectDefaults::from_project(&p).apply(&mut next);
        next.segments[0].evidence_items = p.segments[0].evidence_items.clone();
        for e in &mut next.segments[0].evidence_items { e.id = format!("new-{}", e.id); }
        next.ensure_composition();
        for c in &next.composition.as_ref().unwrap().clips {
            if c.track == timeline::EVIDENCE || c.track == timeline::ANNOTATION {
                if c.evidence_id.as_deref() == Some("new-e0") { assert_eq!(c.transform, moved); }
                if c.evidence_id.as_deref() == Some("new-e1") { assert_eq!(c.transform, untouched); }
            }
        }
    }
}
