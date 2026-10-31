//! Viewport parity tests use actual FFmpeg output, including a synthetic camera HUD.
use crate::{
    composition::Composition, framed_layout as layout, media, model::*, process, timeline,
};
use std::{path::Path, time::Duration};

fn run(s: &Settings, args: &[&str]) {
    process::run(
        &s.ffmpeg,
        &process::args(args),
        None,
        &process::cancel_token(),
        Duration::from_secs(90),
        None,
    )
    .unwrap();
}
fn project(root: &Path) -> Project {
    let mut p = Project {
        root: root.into(),
        ..Default::default()
    };
    p.sources = vec![Source {
        id: "source".into(),
        path: root.join("input.mp4"),
        width: 320,
        height: 180,
        duration_ms: 6000,
        has_audio: true,
        ..Default::default()
    }];
    p.segments = vec![
        Segment {
            id: "raw".into(),
            kind: "source".into(),
            source_id: "source".into(),
            out_ms: 3000,
            duration_ms: 3000,
            ..Default::default()
        },
        Segment {
            id: "review".into(),
            kind: "review".into(),
            source_id: "source".into(),
            in_ms: 3000,
            out_ms: 6000,
            duration_ms: 3000,
            narration: "Test caption".into(),
            ..Default::default()
        },
    ];
    p.output.portrait = true;
    p.output.resolution = 360;
    p.output.fps = 24;
    p.output.layout.enabled = true;
    p.output.layout.framing.trim[1] = 0.2;
    p.output.layout.background = root.join("background.png");
    p.evidence_settings.enabled = false;
    p.subtitles.enabled = false;
    p.rules.min_seconds = 6;
    p.rules.max_seconds = 6;
    p.rules.hook_enabled = false;
    p.rules.intercuts_enabled = false;
    p.card_sound.enabled = false;
    p
}
fn fixture() -> (tempfile::TempDir, Project, Settings) {
    let temp = tempfile::tempdir().unwrap();
    let p = project(temp.path());
    let s = Settings::default();
    image::RgbImage::from_fn(320, 100, |x, _| {
        if x < 160 {
            image::Rgb([240, 200, 10])
        } else {
            image::Rgb([20, 200, 30])
        }
    })
    .save(&p.output.layout.background)
    .unwrap();
    run(&s,&["-y","-v","error","-f","lavfi","-i","color=blue:s=320x180:r=24:d=6,drawbox=x=0:y=0:w=160:h=180:color=green:t=fill,drawbox=x=0:y=0:w=320:h=32:color=red:t=fill","-f","lavfi","-i","sine=frequency=440:sample_rate=48000:duration=6","-c:v","libx264","-preset","ultrafast","-c:a","aac","-shortest",p.sources[0].path.to_str().unwrap()]);
    (temp, p, s)
}
fn frame(s: &Settings, file: &Path, out: &Path) {
    run(
        s,
        &[
            "-y",
            "-v",
            "error",
            "-ss",
            "1",
            "-i",
            file.to_str().unwrap(),
            "-frames:v",
            "1",
            out.to_str().unwrap(),
        ],
    );
}
#[test]
fn defaults_and_override_validation_are_backward_compatible() {
    let p: Project = serde_json::from_str("{}").unwrap();
    assert!(!p.output.layout.enabled);
    let mut cfg = layout::Settings::default();
    cfg.validate().unwrap();
    cfg.framing.trim = [0.5, 0., 0.4, 0.];
    assert!(cfg.validate().is_err());
    cfg.framing = Default::default();
    cfg.framing.zoom = f32::NAN;
    assert!(cfg.validate().is_err());
    cfg.framing = Default::default();
    cfg.top = 0.8;
    assert!(cfg.validate().is_err());
    let mut p = project(Path::new("unused"));
    p.ensure_composition();
    let c = p
        .composition
        .as_ref()
        .unwrap()
        .clips
        .iter()
        .find(|c| c.track == timeline::VIDEO)
        .unwrap()
        .clone();
    p.output.layout.sources.insert(
        "source".into(),
        layout::Framing {
            zoom: 1.5,
            ..Default::default()
        },
    );
    assert_eq!(layout::framing(&p, &c).zoom, 1.5);
    p.output.layout.scenes.insert(
        "raw".into(),
        layout::Framing {
            zoom: 2.,
            ..Default::default()
        },
    );
    assert_eq!(layout::framing(&p, &c).zoom, 2.);
    let moved = layout::dragged(&p, &c, 360., 304., 40., 20.);
    assert!(moved.x < 0.5 && moved.y < 0.5);
    let defaults = ProjectDefaults::from_project(&p);
    assert!(defaults.output.layout.sources.is_empty() && defaults.output.layout.scenes.is_empty());
    assert_eq!(defaults.output.layout.framing, p.output.layout.framing);
}

