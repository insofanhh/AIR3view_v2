//! User-supplied reveal sounds. One local copy per content version; no network/provider.
use std::{path::PathBuf,sync::OnceLock};
#[derive(Clone,Debug,serde::Serialize,serde::Deserialize,PartialEq)]
#[serde(default)]
pub struct CardSoundSettings {
    pub enabled:bool,
    pub style:String,
    pub gain:f32,
    pub sync_style:bool,
}
impl Default for CardSoundSettings {
    fn default()->Self {Self{enabled:true,style:"auto".into(),gain:0.25,sync_style:false}}
}

#[derive(Clone,Debug,serde::Serialize,serde::Deserialize)]
pub struct SoundCue {pub index:usize,pub kind:String}
#[derive(Clone,Copy)]
pub enum Scope {Cards,Evidence,Both}
impl Scope {
    pub fn includes(self,evidence:bool)->bool {matches!(self,Self::Both)||matches!((self,evidence),(Self::Cards,false)|(Self::Evidence,true))}
}
#[derive(Default,Debug,serde::Serialize)]
pub struct ApplyReport {pub total:usize,pub updated:usize,pub locked:usize,pub custom:usize}
pub fn style_label(style:&str)->&'static str {match style{"whoosh"=>"Paper tear (Whoosh)","pop"=>"Pop","ting"=>"Appear (Ting)",_=>"Tự động"}}
pub fn managed_style(path:&std::path::Path)->Option<&str> {
    if path.parent()?.file_name()?.to_str()? != "evidence-sfx-v1" || !path.extension()?.eq_ignore_ascii_case("wav") {return None;}
    let (style,hash)=path.file_stem()?.to_str()?.rsplit_once('-')?;
    (matches!(style,"whoosh"|"pop"|"ting")&&hash.len()==12&&hash.bytes().all(|b|b.is_ascii_hexdigit())).then_some(style)
}
fn cue(p:&crate::model::Project,c:&crate::composition::LayerClip)->Option<SoundCue> {
    if let Some(cue)=&c.sound_cue {return Some(cue.clone());}
    let scene=p.segments.iter().find(|s|s.id==c.scene_id)?;
    if let Some(id)=&c.evidence_id {
        return scene.evidence_items.iter().filter(|e|e.enabled).enumerate().find(|(_,e)|&e.id==id).map(|(index,e)|SoundCue{index,kind:e.kind.clone()});
    }
    if let Some(index)=c.id.strip_prefix(&format!("{}:gfx:",scene.id)).and_then(|s|s.split(':').next()?.parse::<usize>().ok()) {
        return scene.graphics.get(index).map(|g|SoundCue{index,kind:g.kind.clone()});
    }
    // Legacy copies have no cue metadata. Use the closest original card;
    // new copies retain sound_cue directly through LayerClip::clone.
    scene.graphics.iter().enumerate().min_by_key(|(_,g)|{
        let start=p.segments.iter().take_while(|s|s.id!=scene.id).map(|s|s.duration_ms).sum::<u64>();
        (start+g.start_ms).abs_diff(c.start_ms)
    }).map(|(index,g)|SoundCue{index,kind:g.kind.clone()})
}
pub fn identify_cues(p:&crate::model::Project,comp:&mut crate::composition::Composition) {
    for c in comp.clips.iter_mut().filter(|c|c.track==crate::timeline::SFX&&c.sound_cue.is_none()) {
        if c.media.as_deref().and_then(managed_style).is_some(){c.sound_cue=cue(p,c);}
    }
}
/// An explicit editor action changes the selected bank style, independently of
/// asset-version migration. It never regenerates picture/voice layers.
pub fn apply_settings(p:&mut crate::model::Project,scope:Scope,old_evidence_gain:Option<f32>)->ApplyReport {
    use crate::timeline;
    let mut comp=p.composition.take().unwrap_or_else(||crate::composition::Composition::from_project(p));
    identify_cues(p,&mut comp);
    let mut report=ApplyReport::default();let mut updates=vec![];
    for (index,c) in comp.clips.iter().enumerate().filter(|(_,c)|c.track==timeline::SFX&&scope.includes(c.evidence_id.is_some())) {
        report.total+=1;
        if !c.editable(p)||c.group.as_ref().is_some_and(|group|comp.clips.iter().any(|v|v.group.as_ref()==Some(group)&&!v.editable(p))) {report.locked+=1;continue;}
        if c.media.as_deref().and_then(managed_style).is_none(){report.custom+=1;continue;}
        let source=cue(p,c).unwrap_or(SoundCue{index:0,kind:"title".into()});
        let style=if c.evidence_id.is_some(){&p.evidence_settings.sound_style}else{&p.card_sound.style};
        if let Some((path,duration))=selected_sound(style,source.index,&source.kind) {
            let gain=if c.evidence_id.is_some(){old_evidence_gain.map(|old|if old>0.{(c.gain/old*p.evidence_settings.sound_gain).clamp(0.,2.)}else{p.evidence_settings.sound_gain}).unwrap_or(c.gain)}else{c.gain};
            let changed=c.media.as_ref()!=Some(&path);
            let (media_in,length)=replacement_window(p,&comp,c,duration);
            updates.push((index,path,source,gain,changed,media_in,length));
        }
    }
    for (index,path,source,gain,changed,media_in,length) in updates {
        let c=&mut comp.clips[index];report.updated+=1;c.sound_cue=Some(source);c.gain=gain;
        if changed {c.media=Some(path);c.proxy=None;c.media_in_ms=media_in;c.duration_ms=length;c.fade_in_ms=c.fade_in_ms.min(length);c.fade_out_ms=c.fade_out_ms.min(length);}
    }
    comp.script_key=crate::composition::Composition::key(p);p.composition=Some(comp);
    p.rendered=None;report
}
fn replacement_window(p:&crate::model::Project,comp:&crate::composition::Composition,c:&crate::composition::LayerClip,duration:u64)->(u64,u64) {
    let media_in=c.media_in_ms.min(duration.saturating_sub(1));
    let available=if c.timing_manual {c.duration_ms}else if let Some(cue)=cue(p,c).filter(|_|c.evidence_id.is_none()) {
        p.segments.iter().find(|s|s.id==c.scene_id).and_then(|s|s.graphics.get(cue.index).map(|g|g.end_ms.min(s.duration_ms).saturating_sub(g.start_ms))).unwrap_or(c.duration_ms)
    }else{comp.clips.iter().find(|v|v.track==crate::timeline::EVIDENCE&&v.group==c.group&&v.scene_id==c.scene_id).map(|v|v.end().saturating_sub(c.start_ms)).unwrap_or(c.duration_ms)};
    (media_in,(duration-media_in).min(available).max(1))
}
impl CardSoundSettings {
    pub fn validate(&self)->anyhow::Result<()> {
        anyhow::ensure!(matches!(self.style.as_str(),"auto"|"whoosh"|"pop"|"ting") && self.gain.is_finite() && (0.0..=2.0).contains(&self.gain),"Thiết lập âm thanh thẻ thông tin không hợp lệ");
        Ok(())
    }
}
pub fn selected_sound(style:&str,index:usize,kind:&str)->Option<(PathBuf,u64)> {
    static LIBRARY:OnceLock<Vec<Option<(PathBuf,u64)>>>=OnceLock::new();
    let library=LIBRARY.get_or_init(||{
        let root=crate::model::data_dir().join("assets/evidence-sfx-v1");
        let _=std::fs::create_dir_all(&root);
        let _=std::fs::write(root.join("CREDITS.md"),include_bytes!("../resources/sfx/CREDITS.md"));
        let _=std::fs::write(root.join("sources.json"),include_bytes!("../resources/sfx/sources.json"));
        [ ("whoosh",include_bytes!("../resources/sfx/whoosh.wav").as_slice()),
          ("pop",include_bytes!("../resources/sfx/pop.wav").as_slice()),
          ("ting",include_bytes!("../resources/sfx/ting.wav").as_slice()) ].iter().map(|(name,bytes)|{
            let path=root.join(format!("{name}-{}.wav",&crate::media::hash(bytes)[..12]));
            if !path.is_file() {let pending=path.with_extension("pending.wav");std::fs::write(&pending,bytes).ok()?;std::fs::rename(pending,&path).ok()?;}
            // All embedded files are PCM16 mono 48 kHz produced by the asset import.
            let data=bytes.windows(4).position(|w|w==b"data")?;
            let length=u32::from_le_bytes(bytes.get(data+4..data+8)?.try_into().ok()?);
            Some((path,length as u64*1000/96000))
        }).collect()
    });
    let selected=match style {
        "whoosh"=>0,"pop"=>1,"ting"=>2,
        "auto"=>if matches!(kind,"stat"|"fact"|"number"|"date")||index%3==2{2}else if index==0{0}else{1},
        _=>return None,
    };
    library[selected].clone()
}

