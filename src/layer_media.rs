//! Shared decode/export graph for independent clips. Preview outputs raw RGBA,
//! never encoded preview movies. ASS overlays use the same libass as export.
use crate::{composition::*, model::*, process::{self,Cancel}, timeline};
use anyhow::{Context,Result};
use std::{fs,path::{Path,PathBuf},time::{Duration,Instant}};

const RENDERER: &str = "composition-v8-footage-speed";

fn visual_output(p:&Project)->OutputSettings {
    let mut output=p.output.clone();
    output.original_audio_enabled=false;
    output.original_audio_gain=0.15;
    output.footage_speed=Default::default();
    output
}

#[derive(Default,serde::Serialize)]
struct RenderReport {
    renderer: &'static str, status: &'static str, total_ms: u128,
    video_ms: u128, audio_ms: u128, mux_ms: u128,
    intervals: usize, cache_hits: usize, encoded_frames: u64,
    chunks: Vec<serde_json::Value>,
}

/// Cut only at visible picture changes. Audio has its own continuous clock.
/// Align cuts to output frames so concatenation cannot accumulate rounding
/// errors at short evidence/annotation boundaries.
fn intervals(p:&Project,c:&Composition,end:u64)->Vec<(u64,u64,u64)> {
    let fps=p.output.fps.max(1) as u64;
    let frame=|ms:u64|ms.saturating_mul(fps).div_ceil(1000);
    let last=frame(end);
    let mut cuts=vec![0,last];
    for clip in c.clips.iter().filter(|c|!c.audio()&&c.enabled(p)) {
        cuts.extend([frame(clip.start_ms.min(end)),frame(clip.end().min(end))]);
    }
    cuts.extend((30*fps..last).step_by((30*fps) as usize));
    cuts.sort_unstable();cuts.dedup();
    let ms=|f:u64|(f*1000+fps/2)/fps;
    cuts.windows(2).filter(|v|v[1]>v[0]).map(|v|(ms(v[0]),ms(v[1]),v[1]-v[0])).collect()
}

pub fn ass_file(p:&Project,c:&LayerClip)->Result<PathBuf> {
    let seg=p.segments.iter().find(|s|s.id==c.scene_id).context("Layer mất cảnh nguồn")?;
    let mut sub=p.clone();sub.composition=None;sub.segments=vec![seg.clone()];
    let key=crate::media::hash(serde_json::to_vec(&(&sub.segments,&sub.subtitles,visual_output(&sub))).unwrap_or_default());
    let dir=p.root.join("cache/layer-overlays").join(key);fs::create_dir_all(&dir)?;
    let file=dir.join(if c.track==timeline::CAPTIONS {"captions.ass"} else {"graphics.ass"});
    if !file.is_file() {
        crate::subtitle::write(&sub,&dir)?;crate::motion::write(&sub,&dir)?;
    }
    Ok(file)
}
fn filter_path(path:&Path)->String {
    path.to_string_lossy().trim_start_matches("\\\\?\\").replace('\\',"/").replace(':',"\\:").replace('\'',"'\\''")
}
/// Input is seeked in source time; output PTS starts at zero.
pub fn visual_input(p:&Project,c:&LayerClip,local:u64,w:u32,h:u32,fps:u32,proxy:bool)->Result<(Vec<String>,String)> {
    let start=c.source_at(local) as f64/1000.;
    let pts=c.pts();
    let effects=crate::visual_effects::filters(p,c)?;
    if let Some(media)=&c.media {
        let file=if proxy {c.proxy.as_ref().filter(|p|p.is_file()).unwrap_or(media)} else {media};
        let args=if c.still(){vec!["-loop".into(),"1".into(),"-framerate".into(),fps.to_string(),"-i".into(),file.to_string_lossy().into_owned()]}else{vec!["-ss".into(),format!("{start:.6}"),"-t".into(),format!("{:.6}",c.source_duration(c.duration_ms.saturating_sub(local)) as f64/1000.),"-i".into(),file.to_string_lossy().into_owned()]};
        let filter=if c.track==BACKGROUND&&!c.still() {
            let bw=(w/4).max(2);let bh=(h/4).max(2);
            let sigma=(12.*w as f32/p.output.dimensions().0 as f32).max(0.1);
            format!("{pts}{effects},scale={bw}:{bh}:force_original_aspect_ratio=increase,crop={bw}:{bh},gblur=sigma={sigma:.5},eq=brightness=-0.12,scale={w}:{h},setsar=1,fps={fps},format=rgba")
        } else {
            // Preserve aspect ratio; the compositor applies the saved geometry.
            format!("{pts}{effects},scale={w}:{h}:force_original_aspect_ratio=decrease,setsar=1,fps={fps},format=rgba")
        };
        Ok((args,filter))
    } else {
        let file=ass_file(p,c)?;
        let cue=(c.cue_in_ms+local) as f64/1000.;
        // libass line wrapping can change with raster resolution. Shape at the
        // output size first, then downsample the transparent plane for preview.
        let (cw,ch)=p.output.dimensions();
        let args=vec!["-f".into(),"lavfi".into(),"-i".into(),format!("color=c=black@0.0:s={cw}x{ch}:r={fps},format=rgba")];
        let filter=format!("setpts=PTS+{cue:.6}/TB,subtitles=filename='{}':alpha=1,{pts},scale={w}:{h},format=rgba",filter_path(&file));
        Ok((args,filter))
    }
}