#[test]
fn remembered_video_background_and_subtitles_survive_new_projects_and_refresh() {
    let temp = tempfile::tempdir().unwrap();
    let mut old = project(temp.path());
    old.subtitles.enabled = true;
    old.subtitles.offset_x = 24;
    old.subtitles.offset_y = -150;
    old.ensure_composition();
    let mut expected = std::collections::BTreeMap::new();
    for track in [timeline::VIDEO, timeline::BACKGROUND, timeline::CAPTIONS] {
        let clips: Vec<_> = old.composition.as_ref().unwrap().clips.iter()
            .filter(|c| c.track == track).cloned().collect();
        for c in clips {
            let mut t = c.transform.clone();
            t.x = if track == timeline::BACKGROUND { 0.21 } else { 0.53 };
            t.y = if track == timeline::CAPTIONS { 0.745 } else { 0.485 };
            if track == timeline::BACKGROUND { t.width = 2.36; t.height = 1.01; }
            assert!(old.apply_layer_transform(&c.id, &t));
            old.remember_layer_parameters(&c.id);
            expected.insert((c.scene_id.clone(), track), t);
        }
    }
    let defaults = temp.path().join("defaults.json");
    write_json(&defaults, &ProjectDefaults::from_project(&old)).unwrap();
    let mut next = Project::default();
    ProjectDefaults::load(&defaults).unwrap().apply(&mut next);
    next.sources = old.sources.clone();
    next.segments = old.segments.clone();
    for s in &mut next.segments { s.id = format!("next-{}", s.id); }
    next.ensure_composition();
    let check = |p: &Project| {
        for ((scene, track), t) in &expected {
            let c = p.composition.as_ref().unwrap().clips.iter()
                .find(|c| c.scene_id == format!("next-{scene}") && c.track == *track).unwrap();
            assert_eq!(&c.transform, t, "inherited {scene}, track {track}");
        }
        assert_eq!((p.subtitles.offset_x, p.subtitles.offset_y), (24, -150));
        assert_eq!(layout::caption_box(p, &p.segments[1]), Some(expected[&("review".into(), timeline::VIDEO)].clone()));
    };
    check(&next);
    // Reopening and rebuilding after generated assets must keep the placement.
    next = serde_json::from_slice(&serde_json::to_vec(&next).unwrap()).unwrap();
    next.segments[1].duration_ms += 250;
    next.ensure_composition();
    check(&next);
    next.output.layout.brightness = 0.8;
    layout::refresh(&mut next, false);
    check(&next);
    // A deliberate viewport edit wins over old portable video/background geometry.
    next.output.layout.top = 0.03;
    next.output.layout.height = 0.57;
    layout::refresh(&mut next, true);
    let video = next.composition.as_ref().unwrap().clips.iter().find(|c| c.track == timeline::VIDEO).unwrap();
    assert_eq!(video.transform, next.output.layout.viewport());
    let mut later = project(temp.path());
    ProjectDefaults::from_project(&next).apply(&mut later);
    later.ensure_composition();
    assert_eq!(later.composition.as_ref().unwrap().clips.iter().find(|c| c.track == timeline::VIDEO).unwrap().transform, next.output.layout.viewport());
    assert_eq!(later.composition.as_ref().unwrap().clips.iter().find(|c| c.track == timeline::CAPTIONS && c.scene_id == "review").unwrap().transform, expected[&("review".into(), timeline::CAPTIONS)]);
}

