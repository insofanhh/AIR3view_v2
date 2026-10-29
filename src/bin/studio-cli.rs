use anyhow::{Context, Result};
use reaction_studio::{media, model::*, pipeline, process};
use std::{fs, path::{Path, PathBuf}};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let s = Settings::default();
    let cancel = process::cancel_token();
    match args.get(1).map(String::as_str) {
        Some("refresh-sfx-library")=>{
            let root=data_dir().join("projects");let mut projects=0;let mut sounds=0;
            for path in reaction_studio::project_store::list(&root) {
                let changed=reaction_studio::evidence_sfx::refresh_saved_project(&path)
                    .with_context(||format!("Không cập nhật được SFX: {}",path.display()))?;
                if changed>0 {projects+=1;sounds+=changed;println!("{}",serde_json::json!({"project":path,"sounds":changed}));}
            }
            println!("{}",serde_json::json!({"updated_projects":projects,"updated_sounds":sounds}));
        },
        Some("batch-create")=>{
            let template=Project::load(Path::new(args.get(2).context("batch-create <template.json> <urls.txt> [output-folder]")?))?;
            let urls=fs::read_to_string(args.get(3).context("urls.txt")?)?;
            let settings=Settings::load(&data_dir().join("settings.json"))?;
            let output=args.get(4).map(PathBuf::from);
            let batch=reaction_studio::batch::create(&data_dir(),&template.title,&urls,&template,&settings,output.is_some(),output)?;
            println!("{}",serde_json::to_string_pretty(&batch)?);
        },
        Some("batch-run")=>reaction_studio::batch::run(&data_dir(),args.get(2).context("batch-run <batch-id>")?,&std::env::current_exe()?)?,
        Some("batch-export")=>{
            let id=args.get(2).context("batch-export <batch-id> <output-folder> [item-id] [--overwrite]")?;
            let folder=Path::new(args.get(3).context("output-folder")?);
            let item=args.get(4).filter(|a|a.as_str()!="--overwrite").map(String::as_str);
            let plan=reaction_studio::batch::preview_export(&data_dir(),id,item,folder)?;
            for reason in &plan.skipped { eprintln!("Bỏ qua: {reason}"); }
            let b=reaction_studio::batch::queue_export(&data_dir(),&plan,args.iter().any(|a|a=="--overwrite"))?;
            println!("{}",serde_json::json!({"batch_id":b.id,"queued":b.run_items.as_ref().map_or(0,Vec::len)}));
            reaction_studio::batch::run(&data_dir(),id,&std::env::current_exe()?)?;
        },
        Some("batch-item")=>reaction_studio::batch::run_item(&data_dir(),args.get(2).context("batch-item <batch-id> <item-id>")?,args.get(3).context("item-id")?)?,
        Some("batch-action")=>{
            let action:reaction_studio::batch::Action=serde_json::from_str(args.get(3).context("batch-action <batch-id> <action-json>")?)?;
            reaction_studio::batch::action(&data_dir(),args.get(2).context("batch-id")?,action)?;
        },
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
            let path=data_dir().join("settings.json");let settings=if path.is_file(){Settings::load(&path)?}else{s};
            let mut p=Project::load(Path::new(args.get(2).context("evidence <project.json> [--recheck]")?))?;
            if args.iter().any(|a|a=="--recheck"){reaction_studio::evidence_selection::invalidate(&mut p,None);}
            let settings=reaction_studio::workflow::effective_settings(&p,&settings);
            let (tx,rx)=std::sync::mpsc::channel();let reporter=std::thread::spawn(move||while let Ok(e)=rx.recv(){if let pipeline::Event::Stage{lane,message,..}=e{println!("{lane}: {message}");}});
            let result=pipeline::generate_evidence(p,settings,tx,cancel);let _=reporter.join();let p=result?;println!("OK: {} cảnh",p.segments.len());
        },
        Some("repair-script")=>{
            let settings_path=data_dir().join("settings.json");
            let settings=if settings_path.is_file(){Settings::load(&settings_path)?}else{s};
            let mut p=Project::load(Path::new(args.get(2).context("repair-script <project.json> <saved response>")?))?;
            let settings=reaction_studio::workflow::effective_settings(&p,&settings);
            let plan=reaction_studio::providers::repair_saved(&p,&settings,&cancel,Path::new(args.get(3).context("saved response")?))?;
            pipeline::apply_plan(&mut p,plan)?;p.save()?;
            println!("OK: repaired {} scenes / {:.2}s",p.segments.len(),p.duration_ms() as f64/1000.);
        },
        Some("prepare")=>{
            let settings_path=data_dir().join("settings.json");
            let settings=if settings_path.is_file(){Settings::load(&settings_path)?}else{s};
            let project=Project::load(Path::new(args.get(2).context("prepare <project.json>")?))?;
            let (tx,rx)=std::sync::mpsc::channel();let reporter=std::thread::spawn(move||while let Ok(e)=rx.recv(){if let pipeline::Event::Stage{lane,message,..}=e{println!("{lane}: {message}");}});
            let result=pipeline::prepare(project,settings,tx,cancel);let _=reporter.join();let p=result?;println!("OK: {} cảnh",p.segments.len());
        },
        Some("auto-run")|Some("auto-resume")|Some("speech")|Some("assets")|Some("captions")|Some("script-only")|Some("heygen-preflight")=>{
            let settings_path = data_dir().join("settings.json");
            let settings: Settings = if settings_path.is_file() { Settings::load(&settings_path)? } else { s };
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
                    pipeline::apply_plan(&mut project,plan)?;
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
            let settings:Settings=if path.is_file(){Settings::load(&path)?}else{s};
            let url=args.get(2).context("download <YouTube URL> <folder>")?;
            let folder=PathBuf::from(args.get(3).context("download <YouTube URL> <folder>")?);
            fs::create_dir_all(&folder)?;let folder=fs::canonicalize(folder)?;
            let source=media::download(&settings,url,&folder,"src_1".into(),&cancel)?;
            println!("{}",serde_json::json!({"path":source.path,"duration_ms":source.duration_ms,"width":source.width,"height":source.height,"audio":source.has_audio}));
        },
        Some("render")=>{let p=Project::load(Path::new(args.get(2).context("render <project.json> <output.mp4>")?))?;let output=PathBuf::from(args.get(3).context("output.mp4")?);let settings_path=data_dir().join("settings.json");let globals=if settings_path.is_file(){Settings::load(&settings_path)?}else{s};let effective=reaction_studio::workflow::effective_settings(&p,&globals);media::render(&p,&effective,&output,&cancel,|i,n|println!("Bước xuất {i}/{n}"))?;println!("OK {}",output.display());},
        _=>println!("AIR3view CLI\n  doctor\n  download <YouTube URL> <folder>\n  prepare <project.json>\n  auto-run / auto-resume <project.json>\n  batch-create <template.json> <urls.txt> [output-folder]\n  batch-run <batch-id>\n  batch-action <batch-id> <action-json>\n  script-only / speech / assets / captions / evidence <project.json>\n  repair-script <project.json> <saved response>\n  render <project.json> <output.mp4>\n  metadata-inspect <input.mp4>\n  metadata-apply <project.json> <input.mp4> <new-output.mp4>\n  heygen-preflight"),
    }
    Ok(())
}
