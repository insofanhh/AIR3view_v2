use anyhow::{Context, Result};
use reaction_studio::{media, model::*, pipeline, process};
use std::{fs, path::{Path, PathBuf}};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let s = Settings::default();
    let cancel = process::cancel_token();
    match args.get(1).map(String::as_str) {
        Some("metadata-inspect")=>{
            let path=Path::new(args.get(2).context("metadata-inspect <input.mp4>")?);
            println!("{}",serde_json::to_string_pretty(&reaction_studio::export_metadata::inspect(&s,path,&cancel)?)?);
        },
        Some("metadata-apply")=>{
            let p=Project::load(Path::new(args.get(2).context("metadata-apply <project.json> <input.mp4> <new-output.mp4>")?))?;
            let input=Path::new(args.get(3).context("input.mp4")?);let output=Path::new(args.get(4).context("new-output.mp4")?);
            let report=reaction_studio::export_metadata::apply_to_mp4(&p,&s,input,output,&cancel)?;
            println!("{}",serde_json::to_string_pretty(&report)?);
        },
        Some("evidence")=>{
            let p=Project::load(Path::new(args.get(2).context("evidence <project.json>")?))?;
            let (tx,rx)=std::sync::mpsc::channel();let reporter=std::thread::spawn(move||while let Ok(e)=rx.recv(){if let pipeline::Event::Stage{lane,message,..}=e{println!("{lane}: {message}");}});
            let result=pipeline::generate_evidence(p,s,tx,cancel);let _=reporter.join();let p=result?;println!("OK: {} cảnh",p.segments.len());
        },
        Some("repair-script")=>{
            let settings_path=data_dir().join("settings.json");
            let settings=if settings_path.is_file(){serde_json::from_slice(&fs::read(settings_path)?)?}else{s};
            let mut p=Project::load(Path::new(args.get(2).context("repair-script <project.json> <saved response>")?))?;
            let settings=reaction_studio::workflow::effective_settings(&p,&settings);
            let plan=reaction_studio::providers::repair_saved(&p,&settings,&cancel,Path::new(args.get(3).context("saved response")?))?;
            pipeline::apply_plan(&mut p,plan)?;p.save()?;
            println!("OK: repaired {} scenes / {:.2}s",p.segments.len(),p.duration_ms() as f64/1000.);
        },
        Some("prepare")=>{
            let settings_path=data_dir().join("settings.json");
            let settings=if settings_path.is_file(){serde_json::from_slice(&fs::read(settings_path)?)?}else{s};
            let project=Project::load(Path::new(args.get(2).context("prepare <project.json>")?))?;
            let (tx,rx)=std::sync::mpsc::channel();let reporter=std::thread::spawn(move||while let Ok(e)=rx.recv(){if let pipeline::Event::Stage{lane,message,..}=e{println!("{lane}: {message}");}});
            let result=pipeline::prepare(project,settings,tx,cancel);let _=reporter.join();let p=result?;println!("OK: {} cảnh",p.segments.len());
        },
        Some("auto-run")|Some("auto-resume")|Some("speech")|Some("assets")|Some("captions")|Some("script-only")|Some("heygen-preflight")=>{
            let settings_path = data_dir().join("settings.json");
            let settings: Settings = if settings_path.is_file() { serde_json::from_slice(&fs::read(settings_path)?)? } else { s };
            if args[1] == "heygen-preflight" {
                reaction_studio::providers::heygen_preflight(&settings, &cancel)?;
                println!("HeyGen preflight: PASS (auth; wallet > 0 nếu dùng wallet). Không tạo video.");
            } else {
                let project = Project::load(Path::new(args.get(2).context("speech/assets <project.json>")?))?;
                let(tx,rx)=std::sync::mpsc::channel();
                let reporter = std::thread::spawn(move || while let Ok(event) = rx.recv() {
                    if let pipeline::Event::Stage { lane, message, .. } = event { println!("{lane}: {message}"); }
                });
                let settings=reaction_studio::workflow::effective_settings(&project,&settings);
                let result = if args[1]=="auto-run" || args[1]=="auto-resume" {
                    reaction_studio::workflow::run(project,settings,tx,cancel,args[1]=="auto-resume")
                } else if args[1] == "script-only" {
                    let mut project=project;
                    let plan=reaction_studio::providers::analyze(&project,&settings,&cancel)?;
                    project.title=plan.title;project.analysis_summary=plan.analysis_summary;project.segments=plan.segments;
                    project.validate(true)?;project.check_duration()?;project.save()?;
                    drop(tx);
                    Ok(project)
                } else if args[1] == "captions" { pipeline::transcribe(project,settings,worker_path("transcribe.py"),tx,cancel) }
                else if args[1] == "speech" { pipeline::generate_speech(project, settings, tx, cancel) }
                    else { pipeline::generate_assets(project, settings, tx, cancel) };
                let _ = reporter.join();
                let project = result?;
                println!("OK: {} audio; {} avatar; {} ms", project.segments.iter().filter(|seg| seg.audio.is_some()).count(),
                    project.segments.iter().filter(|seg| seg.avatar.is_some()).count(), project.duration_ms());
            }
        },
        Some("doctor")=>println!("{}",reaction_studio::pipeline::diagnostics(&s)),
        Some("download")=>{
            let path=data_dir().join("settings.json");
            let settings:Settings=if path.is_file(){serde_json::from_slice(&fs::read(path)?)?}else{s};
            let url=args.get(2).context("download <YouTube URL> <folder>")?;
            let folder=PathBuf::from(args.get(3).context("download <YouTube URL> <folder>")?);
            fs::create_dir_all(&folder)?;let folder=fs::canonicalize(folder)?;
            let source=media::download(&settings,url,&folder,"src_1".into(),&cancel)?;
            println!("{}",serde_json::json!({"path":source.path,"duration_ms":source.duration_ms,"width":source.width,"height":source.height,"audio":source.has_audio}));
        },
        Some("render")=>{let p=Project::load(Path::new(args.get(2).context("render <project.json> <output.mp4>")?))?;let output=PathBuf::from(args.get(3).context("output.mp4")?);let settings_path=data_dir().join("settings.json");let globals=if settings_path.is_file(){serde_json::from_slice(&fs::read(settings_path)?)?}else{s};let effective=reaction_studio::workflow::effective_settings(&p,&globals);media::render(&p,&effective,&output,&cancel,|i,n|println!("Bước xuất {i}/{n}"))?;println!("OK {}",output.display());},
        _=>println!("AIR3view CLI\n  doctor\n  download <YouTube URL> <folder>\n  prepare <project.json>\n  auto-run / auto-resume <project.json>\n  script-only / speech / assets / captions / evidence <project.json>\n  repair-script <project.json> <saved response>\n  render <project.json> <output.mp4>\n  metadata-inspect <input.mp4>\n  metadata-apply <project.json> <input.mp4> <new-output.mp4>\n  heygen-preflight"),
    }
    Ok(())
}
