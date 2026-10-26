//! End-to-end fixtures use real FFmpeg media, not provider mocks.
use crate::{model::*,process,media,composition,background_music as music,visual_effects as fx,source_normalization as normalize,timeline};
use std::{fs,path::Path,time::Duration};
fn run(s:&Settings,args:&[&str]){process::run(&s.ffmpeg,&process::args(args),None,&process::cancel_token(),Duration::from_secs(60),None).unwrap();}
fn fixture()->(tempfile::TempDir,Project,Settings){
    let temp=tempfile::tempdir().unwrap();let s=Settings::default();let root=temp.path();let input=root.join("source.mp4");let voice=root.join("voice.wav");let song=root.join("song.wav");
    run(&s,&["-y","-v","error","-f","lavfi","-i","color=red:s=320x180:r=24:d=8,drawbox=x=160:y=0:w=160:h=180:color=blue:t=fill","-f","lavfi","-i","sine=frequency=220:sample_rate=48000:duration=8","-c:v","libx264","-preset","ultrafast","-c:a","aac","-shortest",input.to_str().unwrap()]);
    for(path,freq,duration)in[(&voice,"660","4"),(&song,"440","3")]{run(&s,&["-y","-v","error","-f","lavfi","-i",&format!("sine=frequency={freq}:sample_rate=48000:duration={duration}"),"-ac","2",path.to_str().unwrap()]);}
    let mut p=Project{root:root.to_owned(),..Default::default()};p.sources=vec![media::import_local(&s,&input,"src_1".into(),&process::cancel_token()).unwrap()];
    p.rules.min_seconds=8;p.rules.max_seconds=8;p.rules.hook_enabled=false;p.rules.intercuts_enabled=false;p.subtitles.enabled=false;p.evidence_settings.enabled=false;p.output.portrait=false;p.output.resolution=240;p.output.fps=24;
    p.segments=vec![Segment{id:"hook".into(),source_id:"src_1".into(),out_ms:2000,duration_ms:2000,..Default::default()},Segment{id:"review".into(),kind:"review".into(),source_id:"src_1".into(),in_ms:2000,out_ms:6000,duration_ms:4000,narration:"Independent voice remains unchanged".into(),audio:Some(voice),..Default::default()},Segment{id:"intercut".into(),source_id:"src_1".into(),in_ms:6000,out_ms:8000,duration_ms:2000,..Default::default()}];
    p.music=music::Settings{enabled:true,file:song,..Default::default()};p.ensure_composition();(temp,p,s)
}
fn rms(path:&Path,start:f64,end:f64)->f64{let s=Settings::default();let output=process::run(&s.ffmpeg,&process::args(&["-v","error","-ss",&start.to_string(),"-i",path.to_str().unwrap(),"-t",&(end-start).to_string(),"-f","s16le","-ac","1","-ar","8000","-"]),None,&process::cancel_token(),Duration::from_secs(30),None).unwrap();let samples:Vec<f64>=output.stdout.chunks_exact(2).map(|v|i16::from_le_bytes([v[0],v[1]])as f64).collect();(samples.iter().map(|v|v*v).sum::<f64>()/samples.len().max(1)as f64).sqrt()}
#[test]
fn music_loops_ducks_seeks_and_reuses_picture_cache(){
    let (_temp,mut p,s)=fixture();let cancel=process::cancel_token();let original_voice=fs::read(p.segments[1].audio.as_ref().unwrap()).unwrap();
    music::prepare(&p,&s,&cancel).unwrap();let bed=music::prepared_path(&p);
    assert!(rms(&bed,0.2,1.0)<1.);assert!(rms(&bed,2.8,3.2)>100.);assert!(rms(&bed,6.5,7.5)<1.);
    let ducked=rms(&bed,3.5,4.0);p.music.duck=false;p.ensure_composition();music::prepare(&p,&s,&cancel).unwrap();assert!(rms(&music::prepared_path(&p),3.5,4.0)>ducked*2.5);
    let first=p.root.join("first.mp4");media::render(&p,&s,&first,&cancel,|_,_|{}).unwrap();
    p.music.gain=0.08;p.ensure_composition();let second=p.root.join("second.mp4");media::render(&p,&s,&second,&cancel,|_,_|{}).unwrap();
    let report:serde_json::Value=serde_json::from_slice(&fs::read(p.root.join("cache/composition-export/render-report.json")).unwrap()).unwrap();assert_eq!(report["encoded_frames"],0,"Changing music must reuse every picture chunk");
    let (args,filter)=crate::layer_media::audio_args(&p,3500,5000);assert!(args.iter().any(|v|v=="3.500000"));assert!(filter.contains("[mix]"));
    assert_eq!(fs::read(p.segments[1].audio.as_ref().unwrap()).unwrap(),original_voice);
    assert!(media::probe(&s,&second,&cancel).unwrap().duration_ms.abs_diff(8000)<50);
}
#[test]
fn preview_and_export_apply_mirror_and_lut_without_flipping_overlays(){
    let (_temp,mut p,s)=fixture();p.music.enabled=false;p.effects.footage=fx::Effect{mirror_x:true,lut:"warm".into(),..Default::default()};p.ensure_composition();
    let comp=p.composition_snapshot();let layer=comp.clips.iter().find(|c|c.track==timeline::VIDEO).unwrap();
    let img=crate::player::decode_test_frame(&p,&s,layer,0).unwrap();let left=img.get_pixel(img.width()/4,img.height()/2);assert!(left[2]>left[0]+100,"Mirrored left must be blue: {left:?}");
    let caption=composition::LayerClip{track:timeline::CAPTIONS,media:None,..layer.clone()};assert!(fx::selected(&p,&caption).is_none());
    for portrait in[false,true]{p.output.portrait=portrait;p.ensure_composition();let out=p.root.join(format!("mirror-{portrait}.mp4"));media::render(&p,&s,&out,&process::cancel_token(),|_,_|{}).unwrap();let frame=media::frame(&s,&out,500,&process::cancel_token()).unwrap();let left=frame.get_pixel(frame.width()/4,frame.height()/2);assert!(left[2]>left[0]+70,"Export differs from mirror preview");}
    let custom=p.root.join("custom.cube");fs::write(&custom,"LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n").unwrap();
    let path=fx::import(&custom,&p.root.join("assets")).unwrap();p.effects.footage.lut=path.to_string_lossy().into_owned();let old=fx::identity(&p,layer);fs::write(&path,fs::read_to_string(&path).unwrap().replace("1 1 1","0.9 1 1")).unwrap();assert_ne!(old,fx::identity(&p,layer));
    fs::write(&custom,"LUT_3D_SIZE 3\n0 0 0").unwrap();assert!(fx::import(&custom,&p.root.join("assets")).is_err());
}
#[test]
fn normalization_preserves_source_and_legacy_projects_and_defaults(){
    let (_temp,p,s)=fixture();let input=&p.sources[0].path;let bytes=fs::read(input).unwrap();
    for mode in["auto","keep","remux","transcode"]{let mut source=p.sources[0].clone();normalize::prepare(&normalize::Settings{mode:mode.into(),proxy:false},&s,&mut source,&p.root,&process::cancel_token()).unwrap();let info=media::probe(&s,source.input_path(),&process::cancel_token()).unwrap();assert!(info.duration_ms.abs_diff(8000)<60);assert_eq!(fs::read(input).unwrap(),bytes);}
    let old:Project=serde_json::from_str("{\"timeline\":{\"tracks\":[{},{},{},{},{},{},{},{},{},{}]}}").unwrap();assert_eq!(old.timeline.tracks.len(),11);assert!(!old.music.enabled&&!old.effects.footage.active());
    let defaults=ProjectDefaults::from_project(&p);let mut next=Project::default();defaults.apply(&mut next);assert_eq!(next.music.file,p.music.file);assert!(next.effects.layers.is_empty());
}
#[test]
fn normalized_rotation_hdr_and_silent_inputs_keep_time_and_display_geometry(){
    let (_temp,p,s)=fixture();let cancel=process::cancel_token();
    let rotated=p.root.join("rotated.mp4");run(&s,&["-y","-v","error","-display_rotation:v:0","90","-i",p.sources[0].path.to_str().unwrap(),"-c","copy",rotated.to_str().unwrap()]);
    let mut source=media::import_local(&s,&rotated,"rotated".into(),&cancel).unwrap();assert_eq!((source.width,source.height),(180,320));
    normalize::prepare(&normalize::Settings{mode:"transcode".into(),proxy:true},&s,&mut source,&p.root,&cancel).unwrap();let info=media::probe(&s,source.input_path(),&cancel).unwrap();assert_eq!((info.width,info.height),(180,320));assert!(info.duration_ms.abs_diff(8000)<50);
    let hdr=p.root.join("hdr-silent.mp4");run(&s,&["-y","-v","error","-f","lavfi","-i","testsrc2=s=320x180:r=30:d=2","-c:v","libx264","-preset","ultrafast","-color_primaries","bt2020","-color_trc","smpte2084","-colorspace","bt2020nc","-bsf:v","h264_metadata=colour_primaries=9:transfer_characteristics=16:matrix_coefficients=9",hdr.to_str().unwrap()]);
    let mut source=media::import_local(&s,&hdr,"hdr".into(),&cancel).unwrap();normalize::prepare(&normalize::Settings{mode:"auto".into(),proxy:false},&s,&mut source,&p.root,&cancel).unwrap();assert!(source.normalized.is_file());let info=media::probe(&s,source.input_path(),&cancel).unwrap();assert!(!info.has_audio&&info.duration_ms.abs_diff(2000)<50);
}