#[test]
fn background_refresh_preserves_scene_edits_and_locked_geometry() {
    let mut p = project(Path::new("unused"));
    p.ensure_composition();
    let bg = p.composition.as_mut().unwrap().clips.iter_mut()
        .find(|c| c.track == timeline::BACKGROUND).unwrap();
    let id = bg.id.clone();
    bg.transform.x = 0.23;
    let transform = bg.transform.clone();
    p.output.layout.background_x = 0.2;
    layout::refresh(&mut p, false);
    assert_eq!(p.composition.as_ref().unwrap().clips.iter().find(|c| c.id == id).unwrap().transform, transform);
    p.composition.as_mut().unwrap().clips.iter_mut().find(|c| c.id == id).unwrap().locked = true;
    p.output.layout.top = 0.02;
    layout::refresh(&mut p, true);
    assert_eq!(p.composition.as_ref().unwrap().clips.iter().find(|c| c.id == id).unwrap().transform, transform);
}

#[test]
#[ignore = "uses library copies listed in AIR3VIEW_LAYOUT_QA, renders real source samples"]
fn library_layout_preferences_generate_preview_and_export() {
    let qa = std::path::PathBuf::from(std::env::var("AIR3VIEW_LAYOUT_QA").expect("AIR3VIEW_LAYOUT_QA"));
    let paths: Vec<std::path::PathBuf> = serde_json::from_slice(&std::fs::read(qa.join("projects.json")).unwrap()).unwrap();
    let mut reports = vec![];
    for (index, path) in paths.iter().enumerate() {
        let bytes = std::fs::read(path).unwrap();
        let mut old: Project = serde_json::from_slice(&bytes).unwrap();
        old.root = path.parent().unwrap().to_owned();
        let mut next = Project::default();
        let defaults = ProjectDefaults::from_project(&old);
        // Exercise the same serialized settings used by a restarted app/batch.
        let defaults: ProjectDefaults = serde_json::from_slice(&serde_json::to_vec(&defaults).unwrap()).unwrap();
        defaults.apply(&mut next);
        next.sources = old.sources.clone();
        next.segments = old.segments.clone();
        for seg in &mut next.segments { seg.id = format!("new-{}", seg.id); }
        next.ensure_composition();
        let mut counts = [0; 3];
        for clip in &next.composition.as_ref().unwrap().clips {
            if let Some(slot) = [timeline::VIDEO, timeline::BACKGROUND, timeline::CAPTIONS].iter().position(|t| *t == clip.track) {
                let key = crate::layer_defaults::LayerDefaults::key(&next, clip).unwrap();
                if let Some(pref) = next.layer_defaults.presets.get(&key) {
                    assert_eq!(clip.transform, pref.transform, "{}: {key}", old.title);
                    counts[slot] += 1;
                }
            }
        }
        assert!(counts.iter().all(|n| *n > 0));
        next.root = qa.join(format!("project-{index}"));
        std::fs::create_dir_all(&next.root).unwrap();
        let mut seg = next.segments.iter().find(|s| s.kind == "review" && s.audio.as_ref().is_some_and(|a| a.is_file())).unwrap().clone();
        seg.duration_ms = 2000;
        seg.footage_spans.clear();
        seg.out_ms = seg.in_ms + (2000. * seg.source_rate).ceil() as u64;
        seg.words = crate::subtitle::estimated_words(&seg.narration, 2000);
        seg.evidence_items.clear();
        seg.graphics.clear();
        seg.locked = false;
        seg.avatar = None;
        next.segments = vec![seg];
        next.rules.min_seconds = 2;
        next.rules.max_seconds = 2;
        next.rules.hook_enabled = false;
        next.rules.intercuts_enabled = false;
        next.evidence_settings.enabled = false;
        next.music.enabled = false;
        next.composition = None;
        next.ensure_composition();
        next.save().unwrap();
        let settings = Settings { preview_long_edge: 720, ..Default::default() };
        for c in next.composition.as_ref().unwrap().clips.iter().filter(|c| matches!(c.track, timeline::VIDEO | timeline::BACKGROUND | timeline::CAPTIONS)) {
            crate::player::decode_test_frame(&next, &settings, c, 1000).unwrap()
                .save(next.root.join(format!("preview-layer-{}.png", c.track))).unwrap();
        }
        let output = next.root.join("final.mp4");
        media::render(&next, &settings, &output, &process::cancel_token(), |_, _| {}).unwrap();
        let probe = media::probe(&settings, &output, &process::cancel_token()).unwrap();
        assert!(probe.has_audio && probe.duration_ms.abs_diff(2000) < 100);
        assert_eq!((probe.width, probe.height), next.output.dimensions());
        frame(&settings, &output, &next.root.join("export.png"));
        assert_eq!(std::fs::read(path).unwrap(), bytes, "QA cannot modify the library");
        reports.push(serde_json::json!({"title":old.title,"inherited_layers":counts,"width":probe.width,"height":probe.height,"duration_ms":probe.duration_ms,"preview_export":true}));
    }
    write_json(&qa.join("library-verification.json"), &reports).unwrap();
}
#[test]
fn four_edge_crop_scopes_preserve_locks_and_roundtrip_without_touching_clocks() {
    let temp=tempfile::tempdir().unwrap();let mut p=project(temp.path());
    p.output.layout.enabled=false; // Old inactive settings cannot change the picture.
    p.output.layout.framing.trim=[0.1,0.2,0.1,0.2];
    p.ensure_composition();
    let video=|p:&Project,id:&str|p.composition.as_ref().unwrap().clips.iter().find(|c|c.track==timeline::VIDEO&&c.scene_id==id).unwrap().clone();
    let raw=video(&p,"raw");let review=video(&p,"review");
    assert!(!layout::rasterized(&p,&raw));
    let segments=serde_json::to_value(&p.segments).unwrap();
    let f=layout::Framing{trim:[0.08,0.15,0.12,0.1],..Default::default()};
    assert!(layout::set_framing(&mut p,&raw,2,f.clone()));p.ensure_composition();
    assert!(layout::cropped_footage(&p,&raw));
    assert!(!layout::cropped_footage(&p,&review),"an inactive old global crop must not leak to another scene");
    assert_eq!(layout::framing(&p,&raw),&f);
    // Same source scope, with a locked scene retaining its previous visible crop.
    p.segments[0].locked=true;
    let next=layout::Framing{trim:[0.1,0.12,0.2,0.08],..Default::default()};
    assert!(layout::set_framing(&mut p,&review,1,next.clone()));
    assert_eq!(layout::framing(&p,&raw),&f);assert_eq!(layout::framing(&p,&review),&next);
    assert!(!layout::set_framing(&mut p,&raw,2,Default::default()));
    let global=layout::Framing{trim:[0.04,0.06,0.05,0.07],..Default::default()};
    assert!(layout::set_framing(&mut p,&review,0,global.clone()));
    assert_eq!(p.output.layout.framing,global);assert_eq!(layout::framing(&p,&review),&next);
    p.save().unwrap();let stored=Project::load(&p.root.join("project.json")).unwrap();
    assert!(stored.output.layout.crop_unframed);assert_eq!(stored.output.layout,p.output.layout);
    let defaults=ProjectDefaults::from_project(&stored);assert!(defaults.output.layout.crop_unframed);
    assert_eq!(defaults.output.layout.framing,global);assert!(defaults.output.layout.scenes.is_empty());
    p.segments[0].locked=false;
    assert_eq!(serde_json::to_value(&p.segments).unwrap(),segments,"crop cannot edit voice, source timing or duration");
}

