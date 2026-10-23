use crate::{
    media,
    model::*,
    process::{self, Cancel},
    providers, subtitle,
};
use anyhow::{Context, Result};
use std::{fs, path::PathBuf, sync::mpsc::Sender, thread, time::Duration};

#[derive(Debug)]
pub enum Event {
    Workflow(crate::workflow::JobState),
    Stage {
        lane: String,
        message: String,
        done: bool,
    },
    Log(String),
    Metadata { project_id: String, text: String },
    Project(Box<Project>),
    Preview {
        path: PathBuf,
        duration_ms: u64,
    },
    TimelinePreview { path: PathBuf, duration_ms: u64, start_ms: u64 },
    Done(String),
    Error(String),
}
fn stage(tx: &Sender<Event>, lane: &str, message: &str, done: bool) {
    let _ = tx.send(Event::Stage {
        lane: lane.into(),
        message: message.into(),
        done,
    });
}

/// Commit a validated candidate, preserving the old composition until it can be reconciled.
pub fn apply_plan(p:&mut Project,plan:ScriptPlan)->Result<()> {
    let candidate=crate::script_quality::candidate(p,&plan);
    candidate.validate(false)?;candidate.check_duration()?;candidate.validate_opening_hook()?;candidate.validate_intercuts()?;
    p.title=plan.title;p.analysis_summary=plan.analysis_summary;p.segments=plan.segments;
    p.script_warnings=fs::read(p.root.join("analysis/validation-report.json")).ok()
        .and_then(|b|serde_json::from_slice::<crate::script_quality::Report>(&b).ok())
        .map(|r|r.warnings).unwrap_or_default();
    p.ensure_composition();p.rendered=None;
    Ok(())
}

