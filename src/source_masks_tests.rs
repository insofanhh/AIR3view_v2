use crate::{composition::Composition, media, model::*, process, source_masks as masks, timeline};
use std::{path::Path, time::Duration};

#[test]
fn turning_off_evidence_masks_or_deleting_last_region_restores_original_card() {
    let temp=tempfile::tempdir().unwrap();let (mut p,s)=fixture(temp.path());p.evidence_settings.enabled=true;
    p.segments[0].evidence_items.push(crate::evidence::EvidenceItem{source_id:"src".into(),source_time_ms:1500,..Default::default()});
    let prepare=|p:&mut Project|{crate::evidence::prepare(p,&s,&process::cancel_token(),|_|{}).unwrap();let path=p.root.join(p.segments[0].evidence_items[0].asset.as_ref().unwrap());image::open(path).unwrap().to_rgba8()};
    let original=prepare(&mut p);p.source_masks.enabled=true;
    p.source_masks.regions.push(masks::Region{source_id:"src".into(),rect:masks::Rect{x:0.15,y:0.76,w:0.72,h:0.12},start_ms:1000,end_ms:3000,..Default::default()});
    assert!(masks::evidence_pending(&p));let masked=prepare(&mut p);assert_ne!(original,masked);assert!(!masks::evidence_pending(&p));
    p.source_masks.evidence=false;assert!(masks::evidence_pending(&p));assert_eq!(prepare(&mut p),original);
    p.source_masks.evidence=true;assert_eq!(prepare(&mut p),masked);
    p.source_masks.regions.clear();assert!(masks::evidence_pending(&p));assert_eq!(prepare(&mut p),original);
    assert!(ProjectDefaults::from_project(&p).source_masks.evidence_revision.is_empty());
}

#[test]
fn host_context_rebinds_masked_still_after_edit_and_reload() {
    let temp=tempfile::tempdir().unwrap();let (mut p,s)=fixture(temp.path());
    p.script_presentation=crate::footage_first::VERSION.into();p.segments[0].kind="host".into();p.segments[0].in_ms=1500;
    p.composition=None;p.ensure_composition();crate::footage_first::prepare_contexts(&p,&s,&process::cancel_token()).unwrap();
    let old=crate::footage_first::context_path(&p,&p.segments[0]).unwrap();let plain=image::open(&old).unwrap().to_rgba8();
    p.source_masks.enabled=true;p.source_masks.regions.push(masks::Region{source_id:"src".into(),rect:masks::Rect{x:0.15,y:0.76,w:0.72,h:0.12},start_ms:1000,end_ms:3000,..Default::default()});
    let new=crate::footage_first::context_path(&p,&p.segments[0]).unwrap();assert_ne!(old,new);p.ensure_composition();
    assert!(p.composition.as_ref().unwrap().clips.iter().any(|c|c.track==timeline::BACKGROUND&&c.media.as_ref()==Some(&new)));
    crate::footage_first::prepare_contexts(&p,&s,&process::cancel_token()).unwrap();assert_ne!(image::open(&new).unwrap().to_rgba8(),plain);
    p.save().unwrap();let loaded=Project::load(&p.root.join("project.json")).unwrap();assert_eq!(loaded.source_masks,p.source_masks);
}

