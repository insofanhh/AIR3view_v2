use crate::{
    media,
    model::*,
    process::{self, Cancel},
    providers, subtitle,
};
use anyhow::{ensure, Context, Result};
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
    // Resolve once before starting paid analysis. Never relocate already downloaded sources.
    let source_folder = if crate::download_location::needs_download(&p) {
        crate::download_location::folder(&p, &s)?
    } else { p.root.join("sources") };
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
                    &source_folder,
                    source.id.clone(),
                    &cancel,
                )?
            };
            // Commit each download before doing any later expensive work.
            p.sources[index] = local.clone();
            p.save()?;
            crate::source_normalization::prepare(&p.normalization,&s,&mut local,&p.root,&cancel)?;
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
    let normalization=p.normalization.clone();
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
                        &source_folder,
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
                crate::source_normalization::prepare(&normalization,&downloads_s,&mut local,&root,&downloads_cancel)?;
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
    if download.is_ok() {
        analysis=analysis.and_then(|plan|providers::finish_source_review(&p,&s,&cancel,plan,Some(&tx)));
    }
    if let Ok(plan) = &analysis {
        apply_plan(&mut p,plan.clone())?;
    }
    p.status = "Cần kiểm tra".into();
    p.save()?;
    speed_report(&p)?;
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

fn speed_report(p:&Project)->Result<()> {
    if crate::footage_speed::enabled(p) {
        write_json(&p.root.join("analysis/footage-speed-report.json"),&serde_json::json!({
            "settings":p.output.footage_speed,"scenes":p.segments.iter().map(|s|serde_json::json!({
                "scene_id":s.id,"kind":s.kind,"duration_ms":s.duration_ms,"source_rate":s.source_rate,
                "status":s.footage_status,"spans":s.footage_spans,"anchors":s.footage_anchors
            })).collect::<Vec<_>>()
        }))?;
    }
    Ok(())
}