/// The URL analysis branch starts immediately, before media downloads have finished.
pub fn prepare(mut p: Project, s: Settings, tx: Sender<Event>, cancel: Cancel) -> Result<Project> {
    if s.provider == "antigravity" && (s.analysis_input=="local" || p.sources.iter().any(|src|src.url.is_empty()) || (s.analysis_input=="auto" && providers::url_known_unavailable(&p,&s))) {
        p.status = "Tải video trước khi phân tích local".into();
        p.save()?;
        for index in 0..p.sources.len() {
            let source = &p.sources[index];
            stage(
                &tx,
                "Download",
                &format!("Tải video {}/{}", index + 1, p.sources.len()),
                false,
            );
            let mut local = if source.path.is_file() {
                source.clone()
            } else {
                media::download(
                    &s,
                    &source.url,
                    &p.root.join("sources"),
                    source.id.clone(),
                    &cancel,
                )?
            };
            // Commit each download before doing any later expensive work.
            p.sources[index] = local.clone();
            p.save()?;
            local.proxy = media::proxy(&s, &local, &p.root.join("cache/proxy"), &cancel)?;
            p.sources[index] = local;
            p.save()?;
            let _ = tx.send(Event::Project(Box::new(p.clone())));
        }
        stage(&tx, "Download", "Đã tải nguồn", true);
        stage(
            &tx,
            "Analysis",
            "Tạo transcript Whisper và ảnh có timestamp",
            false,
        );
        crate::analysis::prepare(&p, &s, &cancel, |message| {
            stage(&tx, "Analysis", message, false)
        })?;
        stage(
            &tx,
            "Analysis",
            "Antigravity đang đọc transcript + ảnh và viết script JSON",
            false,
        );
        let plan = providers::analyze_progress(&p, &s, &cancel,true,&tx)?;
        apply_plan(&mut p,plan)?;
        p.validate(true)?;
        p.check_duration()?;
        p.status = "Chờ duyệt kịch bản".into();
        p.save()?;
        stage(
            &tx,
            "Analysis",
            "Đã nhận kịch bản JSON từ dữ liệu local",
            true,
        );
        return Ok(p);
    }
    p.status = "Phân tích và tải song song".into();
    p.save()?;
    let analysis_p = p.clone();
    let analysis_s = s.clone();
    let analysis_tx = tx.clone();
    let analysis_cancel = cancel.clone();
    let downloads = p.sources.clone();
    let downloads_s = s.clone();
    let downloads_tx = tx.clone();
    let downloads_cancel = cancel.clone();
    let root = p.root.clone();
    let (mut analysis, download) = thread::scope(|scope| {
        let analysis = scope.spawn(move || {
            stage(
                &analysis_tx,
                "Gemini",
                "Đang đọc URL và viết kịch bản",
                false,
            );
            let result = providers::analyze_progress(&analysis_p, &analysis_s, &analysis_cancel,analysis_s.analysis_input=="local",&analysis_tx);
            stage(
                &analysis_tx,
                "Gemini",
                if result.is_ok() {
                    "Đã nhận kịch bản JSON"
                } else {
                    "Phân tích thất bại"
                },
                true,
            );
            result
        });
        let download = scope.spawn(move || -> Result<Vec<Source>> {
            let mut sources = vec![];
            for (i, src) in downloads.iter().enumerate() {
                stage(
                    &downloads_tx,
                    "Download",
                    &format!("Video {}/{}: đang tải", i + 1, downloads.len()),
                    false,
                );
                let mut local = if src.path.is_file() {
                    src.clone()
                } else {
                    media::download(
                        &downloads_s,
                        &src.url,
                        &root.join("sources"),
                        src.id.clone(),
                        &downloads_cancel,
                    )?
                };
                stage(
                    &downloads_tx,
                    "Download",
                    &format!("Video {}: tạo proxy preview", i + 1),
                    false,
                );
                local.proxy = media::proxy(
                    &downloads_s,
                    &local,
                    &root.join("cache/proxy"),
                    &downloads_cancel,
                )?;
                sources.push(local);
            }
            stage(&downloads_tx, "Download", "Đã tải video và tạo proxy", true);
            Ok(sources)
        });
        (
            analysis
                .join()
                .unwrap_or_else(|_| Err(anyhow::anyhow!("Analysis worker panic"))),
            download
                .join()
                .unwrap_or_else(|_| Err(anyhow::anyhow!("Download worker panic"))),
        )
    });
    // Persist whichever branch succeeded so retry does not throw away paid work or downloads.
    if let Ok(sources) = &download {
        p.sources = sources.clone();
    }
    if analysis.as_ref().err().is_some_and(providers::url_access_unavailable) && download.is_ok() && s.provider=="antigravity" && s.analysis_input=="auto" {
        process::check_cancel(&cancel)?;
        stage(&tx,"Analysis","CLI không hoàn tất đọc URL; chuyển sang transcript + ảnh đã tải",false);
        crate::diagnostics::record(&p.root,"Analysis",None,"url-fallback",serde_json::json!({"error":format!("{:#}",analysis.as_ref().unwrap_err())}))?;
        p.save()?;
        analysis=providers::analyze_progress(&p,&s,&cancel,true,&tx);
    }
    if let Ok(plan) = &analysis {
        apply_plan(&mut p,plan.clone())?;
    }
    p.status = "Cần kiểm tra".into();
    p.save()?;
    let _ = tx.send(Event::Project(Box::new(p.clone())));
    let _ = download?;
    let _ = analysis?;
    process::check_cancel(&cancel)?;
    p.validate(true)?;
    p.check_duration()?;
    p.status = "Chờ duyệt kịch bản".into();
    p.save()?;
    Ok(p)
}

pub fn voice_key(s: &Settings, seg: &Segment) -> String {
    media::hash(format!(
        "tts-v5:{}:{}:{}:{}:{}:{}",
        s.voice,
        s.vieneu_url,
        s.voice_reference_enabled,
        crate::input_assets::file_fingerprint(&s.voice_reference_path),
        s.voice_reference_denoise,
        seg.narration
    ))
}

pub fn voice_key_for_language(s: &Settings, seg: &Segment, language: &str) -> String {
    if crate::content_language::canonical(language) == "vi" { voice_key(s, seg) }
    else { media::hash(format!("{}:{}:{}", crate::content_language::CONTRACT_VERSION,
        crate::content_language::canonical(language), voice_key(s, seg))) }
}

fn checkpoint(p: &Project, tx: &Sender<Event>) -> Result<()> {
    p.save()?;
    let _ = tx.send(Event::Project(Box::new(p.clone())));
    Ok(())
}