fn run(s: &Settings, args: Vec<String>) {
    process::run(
        &s.ffmpeg,
        &args,
        None,
        &process::cancel_token(),
        Duration::from_secs(60),
        None,
    )
    .unwrap();
}
fn native_frame(s: &Settings, file: &Path, time: u64) -> image::RgbaImage {
    let out = process::run(
        &s.ffmpeg,
        &[
            "-v".into(),
            "error".into(),
            "-ss".into(),
            format!("{:.3}", time as f64 / 1000.),
            "-i".into(),
            file.to_string_lossy().into_owned(),
            "-frames:v".into(),
            "1".into(),
            "-f".into(),
            "image2pipe".into(),
            "-vcodec".into(),
            "png".into(),
            "pipe:1".into(),
        ],
        None,
        &process::cancel_token(),
        Duration::from_secs(30),
        None,
    )
    .unwrap();
    image::load_from_memory(&out.stdout).unwrap().to_rgba8()
}
fn fixture(root: &Path) -> (Project, Settings) {
    let mut p = Project {
        root: root.into(),
        ..Default::default()
    };
    let s = Settings {
        preview_long_edge: 640,
        ..Default::default()
    };
    for n in 0..8 {
        let mut img = image::RgbImage::from_fn(320, 180, |x, y| {
            image::Rgb([
                20 + (x / 40) as u8 * 3,
                35 + (n * 17) as u8 + (y / 40) as u8 * 3,
                90,
            ])
        });
        // Independent glyph-like shapes, grouped as two changing subtitle lines.
        if (2..6).contains(&n) {
            for letter in 0..12 {
                for y in 0..9 {
                    for x in 0..5 {
                        if x == 0 || y == 0 || y == 4 || y == 8 || ((letter + n) % 3 == 0 && x == 4)
                        {
                            img.put_pixel(
                                55 + letter * 17 + x,
                                142 + y,
                                image::Rgb([255, 255, 255]),
                            );
                        }
                    }
                }
            }
        }
        for y in 10..34 {
            for x in 278..302 {
                if (x / 3 + y / 3) % 2 == 0 {
                    img.put_pixel(x, y, image::Rgb([255, 240, 10]));
                }
            }
        }
        img.save(root.join(format!("frame{n:02}.png"))).unwrap();
    }
    let input = root.join("source.mp4");
    run(
        &s,
        vec![
            "-y".into(),
            "-v".into(),
            "error".into(),
            "-framerate".into(),
            "2".into(),
            "-i".into(),
            root.join("frame%02d.png").to_string_lossy().into_owned(),
            "-f".into(),
            "lavfi".into(),
            "-i".into(),
            "sine=frequency=550:sample_rate=48000:duration=4".into(),
            "-c:v".into(),
            "libx264".into(),
            "-preset".into(),
            "ultrafast".into(),
            "-r".into(),
            "24".into(),
            "-pix_fmt".into(),
            "yuv420p".into(),
            "-c:a".into(),
            "aac".into(),
            "-shortest".into(),
            input.to_string_lossy().into_owned(),
        ],
    );
    p.sources = vec![Source {
        id: "src".into(),
        path: input,
        width: 320,
        height: 180,
        duration_ms: 4000,
        has_audio: true,
        ..Default::default()
    }];
    p.segments = vec![Segment {
        id: "raw".into(),
        kind: "source".into(),
        source_id: "src".into(),
        out_ms: 4000,
        duration_ms: 4000,
        ..Default::default()
    }];
    p.output.portrait = false;
    p.output.resolution = 360;
    p.output.fps = 24;
    p.evidence_settings.enabled = false;
    p.subtitles.enabled = false;
    p.rules.min_seconds = 4;
    p.rules.max_seconds = 4;
    p.rules.hook_enabled = false;
    p.rules.intercuts_enabled = false;
    p.ensure_composition();
    (p, s)
}

#[test]
fn source_mask_defaults_never_copy_source_regions_and_old_projects_are_unchanged() {
    let old: Project = serde_json::from_str("{}").unwrap();
    assert!(!old.source_masks.enabled);
    let mut p = old;
    p.source_masks.enabled = true;
    p.source_masks.regions.push(masks::Region {
        source_id: "old-source".into(),
        ..Default::default()
    });
    p.source_masks.ignored.push(masks::Ignored {
        source_id: "old-source".into(),
        rect: Default::default(),
    });
    let json = serde_json::to_string(&ProjectDefaults::from_project(&p)).unwrap();
    assert!(!json.contains("old-source"));
    let defaults: ProjectDefaults = serde_json::from_str(&json).unwrap();
    let mut next = Project::default();
    defaults.apply(&mut next);
    assert!(next.source_masks.enabled);
    assert!(next.source_masks.regions.is_empty() && next.source_masks.ignored.is_empty());
    next.source_masks.regions.push(masks::Region {
        source_id: "current-source".into(),
        ..Default::default()
    });
    defaults.apply(&mut next);
    assert_eq!(next.source_masks.regions[0].source_id, "current-source");
    let mut invalid = masks::Settings::default();
    invalid.regions.push(masks::Region {
        rect: masks::Rect {
            x: f32::NAN,
            ..Default::default()
        },
        ..Default::default()
    });
    assert!(invalid.validate().is_err());
}