pub fn audio_args(p:&Project,ms:u64,end:u64)->(Vec<String>,String) {
    let comp=p.composition_snapshot();let mut args=vec![];let mut filters=vec![];let mut n=0;
    for c in comp.clips.iter().filter(|c|c.audio()&&c.enabled(p)&&c.end()>ms&&c.start_ms<end) {
        let Some(path)=&c.media else {continue;};if !path.is_file(){continue;}
        let offset=ms.saturating_sub(c.start_ms);let remain=c.duration_ms-offset;
        args.extend(["-ss".into(),format!("{:.6}",c.source_at(offset) as f64/1000.),"-t".into(),format!("{:.6}",c.source_duration(remain) as f64/1000.),"-i".into(),path.to_string_lossy().into_owned()]);
        // Fade is evaluated in original clip time, including after a seek.
        let fi=c.fade_in_ms.max(1) as f64/1000.;let fo=c.fade_out_ms.max(1) as f64/1000.;let duration=c.duration_ms as f64/1000.;let off=offset as f64/1000.;
        let gain=c.effective_gain(p);
        let fade=if c.fade_in_ms>0 || c.fade_out_ms>0 {
            // A frame-level volume envelope zeros the entire first audio block
            // at t=0, swallowing the attack of short pop/ting sounds. Evaluate
            // fades per sample, with the original offset retained after seeking.
            format!(",aeval='val(ch)*{}*{}*{}':c=same",gain,
                if c.fade_in_ms>0 {format!("min(1,(t+{off})/{fi})")}else{"1".into()},
                if c.fade_out_ms>0 {format!("min(1,({duration}-t-{off})/{fo})")}else{"1".into()})
        } else {format!(",volume={gain}")};
        let delay=c.start_ms.saturating_sub(ms);
        filters.push(format!("[{n}:a]asetpts=PTS-STARTPTS,{},atrim=duration={:.6},aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo{fade},adelay={delay}:all=1[a{n}]",c.tempo(),remain.min(end-c.start_ms.max(ms)) as f64/1000.));n+=1;
    }
    if n==0 {
        args.extend(["-f".into(),"lavfi".into(),"-i".into(),"anullsrc=r=48000:cl=stereo".into()]);
        filters.push("[0:a]anull[mix]".into());
    }else{
        filters.push(format!("{}amix=inputs={n}:normalize=0,alimiter=limit=0.95:latency=1[mix]",(0..n).map(|i|format!("[a{i}]")).collect::<String>()));
    }
    (args,filters.join(";"))
}

