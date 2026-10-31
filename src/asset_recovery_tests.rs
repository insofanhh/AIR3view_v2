use crate::{batch,evidence::{self,EvidenceItem,Proof},input_assets,media,model::*,player,process,project_store,timeline};
use std::{fs,path::Path,time::Duration};

fn fixture(root:&Path)->(Project,Settings) {
    let s=Settings{encoder:"libx264".into(),..Default::default()};
    let source=root.join("source.mp4");
    process::run(&s.ffmpeg,&process::args(&["-y","-v","error","-f","lavfi","-i","testsrc2=size=320x180:rate=12","-t","2","-c:v","libx264","-pix_fmt","yuv420p",&source.to_string_lossy()]),None,&process::cancel_token(),Duration::from_secs(30),None).unwrap();
    let card=root.join("card.png");image::RgbaImage::from_pixel(320,180,image::Rgba([40,180,80,255])).save(card).unwrap();
    let mut p=Project{root:root.to_owned(),..Default::default()};
    p.output.resolution=360;p.output.fps=12;p.subtitles.enabled=false;
    p.rules.min_seconds=1;p.rules.max_seconds=2;
    p.sources.push(Source{id:"src".into(),path:source,duration_ms:2000,width:320,height:180,..Default::default()});
    p.evidence_settings.background="paper".into();
    p.segments.push(Segment{id:"host".into(),kind:"host".into(),duration_ms:1200,evidence_items:vec![
        EvidenceItem{enabled:false,..Default::default()},
        EvidenceItem{id:"card".into(),source_id:"src".into(),source_time_ms:200,asset:Some("card.png".into()),asset_key:"same-card".into(),kind:"fact".into(),value:"1".into(),proof:Proof{quote:"Source".into(),..Default::default()},..Default::default()}
    ],..Default::default()});
    p.ensure_composition();(p,s)
}

#[test]
fn background_repair_tracks_selection_preserves_edits_and_recovers_after_restart() {
    let temp=tempfile::tempdir().unwrap();let (mut p,s)=fixture(temp.path());let cancel=process::cancel_token();
    let bg=p.composition.as_mut().unwrap().clips.iter_mut().find(|c|c.track==timeline::BACKGROUND).unwrap();
    bg.transform.x=0.37;bg.locked=true;bg.z= -7;
    let baseline=bg.clone();
    for portrait in [true,false] { for style in ["paper","blur","dark"] {
        p.output.portrait=portrait;p.evidence_settings.background=style.into();p.ensure_composition();
        evidence::prepare_backgrounds(&p,&s,&cancel).unwrap();
        let path=evidence::background_path(&p,&p.segments[0]);
        assert_eq!(image::image_dimensions(&path).unwrap(),if portrait{(720,1280)}else{(1280,720)});
        let bg=p.composition.as_ref().unwrap().clips.iter().find(|c|c.track==timeline::BACKGROUND).unwrap();
        assert_eq!(bg.transform,baseline.transform);assert_eq!(bg.z,baseline.z);assert!(bg.locked);
        player::decode_test_frame(&p,&s,bg,0).unwrap();
    }}
    p.evidence_settings.background="blur".into();p.ensure_composition();
    let first=evidence::background_path(&p,&p.segments[0]);
    p.segments[0].evidence_items[0].asset_key="disabled key must not matter".into();
    assert_eq!(first,evidence::background_path(&p,&p.segments[0]));
    p.segments[0].evidence_items.swap(0,1);assert_eq!(first,evidence::background_path(&p,&p.segments[0]));
    p.segments[0].evidence_items[0].source_time_ms=700;
    assert_ne!(first,evidence::background_path(&p,&p.segments[0]));p.ensure_composition();
    p.save().unwrap();let p=Project::load(&p.root.join("project.json")).unwrap();
    let output=p.root.join("repaired.mp4");
    crate::layer_media::validate_export_inputs(&p).unwrap();
    crate::layer_media::export(&p,&s,&output,&cancel).unwrap();
    crate::layer_media::validate_assets(&p).unwrap();
    assert!((media::probe(&s,&output,&cancel).unwrap().duration_ms as i64-1200).abs()<150);
    let path=evidence::background_path(&p,&p.segments[0]);fs::remove_file(&path).unwrap();
    // Preview repairs without an export or provider call.
    let mut preview=player::Player::default();preview.composition(p.clone(),s.clone(),0,false);
    let mut delivered=false;
    // Debug image convolution competes with real ASR/render jobs on QA hosts.
    // A fixed overall deadline tolerates scheduling gaps while still requiring
    // an actual repaired image and a delivered preview frame.
    let deadline=std::time::Instant::now()+Duration::from_secs(60);
    while let Some(remaining)=deadline.checked_duration_since(std::time::Instant::now()) {
        match preview.rx.recv_timeout(remaining.min(Duration::from_millis(250))) {
            Ok(player::PlayerEvent::Layers{frames,..})=>{assert!(frames.iter().any(|(id,_)|id==&baseline.id));delivered=true;break;},
            Ok(player::PlayerEvent::Error{message,..})=>panic!("{message}"),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)=>break,
            _=>{}
        }
    }
    assert!(delivered&&path.is_file());
    fs::write(&path,b"corrupt cache").unwrap();evidence::prepare_backgrounds(&p,&s,&cancel).unwrap();assert!(image::image_dimensions(&path).is_ok());
}