/// Replace only files owned by the bundled sound library. A release changes the
/// content hash; unchanged project keys, locked layers and manually copied SFX
/// must still pick up the replacement without rebuilding their picture layers.
pub fn refresh_composition(p:&crate::model::Project,comp:&mut crate::composition::Composition)->usize {
    use crate::timeline;
    let updates:Vec<_>=comp.clips.iter().enumerate().filter_map(|(index,clip)|{
        if clip.track!=timeline::SFX{return None;}
        let media=clip.media.as_ref()?;
        if media.parent()?.file_name()?.to_str()? != "evidence-sfx-v1" {return None;}
        let (style,hash)=media.file_stem()?.to_str()?.rsplit_once('-')?;
        if !media.extension()?.eq_ignore_ascii_case("wav")||hash.len()!=12||!hash.bytes().all(|b|b.is_ascii_hexdigit()){return None;}
        let (path,sound_duration)=selected_sound(style,0,"")?;
        if &path==media{return None;}
        let media_in=clip.media_in_ms.min(sound_duration.saturating_sub(1));
        let available=if clip.timing_manual {
            clip.duration_ms
        }else if clip.evidence_id.is_none() {
            // Graphics share one parent layer, but each sound has its own cue.
            let cue_end=p.segments.iter().find(|s|s.id==clip.scene_id).and_then(|s|{
                let suffix=clip.id.strip_prefix(&format!("{}:gfx:",s.id))?;
                let cue=s.graphics.get(suffix.split(':').next()?.parse::<usize>().ok()?)?;
                Some(clip.start_ms.saturating_add(cue.end_ms.min(s.duration_ms).saturating_sub(cue.start_ms)))
            });
            cue_end.map(|end|end.saturating_sub(clip.start_ms)).unwrap_or(clip.duration_ms)
        }else{
            comp.clips.iter().find(|c|c.track==timeline::EVIDENCE&&c.group==clip.group&&c.scene_id==clip.scene_id)
                .map(|c|c.end().saturating_sub(clip.start_ms)).unwrap_or(clip.duration_ms)
        };
        Some((index,path,media_in,(sound_duration-media_in).min(available).max(1)))
    }).collect();
    let count=updates.len();
    for (index,path,media_in,duration) in updates {
        let clip=&mut comp.clips[index];
        clip.media=Some(path);clip.proxy=None;clip.media_in_ms=media_in;clip.duration_ms=duration;
        clip.fade_in_ms=clip.fade_in_ms.min(duration);clip.fade_out_ms=clip.fade_out_ms.min(duration);
    }
    count
}