#[test]
fn music_real_import_offset_tail_and_cancel_are_checked(){
    let (_temp,mut p,s)=fixture();let cancel=process::cancel_token();
    let input=std::env::var_os("AIR3VIEW_TEST_MUSIC").map(std::path::PathBuf::from).unwrap_or_else(||p.music.file.clone());
    let imported=music::import(&s,&input,&p.root.join("assets/music"),&cancel).unwrap();
    assert_eq!(fs::read(&input).unwrap(),fs::read(&imported).unwrap());
    let info=media::probe(&s,&imported,&cancel).unwrap();p.music.file=imported;p.music.offset_ms=info.duration_ms-1500;p.music.duck=false;p.music.mute_source=false;p.ensure_composition();
    music::prepare(&p,&s,&cancel).unwrap();assert!(rms(&music::prepared_path(&p),5.,6.)>10.);
    p.music.looping=false;p.ensure_composition();music::prepare(&p,&s,&cancel).unwrap();assert!(rms(&music::prepared_path(&p),5.,6.)<1.);
    p.music.offset_ms=info.duration_ms+1000;p.ensure_composition();assert!(music::prepare(&p,&s,&cancel).unwrap_err().to_string().contains("Điểm bắt đầu"));
    p.music.offset_ms=0;p.ensure_composition();let stopped=process::cancel_token();stopped.store(true,std::sync::atomic::Ordering::Relaxed);assert!(music::prepare(&p,&s,&stopped).is_err());
}
#[test]
fn music_and_picture_overrides_follow_independent_timeline_edits(){
    let (_temp,mut p,s)=fixture();let cancel=process::cancel_token();
    p.music.duck=false;p.composition.as_mut().unwrap().clips.iter_mut().filter(|c|c.scene_id=="hook").for_each(|c|{c.start_ms=2500;c.duration_ms=1000;c.linked=false;c.timing_manual=true;});
    p.ensure_composition();music::prepare(&p,&s,&cancel).unwrap();assert!(rms(&music::prepared_path(&p),0.4,0.9)>100.);assert!(rms(&music::prepared_path(&p),2.8,3.1)<1.);
    p.music.gain=0.07;let snapshot=p.composition_snapshot();assert_eq!(snapshot.clips.iter().find(|c|c.id=="music-bed").unwrap().gain,0.07);
    let id=p.composition.as_ref().unwrap().clips.iter().find(|c|c.track==timeline::VIDEO&&c.scene_id=="review").unwrap().id.clone();
    p.composition.as_mut().unwrap().clips.iter_mut().find(|c|c.id==id).unwrap().linked=false;
    p.effects.layers.insert(id.clone(),fx::Effect{mirror_y:true,..Default::default()});
    let split=p.edit_independent_layer(&id,"split",4000,&mut None).unwrap().unwrap();assert!(p.effects.layers[&split].mirror_y);
}