#[test]
fn four_edge_crop_matches_preview_export_and_keeps_audio_with_and_without_layout() {
    let temp=tempfile::tempdir().unwrap();let mut p=project(temp.path());let mut s=Settings::default();
    let picture=temp.path().join("borders.png");
    image::RgbImage::from_fn(384,216,|x,y|{
        if !(32..352).contains(&x)||!(32..184).contains(&y) {image::Rgb([240,0,0])}
        else if (x as i32-192).pow(2)+(y as i32-108).pow(2)<24*24 {image::Rgb([240,240,240])}
        else if x<192 {image::Rgb([0,180,0])}else{image::Rgb([0,0,180])}
    }).save(&picture).unwrap();
    run(&s,&["-y","-v","error","-loop","1","-i",picture.to_str().unwrap(),"-f","lavfi","-i","sine=frequency=700:sample_rate=48000:duration=2","-t","2","-r","24","-c:v","libx264","-preset","ultrafast","-pix_fmt","yuv420p","-c:a","aac","-shortest",p.sources[0].path.to_str().unwrap()]);
    p.sources[0].width=384;p.sources[0].height=216;p.sources[0].duration_ms=2000;
    p.segments.truncate(1);p.segments[0].duration_ms=2000;p.segments[0].out_ms=2000;
    p.output.portrait=false;p.output.resolution=360;p.output.blur_background=false;
    p.rules.min_seconds=2;p.rules.max_seconds=2;s.preview_long_edge=640;
    let cancel=process::cancel_token();
    for framed in [false,true] {
        p.output.layout=layout::Settings{enabled:framed,top:0.1,left:0.2,width:0.6,height:0.6,..Default::default()};
        p.composition=None;p.ensure_composition();
        let c=p.composition.as_ref().unwrap().clips.iter().find(|c|c.track==timeline::VIDEO).unwrap().clone();
        let uncut=temp.path().join(format!("uncut-{framed}.mp4"));
        media::render(&p,&s,&uncut,&cancel,|_,_|{}).unwrap();
        let f=layout::Framing{trim:[0.11,0.17,0.12,0.18],..Default::default()};
        assert!(layout::set_framing(&mut p,&c,2,f));p.ensure_composition();
        let out=temp.path().join(format!("cropped-{framed}.mp4"));media::render(&p,&s,&out,&cancel,|_,_|{}).unwrap();
        let info=media::probe(&s,&out,&cancel).unwrap();assert_eq!((info.width,info.height),(640,360));assert!(info.has_audio&&info.duration_ms.abs_diff(2000)<80);
        let png=temp.path().join(format!("crop-{framed}.png"));frame(&s,&out,&png);let img=image::open(png).unwrap().to_rgb8();
        let c=p.composition.as_ref().unwrap().clips.iter().find(|c|c.track==timeline::VIDEO).unwrap();
        let preview=crate::player::decode_test_frame(&p,&s,c,1000).unwrap();
        let left=((c.transform.x-c.transform.width/2.)*640.).round() as u32;
        let top=((c.transform.y-c.transform.height/2.)*360.).round() as u32;
        let mut error=0u64;let mut whites=vec![];let mut red=0;
        for (x,y,px) in preview.enumerate_pixels(){
            let actual=img.get_pixel(left+x,top+y);
            for i in 0..3{error+=px[i].abs_diff(actual[i]) as u64;}
            if actual[0]>160&&actual[1]<50&&actual[2]<50{red+=1;}
            if actual.0.iter().all(|v|*v>200){whites.push((x,y));}
        }
        assert_eq!(red,0,"all four red borders must be cropped");
        assert!((error as f64/(preview.width()*preview.height()*3) as f64)<6.);
        let extent=|axis:usize|{let values:Vec<_>=whites.iter().map(|&(x,y)|if axis==0{x}else{y}).collect();values.iter().max().unwrap()-values.iter().min().unwrap()};
        assert!(extent(0).abs_diff(extent(1))<5,"crop must preserve a circle's proportions");
        let pcm=|input:&Path,name:&str|{let path=temp.path().join(name);run(&s,&["-y","-v","error","-i",input.to_str().unwrap(),"-vn","-ac","1","-ar","24000","-f","s16le",path.to_str().unwrap()]);std::fs::read(path).unwrap()};
        assert_eq!(pcm(&uncut,"uncut.pcm"),pcm(&out,"cropped.pcm"),"crop must not change the audio samples");
        let mut legacy=layout::Settings::default();legacy.framing.trim=[0.1;4];p.output.layout=legacy;p.composition=None;p.ensure_composition();
        let c=p.composition.as_ref().unwrap().clips.iter().find(|c|c.track==timeline::VIDEO).unwrap();assert!(!layout::rasterized(&p,c));
    }
}
#[test]
fn refresh_preserves_manual_timing_audio_locks_and_host() {
    let mut p = project(Path::new("unused"));
    p.output.layout.enabled = false;
    p.segments.push(Segment {
        id: "host".into(),
        kind: "host".into(),
        source_id: "source".into(),
        duration_ms: 1000,
        audio: Some("voice.wav".into()),
        avatar: Some("host.png".into()),
        ..Default::default()
    });
    p.ensure_composition();
    let old = p.composition.as_ref().unwrap().clone();
    let host = old
        .clips
        .iter()
        .find(|c| c.track == timeline::AVATAR && c.scene_id == "host")
        .unwrap()
        .clone();
    let v = p
        .composition
        .as_mut()
        .unwrap()
        .clips
        .iter_mut()
        .find(|c| c.track == timeline::VIDEO)
        .unwrap();
    v.linked = false;
    v.timing_manual = true;
    v.start_ms = 200;
    v.duration_ms = 2000;
    v.media_in_ms = 500;
    let saved = v.clone();
    p.output.layout.enabled = true;
    layout::refresh(&mut p, true);
    p.ensure_composition();
    let c = p.composition.as_ref().unwrap();
    let v = c.clips.iter().find(|c| c.id == saved.id).unwrap();
    assert_eq!((v.start_ms, v.duration_ms, v.media_in_ms), (200, 2000, 500));
    assert_eq!(v.transform, p.output.layout.viewport());
    let bg = c
        .clips
        .iter()
        .find(|c| c.id == format!("{}:layout-background", saved.id))
        .unwrap();
    assert_eq!((bg.start_ms, bg.duration_ms), (200, 2000));
    assert_eq!(
        serde_json::to_vec(c.clips.iter().find(|c| c.id == host.id).unwrap()).unwrap(),
        serde_json::to_vec(&host).unwrap()
    );
    for audio in old.clips.iter().filter(|c| c.audio()) {
        assert_eq!(
            serde_json::to_vec(c.clips.iter().find(|c| c.id == audio.id).unwrap()).unwrap(),
            serde_json::to_vec(audio).unwrap()
        );
    }
    let comp = p.composition.as_mut().unwrap();
    let v = comp.clips.iter_mut().find(|c| c.id == saved.id).unwrap();
    v.locked = true;
    v.transform.x = 0.3;
    p.output.layout.top = 0.05;
    layout::refresh(&mut p, true);
    assert_eq!(
        p.composition
            .as_ref()
            .unwrap()
            .clips
            .iter()
            .find(|c| c.id == saved.id)
            .unwrap()
            .transform
            .x,
        0.3
    );
    p.output.layout.enabled = false;
    layout::refresh(&mut p, true);
    p.ensure_composition();
    assert!(!p
        .composition
        .as_ref()
        .unwrap()
        .clips
        .iter()
        .any(|c| c.id.contains(":layout-")));
}
#[test]
fn portable_background_survives_deleted_original_and_missing_file_is_clear() {
    let (temp, mut p, s) = fixture();
    let preset = temp.path().join("settings.air3preset");
    crate::settings_package::export(&p, &s, &preset, true, &|_| {}).unwrap();
    std::fs::remove_file(&p.output.layout.background).unwrap();
    assert!(layout::check_file(&p)
        .unwrap_err()
        .to_string()
        .contains("ảnh nền"));
    let loaded = crate::settings_package::load(&preset, &|_| {}).unwrap();
    let cfg = loaded
        .resolve(
            &temp.path().join("other-machine"),
            &Default::default(),
            &|_| {},
        )
        .unwrap();
    cfg.apply(&mut p);
    layout::check_file(&p).unwrap();
    assert!(p.output.layout.background.is_file());
    layout::pin(&temp.path().join("shared"), &mut p.output.layout).unwrap();
    assert!(crate::project_store::uses_directory(
        &p,
        &temp.path().join("shared")
    ));
    p.output.layout.background.clear();
    layout::check_file(&p).unwrap();
}
#[test]
fn rendered_viewport_crops_hud_preserves_aspect_and_matches_preview() {
    let (temp, mut p, mut s) = fixture();
    p.ensure_composition();
    s.preview_long_edge = 640;
    let output = temp.path().join("output");
    std::fs::create_dir(&output).unwrap();
    let final_mp4 = output.join("final.mp4");
    media::render(&p, &s, &final_mp4, &process::cancel_token(), |_, _| {}).unwrap();
    assert_eq!(std::fs::read_dir(output).unwrap().count(), 1);
    let info = media::probe(&s, &final_mp4, &process::cancel_token()).unwrap();
    assert_eq!((info.width, info.height), (360, 640));
    assert!(info.has_audio && info.duration_ms.abs_diff(6000) < 100);
    let png = temp.path().join("frame.png");
    frame(&s, &final_mp4, &png);
    let img = image::open(png).unwrap().to_rgb8();
    assert!(img.get_pixel(180, 30).0.iter().all(|c| *c < 8));
    let red = img
        .enumerate_pixels()
        .filter(|(_, y, c)| *y >= 65 && *y < 365 && c[0] > 150 && c[1] < 60 && c[2] < 60)
        .count();
    assert_eq!(red, 0, "camera HUD must be removed");
    assert!(img.get_pixel(50, 500)[0] > 200 && img.get_pixel(300, 500)[1] > 140);
    for c in p.composition.as_ref().unwrap().clips.iter().filter(|c| {
        c.scene_id == "raw" && (c.track == timeline::VIDEO || layout::background(&p, c))
    }) {
        let preview = crate::player::decode_test_frame(&p, &s, c, 1000)
            .unwrap()
            .to_owned();
        let x = ((c.transform.x - c.transform.width / 2.) * 360.).round() as u32;
        let y = ((c.transform.y - c.transform.height / 2.) * 640.).round() as u32;
        let mut error = 0u64;
        for (px, py, v) in preview.enumerate_pixels() {
            let actual = img.get_pixel(x + px, y + py);
            for i in 0..3 {
                error += v[i].abs_diff(actual[i]) as u64;
            }
        }
        assert!(
            (error as f64 / (preview.width() * preview.height() * 3) as f64) < 6.,
            "preview / export mismatch: {error}"
        );
    }
}
#[test]
fn labels_subtitles_and_speed_keep_independent_clocks() {
    let (temp, mut p, _s) = fixture();
    p.subtitles.enabled = true;
    p.subtitles.size = 28.;
    p.output.layout.title_enabled = true;
    p.output.layout.title = "BOUNDARY TITLE".into();
    p.output.layout.part = "PART 1".into();
    p.output.footage_speed.mode = "fixed".into();
    p.output.footage_speed.preferred = 1.5;
    p.segments[1].audio = Some(p.sources[0].path.clone());
    p.segments[1].in_ms = 0;
    p.segments[1].out_ms = 4500;
    crate::footage_speed::fit(&mut p).unwrap();
    p.ensure_composition();
    let c = p.composition.as_ref().unwrap();
    assert!(c.clips.iter().any(|c| c.scene_id == "review"
        && c.track == timeline::VIDEO
        && (c.playback_rate - 1.5).abs() < 0.001));
    assert!(c.clips.iter().any(|c| c.scene_id == "review"
        && c.track == timeline::VOICE
        && c.playback_rate == 1.
        && c.duration_ms == 3000));
    assert!(c.clips.iter().filter(|c| layout::overlay(c)).count() >= 2);
    crate::subtitle::write(&p, temp.path()).unwrap();
    let ass = std::fs::read_to_string(temp.path().join("captions.ass")).unwrap();
    assert!(ass.contains("\\clip(0,64,360,368)"));
    layout::write_overlay(&p, &p.segments[0], &temp.path().join("title.ass")).unwrap();
    let title = std::fs::read_to_string(temp.path().join("title.ass")).unwrap();
    assert!(title.contains("BOUNDARY TITLE") && title.contains("PART 1"));
    p.output.layout.title.clear();p.title="FIRST TITLE".into();p.ensure_composition();
    let overlay=p.composition.as_ref().unwrap().clips.iter().find(|c|layout::overlay(c)).unwrap().clone();
    let first=crate::layer_media::ass_file(&p,&overlay).unwrap();p.title="RENAMED TITLE".into();
    let renamed=crate::layer_media::ass_file(&p,&overlay).unwrap();assert_ne!(first,renamed);assert!(std::fs::read_to_string(renamed).unwrap().contains("RENAMED TITLE"));
    // Even a tiny moved frame and oversized captions must never panic.
    let v = p
        .composition
        .as_mut()
        .unwrap()
        .clips
        .iter_mut()
        .find(|c| c.track == timeline::VIDEO && c.scene_id == "review")
        .unwrap();
    v.transform.height = 0.02;
    crate::subtitle::write(&p, temp.path()).unwrap();
}
#[test]
#[ignore = "requires the explicitly downloaded test URL in AIR3VIEW_LAYOUT_SOURCE"]
fn real_source_layout_export() {
    let root = std::env::var_os("AIR3VIEW_LAYOUT_QA")
        .map(std::path::PathBuf::from)
        .unwrap();
    std::fs::create_dir_all(&root).unwrap();
    let source = std::env::var_os("AIR3VIEW_LAYOUT_SOURCE")
        .map(std::path::PathBuf::from)
        .unwrap();
    let s = Settings::default();
    let mut p = project(&root);
    p.sources =
        vec![media::import_local(&s, &source, "source".into(), &process::cancel_token()).unwrap()];
    image::RgbImage::from_fn(720, 500, |x, y| {
        image::Rgb([
            20 + (x / 80 % 2 * 12) as u8,
            38 + (y / 80 % 2 * 12) as u8,
            65,
        ])
    })
    .save(&p.output.layout.background)
    .unwrap();
    p.output.resolution = 720;
    p.output.fps = 30;
    p.output.layout.framing.trim[1] = 0.14;
    p.output.layout.height = 0.57;
    p.output.layout.top = 0.03;
    p.output.layout.title_enabled = true;
    p.output.layout.title = "LAYOUT TEST - ORIGINAL FOOTAGE".into();
    p.output.layout.part = "TEST".into();
    p.segments = [8_000, 50_000, 150_000]
        .into_iter()
        .enumerate()
        .map(|(i, at)| Segment {
            id: format!("raw{i}"),
            kind: "source".into(),
            source_id: "source".into(),
            in_ms: at,
            out_ms: at + 4000,
            duration_ms: 4000,
            caption: "Layout verification".into(),
            ..Default::default()
        })
        .collect();
    p.rules.min_seconds = 12;
    p.rules.max_seconds = 12;
    p.subtitles.enabled = true;
    p.subtitles.size = 32.;
    p.subtitles.bottom_margin = 22;
    p.ensure_composition();
    p.save().unwrap();
    let output = root.join("export");
    std::fs::create_dir_all(&output).unwrap();
    media::render(
        &p,
        &s,
        &output.join("final.mp4"),
        &process::cancel_token(),
        |_, _| {},
    )
    .unwrap();
    run(
        &s,
        &[
            "-v",
            "error",
            "-i",
            output.join("final.mp4").to_str().unwrap(),
            "-f",
            "null",
            "-",
        ],
    );
    frame(&s, &output.join("final.mp4"), &root.join("frame.png"));
    assert_eq!(std::fs::read_dir(&output).unwrap().count(), 1);
    assert_eq!(
        Composition::key(&p),
        p.composition.as_ref().unwrap().script_key
    );
}