fn checkpoint(p: &Project, tx: &Sender<Event>) -> Result<()> {
    p.save()?;
    speed_report(p)?;
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
    let scene = p.segments.iter().position(|s| s.id == id).map(|n| (n + 1).to_string()).unwrap_or_else(|| id.into());
    error.context(format!("Cảnh {scene} / {stage_name}"))
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
            seg.footage_spans.clear();
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
pub fn generate_assets(mut p:Project,s:Settings,tx:Sender<Event>,cancel:Cancel)->Result<Project> {
    if crate::us_editorial::enabled(&p.rules) {
        let plan=crate::script_quality::plan(&p);
        let mut report=crate::script_quality::inspect(&p,&plan,&s,false);
        // Timing after partial WAV generation belongs to the measured recovery
        // below. Do not spend the editorial repair budget on those mismatches.
        report.issues.retain(|i|!matches!(i.code.as_str(),"DURATION"|"US_RAW_CADENCE"));
        crate::us_editorial::review(&p,&s,&plan,&cancel,&mut report)?;
        if !report.issues.is_empty() {
            stage(&tx,"Analysis","Kiểm tra lại nội dung US trước giọng/nhân vật",false);
            let plan=providers::finish_source_review(&p,&s,&cancel,plan,Some(&tx))?;
            apply_plan(&mut p,plan)?;checkpoint(&p,&tx)?;
        }
    }
    thread::scope(|scope| {
        let revision=evidence_revision(&p);
        let evidence_input=p.clone();
        let mut cards=p.clone();let es=s.clone();let etx=tx.clone();let ecancel=cancel.clone();
        let worker=scope.spawn(move || {
            let result=prepare_evidence(&mut cards,&es,&ecancel,&etx);
            (cards,result)
        });
        let root=p.root.clone();let result=generate_assets_core(p,s.clone(),tx.clone(),cancel.clone());
        let (cards,evidence_result)=worker.join().map_err(|_|anyhow::anyhow!("Worker dẫn chứng bị dừng"))?;
        let mut p=match result {Ok(p)=>p,Err(error)=>{
            // Keep completed cards alongside the most recent speech/avatar checkpoint.
            if let Ok(mut saved)=Project::load(&root.join("project.json")){merge_evidence(&mut saved,&cards,&evidence_input);let _=checkpoint(&saved,&tx);}
            return Err(error);
        }};
        merge_evidence(&mut p,&cards,&evidence_input);crate::evidence::align(&mut p);p.ensure_composition();
        // A measured-duration repair can replace scene/item IDs while the worker
        // still owns the original script. Reconcile against the final script.
        let revised=revision!=evidence_revision(&p);
        // A failed provider call must not immediately repeat for an unchanged
        // script. Speech/avatar checkpoints are already saved for resume.
        if !revised {
            if let Err(error)=evidence_result { checkpoint(&p,&tx)?; return Err(error.context("Chưa hoàn tất đối chiếu ảnh dẫn chứng; giọng và avatar đã lưu. Thử lại sau khi kiểm tra kết nối/model.")); }
        }
        if revised || crate::evidence::validate_prepared(&p).is_err() {
            crate::diagnostics::record(&p.root,"Dẫn chứng",None,"reconcile_final_script",serde_json::json!({"script_changed":revised}))?;
            stage(&tx,"Dẫn chứng","Đối chiếu và cập nhật ảnh theo kịch bản cuối; tái sử dụng frame đã lưu",false);
            let result=prepare_evidence(&mut p,&s,&cancel,&tx);
            p.ensure_composition();checkpoint(&p,&tx)?;
            result?;
        }
        checkpoint(&p,&tx)?;
        crate::evidence_selection::validate(&p)?;
        crate::evidence::validate_prepared(&p).inspect_err(|e|{let _=crate::diagnostics::failure(&p.root,"Dẫn chứng",None,e);})?;
        crate::host_evidence::report(&p,true)?;
        crate::host_evidence::validate(&p,true)?;
        crate::diagnostics::record(&p.root,"Dẫn chứng",None,"validated",serde_json::json!({"items":p.segments.iter().flat_map(|s|&s.evidence_items).filter(|e|p.evidence_settings.enabled&&e.enabled).count()}))?;
        stage(&tx,"Dẫn chứng","Ảnh và layer dẫn chứng của kịch bản cuối đã kiểm tra đầy đủ",true);Ok(p)
    })
}
fn evidence_revision(p:&Project)->String {
    media::hash(serde_json::to_vec(&(&p.rules.language,&p.evidence_settings,p.output.portrait,crate::evidence_selection::revision(p))).unwrap())
}
fn merge_evidence(p:&mut Project,cards:&Project,baseline:&Project) {
    // A duration repair can keep an ID while changing its claim/crop. Only
    // merge the worker result if every source/content input still matches.
    let view=&*p;
    let mut added=vec![];
    for done in &cards.segments {
        let Some(current)=view.segments.iter().find(|s|s.id==done.id) else{continue;};
        let Some(base)=baseline.segments.iter().find(|s|s.id==done.id) else{continue;};
        let signature=|project:&Project,seg:&Segment|seg.evidence_items.iter().map(|e|
            (e.id.clone(),e.enabled,crate::evidence_selection::request_key(project,seg,e),crate::evidence_selection::protected(project,seg,e))).collect::<Vec<_>>();
        if current.locked || current.narration!=base.narration || current.source_id!=base.source_id
            || current.in_ms!=base.in_ms || current.out_ms!=base.out_ms
            || signature(view,current)!=signature(baseline,base) {continue;}
        for card in &done.evidence_items {
            if crate::host_evidence::automatic(card,&done.id) && !base.evidence_items.iter().any(|e|e.id==card.id)
                && !current.evidence_items.iter().any(|e|e.id==card.id)
                && card.selection.key==crate::evidence_selection::request_key(view,current,card)
                && !view.timeline.tracks[crate::timeline::EVIDENCE].locked
                && !view.timeline.tracks[crate::timeline::ANNOTATION].locked {added.push((done.id.clone(),card.clone()));}
        }
    }
    let keys:std::collections::HashMap<_,_>=view.segments.iter().flat_map(|s|s.evidence_items.iter()
        .filter(move|e|baseline.segments.iter().find(|old|old.id==s.id).is_some_and(|old|{
            old.evidence_items.iter().find(|item|item.id==e.id).is_some_and(|item|
                crate::evidence::content_identity(item)==crate::evidence::content_identity(e)
                && crate::evidence_selection::protected(baseline,old,item)==crate::evidence_selection::protected(view,s,e))
        }))
        .map(move|e|(e.id.clone(),crate::evidence_selection::request_key(view,s,e)))).collect();
    for seg in &mut p.segments {if let Some(done)=cards.segments.iter().find(|s|s.id==seg.id){for item in &mut seg.evidence_items{
        if let Some(card)=done.evidence_items.iter().find(|e|e.id==item.id){
            let valid=keys.get(&item.id).is_some_and(|k|k==&card.selection.request_key||k==&card.selection.key);
            if !valid {continue;}
            item.asset=card.asset.clone();item.annotation_asset=card.annotation_asset.clone();item.asset_key=card.asset_key.clone();
            item.source_time_ms=card.source_time_ms;item.crop=card.crop;item.second_time_ms=card.second_time_ms;item.second_crop=card.second_crop;
            item.frame_id=card.frame_id.clone();item.coordinate_space=card.coordinate_space.clone();item.selection=card.selection.clone();item.enabled=card.enabled;
            item.annotations=card.annotations.clone();
            if item.proof.formula=="delta_ms" {item.value=card.value.clone();item.unit=card.unit.clone();}
        }
    }}}
    for (scene,card) in added {if let Some(seg)=p.segments.iter_mut().find(|s|s.id==scene) {if seg.evidence_items.len()<4 {seg.evidence_items.push(card);}}}
    for warning in &cards.script_warnings {if !p.script_warnings.contains(warning){p.script_warnings.push(warning.clone());}}
}
fn prepare_evidence(p:&mut Project,s:&Settings,cancel:&Cancel,tx:&Sender<Event>)->Result<()> {
    crate::host_evidence::prepare(p,s,cancel,|msg|stage(tx,"Dẫn chứng",msg,false))?;
    crate::evidence::prepare(p,s,cancel,|msg|stage(tx,"Dẫn chứng",msg,false))?;
    crate::evidence_selection::validate(p)?;
    crate::evidence_selection::write_report(p)
}
pub fn generate_evidence(mut p:Project,s:Settings,tx:Sender<Event>,cancel:Cancel)->Result<Project> {
    let backup=p.root.join("analysis/evidence-backups").join(format!("{}.json",uuid::Uuid::new_v4()));
    write_json(&backup,&p)?;
    // Only publish a successful selection + rasterization. Failed/cancelled
    // review keeps the active project's cards and rendered preview intact.
    prepare_evidence(&mut p,&s,&cancel,&tx)?;
    p.ensure_composition();p.rendered=None;
    crate::evidence::validate_prepared(&p)?;
    crate::host_evidence::report(&p,true)?;
    crate::host_evidence::validate(&p,true)?;
    checkpoint(&p,&tx)?;
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
    for _ in 0..=crate::duration_recovery::MAX_AI_REPAIRS {
        // Check measured segments, not a prior editor composition.
        let mut measured=p.clone();measured.composition=None;
        if crate::footage_speed::enabled(&measured) { crate::footage_speed::fit(&mut measured)?; }
        let editorial=crate::script_quality::inspect(&measured,&crate::script_quality::plan(&measured),&s,true);
        if measured.check_duration().is_ok() && (!crate::us_editorial::enabled(&measured.rules)||editorial.issues.is_empty()){break;}
        let state = crate::duration_recovery::Recovery::load(&p, &s)?;
        let mismatch = crate::duration_recovery::Mismatch::from_project(&measured);
        if mismatch.as_ref().is_some_and(|m|m.shortfall()>0 && m.shortfall()<=crate::duration_recovery::SMALL_SHORTFALL_MS) {
            // First surface the typed failure to the same checkpoint retry path
            // used by the UI. On recovery, try real source time before asking AI.
            if state.auto_attempts == 0 { break; }
            if crate::duration_recovery::extend_short_source(&mut measured)? {
                measured.composition=p.composition.take();p=measured;checkpoint(&p,&tx)?;
                stage(&tx,"Phục hồi thời lượng","Đã bổ sung footage hợp lệ; giữ nguyên giọng và Độ dài đã đặt",false);
                break;
            }
        }
        if p.sources.is_empty() { break; }
        // Give AI the measured source clock, including hook/intercut speed,
        // rather than the source intervals' previous unretimed estimates.
        measured.composition=p.composition.take();p=measured;
        checkpoint(&p,&tx)?;
        let Some(round) = crate::duration_recovery::Recovery::reserve_ai(&p, &s)? else { break; };
        stage(&tx,"Audio",&format!("Đang sửa lượng lời theo audio thật ({round}/2), giữ WAV không đổi"),false);
        crate::diagnostics::record(&p.root,"Audio",None,"duration_repair",serde_json::json!({"round":round,"measured_ms":p.duration_ms()}))?;
        write_json(&p.root.join("analysis/duration-backups").join(format!("before-{round}.json")),&p)?;
        match providers::repair_duration(&p,&s,&cancel,round) {
            Ok(plan)=> {
                apply_plan(&mut p,plan)?;
                checkpoint(&p,&tx)?;
                p=generate_speech(p,s.clone(),tx.clone(),cancel.clone())?;
            },
            Err(e)=> {
                process::check_cancel(&cancel)?;
                crate::diagnostics::failure(&p.root,"Duration",None,&e)?;
                if !providers::retryable_timing_repair(&e) {
                    return Err(e.context("Không sửa được lời theo thời lượng; WAV và kịch bản trước sửa được giữ lại"));
                }
            }
        }
    }
    if crate::footage_speed::enabled(&p)&&p.segments.iter().any(|v|v.kind=="review"&&!v.footage_anchors.is_empty()&&v.timing_estimated) {
        stage(&tx,"Tốc độ footage","Căn từ trên WAV thật trước khi khớp mốc hành động",false);
        p=transcribe(p,s.clone(),worker_path("transcribe.py"),tx.clone(),cancel.clone())?;
    }
    // Validate measured voice and footage before entering either avatar provider.
    let mut fitted = p.clone();
    fitted.composition=None;
    let mut footage_changes = Vec::new();
    let validation = (|| -> Result<()> {
        if crate::footage_speed::enabled(&fitted) {
            for message in crate::footage_speed::fit(&mut fitted)? {stage(&tx,"Tốc độ footage",&message,false);}
        }
        fit_budget(&mut fitted)?;
        footage_changes = fit_review_footage(&mut fitted)?;
        fitted.validate(true)?;
        fitted.validate_intercuts()?;
        if crate::us_editorial::enabled(&fitted.rules) {
            let plan=crate::script_quality::plan(&fitted);
            let mut report=crate::script_quality::inspect(&fitted,&plan,&s,true);
            crate::us_editorial::review(&fitted,&s,&plan,&cancel,&mut report)?;
            write_json(&fitted.root.join("analysis/validation-report.json"),&report)?;
            ensure!(report.issues.is_empty(),"Kiểm tra kịch bản US sau WAV chưa đạt: {}",report.message());
        }
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
            "asr-v4:{}:{}:{:?}:{}:{}:{}:{}:{}",
            input.display(),
            meta.len(),
            meta.modified().ok(),
            start,
            seg.duration_ms,
            language,
            s.asr_model,seg.source_rate
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
            "-af".into(),if seg.kind=="source"&&(seg.source_rate-1.).abs()>0.000001 {format!("atempo={}",seg.source_rate)}else{"anull".into()},
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
            "--compute-type".into(),
            s.asr_compute_type.clone(),
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
        if let Some(summary) = crate::asr::summary(output) { let _ = tx.send(Event::Log(summary)); }
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
        crate::footage_speed::align_measured(seg,&words);
        let text=if !seg.caption.trim().is_empty() { &seg.caption } else { &seg.narration };
        if !text.trim().is_empty() {
            let (aligned,estimated)=subtitle::align_text(text,&words,seg.duration_ms);
            seg.words=aligned;seg.timing_estimated=estimated;
        } else {
            seg.words=words;seg.timing_estimated=false;
        }
        p.save()?;
    }
    if crate::footage_speed::enabled(&p) {for message in crate::footage_speed::fit(&mut p)? {stage(&tx,"Tốc độ footage",&message,false);}}
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

#[cfg(test)]
mod evidence_merge_tests {
    use super::*;
    #[test]
    fn generated_host_photo_survives_worker_merge_but_not_later_manual_edits() {
        let p=Project{sources:vec![Source{id:"src".into(),duration_ms:10000,..Default::default()}],
            segments:vec![Segment{id:"host".into(),kind:"host".into(),source_id:"src".into(),out_ms:9000,
                duration_ms:5000,narration:"Officers approach the doorway.".into(),..Default::default()}],..Default::default()};
        let mut worker=p.clone();assert_eq!(crate::host_evidence::seed_missing(&mut worker),1);
        let key=crate::evidence_selection::request_key(&worker,&worker.segments[0],&worker.segments[0].evidence_items[0]);
        let card=&mut worker.segments[0].evidence_items[0];card.selection.key=key;card.selection.status="accepted".into();card.asset=Some("verified.png".into());
        let mut merged=p.clone();merge_evidence(&mut merged,&worker,&p);
        assert_eq!(merged.segments[0].evidence_items.len(),1);
        merge_evidence(&mut merged,&worker,&p);assert_eq!(merged.segments[0].evidence_items.len(),1);
        for edit in 0..4 {
            let mut changed=p.clone();match edit {
                0=>changed.segments[0].narration.push_str(" edited"),
                1=>changed.segments[0].locked=true,
                2=>changed.timeline.tracks[crate::timeline::EVIDENCE].locked=true,
                _=>changed.segments[0].evidence_items.push(Default::default()),
            }
            let count=changed.segments[0].evidence_items.len();merge_evidence(&mut changed,&worker,&p);
            assert_eq!(changed.segments[0].evidence_items.len(),count,"stale worker appended over edit {edit}");
        }
    }
    #[test]
    fn stale_same_id_worker_cannot_overwrite_new_claim_and_preserves_manual_timing() {
        let mut p=Project {segments:vec![Segment{id:"scene".into(),narration:"original review".into(),duration_ms:5000,
            evidence_items:vec![crate::evidence::EvidenceItem{id:"card".into(),claim:"original subject".into(),
                start_ms:1000,duration_ms:2500,timing_manual:true,..Default::default()}],..Default::default()}],..Default::default()};
        let mut worker=p.clone();
        let baseline=p.clone();
        let key=crate::evidence_selection::request_key(&p,&p.segments[0],&p.segments[0].evidence_items[0]);
        let card=&mut worker.segments[0].evidence_items[0];
        card.selection.request_key=key;
        card.crop=[0.,0.,1.,1.];card.asset=Some("verified.png".into());
        p.segments[0].evidence_items[0].claim="new subject using the same id".into();
        merge_evidence(&mut p,&worker,&baseline);
        assert!(p.segments[0].evidence_items[0].asset.is_none());
        p.segments[0].evidence_items[0].claim="original subject".into();
        p.segments[0].evidence_items[0].start_ms=1500;
        merge_evidence(&mut p,&worker,&baseline);
        let item=&p.segments[0].evidence_items[0];
        assert_eq!(item.asset.as_deref(),Some(std::path::Path::new("verified.png")));
        assert_eq!(item.start_ms,1500);assert_eq!(item.duration_ms,2500);assert!(item.timing_manual);
        assert_eq!(p.segments[0].duration_ms,5000);
        for edit in 0..3 {
            let mut changed=baseline.clone();
            match edit {
                0=>changed.segments[0].evidence_items[0].enabled=false,
                1=>changed.segments[0].locked=true,
                _=>changed.segments[0].evidence_items[0].annotations.push(Default::default()),
            }
            merge_evidence(&mut changed,&worker,&baseline);
            assert!(changed.segments[0].evidence_items[0].asset.is_none(),"worker overwrote an edit made after it started");
        }
    }
}