fn avatar_key(s: &Settings, seg: &Segment) -> String {
    media::hash(format!(
        "avatar-v4:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
        s.heygen_base,
        s.avatar_enabled,
        s.avatar_id,
        s.avatar_image_url,
        crate::input_assets::file_fingerprint(&s.avatar_image_path),
        seg.asset_key,
        s.avatar_provider,
        s.cartoon_character,
        seg.emotion,
        seg.gesture,
    ))
}

fn scene_error(p: &Project, stage_name: &str, id: &str, error: anyhow::Error) -> anyhow::Error {
    let _ = crate::diagnostics::failure(&p.root, stage_name, Some(id), &error);
    error.context(format!("Cảnh {id} / {stage_name}"))
}

/// Complete and checkpoint speech independently of any paid cloud operation.
pub fn generate_speech(
    mut p: Project,
    s: Settings,
    tx: Sender<Event>,
    cancel: Cancel,
) -> Result<Project> {
    anyhow::ensure!(crate::content_language::supported(&p.rules.language), "Ngôn ngữ lời review chưa hỗ trợ; chọn Tiếng Việt hoặc English");
    p.validate(true)?;
    p.status = "Đang tạo giọng".into();
    p.rendered = None;
    checkpoint(&p, &tx)?;
    for index in 0..p.segments.len() {
        process::check_cancel(&cancel)?;
        if p.segments[index].kind == "source" {
            continue;
        }
        let mut seg = p.segments[index].clone();
        if seg.locked && seg.audio.as_ref().is_some_and(|a|a.is_file()) {
            checkpoint(&p,&tx)?;
            continue;
        }
        let speech_language = if seg.locked {seg.asset_language.clone()} else {p.rules.language.clone()};
        if seg.asset_window && seg.audio.as_ref().is_some_and(|a|a.is_file()) {
            let mut original = seg.clone(); original.narration=seg.asset_narration.clone();
            if seg.asset_key == voice_key_for_language(&s,&original,&speech_language) {
                // An edit window is not a request to synthesize the full sentence
                // again. Keep the saved take and all local word/graphic timing.
                checkpoint(&p,&tx)?;
                continue;
            }
        }
        let k = voice_key_for_language(&s, &seg, &speech_language);
        if seg.asset_key != k {
            seg.invalidate_voice();
        }
        stage(
            &tx,
            "Audio",
            &format!(
                "VieNeu: cảnh {}/{} ({})",
                index + 1,
                p.segments.len(),
                seg.id
            ),
            false,
        );
        let result = (|| -> Result<()> {
            let audio = providers::tts_for_language(&s, &seg.narration, &speech_language, &p.root.join("cache/audio"), &cancel)?;
            let info = media::probe(&s, &audio, &cancel)?;
            anyhow::ensure!(
                info.has_audio && info.duration_ms >= 100,
                "TTS trả audio rỗng/quá ngắn"
            );
            let old_duration = seg.duration_ms.max(1);
            seg.asset_start_ms = 0;
            seg.avatar_start_ms = 0;
            seg.asset_window = false;
            seg.asset_narration = seg.narration.clone();
            seg.asset_language = crate::content_language::canonical(&speech_language);
            seg.suppress_captions = false;
            for cue in &mut seg.graphics {
                cue.start_ms = cue.start_ms * info.duration_ms / old_duration;
                cue.end_ms = (cue.end_ms * info.duration_ms / old_duration)
                    .max(cue.start_ms + 1)
                    .min(info.duration_ms);
            }
            seg.duration_ms = info.duration_ms;
            seg.audio = Some(audio);
            if seg.words.is_empty() {
                let text = if seg.caption.trim().is_empty() {
                    &seg.narration
                } else {
                    &seg.caption
                };
                seg.words = subtitle::estimated_words(text, seg.duration_ms);
                seg.timing_estimated = true;
            }
            seg.asset_key = k;
            if seg.avatar_key != avatar_key(&s, &seg) {
                seg.avatar = None;
                seg.avatar_key.clear();
            }
            Ok(())
        })();
        if let Err(error) = result {
            return Err(scene_error(&p, "Audio", &seg.id, error));
        }
        p.segments[index] = seg;
        checkpoint(&p, &tx)?;
        crate::diagnostics::record(
            &p.root,
            "Audio",
            Some(&p.segments[index].id),
            "checkpoint",
            serde_json::json!({"duration_ms":p.segments[index].duration_ms}),
        )?;
    }
    let duration_notice = format!(
        "TTS đã lưu; timeline {:.1}s, mục tiêu {}–{}s{}",
        p.duration_ms() as f64 / 1000.0,
        p.rules.min_seconds,
        p.rules.max_seconds,
        if p.check_duration().is_ok() {
            ""
        } else {
            "; cần chỉnh kịch bản"
        }
    );
    p.status = duration_notice.clone();
    checkpoint(&p, &tx)?;
    stage(&tx, "Audio", &duration_notice, true);
    Ok(p)
}