#[test]
fn background_repair_cancellation_concurrency_and_missing_real_source() {
    let temp=tempfile::tempdir().unwrap();let (mut p,s)=fixture(temp.path());
    let cancel=process::cancel_token();cancel.store(true,std::sync::atomic::Ordering::Relaxed);
    assert!(evidence::prepare_backgrounds(&p,&s,&cancel).is_err());
    assert!(!evidence::background_path(&p,&p.segments[0]).exists());
    std::thread::scope(|scope| {
        let a=scope.spawn(||evidence::prepare_backgrounds(&p,&s,&process::cancel_token()));
        let b=scope.spawn(||evidence::prepare_backgrounds(&p,&s,&process::cancel_token()));
        a.join().unwrap().unwrap();b.join().unwrap().unwrap();
    });
    p.evidence_settings.background="blur".into();p.sources[0].path=temp.path().join("missing-source.mp4");p.ensure_composition();
    assert!(evidence::prepare_backgrounds(&p,&s,&process::cancel_token()).is_err());
    assert!(!evidence::background_path(&p,&p.segments[0]).exists());
    // User replacement, hidden and deleted layers are not rebound or recreated.
    let replacement=p.root.join("custom.png");fs::copy(p.root.join("card.png"),&replacement).unwrap();
    let bg=p.composition.as_mut().unwrap().clips.iter_mut().find(|c|c.track==timeline::BACKGROUND).unwrap();bg.media=Some(replacement.clone());
    p.ensure_composition();assert_eq!(p.composition.as_ref().unwrap().clips.iter().find(|c|c.track==timeline::BACKGROUND).unwrap().media.as_ref(),Some(&replacement));
    evidence::prepare_backgrounds(&p,&s,&process::cancel_token()).unwrap();
    p.composition.as_mut().unwrap().clips.retain(|c|c.track!=timeline::BACKGROUND);
    evidence::prepare_backgrounds(&p,&s,&process::cancel_token()).unwrap();
}

#[test]
fn voice_defaults_survive_owner_deletion_and_three_url_batch_restart() {
    let temp=tempfile::tempdir().unwrap();let data=temp.path();let library=data.join("projects");
    let mut a=Project::default();a.root=library.join(&a.id);fs::create_dir_all(a.root.join("inputs")).unwrap();
    let original=b"same reference bytes without reencoding";let voice=a.root.join("inputs").join(format!("{}.wav",media::hash(original)));
    fs::write(&voice,original).unwrap();
    a.production=Some(ProductionSettings{avatar_enabled:false,voice_reference_enabled:true,voice_reference_path:voice.to_string_lossy().into(),..Default::default()});a.save().unwrap();
    let mut defaults=ProjectDefaults::from_project(&a);
    input_assets::pin_production(data,defaults.production.as_mut().unwrap()).unwrap();
    write_json(&data.join("project-defaults.json"),&defaults).unwrap();
    project_store::delete(&library,&a.root.join("project.json"),Some(&a)).unwrap();
    let mut b=Project::default();ProjectDefaults::load(&data.join("project-defaults.json")).unwrap().apply(&mut b);
    let s=Settings{provider:"antigravity".into(),avatar_enabled:false,..Default::default()};
    let urls="https://youtu.be/dBy-8s6ly08\nhttps://youtu.be/r1cvbJJ5G54\nhttps://youtu.be/dNYsRcfWeLU";
    // Also recover a legacy snapshot path by exact digest, never by voice name.
    b.production.as_mut().unwrap().voice_reference_path=voice.to_string_lossy().into();
    let batch=batch::create(data,"After delete",urls,&b,&s,false,None).unwrap();
    for item in &batch.items {
        let child=Project::load(&batch::project_path(data,item).unwrap()).unwrap();
        let path=child.production.as_ref().unwrap().voice_reference_path.clone();
        assert!(Path::new(&path).starts_with(child.root.join("inputs")));assert_eq!(fs::read(path).unwrap(),original);
    }
    assert_eq!(batch.items.len(),3);
    assert!(Path::new(&ProjectDefaults::load(&batch::root(data,&batch.id).unwrap().join("template.json")).unwrap().production.unwrap().voice_reference_path).starts_with(data.join("assets/references")));
    let before=project_store::list(&library).len();
    b.production.as_mut().unwrap().voice_reference_path=data.join("gone/unknown.wav").to_string_lossy().into();
    assert!(batch::create(data,"Missing",urls,&b,&s,false,None).is_err());assert_eq!(project_store::list(&library).len(),before);
    assert_eq!(fs::read_dir(&library).unwrap().count(),before);
}

