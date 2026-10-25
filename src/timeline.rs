//! Linked scene editing. Source footage and generated media have independent
//! in-points; a split must never restart the second half of a WAV/avatar.
use crate::{model::*, subtitle};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};

pub const VIDEO: usize = 0;
pub const AVATAR: usize = 1;
pub const VOICE: usize = 2;
pub const ORIGINAL: usize = 3;
pub const CAPTIONS: usize = 4;
pub const GRAPHICS: usize = 5;
pub const BACKGROUND:usize=6;
pub const EVIDENCE:usize=7;
pub const ANNOTATION:usize=8;
pub const SFX:usize=9;
pub const TRACK_COUNT:usize=10;
#[derive(Clone,Copy,Debug)]
pub enum TrackRole {Video,Avatar,Voice,Original,Captions,Graphics,Background,Evidence,Annotation,Sfx}
pub const TRACK_ROLES:[TrackRole;TRACK_COUNT]=[TrackRole::Video,TrackRole::Avatar,TrackRole::Voice,TrackRole::Original,TrackRole::Captions,TrackRole::Graphics,TrackRole::Background,TrackRole::Evidence,TrackRole::Annotation,TrackRole::Sfx];
impl TrackRole {pub fn z(self)->i32{match self{Self::Background=>0,Self::Video=>10,Self::Evidence=>15,Self::Annotation=>16,Self::Avatar=>20,Self::Graphics=>30,_=>40}}}
pub const TRACK_NAMES: [&str; TRACK_COUNT] = [
    "Video",
    "Nhân vật",
    "Giọng VieNeu",
    "Tiếng gốc",
    "Phụ đề",
    "Thẻ thông tin",
    "Nền",
    "Dẫn chứng",
    "Chú thích",
    "SFX thẻ",
];