#[test]
fn detector_finds_subtitle_shapes_caches_cancels_and_preserves_manual_ignores() {
    let temp = tempfile::tempdir().unwrap();
    let (mut p, s) = fixture(temp.path());
    p.source_masks.enabled = true;
    p.source_masks.logos = false;
    let original = std::fs::read(&p.sources[0].path).unwrap();
    masks::scan(&mut p, &s, &process::cancel_token(), false, |_| {}).unwrap();
    assert!(
        p.source_masks
            .regions
            .iter()
            .any(|r| r.kind == "subtitle" && r.rect.y > 0.7),
        "{}",
        serde_json::to_string(&p.source_masks).unwrap()
    );
    let first = serde_json::to_vec(&p.source_masks.regions).unwrap();
    masks::scan(&mut p, &s, &process::cancel_token(), false, |_| {
        panic!("should reuse cache")
    })
    .unwrap();
    assert_eq!(serde_json::to_vec(&p.source_masks.regions).unwrap(), first);
    // Partial results cannot become a permanent cache hit, even if an old
    // project erroneously marks the same key as completed.
    let receipt=p.root.join("cache/source-masks").join(format!("{}.json",p.source_masks.scan_key));
    let mut partial:serde_json::Value=serde_json::from_slice(&std::fs::read(&receipt).unwrap()).unwrap();
    partial["complete"]=false.into();partial["regions"]=serde_json::json!([]);
    std::fs::write(&receipt,serde_json::to_vec(&partial).unwrap()).unwrap();
    p.source_masks.regions.clear();p.source_masks.scan_complete=true;
    masks::scan(&mut p,&s,&process::cancel_token(),false,|_|{}).unwrap();
    assert!(p.source_masks.scan_complete&&!p.source_masks.regions.is_empty());
    let r = p.source_masks.regions[0].clone();
    p.source_masks.ignored.push(masks::Ignored {
        source_id: "src".into(),
        rect: r.rect,
    });
    p.source_masks.scan_key.clear();
    p.source_masks.regions.push(masks::Region {
        source_id: "src".into(),
        rect: masks::Rect {
            x: 0.1,
            y: 0.1,
            w: 0.1,
            h: 0.1,
        },
        ..Default::default()
    });
    masks::scan(&mut p, &s, &process::cancel_token(), false, |_| {}).unwrap();
    assert!(p.source_masks.regions.iter().any(|r| r.manual));
    assert!(!p
        .source_masks
        .regions
        .iter()
        .any(|v| !v.manual && v.rect.overlap(r.rect) > 0.3));
    let cancel = process::cancel_token();
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(masks::scan(&mut p, &s, &cancel, true, |_| {}).is_err());
    assert_eq!(std::fs::read(&p.sources[0].path).unwrap(), original);
}

