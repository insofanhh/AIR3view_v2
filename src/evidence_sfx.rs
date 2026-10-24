//! Embedded CC0 reveal sounds. One local copy per content version; no network/provider.
use std::{path::PathBuf,sync::OnceLock};
#[derive(Clone,Debug,serde::Serialize,serde::Deserialize,PartialEq)]
#[serde(default)]
pub struct CardSoundSettings {
    pub enabled:bool,
    pub style:String,
    pub gain:f32,
}
impl Default for CardSoundSettings {
    fn default()->Self {Self{enabled:true,style:"auto".into(),gain:0.25}}
}
impl CardSoundSettings {
    pub fn validate(&self)->anyhow::Result<()> {
        anyhow::ensure!(matches!(self.style.as_str(),"auto"|"whoosh"|"pop"|"ting") && self.gain.is_finite() && (0.0..=2.0).contains(&self.gain),"Thiết lập âm thanh thẻ thông tin không hợp lệ");
        Ok(())
    }
}
pub fn sound(index:usize,kind:&str)->Option<(PathBuf,u64)> {
    selected_sound("auto",index,kind)
}
pub fn selected_sound(style:&str,index:usize,kind:&str)->Option<(PathBuf,u64)> {
    static LIBRARY:OnceLock<Vec<Option<(PathBuf,u64)>>>=OnceLock::new();
    let library=LIBRARY.get_or_init(||{
        let root=crate::model::data_dir().join("assets/evidence-sfx-v1");
        let _=std::fs::create_dir_all(&root);
        let _=std::fs::write(root.join("CREDITS.md"),include_bytes!("../resources/sfx/CREDITS.md"));
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