/// Export bounded intervals. A long review never opens every source/alpha
/// decoder at once; unchanged intervals survive unrelated placement edits.
pub fn export(p:&Project,s:&Settings,output:&Path,cancel:&Cancel)->Result<()> {
    export_with_progress(p,s,output,cancel,|_,_|{})
}

pub fn export_with_progress<F:FnMut(usize,usize)>(p:&Project,s:&Settings,output:&Path,cancel:&Cancel,mut progress:F)->Result<()> {
    let clock=Instant::now();
    let mut report=RenderReport{renderer:RENDERER,status:"running",..Default::default()};
    let result=export_inner(p,s,output,cancel,&mut progress,&mut report);
    report.total_ms=clock.elapsed().as_millis();
    report.status=if result.is_ok(){"complete"}else if cancel.load(std::sync::atomic::Ordering::Relaxed){"cancelled"}else{"failed"};
    let folder=p.root.join("cache/composition-export");
    if fs::create_dir_all(&folder).is_ok(){let _=write_json(&folder.join("render-report.json"),&report);}
    result
}

pub(crate) fn validate_assets(p:&Project)->Result<()> {
    let composition=p.composition_snapshot();composition.validate()?;
    crate::evidence::validate(p)?;
    p.music.validate()?;
    for c in composition.clips.iter().filter(|c|c.enabled(p)&&c.track!=timeline::MUSIC) {
        if let Some(media)=&c.media {anyhow::ensure!(media.is_file(),"Thiếu media của layer '{}': {}",c.id,media.display());}
    }
    for seg in p.segments.iter().filter(|_|p.evidence_settings.enabled){for item in seg.evidence_items.iter().filter(|e|e.enabled){
        anyhow::ensure!(item.asset.as_ref().is_some_and(|a|p.root.join(a).is_file()),"Dẫn chứng '{}' chưa tạo thẻ; mở Dẫn chứng → Tạo thẻ / cập nhật",item.label);
    }}
    for c in composition.clips.iter().filter(|c|!c.audio()&&c.enabled(p)&&(c.evidence_id.is_some()||(c.track==BACKGROUND&&c.still()))){
        anyhow::ensure!(c.media.as_ref().is_some_and(|m|m.is_file()),"Thiếu ảnh dẫn chứng cho layer {}",c.id);
    }
    Ok(())
}