/// Evidence extraction owns no project writes while speech/avatar checkpoints advance.
pub fn generate_assets(p:Project,s:Settings,tx:Sender<Event>,cancel:Cancel)->Result<Project> {
    thread::scope(|scope| {
        let revision=evidence_revision(&p);
        let mut cards=p.clone();let es=s.clone();let etx=tx.clone();let ecancel=cancel.clone();
        let worker=scope.spawn(move || {
            let result=crate::evidence::prepare(&mut cards,&es,&ecancel,|msg|stage(&etx,"Dẫn chứng",msg,false));
            (cards,result)
        });
        let root=p.root.clone();let result=generate_assets_core(p,s.clone(),tx.clone(),cancel.clone());
        let (cards,evidence_result)=worker.join().map_err(|_|anyhow::anyhow!("Worker dẫn chứng bị dừng"))?;
        let mut p=match result {Ok(p)=>p,Err(error)=>{
            // Keep completed cards alongside the most recent speech/avatar checkpoint.
            if let Ok(mut saved)=Project::load(&root.join("project.json")){merge_evidence(&mut saved,&cards);let _=checkpoint(&saved,&tx);}
            return Err(error);
        }};
        merge_evidence(&mut p,&cards);crate::evidence::align(&mut p);p.ensure_composition();
        // A measured-duration repair can replace scene/item IDs while the worker
        // still owns the original script. Reconcile against the final script.
        let revised=revision!=evidence_revision(&p);
        if revised || evidence_result.is_err() || crate::evidence::validate_prepared(&p).is_err() {
            crate::diagnostics::record(&p.root,"Dẫn chứng",None,"reconcile_final_script",serde_json::json!({"script_changed":revised,"worker_failed":evidence_result.is_err()}))?;
            stage(&tx,"Dẫn chứng","Đối chiếu và cập nhật ảnh theo kịch bản cuối; tái sử dụng frame đã lưu",false);
            let result=crate::evidence::prepare(&mut p,&s,&cancel,|msg|stage(&tx,"Dẫn chứng",msg,false));
            p.ensure_composition();checkpoint(&p,&tx)?;
            result?;
        }
        checkpoint(&p,&tx)?;
        crate::evidence::validate_prepared(&p).inspect_err(|e|{let _=crate::diagnostics::failure(&p.root,"Dẫn chứng",None,e);})?;
        crate::diagnostics::record(&p.root,"Dẫn chứng",None,"validated",serde_json::json!({"items":p.segments.iter().flat_map(|s|&s.evidence_items).filter(|e|p.evidence_settings.enabled&&e.enabled).count()}))?;
        stage(&tx,"Dẫn chứng","Ảnh và layer dẫn chứng của kịch bản cuối đã kiểm tra đầy đủ",true);Ok(p)
    })
}
fn evidence_revision(p:&Project)->String {
    let ids:Vec<_>=p.segments.iter().map(|s|(&s.id,&s.kind,s.evidence_items.iter().map(|e|(&e.id,e.enabled)).collect::<Vec<_>>())).collect();
    media::hash(serde_json::to_vec(&(&p.rules.language,&p.evidence_settings,p.output.portrait,ids)).unwrap())
}
fn merge_evidence(p:&mut Project,cards:&Project) {
    for seg in &mut p.segments {if let Some(done)=cards.segments.iter().find(|s|s.id==seg.id){for item in &mut seg.evidence_items{
        if let Some(card)=done.evidence_items.iter().find(|e|e.id==item.id){
            item.asset=card.asset.clone();item.annotation_asset=card.annotation_asset.clone();item.asset_key=card.asset_key.clone();
            if item.proof.formula=="delta_ms" {item.value=card.value.clone();item.unit=card.unit.clone();}
        }
    }}}
}
pub fn generate_evidence(mut p:Project,s:Settings,tx:Sender<Event>,cancel:Cancel)->Result<Project> {
    let result=crate::evidence::prepare(&mut p,&s,&cancel,|msg|stage(&tx,"Dẫn chứng",msg,false));
    p.ensure_composition();p.rendered=None;checkpoint(&p,&tx)?;result?;
    crate::evidence::validate_prepared(&p)?;
    stage(&tx,"Dẫn chứng","Đã cập nhật thẻ, giữ nguyên giọng và avatar",true);Ok(p)
}

