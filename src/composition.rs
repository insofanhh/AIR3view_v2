//! Versioned, non-destructive editing state. Coordinates are normalized canvas
//! coordinates (centre/size), never window pixels. Scripts remain source data.
use crate::{media, model::*, timeline};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const BACKGROUND: usize = timeline::BACKGROUND;
pub fn label(p:&Project,c:&LayerClip)->String {
    c.evidence_id.as_ref().and_then(|id|p.segments.iter().find(|s|s.id==c.scene_id).and_then(|s|s.evidence_items.iter().find(|e|&e.id==id))).map(|e|format!("{} · {}",name(c.track),e.label)).unwrap_or_else(||name(c.track).into())
}
pub fn name(track: usize) -> &'static str {
    if track == BACKGROUND { "Nền" } else { timeline::TRACK_NAMES[track] }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Transform {
    pub x: f32, pub y: f32, pub width: f32, pub height: f32,
    pub rotation: f32, pub opacity: f32,
}
impl Default for Transform {
    fn default() -> Self { Self { x:0.5,y:0.5,width:1.,height:1.,rotation:0.,opacity:1. } }
}
impl Transform {
    /// Fit the media inside the current box, in output pixels, without stretching.
    pub fn contain_aspect(&mut self, height_over_width:f32,w:u32,h:u32) {
        let width=self.width*w as f32;let height=self.height*h as f32;
        if width*height_over_width<=height {self.height=width*height_over_width/h as f32;}
        else {self.width=height/height_over_width/w as f32;}
    }
    pub fn pixels(&self, w: u32, h: u32) -> (f32,f32) {
        ((self.x-0.5)*w as f32,(self.y-0.5)*h as f32)
    }
    pub fn set_pixels(&mut self, x:f32,y:f32,w:u32,h:u32) {
        self.x=0.5+x/w as f32; self.y=0.5+y/h as f32;
    }
    pub fn corners(&self) -> [[f32;2];4] {
        let a=self.rotation.to_radians(); let (s,c)=a.sin_cos();
        [[-0.5,-0.5],[0.5,-0.5],[0.5,0.5],[-0.5,0.5]].map(|[x,y]| {
            let (x,y)=(x*self.width,y*self.height);
            [self.x+x*c-y*s,self.y+x*s+y*c]
        })
    }
    // Rotation must be evaluated in output pixels for non-square canvases.
    pub fn pixel_corners(&self,w:f32,h:f32)->[[f32;2];4] {
        let (s,c)=self.rotation.to_radians().sin_cos();
        [[-0.5,-0.5],[0.5,-0.5],[0.5,0.5],[-0.5,0.5]].map(|[x,y]| {
            let (x,y)=(x*self.width*w,y*self.height*h);
            [self.x*w+x*c-y*s,self.y*h+x*s+y*c]
        })
    }
}
pub fn avatar_aspect(path:Option<&std::path::Path>)->f32 {
    type Identity=(PathBuf,u64,Option<std::time::SystemTime>);
    static CACHE:std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeMap<Identity,f32>>>=std::sync::OnceLock::new();
    let Some((path,metadata))=path.and_then(|p|std::fs::metadata(p).ok().filter(|m|m.is_file()).map(|m|(p,m)))else{return 1.;};
    // Aspect lookup must never read/hash entire avatar movies on the UI thread.
    let key=(path.to_owned(),metadata.len(),metadata.modified().ok());let cache=CACHE.get_or_init(Default::default);
    if let Ok(cache)=cache.lock(){if let Some(ratio)=cache.get(&key){return *ratio;}}
    let ratio=crate::media::probe(&Settings::default(),path,&crate::process::cancel_token()).ok()
        .map(|info|info.height.max(1) as f32/info.width.max(1) as f32).unwrap_or(1.);
    if let Ok(mut cache)=cache.lock(){if cache.len()>128{cache.clear();}cache.insert(key,ratio);}
    ratio
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LayerClip {
    pub id:String, pub scene_id:String, pub track:usize,
    pub start_ms:u64, pub duration_ms:u64, pub media_in_ms:u64,
    #[serde(default="crate::footage_speed::one")] pub playback_rate:f64,
    /// In-point in original script cues; independent of footage/voice in-points.
    pub cue_in_ms:u64,
    pub media:Option<PathBuf>, pub proxy:Option<PathBuf>,
    pub transform:Transform, pub z:i32, pub visible:bool, pub locked:bool,
    pub linked:bool, pub gain:f32, pub fade_in_ms:u64, pub fade_out_ms:u64,
    #[serde(default)] pub evidence_id:Option<String>,
    #[serde(default)] pub group:Option<String>,
    #[serde(default)] pub motion:crate::evidence::Motion,
    #[serde(default)] pub timing_manual:bool,
}
impl LayerClip {
    pub fn source_at(&self,local:u64)->u64 {self.media_in_ms+crate::footage_speed::scaled(local,self.playback_rate)}
    pub fn source_duration(&self,duration:u64)->u64 {crate::footage_speed::scaled(duration,self.playback_rate)}
    pub fn pts(&self)->String {format!("setpts=(PTS-STARTPTS)/{:.9}",self.playback_rate)}
    pub fn tempo(&self)->String {if (self.playback_rate-1.).abs()<0.000001 {"anull".into()}else{format!("atempo={:.9}",self.playback_rate)}}
    pub fn active(&self,ms:u64)->bool { self.start_ms<=ms && ms<self.end() }
    pub fn end(&self)->u64 { self.start_ms.saturating_add(self.duration_ms) }
    pub fn audio(&self)->bool { matches!(self.track,timeline::VOICE|timeline::ORIGINAL|timeline::SFX|timeline::MUSIC) }
    pub fn background_original(&self,p:&Project)->bool {
        self.track==timeline::ORIGINAL && p.segments.iter().any(|s|s.id==self.scene_id&&s.kind=="review")
    }
    pub fn effective_gain(&self,p:&Project)->f32 {
        if self.background_original(p) {
            if p.output.original_audio_enabled {self.gain*p.output.original_audio_gain}else{0.}
        }else if self.track==timeline::SFX && self.evidence_id.is_none() {self.gain*p.card_sound.gain}
        else{self.gain}
    }
    pub fn still(&self)->bool {self.media.as_ref().and_then(|p|p.extension()).is_some_and(|e|e.eq_ignore_ascii_case("png")||e.eq_ignore_ascii_case("jpg")||e.eq_ignore_ascii_case("jpeg"))}
    pub fn enabled(&self,p:&Project)->bool {
        self.visible && !p.timeline.tracks[self.track].muted && (!self.background_original(p)||p.output.original_audio_enabled) && (self.track!=timeline::SFX || {
            let (parent,on)=if self.evidence_id.is_some(){(timeline::EVIDENCE,p.evidence_settings.enabled&&p.evidence_settings.sound_enabled)}else{(timeline::GRAPHICS,p.card_sound.enabled)};
            on && !p.timeline.tracks[parent].muted && p.composition.as_ref().is_none_or(|comp|comp.clips.iter().any(|c|c.track==parent&&c.group==self.group&&c.visible))
        })
    }
    pub fn editable(&self,p:&Project)->bool {
        !self.locked && !p.segments.iter().any(|s|s.id==self.scene_id&&s.locked)
            && !p.timeline.tracks[self.track].locked
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Composition { pub version:u32, pub script_key:String, pub clips:Vec<LayerClip>, #[serde(default)] pub removed:Vec<String> }
impl Composition {
    pub fn duration_ms(&self)->u64 { self.clips.iter().map(LayerClip::end).max().unwrap_or(0) }
    pub fn key(p:&Project)->String {
        // Transforms are intentionally absent; adjusting them never rebuilds assets.
        media::hash(serde_json::to_vec(&("evidence-persistent-stack-whoosh-v5",&p.segments,&p.evidence_settings,&p.card_sound.style)).unwrap_or_default())
    }
    pub fn from_project(p:&Project)->Self {
        let (w,h)=p.output.dimensions(); let (w,h)=(w as f32,h as f32);
        let mut result=Self { version:4,script_key:Self::key(p),clips:vec![],removed:vec![] };
        let mut start=0;
        for s in &p.segments {
            let src=p.source(&s.source_id).ok();
            for track in [BACKGROUND,timeline::VIDEO,timeline::AVATAR,timeline::VOICE,timeline::ORIGINAL,timeline::CAPTIONS,timeline::GRAPHICS] {
                if track==BACKGROUND { if s.kind=="host" {if !p.evidence_settings.enabled||!s.evidence_items.iter().any(|e|e.enabled&&e.asset.is_some()){continue;}} else if !p.output.blur_background {continue;} }
                else if !timeline::has_track(s,track) {continue;}
                if track==timeline::CAPTIONS && (!p.subtitles.enabled || s.suppress_captions) {continue;}
                let mut transform=Transform::default();
                let (media,proxy,media_in_ms)=match track {
                    BACKGROUND | timeline::VIDEO | timeline::ORIGINAL => {
                        if track==BACKGROUND&&s.kind=="host" { (Some(crate::evidence::background_path(p,s)),None,0) } else {
                        let Some(src)=src else {continue;};
                        if track==timeline::ORIGINAL && !src.has_audio {continue;}
                        if track==timeline::VIDEO {
                            let ratio=if p.output.fit=="cover" {(w/src.width.max(1) as f32).max(h/src.height.max(1) as f32)}
                                else {(w/src.width.max(1) as f32).min(h/src.height.max(1) as f32)}*p.output.scale;
                            transform.width=src.width.max(1) as f32*ratio/w;
                            transform.height=src.height.max(1) as f32*ratio/h;
                            transform.x=(1.-transform.width)*p.output.x+transform.width/2.;
                            transform.y=(1.-transform.height)*p.output.y+transform.height/2.;
                        }
                        (Some(src.input_path().to_owned()),Some(src.proxy.clone()),s.in_ms)
                        }
                    },
                    timeline::AVATAR => {
                        let aspect=avatar_aspect(s.avatar.as_deref());
                        if s.kind=="host" {
                            transform.contain_aspect(aspect,w as u32,h as u32);
                            if p.evidence_settings.enabled&&s.evidence_items.iter().any(|e|e.enabled){transform=crate::evidence::host_layout(p,aspect);}
                        } else {
                            transform.width=if s.avatar_size=="large" {(p.output.avatar_scale*1.6).min(0.8)} else {p.output.avatar_scale};
                            transform.height=transform.width*w/h*aspect;
                            if transform.height>1. {transform.width/=transform.height;transform.height=1.;}
                            transform.x=(1.-transform.width)*p.output.avatar_x+transform.width/2.;
                            transform.y=(1.-transform.height)*p.output.avatar_y+transform.height/2.;
                        }
                        (s.avatar.clone(),None,s.avatar_start_ms)
                    },
                    timeline::VOICE => (s.audio.clone(),None,s.asset_start_ms),
                    _ => (None,None,0),
                };
                result.clips.push(LayerClip { id:format!("{}-{track}",s.id),scene_id:s.id.clone(),track,
                    start_ms:start,duration_ms:s.duration_ms,media_in_ms,playback_rate:1.,cue_in_ms:0,media,proxy,transform,
                    z:timeline::TRACK_ROLES[track].z(),
                    visible:true,locked:false,linked:true,gain:if track==timeline::ORIGINAL&&s.kind=="review" {1.}else{s.audio_gain},fade_in_ms:0,fade_out_ms:0,
                    evidence_id:None,group:if track==timeline::GRAPHICS {Some(format!("{}:gfx",s.id))}else{None},motion:Default::default(),timing_manual:false });
            }
            for (index,cue) in s.graphics.iter().enumerate().filter(|(_,g)|!g.sound_suppressed&&g.start_ms<g.end_ms&&g.start_ms<s.duration_ms) {
                if let Some((path,sound_duration))=crate::evidence_sfx::selected_sound(&p.card_sound.style,index,&cue.kind) {
                    result.clips.push(LayerClip{id:format!("{}:gfx:{index}:{}",s.id,timeline::SFX),scene_id:s.id.clone(),track:timeline::SFX,start_ms:start+cue.start_ms,duration_ms:sound_duration.min(cue.end_ms.min(s.duration_ms)-cue.start_ms).max(1),media_in_ms:0,playback_rate:1.,cue_in_ms:0,media:Some(path),proxy:None,transform:Transform::default(),z:0,visible:true,locked:false,linked:false,gain:1.,fade_in_ms:5,fade_out_ms:40,evidence_id:None,group:Some(format!("{}:gfx",s.id)),motion:Default::default(),timing_manual:false});
                }
            }
            if p.evidence_settings.enabled {
                let items:Vec<_>=s.evidence_items.iter().filter(|e|e.enabled).collect();
                let stack=p.evidence_settings.presentation=="stack"&&s.kind=="host";
                let windows=crate::evidence::stack_windows(s,&items);
                let mut ends=vec![0u64;p.evidence_settings.density];
                for (index,item) in items.iter().enumerate() {
                    let (cue,duration)=if stack {(windows[index].0,windows[index].1)}else{crate::evidence::window(s,item)};let group=format!("{}:ev:{}",s.id,item.id);
                    // Limit simultaneous cards by staggering excess slots into the next window.
                    let lane=ends.iter().enumerate().min_by_key(|(_,end)|**end).map(|(i,_)|i).unwrap();
                    let reserve=((items.len()-index-1)/p.evidence_settings.density) as u64*(500.min(s.duration_ms/items.len().max(1) as u64));
                    let cue=if stack{cue}else{cue.max(ends[lane])};if cue+100>s.duration_ms{continue;}
                    let duration=duration.min(s.duration_ms.saturating_sub(cue+if stack{0}else{reserve})).max(1);ends[lane]=cue+duration;
                    let mut transform=if stack{crate::evidence::stack_layout(p,index)}else{crate::evidence::layout(p,if index>=p.evidence_settings.density{lane}else{index},items.len().min(p.evidence_settings.density),&item.slot)};
                    if let Some((cw,ch))=item.asset.as_ref().and_then(|a|image::image_dimensions(p.root.join(a)).ok()) {
                        transform.contain_aspect(ch as f32/cw.max(1) as f32,w as u32,h as u32);
                    }
                    for (track,asset) in [(timeline::EVIDENCE,&item.asset),(timeline::ANNOTATION,&item.annotation_asset)] {
                        let Some(asset)=asset else{continue;};let motion_duration=item.motion_duration_ms.max(item.motion_in_ms+duration);
                        let reveal=if track==timeline::ANNOTATION{150.min(motion_duration/3)}else{0};
                        let delay=reveal.saturating_sub(item.motion_in_ms);if delay>=duration{continue;}
                        let kind=if stack&&item.transition=="slide_fade"{"stack_pop".into()}else{item.transition.clone()};
                        result.clips.push(LayerClip{id:format!("{group}:{track}"),scene_id:s.id.clone(),track,start_ms:start+cue+delay,duration_ms:duration-delay,media_in_ms:0,playback_rate:1.,cue_in_ms:item.motion_in_ms+delay,media:Some(p.root.join(asset)),proxy:None,transform:transform.clone(),z:if stack{11+index as i32*2+if track==timeline::ANNOTATION{1}else{0}}else{timeline::TRACK_ROLES[track].z()},visible:true,locked:false,linked:false,gain:1.,fade_in_ms:0,fade_out_ms:0,evidence_id:Some(item.id.clone()),group:Some(group.clone()),motion:crate::evidence::Motion{kind,duration_ms:motion_duration,zoom:item.zoom||p.evidence_settings.motion=="zoom",drift:p.evidence_settings.motion=="drift",delay_ms:reveal,backdrop_ms:if stack{windows[index].2.map(|t|t+item.motion_in_ms)}else{None}},timing_manual:false});
                    }
                    if item.motion_in_ms==0&&item.asset.is_some() {
                        if let Some((path,sound_duration))=crate::evidence_sfx::selected_sound(&p.evidence_settings.sound_style,index,&item.kind){
                            result.clips.push(LayerClip{id:format!("{group}:{}",timeline::SFX),scene_id:s.id.clone(),track:timeline::SFX,start_ms:start+cue,duration_ms:sound_duration.min(duration).max(1),media_in_ms:0,playback_rate:1.,cue_in_ms:0,media:Some(path),proxy:None,transform:Transform::default(),z:0,visible:true,locked:false,linked:false,gain:p.evidence_settings.sound_gain,fade_in_ms:5,fade_out_ms:40,evidence_id:Some(item.id.clone()),group:Some(group.clone()),motion:Default::default(),timing_manual:false});
                        }
                    }
                }
            }
            start+=s.duration_ms;
        }
        let mut expanded=vec![];
        for mut clip in result.clips {
            if matches!(clip.track,BACKGROUND|timeline::VIDEO|timeline::ORIGINAL)&&!clip.still() {
                if let Some(seg)=p.segments.iter().find(|s|s.id==clip.scene_id&&s.kind!="host") {
                    clip.playback_rate=seg.source_rate;
                    if seg.kind=="review"&&!seg.footage_spans.is_empty() {
                        for (index,span) in seg.footage_spans.iter().enumerate() {
                            let mut part=clip.clone();if index>0 {part.id=format!("{}:speed:{index}",clip.id);}
                            part.start_ms+=span.start_ms;part.cue_in_ms+=span.start_ms;
                            part.duration_ms=span.duration_ms;part.media_in_ms=span.source_in_ms;
                            part.playback_rate=span.rate();expanded.push(part);
                        }
                        continue;
                    }
                }
            }
            expanded.push(clip);
        }
        result.clips=expanded;
        if let Some(music)=crate::background_music::clip(p){result.clips.push(music);}
        p.layer_defaults.apply(p, &mut result);
        if !p.layer_defaults.free_aspect {for c in result.clips.iter_mut().filter(|c|matches!(c.track,timeline::AVATAR|timeline::EVIDENCE|timeline::ANNOTATION)) {
            if c.track==timeline::AVATAR {
                c.transform.contain_aspect(avatar_aspect(c.media.as_deref()),w as u32,h as u32);
            }else if let Some((cw,ch))=c.media.as_ref().and_then(|a|image::image_dimensions(a).ok()) {
                c.transform.contain_aspect(ch as f32/cw.max(1) as f32,w as u32,h as u32);
            }
        }}
        result
    }
    pub fn validate(&self)->Result<()> {
        ensure!(matches!(self.version,1..=4),"Phiên bản composition chưa hỗ trợ");
        let mut ids=std::collections::HashSet::new();
        for c in &self.clips {
            ensure!(c.playback_rate.is_finite()&&(0.998..=2.002).contains(&c.playback_rate),"Tốc độ layer không hợp lệ");
            ensure!(ids.insert(&c.id) && c.track<timeline::TRACK_COUNT && c.duration_ms>=1,"Layer clip không hợp lệ");
            let t=&c.transform;
            ensure!([t.x,t.y,t.width,t.height,t.rotation,t.opacity,c.gain].iter().all(|v|v.is_finite())
                && t.width>0. && t.height>0. && t.width<=20. && t.height<=20.
                && t.x.abs()<=100. && t.y.abs()<=100. && (0.0..=1.0).contains(&t.opacity)
                && (0.0..=2.0).contains(&c.gain),"Biến đổi layer không hợp lệ");
        }
        Ok(())
    }
    /// v1 assigned host avatars a full-canvas box, stretching square media.
    /// Repair only that legacy size; retain intentional custom geometry.
    fn upgrade_host_aspect(&mut self,p:&Project)->bool {
        if self.version!=1 {return false;}
        let (w,h)=p.output.dimensions();let mut repaired=false;
        for c in &mut self.clips {
            if c.track==timeline::AVATAR && (c.transform.width-1.).abs()<0.00001 && (c.transform.height-1.).abs()<0.00001
                && p.segments.iter().any(|s|s.id==c.scene_id&&s.kind=="host") {
                let before=c.transform.clone();
                c.transform.contain_aspect(avatar_aspect(c.media.as_deref()),w,h);
                repaired|=c.transform!=before;
            }
        }
        self.version=2;repaired
    }
    /// Shift/trim either one unlinked clip or its linked scene companions.
    pub fn timing(&mut self,id:&str,start:u64,trim_in:u64,duration:u64)->Result<()> {
        let selected=self.clips.iter().find(|c|c.id==id).cloned().ok_or_else(||anyhow::anyhow!("Chưa chọn layer"))?;
        ensure!(duration>=100 && trim_in+duration<=selected.duration_ms,"Điểm cắt layer không hợp lệ");
        let targets:Vec<_>=self.clips.iter().enumerate().filter(|(_,c)|c.id==id || (selected.linked&&c.linked&&c.scene_id==selected.scene_id) || (selected.group.is_some()&&selected.group==c.group)).map(|(i,_)|i).collect();
        ensure!(targets.iter().all(|i|!self.clips[*i].locked),"Nhóm có layer đang khóa");
        for i in targets {
            let c=&mut self.clips[i];
            let from=selected.start_ms+trim_in;let to=from+duration;
            let begin=c.start_ms.max(from);let end=c.end().min(to);
            if begin>=end {c.visible=false;continue;}
            let skip=begin-c.start_ms;c.start_ms=start+begin-from;
            c.media_in_ms=c.source_at(skip);c.cue_in_ms+=skip;c.duration_ms=end-begin;c.timing_manual=true;
        }
        Ok(())
    }
}
impl Project {
    /// Parameter edits respect locks on the whole evidence group. Timing and
    /// source media are not part of the shared parameter set.
    pub fn layer_parameter_targets(&self,id:&str,include_groups:bool)->Vec<String> {
        let Some(comp)=&self.composition else{return vec![];};
        let Some(selected)=comp.clips.iter().find(|c|c.id==id) else{return vec![];};
        let editable=|c:&LayerClip|c.editable(self)&&c.group.as_ref().is_none_or(|g|
            comp.clips.iter().filter(|v|v.group.as_ref()==Some(g)).all(|v|v.editable(self)));
        if !editable(selected){return vec![];}
        let all=self.timeline.tracks[selected.track].sync_parameters;
        let same_kind=|c:&LayerClip|c.track==selected.track &&
            (c.track!=timeline::ORIGINAL||c.background_original(self)==selected.background_original(self));
        let groups:Vec<_>=comp.clips.iter().filter(|c|c.id==id||(all&&same_kind(c)))
            .filter(|c|editable(c)).filter_map(|c|c.group.as_ref()).collect();
        comp.clips.iter().filter(|c|c.id==id||(all&&same_kind(c))||
            (include_groups&&c.group.as_ref().is_some_and(|g|groups.contains(&g))))
            .filter(|c|editable(c)).map(|c|c.id.clone()).collect()
    }
    pub fn apply_layer_transform(&mut self,id:&str,transform:&Transform)->bool {
        let targets=self.layer_parameter_targets(id,true);
        let Some(comp)=&mut self.composition else{return false;};
        let mut changed=false;
        for clip in &mut comp.clips {if targets.contains(&clip.id)&&clip.transform!=*transform {
            clip.transform=transform.clone();changed=true;
        }}
        changed
    }
    pub fn sync_layer_parameters(&mut self,id:&str)->bool {
        let Some(source)=self.composition.as_ref().and_then(|c|c.clips.iter().find(|c|c.id==id)).cloned() else{return false;};
        if !self.timeline.tracks[source.track].sync_parameters{return false;}
        let mut changed=if source.audio(){false}else{self.apply_layer_transform(id,&source.transform)};
        let targets=self.layer_parameter_targets(id,false);
        if let Some(comp)=&mut self.composition {for clip in &mut comp.clips {
            if !targets.contains(&clip.id){continue;}
            if source.audio() {
                let fi=source.fade_in_ms.min(clip.duration_ms);let fo=source.fade_out_ms.min(clip.duration_ms);
                changed|=clip.gain!=source.gain||clip.fade_in_ms!=fi||clip.fade_out_ms!=fo;
                clip.gain=source.gain;clip.fade_in_ms=fi;clip.fade_out_ms=fo;
            }else{changed|=clip.z!=source.z;clip.z=source.z;}
        }}
        changed
    }
    /// Group edits are atomic, including every scene/track lock in the group.
    pub fn edit_independent_layer(&mut self,id:&str,command:&str,cursor:u64,clipboard:&mut Option<Vec<LayerClip>>)->Result<Option<String>> {
        self.ensure_composition();let comp=self.composition.as_ref().unwrap();
        let selected=comp.clips.iter().find(|c|c.id==id).context("Chưa chọn layer")?.clone();
        ensure!(!selected.linked,"Layer còn liên kết với cảnh");
        let mut targets:Vec<_>=comp.clips.iter().filter(|c|c.id==id||(selected.group.is_some()&&c.group==selected.group)).cloned().collect();
        targets.sort_by_key(|c|c.id!=id);
        ensure!(targets.iter().all(|c|c.editable(self)),"Nhóm có layer/cảnh/track đang khóa");
        if command=="copy"{*clipboard=Some(targets);return Ok(Some(id.into()));}
        let comp=self.composition.as_mut().unwrap();let mut next=Some(id.to_owned());
        match command {
            "delete"|"cut"=>{
                if command=="cut"{*clipboard=Some(targets.clone());}
                for c in &targets {comp.removed.push(c.id.clone());}
                comp.clips.retain(|c|!targets.iter().any(|t|t.id==c.id));next=None;
            },
            "duplicate"|"paste"=>{
                let copies=if command=="paste"{clipboard.clone().context("Chưa copy layer")?}else{targets};
                let origin=copies.first().context("Clipboard rỗng")?.start_ms;
                let start=if command=="paste"{cursor}else{selected.end()};
                let group=if copies.len()>1{Some(uuid::Uuid::new_v4().to_string())}else{None};
                for (i,mut c) in copies.into_iter().enumerate(){
                    let effect=self.effects.layers.get(&c.id).cloned();c.id=uuid::Uuid::new_v4().to_string();if let Some(effect)=effect{self.effects.layers.insert(c.id.clone(),effect);}c.group=group.clone();c.timing_manual=true;
                    c.start_ms=(start as i128+c.start_ms as i128-origin as i128).max(0) as u64;
                    if i==0{next=Some(c.id.clone());}comp.clips.push(c);
                }
            },
            "split"=>{
                ensure!(cursor>=selected.start_ms+100&&cursor+100<=selected.end(),"Đặt playhead trong layer, cách hai đầu 0.1 giây");
                let group=if targets.len()>1{Some(uuid::Uuid::new_v4().to_string())}else{None};
                for c in targets {
                    let from=c.start_ms.max(cursor);let end=c.end();
                    if from<end {let mut right=c.clone();right.id=uuid::Uuid::new_v4().to_string();if let Some(effect)=self.effects.layers.get(&c.id).cloned(){self.effects.layers.insert(right.id.clone(),effect);}right.group=group.clone();right.start_ms=from;right.duration_ms=end-from;right.media_in_ms=c.source_at(from-c.start_ms);right.cue_in_ms+=from-c.start_ms;right.timing_manual=true;
                        if c.id==id {next=Some(right.id.clone());}comp.clips.push(right);
                    }
                    if c.start_ms<cursor {let left=comp.clips.iter_mut().find(|l|l.id==c.id).unwrap();left.duration_ms=left.duration_ms.min(cursor-c.start_ms);left.timing_manual=true;}
                    else{comp.removed.push(c.id.clone());comp.clips.retain(|l|l.id!=c.id);}
                }
            },
            "trim_start"|"trim_end"=>{
                ensure!(cursor>=selected.start_ms+100&&cursor+100<=selected.end(),"Đặt playhead trong layer");
                if command=="trim_start"{comp.timing(id,cursor,cursor-selected.start_ms,selected.end()-cursor)?;}
                else{comp.timing(id,selected.start_ms,0,cursor-selected.start_ms)?;}
            },
            _=>anyhow::bail!("Thao tác layer chưa hỗ trợ"),
        }
        self.rendered=None;Ok(next)
    }
    pub fn time_layer(&mut self,id:&str,start:u64,trim:u64,duration:u64)->Result<()> {
        self.ensure_composition();
        let comp=self.composition.as_ref().unwrap();let selected=comp.clips.iter().find(|c|c.id==id).context("Chưa chọn layer")?;
        ensure!(comp.clips.iter().filter(|c|c.id==id || (selected.linked&&c.linked&&c.scene_id==selected.scene_id)||(selected.group.is_some()&&selected.group==c.group)).all(|c|c.editable(self)),"Layer/cảnh/track trong nhóm đang khóa");
        self.composition.as_mut().unwrap().timing(id,start,trim,duration)
    }
    pub fn composition_snapshot(&self)->Composition {
        if let Some(c)=&self.composition { if c.version==4&&c.script_key==Composition::key(self) {let mut c=c.clone();c.upgrade_host_aspect(self);Self::sync_music(&mut c,crate::background_music::clip(self));return c;} }
        Composition::from_project(self)
    }
    pub fn ensure_composition(&mut self) {
        if let Some(mut c)=self.composition.take() {
            if c.upgrade_host_aspect(self) {self.rendered=None;}
            self.composition=Some(c);
        }
        if self.composition.as_ref().is_none_or(|c|c.version<4||c.script_key!=Composition::key(self)) {
            let mut new=Composition::from_project(self);
            let mut repaired_sounds=false;
            // Re-analysis changes the script key. Preserve placement for unchanged
            // scene IDs, but use fresh media and timing from the new assets.
            if let Some(old)=&self.composition {
                let legacy=if old.version<4&&self.evidence_settings.presentation=="stack" {
                    let mut p=self.clone();p.composition=None;p.evidence_settings.presentation="classic".into();Some(Composition::from_project(&p))
                }else{None};
                for c in &mut new.clips { if let Some(prior)=old.clips.iter().find(|o|o.id==c.id) {
                    if !prior.editable(self) {*c=prior.clone();continue;}
                    let new_evidence=c.track==timeline::AVATAR&&self.evidence_settings.enabled
                        &&self.segments.iter().any(|s|s.id==c.scene_id&&s.kind=="host"&&s.evidence_items.iter().any(|e|e.enabled))
                        &&!old.clips.iter().any(|o|o.scene_id==c.scene_id&&o.evidence_id.is_some());
                    let automatic_upgrade=legacy.as_ref().and_then(|l|l.clips.iter().find(|v|v.id==c.id))
                        .is_some_and(|base|base.transform==prior.transform&&prior.editable(self));
                    let preserve=if automatic_upgrade {false}else if new_evidence {
                        let mut legacy=Transform::default();let (w,h)=self.output.dimensions();legacy.contain_aspect(avatar_aspect(prior.media.as_deref()),w,h);
                        prior.transform!=legacy||!prior.editable(self)
                    }else{true};
                    if preserve{
                        c.transform=prior.transform.clone();
                        // A new rig can change square media into a full-body
                        // portrait. Fit it inside the edited box, preserving
                        // placement instead of stretching the new character.
                        if c.media!=prior.media && c.track==timeline::AVATAR && !self.layer_defaults.free_aspect {
                            let (w,h)=self.output.dimensions();
                            c.transform.contain_aspect(avatar_aspect(c.media.as_deref()),w,h);
                        }
                        // Newly rasterized evidence cards follow the photo's
                        // aspect instead of stretching into their old 4:3 box.
                        if c.media!=prior.media && matches!(c.track,timeline::EVIDENCE|timeline::ANNOTATION) && !self.layer_defaults.free_aspect {
                            if let Some((cw,ch))=c.media.as_ref().and_then(|a|image::image_dimensions(a).ok()) {
                                let (w,h)=self.output.dimensions();c.transform.contain_aspect(ch as f32/cw.max(1) as f32,w,h);
                            }
                        }
                    }
                    if !automatic_upgrade {c.z=prior.z;}c.visible=prior.visible;
                    c.locked=prior.locked;c.linked=prior.linked;
                    if prior.group.is_some() || c.track!=timeline::GRAPHICS {c.group=prior.group.clone();}c.timing_manual=prior.timing_manual;
                    if prior.timing_manual {c.motion=prior.motion.clone();}
                    if c.track==timeline::SFX || c.background_original(self) {c.gain=prior.gain;}
                    // Entrance sounds are generated from the current card cues.
                    // Their old absolute times become stale after TTS shortens
                    // a preceding scene or rescales the card within this scene.
                    // Only a deliberate timing edit makes SFX independent.
                    let automatic_sound=c.track==timeline::SFX&&!prior.timing_manual;
                    repaired_sounds|=automatic_sound&&(c.start_ms!=prior.start_ms||c.duration_ms!=prior.duration_ms||c.media_in_ms!=prior.media_in_ms||c.media!=prior.media);
                    repaired_sounds|=c.evidence_id.is_some()&&!prior.timing_manual&&(c.start_ms!=prior.start_ms||c.duration_ms!=prior.duration_ms);
                    if !automatic_sound && !prior.linked && (prior.evidence_id.is_none()||prior.timing_manual) {c.start_ms=prior.start_ms;c.duration_ms=prior.duration_ms;c.media_in_ms=prior.media_in_ms;c.cue_in_ms=prior.cue_in_ms;c.playback_rate=prior.playback_rate;}
                } }
                for clip in old.clips.iter().filter(|c|!c.linked) {
                    if !new.clips.iter().any(|c|c.id==clip.id) && !clip.id.contains(":gfx:") && (clip.evidence_id.is_none()||!clip.id.contains(":ev:")) && self.segments.iter().any(|s|s.id==clip.scene_id&&clip.evidence_id.as_ref().is_none_or(|id|self.evidence_settings.enabled&&s.evidence_items.iter().any(|e|&e.id==id&&e.enabled&&(clip.track!=timeline::ANNOTATION||e.annotation_asset.is_some())))) {new.clips.push(clip.clone());}
                }
                new.removed=old.removed.clone();new.clips.retain(|c|!new.removed.contains(&c.id));
            }
            self.composition=Some(new);
            if repaired_sounds {self.rendered=None;}
        }
        // Evidence metadata is portable: resolve relative assets against the
        // loaded project root even when its unchanged script key is reused.
        let media:Vec<_>=self.composition.as_ref().into_iter().flat_map(|c|c.clips.iter()).filter_map(|c|{
            if c.track==timeline::SFX{return None;}
            let s=self.segments.iter().find(|s|s.id==c.scene_id)?;
            if c.track==BACKGROUND&&s.kind=="host"{return Some((c.id.clone(),crate::evidence::background_path(self,s)));}
            let id=c.evidence_id.as_ref()?;let e=s.evidence_items.iter().find(|e|&e.id==id)?;
            let asset=if c.track==timeline::ANNOTATION{e.annotation_asset.as_ref()}else{e.asset.as_ref()}?;
            Some((c.id.clone(),self.root.join(asset)))
        }).collect();
        if let Some(c)=&mut self.composition{for clip in &mut c.clips{if let Some((_,path))=media.iter().find(|(id,_)|id==&clip.id){clip.media=Some(path.clone());}}}
        let music=crate::background_music::clip(self);
        if let Some(comp)=&mut self.composition{Self::sync_music(comp,music);}
    }
    fn sync_music(comp:&mut Composition,music:Option<LayerClip>) {
        let old=comp.clips.iter().find(|c|c.id=="music-bed").cloned();
        comp.clips.retain(|c|c.id!="music-bed");
        if let Some(mut music)=music.filter(|_|!comp.removed.iter().any(|id|id=="music-bed")) {
            if let Some(old)=old {music.visible=old.visible;music.locked=old.locked;if old.timing_manual{music.start_ms=old.start_ms;music.duration_ms=old.duration_ms;music.media_in_ms=old.media_in_ms;music.timing_manual=true;}}
            comp.clips.push(music);
        }
    }
    pub fn composition_duration(&self)->u64 {self.composition_snapshot().duration_ms()}
}

/// Shared motion evaluator used by both canvas and FFmpeg export.
pub fn evaluated(p:&Project,c:&LayerClip,ms:u64)->Transform {
    let mut t=c.transform.clone();
    c.motion.apply(&mut t,ms.saturating_sub(c.start_ms)+c.cue_in_ms,p.output.dimensions().1);
    if c.track==timeline::VIDEO {
        if let Some(s)=p.segments.iter().find(|s|s.id==c.scene_id) {
            let local=(ms.saturating_sub(c.start_ms)+c.cue_in_ms) as f32/1000.;
            if s.motion=="push_in" {let f=1.+0.05*(local/(s.duration_ms as f32/1000.)).min(1.); t.width*=f;t.height*=f;}
            if s.motion=="reaction_shake" && local<0.45 {t.x+=5.*(55.*local).sin()/p.output.dimensions().0 as f32;}
        }
    }
    t
}

#[cfg(test)] mod tests {
    use super::*;
    #[test]
    fn replacing_square_avatar_with_full_body_preserves_placement_and_proportions() {
        let temp=tempfile::tempdir().unwrap();
        let square=temp.path().join("square.mkv");let tall=temp.path().join("tall.mkv");
        for (path,size) in [(&square,"512x512"),(&tall,"512x704")] {
            crate::process::run(&Settings::default().ffmpeg,&[
                "-y".into(),"-v".into(),"error".into(),"-f".into(),"lavfi".into(),"-i".into(),
                format!("color=c=red:s={size}:r=1:d=1"),"-c:v".into(),"ffv1".into(),path.to_string_lossy().into_owned(),
            ],None,&crate::process::cancel_token(),std::time::Duration::from_secs(30),None).unwrap();
        }
        let mut p=Project::default();
        p.segments.push(Segment{id:"host".into(),kind:"host".into(),duration_ms:1000,avatar:Some(square),..Default::default()});
        p.ensure_composition();
        let c=p.composition.as_mut().unwrap().clips.iter_mut().find(|c|c.track==timeline::AVATAR).unwrap();
        c.transform.x=0.37;c.transform.y=0.42;c.transform.rotation=7.;c.transform.opacity=0.8;
        let before=c.transform.clone();
        p.segments[0].avatar=Some(tall);p.ensure_composition();
        let after=&p.composition.as_ref().unwrap().clips.iter().find(|c|c.track==timeline::AVATAR).unwrap().transform;
        let (w,h)=p.output.dimensions();
        assert!((after.height*h as f32/(after.width*w as f32)-704./512.).abs()<0.001);
        assert_eq!((after.x,after.y,after.rotation,after.opacity),(before.x,before.y,before.rotation,before.opacity));
        assert!(after.width<=before.width && after.height<=before.height);
    }
    #[cfg(windows)]
    #[test]
    fn avatar_aspect_cache_does_not_read_media_and_invalidates_on_replacement() {
        use std::os::windows::fs::OpenOptionsExt;
        let temp=tempfile::tempdir().unwrap();let path=temp.path().join("avatar.mp4");
        let settings=Settings::default();let cancel=crate::process::cancel_token();
        let create=|size:&str|{
            crate::process::run(&settings.ffmpeg,&[
                "-y".into(),"-v".into(),"error".into(),"-f".into(),"lavfi".into(),"-i".into(),
                format!("color=c=red:s={size}:r=1:d=1"),"-c:v".into(),"libx264".into(),"-preset".into(),"ultrafast".into(),
                path.to_string_lossy().into_owned(),
            ],None,&cancel,std::time::Duration::from_secs(30),None).unwrap();
        };
        create("320x180");assert!((avatar_aspect(Some(&path))-0.5625).abs()<0.001);
        let lock=std::fs::OpenOptions::new().read(true).write(true).share_mode(0).open(&path).unwrap();
        for _ in 0..100 {assert!((avatar_aspect(Some(&path))-0.5625).abs()<0.001,"Cached lookup must work without reading locked media");}
        drop(lock);
        create("180x320");assert!((avatar_aspect(Some(&path))-320./180.).abs()<0.001,"Replacing media must invalidate the dimensions cache");
    }
    #[test] fn host_upgrade_keeps_custom_geometry_and_only_repairs_legacy_size() {
        let mut p=Project::default();p.segments.push(Segment{id:"host".into(),kind:"host".into(),duration_ms:1000,..Default::default()});
        let mut c=Composition::from_project(&p);c.version=1;
        let av=c.clips.iter_mut().find(|c|c.track==timeline::AVATAR).unwrap();
        av.transform=Transform{width:0.3,height:0.7,x:0.2,rotation:15.,opacity:0.6,..Default::default()};
        let custom=av.transform.clone();assert!(!c.upgrade_host_aspect(&p));
        assert_eq!(c.clips.iter().find(|c|c.track==timeline::AVATAR).unwrap().transform,custom);
        c.version=1;let av=c.clips.iter_mut().find(|c|c.track==timeline::AVATAR).unwrap();
        av.transform.width=1.;av.transform.height=1.;
        assert!(c.upgrade_host_aspect(&p));let av=c.clips.iter().find(|c|c.track==timeline::AVATAR).unwrap();
        assert_eq!(av.transform.x,custom.x);assert_eq!(av.transform.rotation,custom.rotation);assert_eq!(av.transform.opacity,custom.opacity);
        assert!((av.transform.height-0.5625).abs()<0.001);assert!(!c.upgrade_host_aspect(&p));
    }
    #[test] fn pixel_roundtrip_and_non_square_rotation() {
        for (w,h) in [(1080,1920),(1920,1080)] {for x in [-2000.,0.,2000.] {
            let mut t=Transform::default();t.set_pixels(x,-x,w,h);
            let (a,b)=t.pixels(w,h);assert!((a-x).abs()<0.001 && (b+x).abs()<0.001);
        }}
        let t=Transform{width:0.2,height:0.2,rotation:90.,..Default::default()};
        let c=t.pixel_corners(1080.,1920.);assert!((c[1][1]-c[0][1]-216.).abs()<0.01);
    }
    #[test] fn migration_preserves_legacy_avatar_geometry_and_trim_inpoints() {
        let mut p=Project::default();p.segments.push(Segment {id:"s".into(),kind:"review".into(),duration_ms:4000,asset_start_ms:300,avatar_start_ms:500,..Default::default()});
        let mut c=Composition::from_project(&p);let av=c.clips.iter().find(|c|c.track==1).unwrap().clone();
        assert!((av.transform.x-av.transform.width/2.-(1.-av.transform.width)*p.output.avatar_x).abs()<0.0001);
        c.clips.iter_mut().find(|c|c.id==av.id).unwrap().linked=false;
        c.timing(&av.id,1000,700,2000).unwrap();let edited=c.clips.iter().find(|c|c.id==av.id).unwrap();
        assert_eq!(edited.media_in_ms,1200);assert_eq!(edited.start_ms,1000);
        assert_eq!(c.clips.iter().find(|c|c.track==2).unwrap().media_in_ms,300);
        p.composition=Some(c);p.timeline.tracks[timeline::VOICE].locked=true;
        assert!(p.time_layer(&av.id,0,0,1000).is_ok(),"An unlinked avatar ignores unrelated track locks");
        p.composition.as_mut().unwrap().clips.iter_mut().find(|c|c.id==av.id).unwrap().linked=true;
        assert!(p.time_layer(&av.id,0,0,500).is_err(),"Linked timing must respect every companion track lock");
    }
}