#[test]
fn batch_write_failure_rolls_back_children_and_delete_checks_saved_dependencies() {
    let temp=tempfile::tempdir().unwrap();let data=temp.path();
    let mut p=Project{production:Some(ProductionSettings{avatar_enabled:false,..Default::default()}),..Default::default()};
    let s=Settings{provider:"antigravity".into(),avatar_enabled:false,..Default::default()};
    fs::write(data.join("batches"),b"locked parent fixture").unwrap();
    assert!(batch::create(data,"Fail publish","https://youtu.be/dBy-8s6ly08",&p,&s,false,None).is_err());
    assert_eq!(fs::read_dir(data.join("projects")).unwrap().count(),0);
    p.root=data.join("projects").join(&p.id);p.save().unwrap();let voice=p.root.join("voice.wav");fs::write(&voice,b"voice").unwrap();
    write_json(&data.join("project-defaults.json"),&serde_json::json!({"production":{"voice_reference_path":voice}})).unwrap();
    assert!(project_store::delete(&data.join("projects"),&p.root.join("project.json"),None).unwrap_err().to_string().contains("kho chung"));
    assert!(p.root.exists());
}

#[test]
fn voice_recovery_uses_recorded_identity_and_rejects_a_different_voice() {
    let temp=tempfile::tempdir().unwrap();let data=temp.path();
    let source=data.join("old-normalized-filename.wav");fs::write(&source,b"normalized bytes").unwrap();
    let pinned=input_assets::shared_file(data,&source.to_string_lossy(),"giọng tham chiếu").unwrap();
    fs::remove_file(&source).unwrap();
    assert_eq!(input_assets::shared_file(data,&source.to_string_lossy(),"giọng tham chiếu").unwrap(),pinned);
    let missing=data.join("deleted").join(format!("{}.wav",media::hash(b"another voice")));
    fs::create_dir_all(data.join("assets/voices")).unwrap();
    fs::write(data.join("assets/voices").join(missing.file_name().unwrap()),b"same filename but wrong bytes").unwrap();
    assert!(input_assets::shared_file(data,&missing.to_string_lossy(),"giọng tham chiếu").is_err());
    #[cfg(windows)] {
        use std::os::windows::fs::OpenOptionsExt;
        let _lock=fs::OpenOptions::new().read(true).share_mode(0).open(&pinned).unwrap();
        assert!(input_assets::shared_file(data,&pinned.to_string_lossy(),"giọng tham chiếu").is_err());
    }
}

#[test]
fn inactive_reference_files_are_pinned_without_enabling_providers() {
    let temp=tempfile::tempdir().unwrap();let data=temp.path();
    let voice=data.join("voice.wav");let photo=data.join("photo.png");
    fs::write(&voice,b"unused voice").unwrap();fs::write(&photo,b"unused photo").unwrap();
    let mut p=ProductionSettings{voice_reference_enabled:false,voice_reference_path:voice.to_string_lossy().into(),avatar_enabled:false,avatar_provider:"cartoon2d".into(),avatar_image_path:photo.to_string_lossy().into(),..Default::default()};
    input_assets::pin_production(data,&mut p).unwrap();
    assert!(!p.voice_reference_enabled&&!p.avatar_enabled);assert_eq!(p.avatar_provider,"cartoon2d");
    assert!(Path::new(&p.voice_reference_path).starts_with(data.join("assets/references")));
    assert!(Path::new(&p.avatar_image_path).starts_with(data.join("assets/references")));
    p.voice_reference_path=data.join("old-unused-missing.wav").to_string_lossy().into();
    input_assets::pin_production(data,&mut p).unwrap();
}
