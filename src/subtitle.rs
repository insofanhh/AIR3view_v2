use crate::model::{Project, SubtitleStyle, Word};
use anyhow::Result;
use std::{fs, path::Path};

pub(crate) fn ass_time(ms: u64) -> String {
    format!(
        "{}:{:02}:{:02}.{:02}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms / 10 % 100
    )
}
fn srt_time(ms: u64, separator: char) -> String {
    format!(
        "{:02}:{:02}:{:02}{}{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        separator,
        ms % 1000
    )
}
fn color(c: [u8; 3]) -> String {
    format!("&H00{:02X}{:02X}{:02X}", c[2], c[1], c[0])
}
/// Never interpret imported captions as ASS control tags.
pub(crate) fn safe(text: &str) -> String {
    text.replace('\\', "／")
        .replace('{', "（")
        .replace('}', "）")
        .replace(['\r', '\n'], " ")
}
pub fn estimated_words(text: &str, duration: u64) -> Vec<Word> {
    let items: Vec<_> = text.split_whitespace().collect();
    let count = items.len() as u64;
    if count == 0 {
        return vec![];
    }
    items
        .into_iter()
        .enumerate()
        .map(|(i, t)| Word {
            text: t.into(),
            start_ms: duration * i as u64 / count,
            end_ms: duration * (i as u64 + 1) / count,
        })
        .collect()
}
/// Same bottom-center anchor in ASS, layered preview, and the static layout preview.
pub fn anchor(style:&SubtitleStyle,w:u32,h:u32)->[f32;2] {
    [w as f32/2.+style.offset_x as f32,h as f32-style.bottom_margin as f32+style.offset_y as f32]
}
pub fn write(p: &Project, folder: &Path) -> Result<()> {
    fs::create_dir_all(folder)?;
    let (w, h) = p.output.dimensions();
    let st = &p.subtitles;
    let font = st.font.replace([',', '\n', '\r'], " ");
    let mut ass=format!("[Script Info]\nScriptType: v4.00+\nPlayResX: {w}\nPlayResY: {h}\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,{font},{},{},{},{},&H80000000,-1,0,0,0,100,100,0,0,1,{},1,2,40,40,{},1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",st.size,color(st.highlight),color(st.color),color(st.outline),st.outline_width,st.bottom_margin);
    // Imported/ASR words have variable width; permit soft wrapping in addition
    // to the explicit two-line split, so narrow portraits do not clip text.
    ass = ass.replacen("WrapStyle: 2", "WrapStyle: 0", 1);
    let mut srt = String::new();
    let mut vtt = "WEBVTT\n\n".to_string();
    let timed_segments:Vec<(u64,crate::model::Segment)>=if p.composition.is_some() {
        let mut clips=p.composition_snapshot().clips;clips.sort_by_key(|c|c.start_ms);
        clips.iter().filter(|c|c.track==crate::timeline::CAPTIONS&&c.enabled(p)).filter_map(|c|{
            let mut seg=p.segments.iter().find(|s|s.id==c.scene_id)?.clone();
            let text=if seg.caption.trim().is_empty(){&seg.narration}else{&seg.caption};
            let words=if seg.words.is_empty(){estimated_words(text,seg.duration_ms)}else{seg.words.clone()};
            seg.words=words.into_iter().filter(|w|w.end_ms>c.cue_in_ms&&w.start_ms<c.cue_in_ms+c.duration_ms).map(|w|Word{text:w.text,start_ms:w.start_ms.saturating_sub(c.cue_in_ms),end_ms:w.end_ms.saturating_sub(c.cue_in_ms).min(c.duration_ms)}).collect();
            if seg.words.is_empty(){return None;}
            seg.duration_ms=c.duration_ms;Some((c.start_ms,seg))
        }).collect()
    } else {
        let mut offset=0;p.segments.iter().map(|s|{let at=offset;offset+=s.duration_ms;(at,s.clone())}).collect()
    };
    let mut n = 1;
    for (offset,seg) in timed_segments {
        if p.timeline.tracks[crate::timeline::CAPTIONS].muted || seg.suppress_captions {continue;}
        let text = if seg.caption.trim().is_empty() {
            &seg.narration
        } else {
            &seg.caption
        };
        let words = if !seg.words.is_empty() {
            seg.words.clone()
        } else {
            estimated_words(text, seg.duration_ms)
        };
        for group in words.chunks(st.words_per_line.clamp(2, 12) * 2) {
            let start = offset + group[0].start_ms;
            let end = offset + group.last().unwrap().end_ms.min(seg.duration_ms);
            if end <= start {
                continue;
            }
            let mut plain = String::new();
            let mut formatted = String::new();
            for (i, word) in group.iter().enumerate() {
                if i > 0 {
                    plain.push(' ');
                    formatted.push_str(if i == st.words_per_line { "\\N" } else { " " });
                }
                plain.push_str(&word.text);
                if st.karaoke {
                    formatted.push_str(&format!(
                        "{{\\k{}}}",
                        (word.end_ms.saturating_sub(word.start_ms) / 10).max(1)
                    ));
                }
                formatted.push_str(&safe(&word.text));
            }
            if !st.karaoke {
                formatted = format!("{{\\c{}}}{formatted}", color(st.color));
            }
            let mut margin_l=0;let mut margin_r=0;
            if let Some(t)=crate::framed_layout::caption_box(p,&seg) {
                let left=(t.x-t.width/2.)*w as f32;let right=(t.x+t.width/2.)*w as f32;
                let top=(t.y-t.height/2.)*h as f32;let bottom=(t.y+t.height/2.)*h as f32;
                let x=t.x*w as f32+st.offset_x as f32;
                let floor=(top+st.size).min(bottom-4.);
                let y=(bottom-st.bottom_margin as f32+st.offset_y as f32).clamp(floor,bottom-4.);
                margin_l=(left+12.).max(0.) as u32;margin_r=(w as f32-right+12.).max(0.) as u32;
                formatted=format!("{{\\an2\\pos({x:.1},{y:.1})\\clip({left:.0},{top:.0},{right:.0},{bottom:.0})}}{formatted}");
            }else if st.offset_x!=0 || st.offset_y!=0 {
                let [x,y]=anchor(st,w,h);
                formatted=format!("{{\\an2\\pos({x:.1},{y:.1})}}{formatted}");
            }
            ass.push_str(&format!(
                "Dialogue: 0,{},{},Default,,{margin_l},{margin_r},0,,{}\n",
                ass_time(start),
                ass_time(end),
                formatted
            ));
            srt.push_str(&format!(
                "{n}\n{} --> {}\n{plain}\n\n",
                srt_time(start, ','),
                srt_time(end, ',')
            ));
            vtt.push_str(&format!(
                "{} --> {}\n{plain}\n\n",
                srt_time(start, '.'),
                srt_time(end, '.')
            ));
            n += 1;
        }
    }
    fs::write(folder.join("captions.ass"), ass)?;
    fs::write(folder.join("captions.srt"), srt)?;
    fs::write(folder.join("captions.vtt"), vtt)?;
    Ok(())
}
/// Align authoritative script text to recognized word anchors. ASR supplies timing,
/// not replacement spelling. Unmatched words use only the gap between anchors.
/// A poor match falls back to an explicitly estimated take instead of false precision.
pub fn align_text(text: &str, spoken: &[Word], duration: u64) -> (Vec<Word>, bool) {
    let target:Vec<_>=text.split_whitespace().collect();
    let norm=|s:&str|s.chars().filter(|c|c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
    let (n,m)=(target.len(),spoken.len());
    if n==0 { return (vec![],true); }
    if m==0 || n.saturating_mul(m)>500_000 {return (estimated_words(text,duration),true);}
    let a:Vec<_>=target.iter().map(|s|norm(s)).collect();
    let b:Vec<_>=spoken.iter().map(|s|norm(&s.text)).collect();
    let mut dp=vec![0u16;(n+1)*(m+1)];
    for i in (0..n).rev() {for j in (0..m).rev() {
        dp[i*(m+1)+j]=if !a[i].is_empty() && a[i]==b[j] {1+dp[(i+1)*(m+1)+j+1]}
            else {dp[(i+1)*(m+1)+j].max(dp[i*(m+1)+j+1])};
    }}
    let mut anchors=vec![];let(mut i,mut j)=(0,0);
    while i<n && j<m {
        if !a[i].is_empty() && a[i]==b[j] {anchors.push((i,j));i+=1;j+=1;}
        else if dp[(i+1)*(m+1)+j]>=dp[i*(m+1)+j+1] {i+=1;} else {j+=1;}
    }
    if anchors.len()*100<n*65 || duration<n as u64 {return (estimated_words(text,duration),true);}
    let exact=anchors.len()==n && n==m;
    let mut words=vec![];let mut next=0;let mut at=0;
    for (index,observed) in anchors.iter().map(|&(i,j)|(i,Some(j))).chain(std::iter::once((n,None))) {
        let stop=observed.map(|j|spoken[j].start_ms.min(duration)).unwrap_or(duration);
        let count=index-next;
        if stop<at || (count>0 && stop-at<count as u64) {return (estimated_words(text,duration),true);}
        for k in 0..count {
            words.push(Word{text:target[next+k].into(),start_ms:at+(stop-at)*k as u64/count as u64,
                end_ms:at+(stop-at)*(k+1) as u64/count as u64});
        }
        if let Some(j)=observed {
            let end=spoken[j].end_ms.min(duration);
            if end<=stop {return (estimated_words(text,duration),true);}
            words.push(Word{text:target[index].into(),start_ms:stop,end_ms:end});at=end;next=index+1;
        }
    }
    (words,!exact)
}

pub fn preview_color(style: &SubtitleStyle) -> [u8; 3] {
    style.color
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Segment;
    #[test]
    fn escaping_and_timeline() {
        let t = tempfile::tempdir().unwrap();
        let p = Project {
            segments: vec![
                Segment {
                    duration_ms: 2000,
                    caption: "Xin {\\pos(0,0)} chào".into(),
                    ..Default::default()
                },
                Segment {
                    duration_ms: 1000,
                    caption: "Kết thúc".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        write(&p, t.path()).unwrap();
        let a = fs::read_to_string(t.path().join("captions.ass")).unwrap();
        assert!(!a.contains("{\\pos"));
        let s = fs::read_to_string(t.path().join("captions.srt")).unwrap();
        assert!(s.contains("00:00:02,000 --> 00:00:03,000"));
    }
}