#[derive(Clone, Default, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct TrackState {
    pub muted: bool,
    pub locked: bool,
    pub sync_parameters: bool,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct TimelineSettings {
    #[serde(deserialize_with="deserialize_tracks")]
    pub tracks: Vec<TrackState>,
    pub snapping: bool,
}
impl Default for TimelineSettings {
    fn default() -> Self {
        Self {
            tracks: vec![TrackState::default();TRACK_COUNT],
            snapping: true,
        }
    }
}
fn deserialize_tracks<'de,D:serde::Deserializer<'de>>(d:D)->std::result::Result<Vec<TrackState>,D::Error>{let mut tracks=Vec::<TrackState>::deserialize(d)?;tracks.resize(TRACK_COUNT,TrackState::default());tracks.truncate(TRACK_COUNT);Ok(tracks)}
pub fn starts(p: &Project) -> Vec<u64> {
    let mut time = 0;
    p.segments
        .iter()
        .map(|s| {
            let start = time;
            time += s.duration_ms;
            start
        })
        .collect()
}
pub fn locate(p: &Project, ms: u64) -> Option<(usize, u64)> {
    let mut start = 0;
    for (i, s) in p.segments.iter().enumerate() {
        if ms < start + s.duration_ms || i + 1 == p.segments.len() {
            return Some((
                i,
                ms.saturating_sub(start)
                    .min(s.duration_ms.saturating_sub(1)),
            ));
        }
        start += s.duration_ms;
    }
    None
}
pub fn has_track(s: &Segment, track: usize) -> bool {
    match track {
        VIDEO => s.kind != "host",
        AVATAR => s.kind != "source",
        VOICE => s.kind != "source",
        ORIGINAL => s.kind == "source" || s.kind == "review",
        CAPTIONS => !s.caption.trim().is_empty() || !s.narration.trim().is_empty(),
        GRAPHICS => !s.graphics.is_empty(),
        EVIDENCE => s.evidence_items.iter().any(|e|e.enabled),
        ANNOTATION => s.evidence_items.iter().any(|e|e.enabled&&!e.annotations.is_empty()),
        _ => false,
    }
}
pub fn editable(p: &Project, index: usize) -> Result<()> {
    let s = p.segments.get(index).context("Chưa chọn cảnh")?;
    ensure!(!s.locked, "Cảnh đang khóa");
    for (i, track) in p.timeline.tracks.iter().enumerate() {
        ensure!(
            !track.locked || !has_track(s, i),
            "Track {} đang khóa; mở khóa để sửa nhóm cảnh liên kết",
            TRACK_NAMES[i]
        );
    }
    Ok(())
}
fn slice_segment(s: &Segment, from: u64, to: u64) -> Segment {
    let mut part = s.clone();
    part.duration_ms = to - from;
    part.evidence_items=s.evidence_items.iter().filter_map(|e|{
        let end=(e.start_ms+e.duration_ms).min(s.duration_ms);
        if e.start_ms>=to||end<=from{return None;}
        let mut part=e.clone();let skip=from.saturating_sub(e.start_ms);
        part.start_ms=e.start_ms.saturating_sub(from);part.duration_ms=end.min(to)-e.start_ms.max(from);
        part.motion_in_ms+=skip;part.motion_duration_ms=e.motion_duration_ms.max(e.duration_ms);
        part.timing_manual=true;Some(part)
    }).collect();
    if s.kind != "host" {
        part.footage_spans=crate::footage_speed::slice(s,from,to);
        part.footage_anchors=s.footage_anchors.iter().filter(|a|a.timeline_ms>=from&&a.timeline_ms<to).cloned().map(|mut a|{a.timeline_ms-=from;a}).collect();
        let source_at=|t|s.footage_spans.iter().find(|v|t>=v.start_ms&&t<=v.start_ms+v.duration_ms)
            .map(|v|v.source_in_ms+crate::footage_speed::scaled(t-v.start_ms,v.rate()))
            .unwrap_or(s.in_ms+crate::footage_speed::scaled(t,s.source_rate));
        part.in_ms = source_at(from).min(s.out_ms.saturating_sub(1));
        part.out_ms = if to == s.duration_ms {
            s.out_ms
        } else {
            source_at(to).min(s.out_ms)
        };
        if part.kind == "source" {
            part.out_ms = part.in_ms + crate::footage_speed::scaled(part.duration_ms,s.source_rate);
        }
    }
    part.asset_start_ms = s.asset_start_ms + from;
    part.avatar_start_ms = s.avatar_start_ms + from;
    part.asset_window = true;
    if part.asset_narration.is_empty() {
        part.asset_narration = s.narration.clone();
    }
    let text = if s.caption.trim().is_empty() {
        &s.narration
    } else {
        &s.caption
    };
    let words = if s.words.is_empty() {
        subtitle::estimated_words(text, s.duration_ms)
    } else {
        s.words.clone()
    };
    // Assign each word to one half by its midpoint; crossing words aren't duplicated.
    part.words = words
        .into_iter()
        .filter(|w| {
            let midpoint = w.start_ms + (w.end_ms - w.start_ms) / 2;
            midpoint >= from && midpoint < to
        })
        .map(|mut w| {
            w.start_ms = w.start_ms.saturating_sub(from);
            w.end_ms = w.end_ms.saturating_sub(from).min(to - from);
            w
        })
        .filter(|w| w.start_ms < w.end_ms)
        .collect();
    let sliced_text = part
        .words
        .iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    part.suppress_captions = part.words.is_empty();
    if s.caption.trim().is_empty() {
        // Keep a valid script in a slice that consists only of a speech pause.
        if !sliced_text.is_empty() {
            part.narration = sliced_text;
        }
    } else {
        part.caption = sliced_text;
        let narration = subtitle::estimated_words(&s.narration, s.duration_ms)
            .into_iter()
            .filter(|w| {
                w.start_ms + (w.end_ms - w.start_ms) / 2 >= from
                    && w.start_ms + (w.end_ms - w.start_ms) / 2 < to
            })
            .map(|w| w.text)
            .collect::<Vec<_>>()
            .join(" ");
        if !narration.is_empty() {
            part.narration = narration;
        }
    }
    part.graphics = s
        .graphics
        .iter()
        .filter_map(|g| {
            let mut g = g.clone();
            g.sound_suppressed |= g.start_ms < from;
            g.start_ms = g.start_ms.max(from).saturating_sub(from);
            g.end_ms = g.end_ms.min(to).saturating_sub(from);
            (g.start_ms < g.end_ms).then_some(g)
        })
        .collect();
    part
}
pub fn split(p: &mut Project, index: usize, local_ms: u64) -> Result<()> {
    editable(p, index)?;
    let s = &p.segments[index];
    ensure!(
        local_ms >= 100 && local_ms + 100 <= s.duration_ms,
        "Đặt playhead cách hai đầu cảnh ít nhất 0.1 giây"
    );
    let left = slice_segment(s, 0, local_ms);
    let mut right = slice_segment(s, local_ms, s.duration_ms);
    right.id = uuid::Uuid::new_v4().to_string();
    p.segments[index] = left;
    p.segments.insert(index + 1, right);
    p.rendered = None;
    Ok(())
}
pub fn trim(p: &mut Project, index: usize, from: u64, to: u64) -> Result<()> {
    editable(p, index)?;
    for i in index + 1..p.segments.len() {
        editable(p, i)?;
    }
    ensure!(
        from < to && to <= p.segments[index].duration_ms && to - from >= 100,
        "Giữ lại ít nhất 0.1 giây trong cảnh"
    );
    p.segments[index] = slice_segment(&p.segments[index], from, to);
    p.rendered = None;
    Ok(())
}
pub fn delete(p: &mut Project, index: usize) -> Result<()> {
    editable(p, index)?;
    // Removing before a locked scene would shift that scene in output time.
    for i in index + 1..p.segments.len() {
        editable(p, i)?;
    }
    p.segments.remove(index);
    p.rendered = None;
    Ok(())
}
pub fn move_scene(p: &mut Project, from: usize, to: usize) -> Result<()> {
    ensure!(
        from < p.segments.len() && to < p.segments.len(),
        "Vị trí cảnh không hợp lệ"
    );
    for i in from.min(to)..=from.max(to) {
        editable(p, i)?;
    }
    let s = p.segments.remove(from);
    p.segments.insert(to, s);
    p.rendered = None;
    Ok(())
}
pub fn insert(p: &mut Project, after: usize, mut s: Segment) -> Result<usize> {
    let at = (after + 1).min(p.segments.len());
    for i in at..p.segments.len() {
        editable(p, i)?;
    }
    // Also respect locked tracks when inserting the clipboard contents.
    for (i, name) in TRACK_NAMES.iter().enumerate() {
        ensure!(
            !p.timeline.tracks[i].locked || !has_track(&s, i),
            "Track {} đang khóa",
            name
        );
    }
    s.id = uuid::Uuid::new_v4().to_string();
    s.locked = false;
    p.segments.insert(at, s);
    p.rendered = None;
    Ok(at)
}
pub fn snap(ms: u64, p: &Project, threshold_ms: u64) -> u64 {
    starts(p)
        .into_iter()
        .chain([p.duration_ms()])
        .min_by_key(|t| t.abs_diff(ms))
        .filter(|t| t.abs_diff(ms) <= threshold_ms)
        .unwrap_or(ms)
}