fn generate_assets_core(
    p: Project,
    s: Settings,
    tx: Sender<Event>,
    cancel: Cancel,
) -> Result<Project> {
    let provider = if s.avatar_enabled {
        s.avatar_provider.as_str()
    } else {
        "disabled"
    };
    anyhow::ensure!(
        matches!(provider, "cartoon2d" | "heygen" | "disabled"),
        "Chọn nhân vật 2D local hoặc HeyGen trong Cài đặt"
    );
    crate::diagnostics::record(
        &p.root,
        "Assets",
        None,
        "started",
        serde_json::json!({"avatar_provider":provider}),
    )?;
    stage(
        &tx,
        "Avatar",
        match provider {
            "cartoon2d" => "Đã chọn nhân vật 2D local • không gọi API avatar bên ngoài",
            "heygen" => "Đã chọn HeyGen Avatar IV",
            _ => "Nhân vật đang tắt",
        },
        false,
    );
    let mut p = generate_speech(p, s.clone(), tx.clone(), cancel.clone())?;
    for round in 1..=if p.sources.is_empty(){0}else{crate::script_quality::MAX_REPAIRS} {
        // Check measured segments, not a prior editor composition.
        let mut measured=p.clone();measured.composition=None;
        if measured.check_duration().is_ok(){break;}
        stage(&tx,"Audio",&format!("Đang sửa lượng lời theo audio thật ({round}/2), giữ WAV không đổi"),false);
        crate::diagnostics::record(&p.root,"Audio",None,"duration_repair",serde_json::json!({"round":round,"measured_ms":p.duration_ms()}))?;
        match providers::repair_duration(&p,&s,&cancel,round) {
            Ok(plan)=> {
                apply_plan(&mut p,plan)?;
                checkpoint(&p,&tx)?;
                p=generate_speech(p,s.clone(),tx.clone(),cancel.clone())?;
            },
            Err(e)=> {
                process::check_cancel(&cancel)?;
                crate::diagnostics::failure(&p.root,"Duration",None,&e)?;
                if round==crate::script_quality::MAX_REPAIRS {return Err(e.context("Không thể khớp thời lượng sau 2 lượt sửa; WAV đã tạo được giữ lại"));}
            }
        }
    }
    // Validate measured voice and footage before entering either avatar provider.
    let mut fitted = p.clone();
    fitted.composition=None;
    let mut footage_changes = Vec::new();
    let validation = (|| -> Result<()> {
        fit_budget(&mut fitted)?;
        footage_changes = fit_review_footage(&mut fitted)?;
        fitted.validate(true)?;
        fitted.validate_intercuts()?;
        Ok(())
    })();
    checkpoint(&p, &tx)?;
    if let Err(error) = validation {
        let _ = crate::diagnostics::failure(&p.root, "Timeline", None, &error);
        return Err(
            error.context("TTS đã lưu; kiểm tra thời lượng và footage trước khi tạo nhân vật")
        );
    }
    let previous=p.composition.take();
    p = fitted;
    p.composition=previous;
    p.ensure_composition();
    checkpoint(&p, &tx)?;
    for change in footage_changes {
        let index = p
            .segments
            .iter()
            .position(|seg| seg.id == change.scene_id)
            .unwrap()
            + 1;
        stage(
            &tx,
            "Timeline",
            &format!(
                "Cảnh {index}: mở rộng footage {:.2}–{:.2}s → {:.2}–{:.2}s để khớp voice",
                change.old_in_ms as f64 / 1000.0,
                change.old_out_ms as f64 / 1000.0,
                change.in_ms as f64 / 1000.0,
                change.out_ms as f64 / 1000.0
            ),
            false,
        );
        crate::diagnostics::record(
            &p.root,
            "Timeline",
            Some(&change.scene_id),
            "footage_extended",
            serde_json::to_value(&change)?,
        )?;
    }
    crate::diagnostics::record(
        &p.root,
        "Timeline",
        None,
        "validated",
        serde_json::json!({"duration_ms":p.duration_ms()}),
    )?;
    if s.avatar_enabled {
        for seg in &mut p.segments {
            if seg.kind == "source" || !seg.asset_window {continue;}
            let audio=seg.audio.as_ref().context("Cảnh đã trim thiếu WAV")?;
            let folder=p.root.join("cache/audio/windows"); fs::create_dir_all(&folder)?;
            let key=media::hash(format!("audio-window-v1:{}:{}:{}",media::hash(fs::read(audio)?),seg.asset_start_ms,seg.duration_ms));
            let cropped=folder.join(format!("{key}.wav"));
            if !cropped.is_file() {
                process::run(&s.ffmpeg,&process::args(&["-y","-v","error","-ss",&format!("{:.3}",seg.asset_start_ms as f64/1000.),"-i",&audio.to_string_lossy(),"-t",&format!("{:.3}",seg.duration_ms as f64/1000.),"-ar","48000","-ac","1","-c:a","pcm_s16le",&cropped.to_string_lossy()]),None,&cancel,Duration::from_secs(s.timeout_seconds),None)?;
            }
            seg.audio=Some(cropped);seg.asset_start_ms=0;
        }
        checkpoint(&p,&tx)?;
    }
    if s.avatar_enabled && s.avatar_provider == "cartoon2d" {
        for index in 0..p.segments.len() {
            if p.segments[index].kind == "source" {
                continue;
            }
            process::check_cancel(&cancel)?;
            stage(
                &tx,
                "Avatar",
                &format!(
                    "2D local: cảnh {}/{} • Rhubarb + biểu cảm",
                    index + 1,
                    p.segments.len()
                ),
                false,
            );
            let path = crate::cartoon::generate(
                &s,
                &p.segments[index],
                &p.root.join("cache/cartoon"),
                &cancel,
            )
            .map_err(|error| scene_error(&p, "Avatar 2D", &p.segments[index].id, error))?;
            p.segments[index].avatar = Some(path);
            p.segments[index].avatar_start_ms = 0;
            p.segments[index].avatar_key = avatar_key(&s, &p.segments[index]);
            checkpoint(&p, &tx)?;
            crate::diagnostics::record(
                &p.root,
                "Avatar 2D",
                Some(&p.segments[index].id),
                "checkpoint",
                serde_json::json!({"provider":"cartoon2d","status":"completed"}),
            )?;
        }
    } else if s.avatar_enabled {
        anyhow::ensure!(
            s.avatar_provider == "heygen",
            "Chọn nhân vật 2D local hoặc HeyGen trong Cài đặt"
        );
        let folder = p.root.join("cache/avatar");
        let mut needs_submission = vec![false; p.segments.len()];
        for (index, seg) in p
            .segments
            .iter()
            .enumerate()
            .filter(|(_, seg)| seg.kind != "source")
        {
            needs_submission[index] = providers::heygen_needs_submission(
                &s,
                seg.audio.as_ref().expect("speech checkpoint"),
                &folder,
                &cancel,
            )?;
        }
        let mut preflight_error = None;
        if needs_submission.iter().any(|needs| *needs) {
            stage(
                &tx,
                "Avatar",
                "Kiểm tra xác thực và số dư API HeyGen",
                false,
            );
            if let Err(error) = providers::heygen_preflight(&s, &cancel) {
                let _ = crate::diagnostics::failure(&p.root, "HeyGen preflight", None, &error);
                preflight_error = Some(error.context("TTS đã lưu; HeyGen preflight"));
            }
        }
        // Bound remote concurrency to two; commit each completed scene immediately.
        for start in (0..p.segments.len()).step_by(2) {
            let end = (start + 2).min(p.segments.len());
            let mut first_error = None;
            thread::scope(|scope| -> Result<()> {
                let (results_tx, results_rx) = std::sync::mpsc::channel();
                for (index, needs_new_job) in
                    needs_submission.iter().enumerate().take(end).skip(start)
                {
                    let seg = p.segments[index].clone();
                    if seg.kind == "source" || (preflight_error.is_some() && *needs_new_job) {
                        continue;
                    }
                    let results_tx = results_tx.clone();
                    let s = &s;
                    let folder = &folder;
                    let cancel = &cancel;
                    stage(
                        &tx,
                        "Avatar",
                        &format!("HeyGen: cảnh {} ({})", index + 1, seg.id),
                        false,
                    );
                    scope.spawn(move || {
                        let result = providers::heygen(
                            s,
                            seg.audio.as_ref().expect("speech checkpoint"),
                            folder,
                            cancel,
                        );
                        let _ = results_tx.send((index, result));
                    });
                }
                drop(results_tx);
                for (index, result) in results_rx {
                    match result {
                        Ok(path) => {
                            p.segments[index].avatar = Some(path);
                            p.segments[index].avatar_start_ms = 0;
                            p.segments[index].avatar_key = avatar_key(&s, &p.segments[index]);
                            checkpoint(&p, &tx)?;
                            crate::diagnostics::record(
                                &p.root,
                                "Avatar",
                                Some(&p.segments[index].id),
                                "checkpoint",
                                serde_json::json!({"status":"completed"}),
                            )?;
                        }
                        Err(error) => {
                            let error = scene_error(&p, "Avatar", &p.segments[index].id, error);
                            if first_error.is_none() {
                                first_error = Some(error);
                            }
                        }
                    }
                }
                Ok(())
            })?;
            if let Some(error) = first_error {
                return Err(error);
            }
        }
        if let Some(error) = preflight_error {
            return Err(error);
        }
    } else {
        for seg in &mut p.segments {
            seg.avatar = None;
        }
        checkpoint(&p, &tx)?;
        anyhow::ensure!(
            !p.segments.iter().any(|seg| seg.kind == "host"),
            "TTS đã lưu. Cảnh host cần nhân vật; chọn 2D local/HeyGen hoặc đổi loại cảnh thành review."
        );
    }
    p.status = "Tài nguyên đã sẵn sàng".into();
    checkpoint(&p, &tx)?;
    stage(
        &tx,
        "Avatar",
        if s.avatar_enabled {
            "Avatar hoàn tất"
        } else {
            "Avatar đang tắt"
        },
        true,
    );
    Ok(p)
}