/// Explicit library maintenance: patch only sound fields in the original JSON,
/// retaining unknown fields and all script/voice/picture assets. No AI calls.
pub fn refresh_saved_project(path:&std::path::Path)->anyhow::Result<usize> {
    use anyhow::Context;
    use serde_json::value::{RawValue,to_raw_value};
    type Fields=std::collections::BTreeMap<String,Box<RawValue>>;
    let bytes=std::fs::read(path)?;
    // Raw JSON keeps untouched decimals exactly as supplied. Deserializing the
    // full document through Value can round image-quality numbers and change
    // script/cache fingerprints even though only sound media was edited.
    let mut value:Fields=serde_json::from_slice(&bytes)?;
    let mut project:crate::model::Project=serde_json::from_slice(&bytes)?;
    project.root=std::path::absolute(path)?.parent().context("Project không có thư mục")?.to_owned();
    let Some(mut comp)=project.composition.take()else{return Ok(0);};
    let changed=refresh_composition(&project,&mut comp);
    if changed==0{return Ok(0);}
    let mut composition:Fields=serde_json::from_str(value.get("composition").context("Thiếu composition")?.get())?;
    let mut clips:Vec<Fields>=serde_json::from_str(composition.get("clips").context("Composition không có clips")?.get())?;
    for (saved,clip) in clips.iter_mut().zip(&comp.clips) {
        if clip.track!=crate::timeline::SFX {continue;}
        let fresh=serde_json::to_value(clip)?;
        for field in ["media","proxy","media_in_ms","duration_ms","fade_in_ms","fade_out_ms"] {
            if saved.get(field).map(|v|v.get())!=Some(to_raw_value(&fresh[field])?.get()) {
                saved.insert(field.into(),to_raw_value(&fresh[field])?);
            }
        }
    }
    composition.insert("clips".into(),to_raw_value(&clips)?);
    value.insert("composition".into(),to_raw_value(&composition)?);
    if value.get("rendered").is_some_and(|v|v.get()!="null") {
        value.insert("rendered".into(),to_raw_value(&Option::<String>::None)?);
        if value.get("status").is_some_and(|v|serde_json::from_str::<String>(v.get()).ok().as_deref()==Some("Đã xuất video")) {
            value.insert("status".into(),to_raw_value("Sẵn sàng biên tập / xuất MP4")?);
        }
    }
    let backup=path.with_extension("pre-reveal-sfx-v2.json");
    if !backup.exists(){std::fs::write(&backup,&bytes)?;}
    crate::model::write_json(path,&value)?;
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{composition::Composition,model::*,timeline,process};

    fn project()->Project {
        let mut p=Project::default();
        p.evidence_settings.enabled=false;
        p.segments=vec![Segment{id:"host".into(),kind:"host".into(),duration_ms:4000,graphics:vec![
            GraphicCue{kind:"number".into(),start_ms:800,end_ms:2000,label:"Verified count".into(),value:"3".into(),..Default::default()},
            GraphicCue{kind:"title".into(),start_ms:2400,end_ms:3800,label:"Reaction".into(),..Default::default()}
        ],..Default::default()}];
        p
    }

    fn shortened_speech_project()->Project {
        let mut p=project();
        let mut voice=p.segments.remove(0);
        voice.duration_ms=8000;
        voice.graphics[0].start_ms=1600;voice.graphics[0].end_ms=4000;
        voice.graphics[1].start_ms=6000;voice.graphics[1].end_ms=7800;
        p.segments=vec![Segment{id:"intro".into(),kind:"host".into(),duration_ms:8000,..Default::default()},voice];
        p.ensure_composition();
        // Speech checkpoints shorten both a preceding scene and this scene.
        p.segments[0].duration_ms=3000;p.segments[1].duration_ms=4000;
        p.segments[1].graphics[0].start_ms=800;p.segments[1].graphics[0].end_ms=2000;
        p.segments[1].graphics[1].start_ms=2400;p.segments[1].graphics[1].end_ms=3800;
        p
    }

    #[test]
    fn editor_sound_styles_keep_independent_groups_copies_locks_and_manual_timing() {
        let mut p=project();p.evidence_settings.enabled=true;
        p.segments[0].evidence_items.push(crate::evidence::EvidenceItem{id:"photo".into(),kind:"fact".into(),asset:Some("photo.png".into()),..Default::default()});
        p.ensure_composition();
        let old=p.clone();let comp=p.composition.as_mut().unwrap();
        let mut copy=comp.clips.iter().find(|c|c.track==timeline::SFX&&c.evidence_id.is_none()).unwrap().clone();
        copy.id="copied-card".into();copy.start_ms=100;copy.media_in_ms=20;copy.duration_ms=80;copy.timing_manual=true;copy.gain=0.7;comp.clips.push(copy);
        let mut custom=comp.clips.last().unwrap().clone();custom.id="custom-sound".into();custom.media=Some("my-audio.wav".into());comp.clips.push(custom);
        let locked=comp.clips.iter_mut().find(|c|c.id=="host:gfx:1:9").unwrap();locked.group=None;locked.locked=true;let locked_media=locked.media.clone();
        p.card_sound.style="whoosh".into();let report=apply_settings(&mut p,Scope::Cards,None);
        assert_eq!((report.total,report.updated,report.locked,report.custom),(4,2,1,1));
        let comp=p.composition.as_ref().unwrap();
        let copied=comp.clips.iter().find(|c|c.id=="copied-card").unwrap();assert_eq!((copied.start_ms,copied.media_in_ms,copied.duration_ms,copied.gain),(100,20,80,0.7));
        assert_eq!(copied.media,selected_sound("whoosh",0,"").map(|v|v.0));
        assert_eq!(comp.clips.iter().find(|c|c.id=="host:gfx:1:9").unwrap().media,locked_media);
        assert_eq!(comp.clips.iter().find(|c|c.evidence_id.is_some()&&c.track==timeline::SFX).unwrap().media,old.composition.as_ref().unwrap().clips.iter().find(|c|c.evidence_id.is_some()&&c.track==timeline::SFX).unwrap().media);
        p.composition.as_mut().unwrap().clips.iter_mut().find(|c|c.id=="host:gfx:1:9").unwrap().locked=false;
        for style in ["pop","ting","auto","whoosh"] {
            p.card_sound.style=style.into();p.evidence_settings.sound_style=style.into();let report=apply_settings(&mut p,Scope::Both,None);assert_eq!(report.updated,4);
            for c in p.composition_snapshot().clips.iter().filter(|c|c.track==timeline::SFX&&c.id!="custom-sound") {
                let cue=c.sound_cue.as_ref().unwrap();assert_eq!(c.media,selected_sound(style,cue.index,&cue.kind).map(|v|v.0));
            }
            assert_eq!(serde_json::to_vec(&p.segments).unwrap(),serde_json::to_vec(&old.segments).unwrap());
            assert_eq!(serde_json::to_vec(&p.composition.as_ref().unwrap().clips.iter().filter(|c|c.track!=timeline::SFX).collect::<Vec<_>>()).unwrap(),serde_json::to_vec(&old.composition.as_ref().unwrap().clips.iter().filter(|c|c.track!=timeline::SFX).collect::<Vec<_>>()).unwrap());
        }
        let encoded=serde_json::to_vec(&p).unwrap();let loaded:Project=serde_json::from_slice(&encoded).unwrap();assert_eq!(loaded.card_sound.style,"whoosh");assert!(loaded.composition.as_ref().unwrap().clips.iter().find(|c|c.id=="copied-card").unwrap().sound_cue.is_some());
    }

    #[test]
    #[ignore = "Reads the user's library only with an explicit QA output directory"]
    fn editor_sound_library_projects_and_actual_mp4() {
        let qa=PathBuf::from(std::env::var_os("AIR3VIEW_SFX_LIBRARY_QA").expect("Set AIR3VIEW_SFX_LIBRARY_QA"));std::fs::create_dir_all(&qa).unwrap();
        let paths=crate::project_store::list(&data_dir().join("projects"));assert!(!paths.is_empty());
        let settings=Settings{encoder:"libx264".into(),..Default::default()};let cancel=process::cancel_token();let mut results=vec![];
        for (number,path) in paths.iter().enumerate() {
            // Load without any Project::load migrations or writes to the library.
            let original=std::fs::read(path).unwrap();let mut p:Project=serde_json::from_slice(&original).unwrap();p.root=path.parent().unwrap().into();
            let segments=serde_json::to_vec(&p.segments).unwrap();let pictures=serde_json::to_vec(&p.composition.as_ref().unwrap().clips.iter().filter(|c|c.track!=timeline::SFX).collect::<Vec<_>>()).unwrap();
            let mut checks=0;
            for scope in [Scope::Cards,Scope::Evidence,Scope::Both] {for style in ["whoosh","pop","ting","auto"] {
                let outside:Vec<_>=p.composition.as_ref().unwrap().clips.iter().filter(|c|c.track==timeline::SFX&&!scope.includes(c.evidence_id.is_some())).map(|c|(c.id.clone(),c.media.clone(),c.start_ms,c.gain)).collect();
                if scope.includes(false){p.card_sound.style=style.into();}
                if scope.includes(true){p.evidence_settings.sound_style=style.into();}
                let report=apply_settings(&mut p,scope,None);assert_eq!((report.locked,report.custom),(0,0),"QA expects the current unlocked library fixtures");
                let comp=p.composition_snapshot();
                for c in comp.clips.iter().filter(|c|c.track==timeline::SFX&&scope.includes(c.evidence_id.is_some())) {
                    let cue=c.sound_cue.as_ref().unwrap();let (asset,duration)=selected_sound(style,cue.index,&cue.kind).unwrap();assert_eq!(c.media.as_ref(),Some(&asset));assert!(c.media_in_ms+c.duration_ms<=duration);
                }
                assert_eq!(outside,comp.clips.iter().filter(|c|c.track==timeline::SFX&&!scope.includes(c.evidence_id.is_some())).map(|c|(c.id.clone(),c.media.clone(),c.start_ms,c.gain)).collect::<Vec<_>>());
                assert_eq!(segments,serde_json::to_vec(&p.segments).unwrap());assert_eq!(pictures,serde_json::to_vec(&comp.clips.iter().filter(|c|c.track!=timeline::SFX).collect::<Vec<_>>()).unwrap());checks+=1;
            }}
            let style=["whoosh","pop","ting"][number%3];p.card_sound.style=style.into();p.evidence_settings.sound_style=style.into();
            let host=p.segments.iter().find(|s|s.kind=="host"&&s.audio.is_some()&&s.avatar.is_some()).expect("Existing project must have a prepared Host").clone();
            let old_root=p.root.clone();p.root=qa.join(&p.id);std::fs::create_dir_all(&p.root).unwrap();p.segments=vec![host];p.composition=None;p.rendered=None;
            for item in &mut p.segments[0].evidence_items {for asset in [&mut item.asset,&mut item.annotation_asset].into_iter().flatten() {
                let input=old_root.join(&*asset);let relative=PathBuf::from("assets/evidence").join(input.file_name().unwrap());let target=p.root.join(&relative);std::fs::create_dir_all(target.parent().unwrap()).unwrap();std::fs::copy(input,target).unwrap();*asset=relative;
            }}
            p.rules.min_seconds=1;p.rules.max_seconds=120;p.rules.hook_enabled=false;p.output.resolution=360;p.output.fps=24;p.music.enabled=false;
            p.card_sound.enabled=true;p.evidence_settings.sound_enabled=true;p.timeline.tracks[timeline::SFX].muted=false;p.timeline.tracks[timeline::GRAPHICS].muted=false;p.timeline.tracks[timeline::EVIDENCE].muted=false;p.ensure_composition();
            let output=p.root.join("verified.mp4");crate::layer_media::export(&p,&settings,&output,&cancel).unwrap();
            process::run(&settings.ffmpeg,&["-v".into(),"error".into(),"-i".into(),output.to_string_lossy().into_owned(),"-f".into(),"null".into(),"NUL".into()],None,&cancel,std::time::Duration::from_secs(60),None).unwrap();
            let first_report:serde_json::Value=serde_json::from_slice(&std::fs::read(p.root.join("cache/composition-export/render-report.json")).unwrap()).unwrap();
            p.card_sound.style="pop".into();p.evidence_settings.sound_style="pop".into();apply_settings(&mut p,Scope::Both,None);
            let updated=p.root.join("style-changed.mp4");crate::layer_media::export(&p,&settings,&updated,&cancel).unwrap();
            let report:serde_json::Value=serde_json::from_slice(&std::fs::read(p.root.join("cache/composition-export/render-report.json")).unwrap()).unwrap();assert_eq!(report["encoded_frames"],0,"SFX style changes must reuse all picture frames");
            assert_eq!(original,std::fs::read(path).unwrap(),"QA must not modify the user's project");
            results.push(serde_json::json!({"project":path,"id":p.id,"style_checks":checks,"rendered_style":style,"mp4":output,"changed_mp4":updated,"encoded_frames":first_report["encoded_frames"],"encoded_frames_after_sfx_change":report["encoded_frames"],"library_untouched":true}));
        }
        crate::model::write_json(&qa.join("library-report.json"),&results).unwrap();println!("SFX_LIBRARY_QA_PASS projects={}",results.len());
    }

    #[test]
    fn replacement_library_is_trimmed_pcm_and_audible_without_clipping() {
        use rodio::Source;
        for (style,duration) in [("whoosh",644),("ting",320),("pop",202)] {
            let (path,ms)=selected_sound(style,0,"").unwrap();assert_eq!(ms,duration);
            let provenance:serde_json::Value=serde_json::from_slice(include_bytes!("../resources/sfx/sources.json")).unwrap();
            assert_eq!(crate::media::hash(std::fs::read(&path).unwrap()),provenance[style]["output_sha256"].as_str().unwrap());
            let decoder=rodio::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap())).unwrap();
            assert_eq!((decoder.channels(),decoder.sample_rate()),(1,48000));
            let samples=decoder.convert_samples::<f32>().collect::<Vec<_>>();
            assert!(samples.iter().all(|v|v.abs()<0.9),"{style}: peak limiter must leave headroom");
            let rms=|slice:&[f32]|(slice.iter().map(|v|v*v).sum::<f32>()/slice.len() as f32).sqrt();
            assert!(rms(&samples)>0.04,"{style}: replacement is too quiet");
            assert!(rms(&samples[..2400])>0.001,"{style}: sound must start within 50 ms");
            assert!(samples.last().unwrap().abs()<0.002,"{style}: cut must not click");
        }
    }

    #[test]
    fn legacy_sfx_update_preserves_picture_edits_locks_gain_and_custom_audio() {
        let mut p=project();p.ensure_composition();p.rendered=Some("old.mp4".into());
        let segments=serde_json::to_vec(&p.segments).unwrap();
        let pictures=|p:&Project|serde_json::to_vec(&p.composition.as_ref().unwrap().clips.iter().filter(|c|c.track!=timeline::SFX).collect::<Vec<_>>()).unwrap();
        let before=pictures(&p);let key=p.composition.as_ref().unwrap().script_key.clone();
        let c=p.composition.as_mut().unwrap();
        for sound in c.clips.iter_mut().filter(|c|c.track==timeline::SFX) {
            let name=sound.media.as_ref().unwrap().file_name().unwrap().to_str().unwrap().split('-').next().unwrap();
            sound.media=Some(PathBuf::from(format!("old-install/assets/evidence-sfx-v1/{name}-0123456789ab.wav")));
            sound.duration_ms=50;sound.locked=true;sound.gain=0.37;sound.visible=false;
        }
        let mut manual=c.clips.iter().find(|c|c.track==timeline::SFX).unwrap().clone();
        manual.id="manual-copy".into();manual.timing_manual=true;manual.start_ms=100;manual.duration_ms=60;manual.media_in_ms=50;
        c.clips.push(manual.clone());
        manual.id="custom-import".into();manual.media=Some("user-audio/ting-0123456789ab.wav".into());c.clips.push(manual);
        let snapshot=p.composition_snapshot();
        assert!(snapshot.clips.iter().find(|c|c.id=="host:gfx:0:9").unwrap().media.as_ref().unwrap().is_file());
        assert!(p.composition.as_ref().unwrap().clips.iter().find(|c|c.id=="host:gfx:0:9").unwrap().media.as_ref().unwrap().to_string_lossy().starts_with("old-install"),"Snapshot must be read-only");
        p.ensure_composition();assert!(p.rendered.is_none());
        assert_eq!(segments,serde_json::to_vec(&p.segments).unwrap());assert_eq!(before,pictures(&p));
        let c=p.composition.as_ref().unwrap();assert_eq!(c.script_key,key);
        for sound in c.clips.iter().filter(|c|c.track==timeline::SFX) {
            assert!(sound.locked&&!sound.visible);assert_eq!(sound.gain,0.37);
            match sound.id.as_str() {
                "manual-copy"=>assert_eq!((sound.start_ms,sound.duration_ms,sound.media_in_ms),(100,60,50)),
                "custom-import"=>assert_eq!(sound.media.as_ref().unwrap(),&PathBuf::from("user-audio/ting-0123456789ab.wav")),
                "host:gfx:0:9"=>assert_eq!((sound.start_ms,sound.duration_ms),(800,320)),
                "host:gfx:1:9"=>assert_eq!((sound.start_ms,sound.duration_ms),(2400,202)),
                id=>panic!("Unexpected sound: {id}"),
            }
        }
        let once=serde_json::to_vec(c).unwrap();p.rendered=Some("new.mp4".into());p.ensure_composition();
        assert_eq!(once,serde_json::to_vec(p.composition.as_ref().unwrap()).unwrap());assert!(p.rendered.is_some());
    }

    #[test]
    fn saved_library_refresh_retains_unknown_json_and_does_not_rerun_assets() {
        let temp=tempfile::tempdir().unwrap();let path=temp.path().join("project.json");
        let mut p=project();p.ensure_composition();p.rendered=Some("final.mp4".into());p.status="Đã xuất video".into();
        for c in p.composition.as_mut().unwrap().clips.iter_mut().filter(|c|c.track==timeline::SFX) {
            c.media=Some("old/assets/evidence-sfx-v1/ting-0123456789ab.wav".into());
        }
        let mut raw=serde_json::to_value(p).unwrap();raw["future_project_field"]=serde_json::json!({"keep":true});
        for clip in raw["composition"]["clips"].as_array_mut().unwrap(){clip["future_clip_field"]=serde_json::json!([1,2,3]);}
        let mut serialized=serde_json::to_string(&raw).unwrap();
        serialized.pop();serialized.push_str(",\"precise_cache_metric\":122.43562825520833}");
        std::fs::write(&path,serialized).unwrap();
        assert_eq!(refresh_saved_project(&path).unwrap(),2);
        let migrated:serde_json::Value=serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("122.43562825520833"),"Sound migration must not round cached source metrics");
        assert_eq!(raw["segments"],migrated["segments"]);assert_eq!(raw["rules"],migrated["rules"]);
        assert_eq!(raw["future_project_field"],migrated["future_project_field"]);
        assert!(migrated["composition"]["clips"].as_array().unwrap().iter().all(|c|c["future_clip_field"]==serde_json::json!([1,2,3])));
        assert!(migrated["rendered"].is_null());assert!(path.with_extension("pre-reveal-sfx-v2.json").is_file());
        let bytes=std::fs::read(&path).unwrap();assert_eq!(refresh_saved_project(&path).unwrap(),0);assert_eq!(bytes,std::fs::read(path).unwrap());
    }

    #[test]
    fn tts_duration_changes_reanchor_automatic_sounds_and_legacy_tail() {
        let mut p=shortened_speech_project();
        p.rendered=Some("old-output-with-tail.mp4".into());
        let before=serde_json::to_vec(&p.segments).unwrap();
        let last=p.composition.as_ref().unwrap().clips.iter().rfind(|c|c.track==timeline::SFX).unwrap();
        assert_eq!(last.start_ms,14000,"Fixture must reproduce the stale tail");
        p.ensure_composition();
        assert!(p.rendered.is_none(),"The old exported tail must not remain selected as current output");
        assert_eq!(p.composition_duration(),7000,"Automatic SFX must not extend video beyond its picture/voice");
        let sounds:Vec<_>=p.composition_snapshot().clips.into_iter().filter(|c|c.track==timeline::SFX).collect();
        assert_eq!(sounds.iter().map(|c|c.start_ms).collect::<Vec<_>>(),vec![3800,5400]);
        assert_eq!(before,serde_json::to_vec(&p.segments).unwrap(),"Repair must preserve the approved editorial cuts and narration");
        // Old project files can have the stale absolute times with a saved key.
        p.composition.as_mut().unwrap().clips.iter_mut().find(|c|c.id==sounds[1].id).unwrap().start_ms=14000;
        p.composition.as_mut().unwrap().script_key="evidence-even-windows-adaptive-v3".into();
        p.ensure_composition();assert_eq!(p.composition_duration(),7000);
        // Intentional SFX trims/moves, gain and visibility remain user edits.
        let c=p.composition.as_mut().unwrap().clips.iter_mut().find(|c|c.id==sounds[1].id).unwrap();
        c.start_ms=5500;c.duration_ms=100;c.media_in_ms=30;c.timing_manual=true;c.gain=0.7;c.visible=false;
        p.card_sound.style="pop".into();p.ensure_composition();
        let c=p.composition.as_ref().unwrap().clips.iter().find(|c|c.id==sounds[1].id).unwrap();
        assert_eq!((c.start_ms,c.duration_ms,c.media_in_ms),(5500,100,30));assert_eq!(c.gain,0.7);assert!(!c.visible);
    }

    #[test]
    fn real_export_after_shorter_speech_has_no_late_sound_or_black_tail() {
        use rodio::Source;
        let temp=tempfile::tempdir().unwrap();let mut p=shortened_speech_project();p.root=temp.path().into();
        p.output.resolution=360;p.output.fps=24;p.subtitles.enabled=false;
        p.timeline.tracks[timeline::AVATAR].muted=true;p.ensure_composition();
        let s=Settings{encoder:"libx264".into(),..Default::default()};let cancel=process::cancel_token();
        let output=p.root.join("fixed.mp4");crate::layer_media::export(&p,&s,&output,&cancel).unwrap();
        let info=crate::media::probe(&s,&output,&cancel).unwrap();assert!(info.duration_ms.abs_diff(7000)<100);
        let wav=p.root.join("audio.wav");
        process::run(&s.ffmpeg,&["-y".into(),"-v".into(),"error".into(),"-i".into(),output.to_string_lossy().into_owned(),"-vn".into(),"-ac".into(),"1".into(),"-ar".into(),"48000".into(),"-c:a".into(),"pcm_s16le".into(),wav.to_string_lossy().into_owned()],None,&cancel,std::time::Duration::from_secs(30),None).unwrap();
        let samples=rodio::Decoder::new(std::io::BufReader::new(std::fs::File::open(wav).unwrap())).unwrap().convert_samples::<f32>().collect::<Vec<_>>();
        let rms=|a:usize,b:usize|(samples[a*48..b*48].iter().map(|x|x*x).sum::<f32>()/((b-a)*48) as f32).sqrt();
        assert!(rms(3810,3980)>0.001&&rms(5410,5580)>0.001,"SFX must remain audible at the actual card entrances");
        assert!(rms(6200,6950)<0.0001,"No added ending sound is allowed");
        let end=p.composition_duration();let (args,_)=crate::layer_media::audio_args(&p,end-1000,end);
        assert!(args.iter().any(|a|a.starts_with("anullsrc")),"Preview's final interval must also be silent without a card entrance");
    }

    #[test]
    fn card_sounds_follow_cues_mute_groups_splits_and_legacy_projects() {
        let mut p=project();p.ensure_composition();
        let sound=|p:&Project|p.composition_snapshot().clips.into_iter().filter(|c|c.track==timeline::SFX).collect::<Vec<_>>();
        let clips=sound(&p);assert_eq!(clips.len(),2);assert_eq!(clips[0].start_ms,800);
        assert!(clips[0].media.as_ref().unwrap().file_name().unwrap().to_string_lossy().starts_with("ting-"));
        assert!(clips.iter().all(|c|c.enabled(&p)));
        p.card_sound.enabled=false;assert!(sound(&p).iter().all(|c|!c.enabled(&p)));
        p.card_sound.enabled=true;p.card_sound.gain=0.6;assert!((sound(&p)[0].effective_gain(&p)-0.6).abs()<0.001);
        p.timeline.tracks[timeline::GRAPHICS].muted=true;assert!(sound(&p).iter().all(|c|!c.enabled(&p)));
        p.timeline.tracks[timeline::GRAPHICS].muted=false;p.timeline.tracks[timeline::SFX].muted=true;
        assert!(sound(&p).iter().all(|c|!c.enabled(&p)));p.timeline.tracks[timeline::SFX].muted=false;
        p.composition.as_mut().unwrap().clips.iter_mut().find(|c|c.track==timeline::GRAPHICS).unwrap().visible=false;
        assert!(sound(&p).iter().all(|c|!c.enabled(&p)));
        p=project();p.ensure_composition();
        p.time_layer("host-5",400,0,4000).unwrap();
        assert_eq!(sound(&p)[0].start_ms,1200,"Moving the linked graphics must move its entrance sound");
        p=project();timeline::split(&mut p,0,1200).unwrap();p.ensure_composition();
        let sounds=sound(&p);assert_eq!(sounds.len(),2,"A split inside the first card must not replay its entrance");
        assert_eq!(sounds.iter().map(|c|c.start_ms).collect::<Vec<_>>(),vec![800,2400]);
        p.card_sound.style="pop".into();p.ensure_composition();
        assert!(sound(&p).iter().all(|c|c.media.as_ref().unwrap().file_name().unwrap().to_string_lossy().starts_with("pop-")));
        p.segments[0].graphics.clear();p.ensure_composition();assert_eq!(sound(&p).len(),1,"Deleted cards must not leave orphan sound clips");
        let legacy:Project=serde_json::from_str(r#"{"segments":[{"graphics":[{"kind":"fact","start_ms":100,"end_ms":900}]}]}"#).unwrap();
        assert_eq!(legacy.card_sound,CardSoundSettings::default());assert!(!legacy.segments[0].graphics[0].sound_suppressed);
        let mut invalid=CardSoundSettings{gain:f32::NAN,..Default::default()};assert!(invalid.validate().is_err());
        invalid.gain=0.5;invalid.style="missing".into();assert!(invalid.validate().is_err());
    }

    #[test]
    fn real_mp4_card_audio_has_correct_timing_gain_mute_and_reuses_video_cache() {
        use rodio::Source;
        let temp=tempfile::tempdir().unwrap();let root=std::env::var_os("AIR3VIEW_QA_DIR").map(PathBuf::from).unwrap_or_else(||temp.path().into());
        std::fs::create_dir_all(&root).unwrap();let mut p=project();p.root=root.clone();
        p.output.resolution=360;p.output.fps=24;p.subtitles.enabled=false;
        p.timeline.tracks[timeline::AVATAR].muted=true;
        let s=Settings{encoder:"libx264".into(),..Default::default()};let cancel=process::cancel_token();
        let read=|file:&std::path::Path|{
            let wav=file.with_extension("decoded.wav");
            process::run(&s.ffmpeg,&["-y".into(),"-v".into(),"error".into(),"-i".into(),file.to_string_lossy().into_owned(),"-vn".into(),"-ac".into(),"1".into(),"-ar".into(),"48000".into(),"-c:a".into(),"pcm_s16le".into(),wav.to_string_lossy().into_owned()],None,&cancel,std::time::Duration::from_secs(30),None).unwrap();
            rodio::Decoder::new(std::io::BufReader::new(std::fs::File::open(wav).unwrap())).unwrap().convert_samples::<f32>().collect::<Vec<_>>()
        };
        let rms=|v:&[f32],a:usize,b:usize|(v[a*48..b*48].iter().map(|x|x*x).sum::<f32>()/((b-a)*48) as f32).sqrt();
        p.card_sound.gain=0.5;p.ensure_composition();
        let full=root.join("full.mp4");crate::layer_media::export(&p,&s,&full,&cancel).unwrap();let high=read(&full);
        assert!(rms(&high,0,650)<0.0001,"Must be silent before the first card");
        let amplitude=rms(&high,850,1100);assert!(amplitude>0.001,"Missing card sound in actual MP4");
        assert!(rms(&high,2405,2480)>0.005,"Second entrance sound missing: {}; first: {amplitude}; graph: {:?}",rms(&high,2405,2480),crate::layer_media::audio_args(&p,0,4000));
        p.card_sound.gain=0.25;let low_file=root.join("low.mp4");crate::layer_media::export(&p,&s,&low_file,&cancel).unwrap();let low=read(&low_file);
        let ratio=rms(&low,850,1100)/amplitude;assert!((0.43..0.57).contains(&ratio),"Expected half volume, got {ratio}");
        let report:serde_json::Value=serde_json::from_slice(&std::fs::read(p.root.join("cache/composition-export/render-report.json")).unwrap()).unwrap();
        assert_eq!(report["encoded_frames"],0,"Changing SFX volume must reuse picture cache");
        p.card_sound.enabled=false;let mute=root.join("mute.mp4");crate::layer_media::export(&p,&s,&mute,&cancel).unwrap();let silent=read(&mute);
        assert!(rms(&silent,0,3900)<0.0001,"Disabled SFX must not leak into export");
        assert_eq!(Composition::from_project(&p).clips.iter().filter(|c|c.track==timeline::SFX).count(),2);
    }
}