#[derive(Default)]
pub struct EditHistory {
    pub past: Vec<Project>,
    pub future: Vec<Project>,
}
impl EditHistory {
    pub fn record(&mut self, before: Project) {
        if self
            .past
            .last()
            .is_none_or(|p| serde_json::to_vec(p).ok() != serde_json::to_vec(&before).ok())
        {
            self.past.push(before);
            if self.past.len() > 80 {
                self.past.remove(0);
            }
        }
        self.future.clear();
    }
    pub fn undo(&mut self, current: &mut Project) -> bool {
        if let Some(previous) = self.past.pop() {
            self.future.push(std::mem::replace(current, previous));
            true
        } else {
            false
        }
    }
    pub fn redo(&mut self, current: &mut Project) -> bool {
        if let Some(next) = self.future.pop() {
            self.past.push(std::mem::replace(current, next));
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn project() -> Project {
        Project {
            sources: vec![Source {
                id: "s".into(),
                duration_ms: 20000,
                ..Default::default()
            }],
            segments: vec![Segment {
                kind: "review".into(),
                source_id: "s".into(),
                in_ms: 1000,
                out_ms: 11000,
                duration_ms: 10000,
                narration: "Một hai ba bốn năm sáu bảy tám chín mười".into(),
                audio: Some("voice.wav".into()),
                avatar: Some("avatar.mkv".into()),
                graphics: vec![GraphicCue {
                    start_ms: 3000,
                    end_ms: 7000,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }
    }
    #[test]
    fn split_trim_preserve_media_offsets_words_and_graphics() {
        let mut p = project();
        split(&mut p, 0, 4000).unwrap();
        assert_eq!(p.duration_ms(), 10000);
        assert_eq!(
            (p.segments[1].in_ms, p.segments[1].asset_start_ms),
            (5000, 4000)
        );
        assert_eq!(p.segments[0].graphics[0].end_ms, 4000);
        assert_eq!(p.segments[1].graphics[0].end_ms, 3000);
        assert_eq!(p.segments[1].words[0].text, "năm");
        trim(&mut p, 1, 1000, 5000).unwrap();
        assert_eq!(
            (p.segments[1].asset_start_ms, p.segments[1].in_ms),
            (5000, 6000)
        );
        assert_eq!(p.segments[1].words[0].text, "sáu");
        p.validate(false).unwrap();
        assert_eq!(locate(&p, 4000), Some((1, 0)));
    }
    #[test]
    fn history_locked_linked_edits_clipboard_and_snap() {
        let mut p = project();
        let mut history = EditHistory::default();
        history.record(p.clone());
        split(&mut p, 0, 4000).unwrap();
        assert_eq!(snap(4099, &p, 100), 4000);
        p.segments[1].locked = true;
        assert!(move_scene(&mut p, 0, 1).is_err());
        assert!(delete(&mut p, 0).is_err());
        p.segments[1].locked = false;
        p.timeline.tracks[VOICE].locked = true;
        assert!(trim(&mut p, 0, 100, 2000).is_err());
        p.timeline.tracks[VOICE].locked = false;
        let clip = p.segments[1].clone();
        insert(&mut p, 1, clip).unwrap();
        assert_ne!(p.segments[1].id, p.segments[2].id);
        assert!(history.undo(&mut p));
        assert_eq!(p.segments.len(), 1);
        assert!(history.redo(&mut p));
        assert_eq!(p.segments.len(), 3);
    }
}