pub fn transcribe(
    mut p: Project,
    s: Settings,
    worker: PathBuf,
    tx: Sender<Event>,
    cancel: Cancel,
) -> Result<Project> {
    let folder = p.root.join("cache/transcript");
    fs::create_dir_all(&folder)?;
    let mut jobs = Vec::new();
    let mut outputs = Vec::new();
    for i in 0..p.segments.len() {
        process::check_cancel(&cancel)?;
        stage(
            &tx,
            "Subtitle",
            &format!("Căn từ {}/{}", i + 1, p.segments.len()),
            false,
        );
        let seg = &p.segments[i];
        let temp = folder.join(format!("{}.wav", seg.id));
        let (input, start) = if let Some(audio) = &seg.audio {
            (audio.clone(), seg.asset_start_ms)
        } else {
            (p.source(&seg.source_id)?.path.clone(), seg.in_ms)
        };
        let language = if seg.kind == "source" {
            s.asr_language.clone()
        } else if seg.audio.is_some() { seg.asset_language.clone() } else { p.rules.language.clone() };
        let meta = fs::metadata(&input)?;
        let key = media::hash(format!(
            "asr-v2:{}:{}:{:?}:{}:{}:{}:{}",
            input.display(),
            meta.len(),
            meta.modified().ok(),
            start,
            seg.duration_ms,
            language,
            s.asr_model
        ));
        let output = folder.join(format!("{key}.words.json"));
        outputs.push(output.clone());
        if output.is_file() {
            continue;
        }
        let args = vec![
            "-y".into(),
            "-v".into(),
            "error".into(),
            "-ss".into(),
            format!("{:.3}", start as f64 / 1000.0),
            "-i".into(),
            input.to_string_lossy().into_owned(),
            "-t".into(),
            format!("{:.3}", seg.duration_ms as f64 / 1000.0),
            "-vn".into(),
            "-ar".into(),
            "16000".into(),
            "-ac".into(),
            "1".into(),
            temp.to_string_lossy().into_owned(),
        ];
        process::run(
            &s.ffmpeg,
            &args,
            None,
            &cancel,
            Duration::from_secs(s.timeout_seconds),
            None,
        )?;
        jobs.push(serde_json::json!({"input":temp,"output":output,"language":language}));
    }
    if !jobs.is_empty() {
        let batch = folder.join("batch.json");
        write_json(&batch, &jobs)?;
        let args = vec![
            worker.to_string_lossy().into_owned(),
            "--batch".into(),
            batch.to_string_lossy().into_owned(),
            "--model".into(),
            s.asr_model.clone(),
            "--device".into(),
            s.asr_device.clone(),
        ];
        process::run(
            &s.python,
            &args,
            None,
            &cancel,
            Duration::from_secs(s.timeout_seconds),
            None,
        )?;
    }
    for (i, output) in outputs.iter().enumerate() {
        let words: Vec<Word> = serde_json::from_slice(&fs::read(output)?)?;
        anyhow::ensure!(
            words
                .iter()
                .all(|w| w.start_ms < w.end_ms && w.end_ms <= p.segments[i].duration_ms + 100),
            "ASR trả timestamp ngoài cảnh"
        );
        if p.segments[i].kind == "source" && p.segments[i].caption.trim().is_empty() {
            if crate::content_language::english(&p.rules.language) && !crate::content_language::source_is_english(&words) {
                // Keep original audio. A source transcript is not an English
                // translation; never publish raw foreign ASR as English subtitles.
                p.segments[i].words.clear();
                p.segments[i].timing_estimated = true;
                p.save()?;
                continue;
            }
            p.segments[i].caption = words
                .iter()
                .map(|w| w.text.clone())
                .collect::<Vec<_>>()
                .join(" ");
        }
        let seg=&mut p.segments[i];
        let text=if !seg.caption.trim().is_empty() { &seg.caption } else { &seg.narration };
        if !text.trim().is_empty() {
            let (aligned,estimated)=subtitle::align_text(text,&words,seg.duration_ms);
            seg.words=aligned;seg.timing_estimated=estimated;
        } else {
            seg.words=words;seg.timing_estimated=false;
        }
        p.save()?;
    }
    crate::evidence::align(&mut p);p.ensure_composition();checkpoint(&p,&tx)?;
    stage(&tx, "Subtitle", "Đã căn từ bằng Whisper", true);
    Ok(p)
}

