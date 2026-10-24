//! One automatic job, persisted between stages; the UI never launches subjobs.
use crate::{
    diagnostics, media,
    model::*,
    pipeline::{self, Event},
    process::{self, Cancel},
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::Path,
    sync::{atomic::Ordering, mpsc::Sender},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Prepare,
    Assets,
    Captions,
    Ready,
}
impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Prepare => "Nguồn & kịch bản",
            Self::Assets => "Giọng & nhân vật",
            Self::Captions => "Phụ đề",
            Self::Ready => "Sẵn sàng biên tập",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Running,
    Failed,
    Cancelled,
    Complete,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobState {
    pub id: String,
    pub project_id: String,
    pub fingerprint: String,
    #[serde(default)]
    pub stage_keys: StageKeys,
    pub phase: Phase,
    pub status: Status,
    pub message: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StageKeys {
    pub prepare: String,
    pub assets: String,
    pub captions: String,
}
impl StageKeys {
    fn changed_phase(&self,other:&Self)->Phase {
        if self.prepare!=other.prepare {Phase::Prepare}
        else if self.assets!=other.assets {Phase::Assets}
        else if self.captions!=other.captions {Phase::Captions}
        else {Phase::Ready}
    }
}
fn stage_keys(p:&Project,s:&Settings)->StageKeys {
    let sources:Vec<_>=p.sources.iter().map(|v|(&v.id,if v.url.is_empty(){
        let m=fs::metadata(&v.path).ok();format!("{}:{:?}:{:?}",v.path.display(),m.as_ref().map(|m|m.len()),m.and_then(|m|m.modified().ok()))
    }else{v.url.clone()})).collect();
    let prepare=media::hash(serde_json::to_vec(&(crate::script_quality::VERSION,sources,&p.input_urls,&p.rules,&p.evidence_settings,
        (&s.provider,&s.cli_path,&s.cli_args,&s.cli_model,&s.analysis_input,&s.gemini_base,&s.gemini_model,
        s.visual_sample_seconds,&s.asr_model,&s.asr_language))).unwrap());
    let assets=media::hash(serde_json::to_vec(&("assets-final-evidence-v1",&prepare,&s.vieneu_url,&s.voice,s.voice_reference_enabled,s.voice_reference_denoise,
        crate::input_assets::file_fingerprint(&s.voice_reference_path),s.avatar_enabled,&s.avatar_provider,&s.cartoon_character,
        &s.heygen_base,&s.avatar_id,&s.avatar_image_url,crate::input_assets::file_fingerprint(&s.avatar_image_path))).unwrap());
    let align=p.production.as_ref().is_none_or(|v|v.align_subtitles);
    let captions=media::hash(serde_json::to_vec(&(&assets,&p.subtitles,align,&s.asr_model,&s.asr_language)).unwrap());
    StageKeys{prepare,assets,captions}
}
impl JobState {
    pub fn load(root: &Path) -> Option<Self> {
        serde_json::from_slice(&fs::read(root.join("workflow.json")).ok()?).ok()
    }
    fn publish(&self, root: &Path, tx: &Sender<Event>) -> Result<()> {
        write_json(&root.join("workflow.json"), self)?;
        let _ = tx.send(Event::Workflow(self.clone()));
        Ok(())
    }
    pub fn can_resume(&self) -> bool {
        self.status != Status::Complete
    }
}
pub fn effective_settings(p: &Project, s: &Settings) -> Settings {
    p.production
        .as_ref()
        .map(|v| v.apply(s))
        .unwrap_or_else(|| s.clone())
}
fn fingerprint(p: &Project, s: &Settings) -> String {
    let s = Settings {preview_long_edge:0,youtube_download_folder:String::new(),..s.clone()};
    let sources: Vec<_> = p
        .sources
        .iter()
        .map(|v| {
            (
                &v.id,
                if v.url.is_empty() {
                    let metadata = fs::metadata(&v.path).ok();
                    format!("{}:{:?}:{:?}", v.path.display(), metadata.as_ref().map(|m| m.len()), metadata.and_then(|m| m.modified().ok()))
                } else {
                    v.url.clone()
                },
            )
        })
        .collect();
    media::hash(
        serde_json::to_vec(&(
            sources,
            &p.input_urls,
            &p.rules,
            &p.output,
            &p.subtitles,
            &p.evidence_settings,
            &p.production,
            &s,
            crate::input_assets::file_fingerprint(&s.voice_reference_path),
            crate::input_assets::file_fingerprint(&s.avatar_image_path),
        ))
        .unwrap(),
    )
}
pub fn preflight(p: &Project, s: &Settings) -> Result<()> {
    p.validate_configuration()?;
    if crate::download_location::needs_download(p) {crate::download_location::validate(s)?;}
    ensure!(crate::content_language::supported(&p.rules.language), "Ngôn ngữ lời review chưa hỗ trợ; chọn Tiếng Việt hoặc English");
    ensure!(
        !p.sources.is_empty(),
        "Thêm ít nhất một URL hoặc video từ máy."
    );
    ensure!(
        p.rules.min_seconds > 0
            && p.rules.max_seconds >= p.rules.min_seconds
            && p.rules.max_seconds <= 7200,
        "Độ dài tối thiểu/tối đa không hợp lệ."
    );
    ensure!(
        !p.rules.prompt.trim().is_empty(),
        "Nhập prompt cho kịch bản."
    );
    for src in &p.sources {
        ensure!(
            src.path.is_file() || !src.url.is_empty(),
            "Nguồn {} không còn trên máy.",
            src.id
        );
        if !src.url.is_empty() {
            validate_youtube_url(&src.url)?;
        }
    }
    for range in p
        .rules
        .required_ranges
        .iter()
        .chain(&p.rules.excluded_ranges)
    {
        let src = p.source(&range.source_id)?;
        ensure!(
            range.start_ms < range.end_ms
                && (src.duration_ms == 0 || range.end_ms <= src.duration_ms),
            "Vùng giữ/loại vượt nguồn {}.",
            src.id
        );
    }
    ensure!(
        !p.rules
            .required_ranges
            .iter()
            .any(|r| p
                .rules
                .excluded_ranges
                .iter()
                .any(|x| r.source_id == x.source_id
                    && r.start_ms < x.end_ms
                    && x.start_ms < r.end_ms)),
        "Vùng bắt buộc giữ và loại bỏ đang chồng lên nhau."
    );
    if s.provider == "gemini" {
        ensure!(p.sources.iter().all(|src|!src.url.is_empty()),"Gemini URL cần nguồn YouTube; chọn Antigravity để phân tích video từ máy.");
        ensure!(
            crate::providers::environment_key(&s.gemini_key_env).is_some(),
            "Chưa cấu hình Gemini API key trong Cài đặt kết nối."
        );
    }
    ensure!(
        matches!(s.provider.as_str(), "gemini" | "antigravity"),
        "Chọn Gemini hoặc Antigravity trong Cài đặt kết nối."
    );
    if s.voice_reference_enabled {
        ensure!(
            Path::new(&s.voice_reference_path).is_file(),
            "Audio tham chiếu không còn tồn tại. Chọn lại trong Giọng & nhân vật."
        );
    }
    if s.avatar_enabled {
        ensure!(
            matches!(s.avatar_provider.as_str(), "cartoon2d" | "heygen"),
            "Chọn nhân vật 2D hoặc HeyGen."
        );
        if s.avatar_provider == "heygen" {
            ensure!(
                crate::providers::environment_key(&s.heygen_key_env).is_some(),
                "HeyGen chưa có key. Cấu hình key hoặc chọn 2D local."
            );
            ensure!(
                !s.avatar_id.trim().is_empty()
                    || !s.avatar_image_url.trim().is_empty()
                    || Path::new(&s.avatar_image_path).is_file(),
                "HeyGen cần ảnh nhân vật hoặc avatar ID."
            );
        }
    }
    Ok(())
}

pub fn run(
    p: Project,
    settings: Settings,
    tx: Sender<Event>,
    cancel: Cancel,
    resume: bool,
) -> Result<Project> {
    let s = effective_settings(&p, &settings);
    preflight(&p, &s)?;
    run_stages(
        p,
        s,
        tx,
        cancel,
        resume,
        |phase, p, s, tx, cancel| match phase {
            Phase::Prepare => pipeline::prepare(p, s.clone(), tx.clone(), cancel.clone()),
            Phase::Assets => pipeline::generate_assets(p, s.clone(), tx.clone(), cancel.clone()),
            Phase::Captions => {
                if p.subtitles.enabled && p.production.as_ref().is_none_or(|v| v.align_subtitles) {
                    pipeline::transcribe(
                        p,
                        s.clone(),
                        worker_path("transcribe.py"),
                        tx.clone(),
                        cancel.clone(),
                    )
                } else {
                    Ok(p)
                }
            }
            Phase::Ready => Ok(p),
        },
    )
}
/// Windows denies a second writer even after a crash; the OS releases this handle.
struct JobLease { _file:fs::File, #[cfg(not(windows))] path:std::path::PathBuf }
impl JobLease {
    fn acquire(root:&Path)->Result<Self> {
        fs::create_dir_all(root)?;
        let path=root.join("workflow.lock");
        let mut options=fs::OpenOptions::new();options.read(true).write(true);
        #[cfg(windows)] {use std::os::windows::fs::OpenOptionsExt;options.create(true).share_mode(0);}
        #[cfg(not(windows))] {options.create_new(true);}
        let file=options.open(&path).context("Dự án đang có một quy trình khác xử lý; chờ hoặc hủy tác vụ đó trước")?;
        Ok(Self{_file:file,#[cfg(not(windows))] path})
    }
}
#[cfg(not(windows))]
impl Drop for JobLease {fn drop(&mut self){let _=fs::remove_file(&self.path);}}
fn run_stages(
    mut p: Project,
    s: Settings,
    tx: Sender<Event>,
    cancel: Cancel,
    resume: bool,
    mut perform: impl FnMut(Phase, Project, &Settings, &Sender<Event>, &Cancel) -> Result<Project>,
) -> Result<Project> {
    let _lease=JobLease::acquire(&p.root)?;
    let key = fingerprint(&p, &s);
    let keys=stage_keys(&p,&s);
    let mut job = if resume {
        let job = JobState::load(&p.root)
            .context("Chưa có tác vụ để tiếp tục. Bấm Tạo review tự động.")?;
        ensure!(job.project_id==p.id,"Checkpoint không thuộc dự án này");
        let mut job=job;
        if job.stage_keys.prepare.is_empty() {
            ensure!(job.fingerprint==key,"Thiết lập hoặc nguồn đã thay đổi; bấm Tạo review tự động để chạy theo cấu hình mới.");
            ensure!(job.can_resume(), "Tác vụ đã hoàn tất.");
        }else{
            let changed=job.stage_keys.changed_phase(&keys);
            ensure!(job.can_resume() || changed!=Phase::Ready,"Tác vụ đã hoàn tất.");
            job.phase=job.phase.min(changed);
        }
        job.fingerprint=key;job.stage_keys=keys.clone();job
    } else {
        let phase=JobState::load(&p.root).filter(|j|j.project_id==p.id&&!j.stage_keys.prepare.is_empty())
            .map(|j| {let changed=j.stage_keys.changed_phase(&keys);
                if changed==Phase::Ready {Phase::Prepare}else{j.phase.min(changed)}}).unwrap_or(Phase::Prepare);
        JobState {
            id: uuid::Uuid::new_v4().to_string(),
            project_id: p.id.clone(),
            fingerprint: key,
            stage_keys: keys,
            phase,
            status: Status::Running,
            message: String::new(),
        }
    };
    job.status = Status::Running;
    job.message = job.phase.label().into();
    job.publish(&p.root, &tx)?;
    let root = p.root.clone();
    let result = (|| -> Result<Project> {
        loop {
            process::check_cancel(&cancel)?;
            if job.phase == Phase::Ready {
                break;
            }
            let phase = job.phase;
            job.message = phase.label().into();
            job.publish(&root, &tx)?;
            p = perform(phase, p, &s, &tx, &cancel)
                .with_context(|| format!("Quy trình tự động / {}", phase.label()))?;
            process::check_cancel(&cancel)?;
            if phase == Phase::Prepare {
                p.validate(true)?;
                p.check_duration()?;
                p.validate_opening_hook()?;
                ensure!(
                    p.segments
                        .iter()
                        .any(|v| v.kind != "source" && !v.narration.trim().is_empty()),
                    "AI cần trả ít nhất một cảnh có lời review."
                );
            }
            p.save()?;
            let _ = tx.send(Event::Project(Box::new(p.clone())));
            job.phase = match phase {
                Phase::Prepare => Phase::Assets,
                Phase::Assets => Phase::Captions,
                _ => Phase::Ready,
            };
            job.publish(&root, &tx)?;
        }
        if let Err(error)=crate::evidence::validate_prepared(&p) {
            job.phase=Phase::Assets;
            return Err(error.context("Dẫn chứng chưa sẵn sàng; tiếp tục để cập nhật ảnh theo kịch bản cuối"));
        }
        p.status = "Sẵn sàng biên tập / xuất MP4".into();
        p.save()?;
        job.status = Status::Complete;
        job.message = if p.script_warnings.is_empty(){"Kịch bản, giọng và nhân vật đã sẵn sàng".into()}
            else{format!("Sẵn sàng với {} cảnh báo dẫn chứng; xem chi tiết bên dưới",p.script_warnings.len())};
        job.publish(&root, &tx)?;
        Ok(p)
    })();
    if let Err(error) = &result {
        job.status = if cancel.load(Ordering::Relaxed) {
            Status::Cancelled
        } else {
            Status::Failed
        };
        job.message = diagnostics::failure_text(&format!("{error:#}"));
        let _ = job.publish(&root, &tx);
        if let Ok(saved) = Project::load(&root.join("project.json")) {
            let _ = tx.send(Event::Project(Box::new(saved)));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn project(root: &Path) -> Project {
        fs::write(root.join("source.mp4"), b"test placeholder").unwrap();
        let mut p = Project {
            root: root.into(),
            sources: vec![Source {
                id: "src_1".into(),
                path: root.join("source.mp4"),
                duration_ms: 10000,
                ..Default::default()
            }],
            ..Default::default()
        };
        p.rules.min_seconds = 1;
        p.rules.max_seconds = 20;
        p.rules.hook_enabled = false;
        p
    }
    fn plan(mut p: Project) -> Project {
        p.segments = vec![Segment {
            kind: "review".into(),
            source_id: "src_1".into(),
            out_ms: 3000,
            duration_ms: 3000,
            narration: "Lời bình kiểm thử".into(),
            ..Default::default()
        }];
        p
    }
    #[test]
    fn resume_starts_at_failed_phase_keeps_partial_media_and_rejects_changed_configuration() {
        let temp = tempfile::tempdir().unwrap();
        let p = project(temp.path());
        let s = Settings::default();
        let (tx, _) = std::sync::mpsc::channel();
        let mut phases = vec![];
        let result = run_stages(
            p,
            s.clone(),
            tx.clone(),
            process::cancel_token(),
            false,
            |phase, p, _, _, _| {
                phases.push(phase);
                if phase == Phase::Prepare {
                    return Ok(plan(p));
                }
                let mut p = p;
                let audio = p.root.join("partial.wav");
                fs::write(&audio, b"saved voice").unwrap();
                p.segments[0].audio = Some(audio);
                p.save()?;
                anyhow::bail!("Intentional avatar failure")
            },
        );
        assert!(result.is_err());
        assert_eq!(phases, vec![Phase::Prepare, Phase::Assets]);
        let saved = Project::load(&temp.path().join("project.json")).unwrap();
        let failed = JobState::load(temp.path()).unwrap();
        assert_eq!(failed.phase, Phase::Assets);
        assert_eq!(failed.status, Status::Failed);
        let mut changed = saved.clone();
        changed.rules.prompt.push_str(" changed");
        assert!(run_stages(
            changed,
            s.clone(),
            tx.clone(),
            process::cancel_token(),
            true,
            |phase, _, _, _, _| {assert_eq!(phase,Phase::Prepare);anyhow::bail!("Test stops after selecting the invalidated stage")}
        )
        .is_err());
        failed.publish(temp.path(),&tx).unwrap();
        let mut changed=saved.clone();
        changed.input_urls="https://www.youtube.com/watch?v=dQw4w9WgXcQ".into();
        assert!(run_stages(changed,s.clone(),tx.clone(),process::cancel_token(),true,
            |phase,_,_,_,_|{assert_eq!(phase,Phase::Prepare);anyhow::bail!("Test stops before source work")}).is_err());
        failed.publish(temp.path(),&tx).unwrap();
        phases.clear();
        let done = run_stages(
            saved,
            s,
            tx,
            process::cancel_token(),
            true,
            |phase, p, _, _, _| {
                phases.push(phase);
                assert!(p.segments[0].audio.as_ref().unwrap().is_file());
                Ok(p)
            },
        )
        .unwrap();
        assert_eq!(phases, vec![Phase::Assets, Phase::Captions]);
        assert_eq!(done.segments[0].narration, "Lời bình kiểm thử");
        let ready = JobState::load(temp.path()).unwrap();
        assert_eq!(ready.id, failed.id);
        assert_eq!(ready.status, Status::Complete);
    }
    #[test]
    fn stage_keys_only_invalidate_dependent_work_and_lease_blocks_other_writers() {
        let temp=tempfile::tempdir().unwrap();let p=project(temp.path());let s=Settings::default();
        let keys=stage_keys(&p,&s);
        let mut changed=p.clone();changed.subtitles.size+=2.;
        assert_eq!(keys.changed_phase(&stage_keys(&changed,&s)),Phase::Captions);
        changed=p.clone();changed.output.portrait= !changed.output.portrait;
        assert_eq!(keys.changed_phase(&stage_keys(&changed,&s)),Phase::Ready);
        changed=p.clone();changed.rules.min_seconds+=1;
        assert_eq!(keys.changed_phase(&stage_keys(&changed,&s)),Phase::Prepare);
        let voice=Settings{voice:"Another voice".into(),..s.clone()};
        assert_eq!(keys.changed_phase(&stage_keys(&p,&voice)),Phase::Assets);
        let lease=JobLease::acquire(temp.path()).unwrap();
        assert!(JobLease::acquire(temp.path()).is_err());drop(lease);
        assert!(JobLease::acquire(temp.path()).is_ok());
    }
    #[test]
    fn subtitle_only_change_resumes_at_captions_after_completed_job() {
        let temp=tempfile::tempdir().unwrap();let p=project(temp.path());let s=Settings::default();
        let (tx,_)=std::sync::mpsc::channel();
        let mut p=run_stages(p,s.clone(),tx.clone(),process::cancel_token(),false,
            |phase,p,_,_,_|Ok(if phase==Phase::Prepare{plan(p)}else{p})).unwrap();
        p.subtitles.size+=4.;let mut phases=vec![];
        run_stages(p,s,tx,process::cancel_token(),false,|phase,p,_,_,_|{phases.push(phase);Ok(p)}).unwrap();
        assert_eq!(phases,vec![Phase::Captions]);
    }
    #[test]
    fn missing_final_evidence_fails_completion_and_resumes_at_assets() {
        let temp=tempfile::tempdir().unwrap();let p=project(temp.path());let s=Settings::default();
        let (tx,_)=std::sync::mpsc::channel();
        let result=run_stages(p,s.clone(),tx.clone(),process::cancel_token(),false,|phase,p,_,_,_| {
            let mut p=if phase==Phase::Prepare{plan(p)}else{p};
            if phase==Phase::Assets {
                p.segments[0].evidence_items.push(crate::evidence::EvidenceItem{source_id:"src_1".into(),..Default::default()});
            }
            Ok(p)
        });
        assert!(result.is_err());
        let state=JobState::load(temp.path()).unwrap();
        assert_eq!(state.status,Status::Failed);assert_eq!(state.phase,Phase::Assets);
        let saved=Project::load(&temp.path().join("project.json")).unwrap();
        assert!(run_stages(saved,s,tx,process::cancel_token(),true,|phase,_,_,_,_| {
            assert_eq!(phase,Phase::Assets);anyhow::bail!("Stop before fixture media processing")
        }).is_err());
    }
    #[test]
    fn cancellation_does_not_advance_to_assets_and_can_resume_after_restart() {
        let temp = tempfile::tempdir().unwrap();
        let p = project(temp.path());
        let (tx, _) = std::sync::mpsc::channel();
        let cancel = process::cancel_token();
        let result = run_stages(
            p,
            Settings::default(),
            tx.clone(),
            cancel.clone(),
            false,
            |phase, p, _, _, cancel| {
                assert_eq!(phase, Phase::Prepare);
                let p = plan(p);
                p.save()?;
                cancel.store(true, Ordering::Relaxed);
                Ok(p)
            },
        );
        assert!(result.is_err());
        let job = JobState::load(temp.path()).unwrap();
        assert_eq!(job.phase, Phase::Prepare);
        assert_eq!(job.status, Status::Cancelled);
        let saved = Project::load(&temp.path().join("project.json")).unwrap();
        assert!(run_stages(
            saved,
            Settings::default(),
            tx,
            process::cancel_token(),
            true,
            |_, p, _, _, _| Ok(p)
        )
        .is_ok());
    }
    #[test]
    fn project_choices_override_global_defaults_but_keep_connection_settings() {
        let mut p = Project::default();
        let s = Settings {
            voice: "Old voice".into(),
            avatar_provider: "cartoon2d".into(),
            avatar_enabled: true,
            ..Default::default()
        };
        p.production = Some(ProductionSettings::from_settings(&s));
        p.input_urls = "do not inherit".into();
        let globals = Settings {
            voice: "New voice".into(),
            avatar_provider: "heygen".into(),
            vieneu_url: "http://127.0.0.1:5555/speech".into(),
            ..s
        };
        let effective = effective_settings(&p, &globals);
        let relocated=Settings{youtube_download_folder:"D:\\YouTube".into(),..globals.clone()};
        assert_eq!(stage_keys(&p,&globals).changed_phase(&stage_keys(&p,&relocated)),Phase::Ready);
        assert_eq!(fingerprint(&p,&globals),fingerprint(&p,&relocated),"Changing storage must not repeat AI/TTS work");
        assert_eq!(effective.voice, "Old voice");
        assert_eq!(effective.avatar_provider, "cartoon2d");
        assert_eq!(effective.vieneu_url, globals.vieneu_url);
        let mut next = Project::default();
        ProjectDefaults::from_project(&p).apply(&mut next);
        assert!(next.input_urls.is_empty());
        assert!(next.sources.is_empty());
        assert_eq!(next.production.unwrap().voice, "Old voice");
        let mut legacy = serde_json::to_value(&p).unwrap();
        legacy.as_object_mut().unwrap().remove("production");
        legacy.as_object_mut().unwrap().remove("input_urls");
        let legacy: Project = serde_json::from_value(legacy).unwrap();
        assert!(legacy.production.is_none());
        assert_eq!(effective_settings(&legacy, &globals).voice, "New voice");
    }
    #[test]
    fn relative_project_path_becomes_absolute_before_cli_working_directory_changes() {
        let temp=tempfile::tempdir_in(".").unwrap();
        let mut p=project(temp.path());
        p.root=temp.path().into();p.save().unwrap();
        let loaded=Project::load(&temp.path().join("project.json")).unwrap();
        assert!(loaded.root.is_absolute());
        assert!(loaded.root.join("project.json").is_file());
    }
}