#[test]
fn masks_follow_source_clock_seek_speed_crop_flip_and_preserve_audio() {
    let temp = tempfile::tempdir().unwrap();
    let (mut p, s) = fixture(temp.path());
    p.source_masks.enabled = true;
    p.source_masks.auto_scan = false;
    p.source_masks.padding = 0.;
    p.source_masks.strength = 30.;
    p.source_masks.regions = vec![masks::Region {
        source_id: "src".into(),
        rect: masks::Rect {
            x: 0.15,
            y: 0.76,
            w: 0.72,
            h: 0.12,
        },
        start_ms: 1000,
        end_ms: 2800,
        ..Default::default()
    }];
    let plain = temp.path().join("plain.mp4");
    let blurred = temp.path().join("blur.mp4");
    let mut original = p.clone();
    original.source_masks.enabled = false;
    media::render(&original, &s, &plain, &process::cancel_token(), |_, _| {}).unwrap();
    media::render(&p, &s, &blurred, &process::cancel_token(), |_, _| {}).unwrap();
    let at = |file: &Path, time| native_frame(&s, file, time);
    for (time, active) in [(500, false), (1500, true), (3200, false)] {
        let a = at(&plain, time);
        let b = at(&blurred, time);
        let mut inside = 0u64;
        let mut outside = 0u64;
        let mut ni = 0;
        let mut no = 0;
        for (x, y, px) in a.enumerate_pixels() {
            let d = (0..3)
                .map(|i| px[i].abs_diff(b.get_pixel(x, y)[i]) as u64)
                .sum::<u64>();
            if x > 90 && x < 560 && y > 280 && y < 315 {
                inside += d;
                ni += 3;
            } else if y < 250 {
                outside += d;
                no += 3;
            }
        }
        assert!(outside as f64 / (no as f64) < 2.5);
        if active {
            assert!(inside as f64 / ni as f64 > 5.);
        } else {
            assert!(inside as f64 / (ni as f64) < 2.5);
        }
    }
    let pcm = |file: &Path, name: &str| {
        let path = temp.path().join(name);
        run(
            &s,
            vec![
                "-y".into(),
                "-v".into(),
                "error".into(),
                "-i".into(),
                file.to_string_lossy().into_owned(),
                "-vn".into(),
                "-f".into(),
                "s16le".into(),
                path.to_string_lossy().into_owned(),
            ],
        );
        std::fs::read(path).unwrap()
    };
    assert_eq!(pcm(&plain, "plain.pcm"), pcm(&blurred, "blur.pcm"));
    p.segments[0].kind = "review".into();
    p.segments[0].narration = "Original voice stays at one times speed".into();
    p.segments[0].audio = Some(p.sources[0].path.clone());
    p.segments[0].in_ms = 500;
    p.segments[0].out_ms = 3500;
    p.segments[0].duration_ms = 2000;
    p.segments[0].source_rate = 1.5;
    p.source_masks.regions[0].start_ms = 2000;
    p.source_masks.regions[0].end_ms = 2700;
    p.rules.min_seconds = 2;
    p.rules.max_seconds = 2;
    p.composition = None;
    p.ensure_composition();
    let clip = p
        .composition
        .as_ref()
        .unwrap()
        .clips
        .iter()
        .find(|c| c.track == timeline::VIDEO)
        .unwrap()
        .clone();
    let filters = masks::filters_at(&p, &clip, 1000).unwrap();
    assert!(filters.contains("gte(t,0.000000)") && filters.contains("lt(t,0.466667)"));
    for framed in [false, true] {
        p.output.layout.enabled = framed;
        p.output.layout.framing.trim = [0.05, 0.1, 0.04, 0.05];
        p.effects.footage.mirror_x = true;
        p.composition = None;
        p.ensure_composition();
        let c = p
            .composition
            .as_ref()
            .unwrap()
            .clips
            .iter()
            .find(|c| c.track == timeline::VIDEO)
            .unwrap();
        for point in [[0.2, 0.3], [0.8, 0.9]] {
            let q = masks::source_to_layer(&p, c, point, false);
            let back = masks::source_to_layer(&p, c, q, true);
            assert!((back[0] - point[0]).abs() < 0.00001 && (back[1] - point[1]).abs() < 0.00001);
        }
        let out = temp.path().join(format!("speed-{framed}.mp4"));
        media::render(&p, &s, &out, &process::cancel_token(), |_, _| {}).unwrap();
        let preview = crate::player::decode_test_frame(&p, &s, c, 1200).unwrap();
        assert!(preview.width() > 0);
        let a = native_frame(&s, &out, 1200);
        assert_eq!(a.dimensions(), p.output.dimensions());
    }
    // Unrelated source masks cannot invalidate a clip cache.
    let key = masks::clip_identity(&p, &clip);
    p.source_masks.regions.push(masks::Region {
        source_id: "other".into(),
        ..Default::default()
    });
    assert_eq!(key, masks::clip_identity(&p, &clip));
    // A custom video background is not footage from the selected source.
    let mut unrelated = clip.clone();
    unrelated.track = timeline::BACKGROUND;
    unrelated.media = Some("custom-background.mp4".into());
    assert!(masks::filters(&p, &unrelated).unwrap().is_empty());
}

#[test]
fn evidence_masks_only_touch_selected_source_pixels_and_templates_are_portable() {
    let temp = tempfile::tempdir().unwrap();
    let (mut p, _) = fixture(temp.path());
    let mut img = image::open(temp.path().join("frame03.png"))
        .unwrap()
        .to_rgba8();
    let old = img.clone();
    p.source_masks.enabled = true;
    p.source_masks.padding = 0.;
    p.source_masks.regions.push(masks::Region {
        source_id: "src".into(),
        rect: masks::Rect {
            x: 0.1,
            y: 0.7,
            w: 0.8,
            h: 0.2,
        },
        start_ms: 0,
        end_ms: 4000,
        ..Default::default()
    });
    masks::mask_image(&p, "src", 1500, &mut img);
    assert_ne!(img, old);
    assert_eq!(img.get_pixel(10, 10), old.get_pixel(10, 10));
    let template = masks::import_template(
        &temp.path().join("frame03.png"),
        &temp.path().join("templates"),
        0.2,
    )
    .unwrap();
    assert!(template.path.is_file());
    p.source_masks.templates.push(template);
    p.save().unwrap();
    let saved = Project::load(&p.root.join("project.json")).unwrap();
    assert_eq!(saved.source_masks, p.source_masks);
    let defaults = ProjectDefaults::from_project(&p);
    assert!(defaults.source_masks.regions.is_empty());
    assert_eq!(defaults.source_masks.templates.len(), 1);
    let path = temp.path().join("settings.air3preset");
    crate::settings_package::export(&p, &Settings::default(), &path, true, &|_| {}).unwrap();
    let loaded = crate::settings_package::load(&path, &|_| {}).unwrap();
    assert_eq!(loaded.manifest.settings.source_masks.templates.len(), 1);
    assert!(loaded.manifest.settings.source_masks.regions.is_empty());
    let _ = Composition::from_project(&p);
    p.source_masks.regions[0].kind = "subtitle".into();
    let key = masks::identity(&p);
    p.source_masks.subtitles = false;
    assert_ne!(key, masks::identity(&p));
}