pub fn diagnostics(s: &Settings) -> String {
    let cancel = process::cancel_token();
    let mut lines = vec![
        format!("FFmpeg: {}", process::version(&s.ffmpeg)),
        format!("ffprobe: {}", process::version(&s.ffprobe)),
        format!("YouTube JS runtime: {}", media::youtube_js_runtime(s)),
    ];
    for (name, path) in [
        ("yt-dlp", &s.ytdlp),
        ("Python", &s.python),
        ("Node.js (YouTube)", &s.youtube_node),
        ("Antigravity CLI", &s.cli_path),
    ] {
        let flag = if name == "Antigravity CLI" {
            "--help"
        } else {
            "--version"
        };
        let state = match process::run(
            path,
            &[flag.into()],
            None,
            &cancel,
            Duration::from_secs(10),
            None,
        ) {
            Ok(o) => String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .unwrap_or("OK")
                .to_string(),
            Err(e) => format!("{e:#}"),
        };
        lines.push(format!("{name}: {state}"));
    }
    for name in [&s.gemini_key_env, &s.heygen_key_env] {
        lines.push(format!(
            "{name}: {}",
            if providers::environment_key(name).is_some() {
                "Đã đặt"
            } else {
                "Chưa đặt"
            }
        ));
    }
    lines.join("\n")
}