fn export_inner<F:FnMut(usize,usize)>(p:&Project,s:&Settings,output:&Path,cancel:&Cancel,progress:&mut F,report:&mut RenderReport)->Result<()> {
    crate::background_music::prepare(p,s,cancel)?;
    p.card_sound.validate()?;
    let metadata=crate::export_metadata::prepare(p,s)?;
    validate_assets(p)?;
    let composition=p.composition_snapshot();
    let end=composition.duration_ms();anyhow::ensure!(end>0,"Composition rỗng");
    let folder=p.root.join("cache/composition-chunks");fs::create_dir_all(&folder)?;
    let intervals=intervals(p,&composition,end);
    report.intervals=intervals.len();let total=intervals.len()+2;
    progress(0,total);
    let work_root=p.root.join("cache/composition-export");fs::create_dir_all(&work_root)?;
    // Private scratch files also make cancellation/retry and simultaneous
    // exports safe; a partial encode is never published as a cached chunk.
    let scratch=tempfile::Builder::new().prefix("run-").tempdir_in(&work_root)?;
    let work=scratch.path();
    let video_clock=Instant::now();
    let mut paths=vec![];
    for (i,&(a,b,frames)) in intervals.iter().enumerate() {
        process::check_cancel(cancel)?;let chunk_clock=Instant::now();
        let mut part=p.clone();let mut clips=vec![];
        let fps=p.output.fps.max(1) as u64;let first_frame=(a*fps+500)/1000;
        for c in &composition.clips {
            // A clip starting between the last sample and this interval's
            // end has no visible frame here, and must not invalidate its cache.
            if c.audio() || !c.enabled(p) || (c.start_ms*fps).div_ceil(1000)>=first_frame+frames || (c.end()*fps).div_ceil(1000)<=first_frame {continue;}
            let mut clip=c.clone();let skip=a.saturating_sub(c.start_ms);
            clip.start_ms=c.start_ms.saturating_sub(a);clip.media_in_ms=clip.source_at(skip);clip.cue_in_ms+=skip;
            clip.duration_ms=c.end().min(b)-c.start_ms.max(a);clips.push(clip);
        }
        // A hidden/empty interval still has a duration and must export as blank.
        if clips.is_empty() {clips.push(LayerClip{id:"blank".into(),scene_id:String::new(),track:BACKGROUND,start_ms:0,duration_ms:b-a,media_in_ms:0,playback_rate:1.,cue_in_ms:0,media:None,proxy:None,transform:Transform::default(),z:0,visible:false,locked:false,linked:false,gain:0.,fade_in_ms:0,fade_out_ms:0,evidence_id:None,group:None,motion:Default::default(),timing_manual:false});}
        // Hash only script cues consumed by this interval. A different evidence
        // card in the same scene must not invalidate an unrelated interval.
        let scenes:Vec<_>=clips.iter().filter_map(|c|p.segments.iter().find(|s|s.id==c.scene_id).map(|s|{
            match c.track {
                timeline::CAPTIONS=>serde_json::json!({"scene":s.id,"track":c.track,"words":s.words,"caption":s.caption,"narration":s.narration,"duration":s.duration_ms,"timing_estimated":s.timing_estimated}),
                timeline::GRAPHICS=>serde_json::json!({"scene":s.id,"track":c.track,"graphics":s.graphics}),
                timeline::VIDEO=>serde_json::json!({"scene":s.id,"track":c.track,"motion":s.motion,"duration":s.duration_ms}),
                _=>serde_json::Value::Null,
            }
        })).filter(|v|!v.is_null()).collect();
        let identities:Vec<_>=clips.iter().filter_map(|c|c.media.as_ref()).map(|path|{
            let m=fs::metadata(path).ok();(path,m.as_ref().map(|m|m.len()),m.and_then(|m|m.modified().ok()))
        }).collect();
        let effects:Vec<_>=clips.iter().map(|c|crate::visual_effects::identity(p,c)).collect();
        let key=crate::media::hash(serde_json::to_vec(&(clips.clone(),scenes,identities,effects,visual_output(p),&p.subtitles,&s.encoder,frames,RENDERER)).unwrap_or_default());
        part.composition=Some(Composition{version:4,script_key:Composition::key(p),clips,removed:vec![]});
        let path=folder.join(format!("{key}.mp4"));
        let receipt=path.with_extension("json");
        let cached=fs::metadata(&path).ok().filter(|m|m.len()>0).is_some_and(|m|
            fs::read(&receipt).ok().and_then(|v|serde_json::from_slice::<serde_json::Value>(&v).ok())
                .is_some_and(|v|v["bytes"].as_u64()==Some(m.len())&&v["frames"].as_u64()==Some(frames)));
        if cached {report.cache_hits+=1;} else {
            export_interval(&part,s,&path,cancel,b-a,frames,work)?;
            let pending_receipt=work.join("receipt.pending.json");
            fs::write(&pending_receipt,serde_json::to_vec(&serde_json::json!({"bytes":fs::metadata(&path)?.len(),"frames":frames}))?)?;
            fs::rename(pending_receipt,&receipt)?;
            report.encoded_frames+=frames;
        }
        report.chunks.push(serde_json::json!({"start_ms":a,"end_ms":b,"frames":frames,"cached":cached,"wall_ms":chunk_clock.elapsed().as_millis(),"file":path}));
        paths.push(path);
        progress(i+1,total);
    }
    report.video_ms=video_clock.elapsed().as_millis();
    let list=paths.iter().map(|path|format!("file '{}'\n",path.to_string_lossy().replace('\\',"/").replace('\'',"'\\''"))).collect::<String>();
    fs::write(work.join("concat.txt"),list)?;
    let audio_clock=Instant::now();
    let (mut audio,filter)=audio_args(p,0,end);audio.splice(0..0,["-y".into(),"-v".into(),"error".into(),"-filter_complex_threads".into(),"1".into()]);
    let wav=work.join("full-mix.pending.wav");
    audio.extend(["-filter_complex".into(),format!("{filter};[mix]apad=whole_dur={:.6}[fullmix]",end as f64/1000.),"-map".into(),"[fullmix]".into(),"-t".into(),format!("{:.6}",end as f64/1000.),"-c:a".into(),"pcm_s16le".into(),wav.to_string_lossy().into_owned()]);
    process::run(&s.ffmpeg,&audio,None,cancel,Duration::from_secs(s.timeout_seconds),None)?;
    report.audio_ms=audio_clock.elapsed().as_millis();progress(intervals.len()+1,total);
    let mux_clock=Instant::now();
    let pending=work.join("composition.pending.mp4");
    let mut mux_args=vec!["-y".into(),"-v".into(),"error".into(),"-f".into(),"concat".into(),"-safe".into(),"0".into(),"-i".into(),work.join("concat.txt").to_string_lossy().into_owned(),"-i".into(),wav.to_string_lossy().into_owned(),"-map".into(),"0:v:0".into(),"-map".into(),"1:a:0".into(),"-c:v".into(),"copy".into(),"-c:a".into(),"aac".into(),"-t".into(),format!("{:.6}",end as f64/1000.),"-movflags".into(),"+faststart".into()];
    mux_args.extend(crate::content_language::mux_args(p));
    mux_args.extend(metadata.args());mux_args.push(pending.to_string_lossy().into_owned());
    process::run(&s.ffmpeg,&mux_args,None,cancel,Duration::from_secs(s.timeout_seconds),None)?;
    let probe=crate::media::probe(s,&pending,cancel)?;
    anyhow::ensure!((probe.width,probe.height)==p.output.dimensions()&&probe.duration_ms.abs_diff(end)<=500,"Output composition sai kích thước/thời lượng");
    crate::export_metadata::finish(p,s,&pending,output,metadata,(mux_clock.elapsed().as_millis(),"render"),cancel)?;
    fs::remove_file(pending)?;fs::remove_file(wav)?;
    if let Some(parent)=output.parent(){
        crate::subtitle::write(p,parent)?;
        let sources:Vec<_>=p.sources.iter().map(|s|serde_json::json!({"source_id":s.id,"url":s.url,"title":s.title})).collect();
        write_json(&parent.join("sources.json"),&sources)?;
        let evidence:Vec<_>=p.segments.iter().map(|s|serde_json::json!({"scene_id":s.id,"items":s.evidence_items})).collect();
        write_json(&parent.join("evidence-sources.json"),&evidence)?;
    }
    report.mux_ms=mux_clock.elapsed().as_millis();progress(total,total);
    Ok(())
}
fn export_interval(p:&Project,s:&Settings,output:&Path,cancel:&Cancel,end:u64,frames:u64,work:&Path)->Result<()> {
    let c=p.composition_snapshot();c.validate()?;
    let (w,h)=p.output.dimensions();let fps=p.output.fps;
    let background=if p.output.editorial {"0xF4F0E6"}else{"0x0B1220"};
    let mut args=vec!["-y".into(),"-v".into(),"error".into(),"-filter_complex_threads".into(),"1".into(),"-f".into(),"lavfi".into(),"-i".into(),format!("color=c={background}:s={w}x{h}:r={fps}:d={:.6}",end as f64/1000.)];
    let mut layers:Vec<_>=c.clips.iter().filter(|c|!c.audio()&&c.enabled(p)).collect();layers.sort_by_key(|c|c.z);
    let mut graph=vec!["[0:v]format=rgba[base0]".into()];let mut n=0;
    let mut inputs:Vec<(Vec<String>,usize)>=vec![];
    for clip in layers {
        if clip.media.as_ref().is_some_and(|m|!m.is_file()){continue;}
        n+=1;
        // libass can paint ordinary captions straight onto the composed frame.
        // Avoid a full-size transparent decoder, scale and overlay per subtitle
        // layer. Edited geometry retains the general independent-layer path.
        if clip.track==timeline::CAPTIONS && clip.media.is_none()
            && clip.transform==Transform::default() && clip.motion==crate::evidence::Motion::default()
            && clip.start_ms==0 && clip.duration_ms*fps as u64>=(frames-1)*1000 {
            // Keep the original ASS grouping and karaoke clock at interval cuts.
            // Regenerating captions from trimmed words would change line breaks
            // and restart the highlight halfway through a spoken word.
            let file=ass_file(p,clip)?;let cue=clip.cue_in_ms as f64/1000.;
            graph.push(format!("[base{}]setpts=PTS+{cue:.6}/TB,subtitles=filename='{}':alpha=1,setpts=PTS-{cue:.6}/TB[base{n}]",n-1,filter_path(&file)));
            continue;
        }
        let t=&clip.transform;let tw=(t.width*w as f32).round().max(2.) as u32;let th=(t.height*h as f32).round().max(2.) as u32;
        let (input,filter)=if let Some(file)=clip.media.as_ref().filter(|_|clip.track!=BACKGROUND||clip.still()) {
            let input=if clip.still(){vec!["-framerate".into(),fps.to_string(),"-i".into(),file.to_string_lossy().into_owned()]}else{
                vec!["-ss".into(),format!("{:.6}",clip.media_in_ms as f64/1000.),"-i".into(),file.to_string_lossy().into_owned()]
            };
            // Raster at the saved layer size, rather than enlarging every
            // small card/avatar to the canvas and immediately shrinking it.
            (input,format!("{}{},scale={tw}:{th},setsar=1,fps={fps},format=rgba",clip.pts(),crate::visual_effects::filters(p,clip)?))
        }else{
            // ASS must still be shaped at full output size; blurred video
            // backgrounds retain their original fill/crop/blur semantics.
            let (input,filter)=visual_input(p,clip,0,w,h,fps,false)?;
            (input,format!("{filter},scale={tw}:{th},format=rgba"))
        };
        // Video and its blurred background use the same source and seek point.
        // Decode that stream once, then split into independent filter branches.
        let index=if let Some(i)=inputs.iter().position(|(a,_)|a==&input){i}else{
            args.extend(input.clone());inputs.push((input,0));inputs.len()-1
        };
        let branch=inputs[index].1;inputs[index].1+=1;let input_index=index+1;
        let mut transform=format!("colorchannelmixer=aa={:.6}",t.opacity);
        // Decode and resample a still only once. Animate the repeated RGBA
        // texture below; fades, zoom, blur and rotation remain seekable.
        if clip.still(){transform.push_str(&format!(",loop=loop=-1:size=1:start=0,setpts=N/({fps}*TB)"));}
        transform.push_str(&clip.motion.filters(clip.cue_in_ms));
        if clip.motion.backdrop_ms.is_some() {
            let at=clip.motion.backdrop_ms.unwrap() as f64/1000.;let cue=clip.cue_in_ms as f64/1000.;let sigma=8.*tw as f64/800.;
            transform.push_str(&format!(",gblur=sigma={sigma:.5}:enable='gte(t+{cue},{at})',colorchannelmixer=aa=0.55:enable='gte(t+{cue},{at})'"));
        }
        // rotate's output bounds are configured from its first input frame.
        // Rotate the fixed raster before animated scaling, or a pop/zoom that
        // starts small permanently crops the card to that initial rectangle.
        if t.rotation.abs()>0.001 {let angle=t.rotation.to_radians();transform.push_str(&format!(",rotate={angle}:ow=rotw({angle}):oh=roth({angle}):c=none"));}
        if clip.motion.zoom||clip.motion.kind=="stack_pop"||clip.motion.backdrop_ms.is_some() {let scale=clip.motion.scale_expression(clip.cue_in_ms);transform.push_str(&format!(",scale=w='iw*({scale})':h='ih*({scale})':eval=frame"));}
        if clip.track==timeline::VIDEO {
            if let Some(seg)=p.segments.iter().find(|s|s.id==clip.scene_id) {if seg.motion=="push_in" {
                transform.push_str(&format!(",scale=w='iw*(1+0.05*min((t+{})/{},1))':h='ih*(1+0.05*min((t+{})/{},1))':eval=frame",clip.cue_in_ms as f64/1000.,seg.duration_ms as f64/1000.,clip.cue_in_ms as f64/1000.,seg.duration_ms as f64/1000.));
            }}
        }
        let start=clip.start_ms as f64/1000.;let stop=clip.end() as f64/1000.;
        graph.push(format!("[decoded{input_index}_{branch}]{filter},tpad=stop_mode=clone:stop_duration=2,trim=duration={:.6},{transform},setpts=PTS+{start:.6}/TB[layer{n}]",clip.duration_ms as f64/1000.));
        let shake=if clip.track==timeline::VIDEO && p.segments.iter().any(|s|s.id==clip.scene_id&&s.motion=="reaction_shake") {
            format!("+if(lt(t-{start}+{},0.45),5*sin(55*(t-{start}+{})),0)",clip.cue_in_ms as f64/1000.,clip.cue_in_ms as f64/1000.)
        }else{String::new()};
        let shift=clip.motion.shift_expression(clip.cue_in_ms);
        let shift=shift.replace("(t+",&format!("(t-{start}+"));
        let retreat=clip.motion.retreat_expression(clip.cue_in_ms,&format!("(t-{start})"));
        let drift=clip.motion.drift_expression(clip.cue_in_ms,&format!("(t-{start})"));
        graph.push(format!("[base{}][layer{n}]overlay=x='{}-w/2{shake}-{}*({retreat})+{}*({drift})':y='{}-h/2{shift}-{}*({retreat})-{}*({drift})':enable='gte(t,{start:.6})*lt(t,{stop:.6})':eof_action=pass:repeatlast=0:format=auto[base{n}]",n-1,t.x*w as f32,0.065*w as f32,0.012*w as f32,t.y*h as f32,0.018*h as f32,0.008*h as f32));
    }
    for (i,(_,count)) in inputs.iter().enumerate() {
        let input=i+1;
        let labels=(0..*count).map(|b|format!("[decoded{input}_{b}]")).collect::<String>();
        graph.insert(0,format!("[{input}:v]{}{labels}",if *count==1{"null".into()}else{format!("split={count}")}));
    }
    let graphfile=work.join("filters.txt");fs::write(&graphfile,graph.join(";\n"))?;
    let pending=work.join("final.pending.mp4");
    args.extend(["-/filter_complex".into(),graphfile.to_string_lossy().into_owned(),"-map".into(),format!("[base{n}]"),"-an".into(),"-frames:v".into(),frames.to_string(),"-c:v".into(),s.encoder.clone()]);
    if s.encoder=="libx264" {args.extend(["-preset".into(),"veryfast".into(),"-crf".into(),"21".into()]);}
    // Independent very short GOPs must not introduce negative/reordered DTS
    // across concat boundaries. Otherwise fast seeks can select a later frame.
    args.extend(["-bf".into(),"0".into(),"-pix_fmt".into(),"yuv420p".into(),pending.to_string_lossy().into_owned()]);
    process::run(&s.ffmpeg,&args,None,cancel,Duration::from_secs(s.timeout_seconds),None)?;
    if let Some(parent)=output.parent(){fs::create_dir_all(parent)?;}
    fs::rename(&pending,output)?;
    Ok(())
}