#[test]
fn cropped_mirrored_preview_and_export_put_blur_in_the_same_source_region() {
    let temp = tempfile::tempdir().unwrap();
    let (mut p, s) = fixture(temp.path());
    p.output.layout.crop_unframed = true;
    p.output.layout.framing.trim = [0.05, 0.1, 0.04, 0.05];
    p.effects.footage.mirror_x = true;
    p.source_masks.enabled = true;
    p.source_masks.auto_scan = false;
    p.source_masks.padding = 0.;
    let rect = masks::Rect {
        x: 0.15,
        y: 0.76,
        w: 0.72,
        h: 0.12,
    };
    p.source_masks.regions.push(masks::Region {
        source_id: "src".into(),
        rect,
        start_ms: 1000,
        end_ms: 3000,
        ..Default::default()
    });
    p.composition = None;
    p.ensure_composition();
    let c = p
        .composition
        .as_ref()
        .unwrap()
        .clips
        .iter()
        .find(|c| c.track == timeline::VIDEO)
        .unwrap();
    let mut plain = p.clone();
    plain.source_masks.enabled = false;
    let a = crate::player::decode_test_frame(&plain, &s, c, 1500).unwrap();
    let b = crate::player::decode_test_frame(&p, &s, c, 1500).unwrap();
    let q0 = masks::source_to_layer(&p, c, [rect.x, rect.y], false);
    let q1 = masks::source_to_layer(&p, c, [rect.x + rect.w, rect.y + rect.h], false);
    let bounds = [
        q0[0].min(q1[0]),
        q0[1].min(q1[1]),
        q0[0].max(q1[0]),
        q0[1].max(q1[1]),
    ];
    let check = |a: &image::RgbaImage, b: &image::RgbaImage| {
        assert_eq!(a.dimensions(), b.dimensions());
        let (w, h) = a.dimensions();
        let mut changed = 0;
        let mut off = 0;
        for (x, y, px) in a.enumerate_pixels() {
            let d = (0..3)
                .map(|i| px[i].abs_diff(b.get_pixel(x, y)[i]) as u32)
                .sum::<u32>();
            if d > 35 {
                changed += 1;
                let q = [x as f32 / w as f32, y as f32 / h as f32];
                if q[0] < bounds[0] - 0.03
                    || q[0] > bounds[2] + 0.03
                    || q[1] < bounds[1] - 0.03
                    || q[1] > bounds[3] + 0.03
                {
                    off += 1;
                }
            }
        }
        assert!(changed > 300, "No visible blur");
        assert!(
            off * 20 < changed,
            "Blur outside mapped source region: {off}/{changed}"
        );
    };
    check(&a, &b);
    // Isolate the video viewport, so output pixels and preview texture use the same box.
    for proj in [&mut p, &mut plain] {
        let comp = proj.composition.as_mut().unwrap();
        comp.clips
            .retain(|c| c.track == timeline::VIDEO || c.audio());
        for c in &mut comp.clips {
            if c.track == timeline::VIDEO {
                c.transform = Default::default();
            }
        }
    }
    let pa = temp.path().join("mapped-plain.mp4");
    let pb = temp.path().join("mapped-blur.mp4");
    media::render(&plain, &s, &pa, &process::cancel_token(), |_, _| {}).unwrap();
    media::render(&p, &s, &pb, &process::cancel_token(), |_, _| {}).unwrap();
    check(&native_frame(&s, &pa, 1500), &native_frame(&s, &pb, 1500));
}
