//! Embedded native preview: FFmpeg decodes local frames, egui draws textures, rodio plays PCM.
use crate::{
    media,
    model::Settings,
    process::{self, Cancel},
};
use rodio::Source as AudioSource;
use anyhow::Context;
use std::{
    io::{BufReader, Read},
    path::PathBuf,
    process::{Child, ChildStdout, Stdio},
    sync::{
        atomic::Ordering,
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::Duration,
};

pub enum PlayerEvent {
    Backgrounds { generation:u64, count:usize },
    Layers { generation:u64, ms:u64, frames:Vec<(String,image::RgbaImage)> },
    Frame {
        generation: u64,
        ms: u64,
        image: image::RgbaImage,
    },
    Stopped {
        generation: u64,
    },
    Error {
        generation: u64,
        message: String,
    },
}
pub struct Player {
    pub rx: Receiver<PlayerEvent>,
    tx: SyncSender<PlayerEvent>,
    cancel: Cancel,
    pub generation: u64,
    pub playing: bool,
    pub preparing_backgrounds: usize,
    audio_mix: std::sync::Arc<std::sync::Mutex<Vec<(String,f32,bool)>>>,
    sfx_updates:std::sync::Arc<std::sync::Mutex<Option<Vec<crate::composition::LayerClip>>>>,
    audition_cancel:Cancel,
}
impl Default for Player {
    fn default() -> Self {
        let (tx, rx) = mpsc::sync_channel(3);
        Self {
            rx,
            tx,
            cancel: process::cancel_token(),
            generation: 0,
            playing: false,
            preparing_backgrounds:0,
            audio_mix:Default::default(),
            sfx_updates:Default::default(),audition_cancel:process::cancel_token(),
        }
    }
}
impl Player {
    pub fn composition(&mut self,mut p:crate::model::Project,s:Settings,ms:u64,play:bool) {
        p.ensure_composition();
        self.stop();self.cancel=process::cancel_token();self.playing=play;
        self.update_audio(&p);
        self.sfx_updates=Default::default();let sfx_updates=self.sfx_updates.clone();
        let audio_mix=self.audio_mix.clone();
        let cancel=self.cancel.clone();let tx=self.tx.clone();let generation=self.generation;
        thread::spawn(move || {
            if let Err(e)=composition_worker(&p,&s,ms,play,&cancel,&tx,generation,audio_mix,sfx_updates) {
                let _=tx.try_send(PlayerEvent::Error{generation,message:format!("Layer preview: {e:#}")});
            }
            let _=tx.try_send(PlayerEvent::Stopped{generation});
        });
    }
    pub fn update_audio(&self,p:&crate::model::Project) {
        if let Ok(mut controls)=self.audio_mix.lock(){*controls=p.composition_snapshot().clips.iter().filter(|c|c.audio()).map(|c|(c.id.clone(),c.effective_gain(p),c.enabled(p))).collect();}
    }
    pub fn refresh_sfx(&self,p:&crate::model::Project) {
        self.update_audio(p);
        if let Ok(mut pending)=self.sfx_updates.lock(){*pending=Some(p.composition_snapshot().clips.into_iter().filter(|c|c.track==crate::timeline::SFX).collect());}
    }
    pub fn audition(&mut self,style:&str,kind:&str,gain:f32)->anyhow::Result<()> {
        let (path,_)=crate::evidence_sfx::selected_sound(style,0,kind).context("Không đọc được thư viện SFX")?;
        self.audition_cancel.store(true,Ordering::Relaxed);self.audition_cancel=process::cancel_token();
        let cancel=self.audition_cancel.clone();let tx=self.tx.clone();let generation=self.generation;
        thread::spawn(move||{
            let result=(||->anyhow::Result<()>{
                let (_stream,handle)=rodio::OutputStream::try_default().context("Không mở được thiết bị âm thanh để nghe thử")?;
                let sink=rodio::Sink::try_new(&handle)?;sink.set_volume(gain.clamp(0.,2.));
                sink.append(rodio::Decoder::new(BufReader::new(std::fs::File::open(path)?))?);
                while !cancel.load(Ordering::Relaxed)&&!sink.empty(){thread::sleep(Duration::from_millis(10));}
                sink.stop();Ok(())
            })();
            if let Err(error)=result{let _=tx.try_send(PlayerEvent::Error{generation,message:format!("Nghe thử SFX: {error:#}")});}
        });Ok(())
    }
    pub fn stop(&mut self) {
        self.audition_cancel.store(true,Ordering::Relaxed);
        self.cancel.store(true, Ordering::Relaxed);
        self.playing = false;
        self.preparing_backgrounds=0;
        self.generation += 1;
    }
    pub fn seek(&mut self, s: Settings, path: PathBuf, ms: u64) {
        self.stop();
        self.cancel = process::cancel_token();
        let cancel = self.cancel.clone();
        let generation = self.generation;
        let tx = self.tx.clone();
        thread::spawn(move || match media::frame(&s, &path, ms, &cancel) {
            Ok(image) => {
                let _ = tx.try_send(PlayerEvent::Frame {
                    generation,
                    ms,
                    image,
                });
            }
            Err(e) => {
                let _ = tx.try_send(PlayerEvent::Error {
                    generation,
                    message: format!("{e:#}"),
                });
            }
        });
    }
    pub fn play(&mut self, s: Settings, path: PathBuf, ms: u64, end_ms: u64) {
        self.play_with_gain(s, path, ms, end_ms, 1.0);
    }
    pub fn play_with_gain(&mut self, s: Settings, path: PathBuf, ms: u64, end_ms: u64, gain: f32) {
        self.stop();
        self.cancel = process::cancel_token();
        self.playing = true;
        let cancel = self.cancel.clone();
        let generation = self.generation;
        let tx = self.tx.clone();
        thread::spawn(move || {
            if let Err(e) = play_worker(&s, &path, ms, end_ms, &cancel, &tx, generation, gain) {
                let _ = tx.try_send(PlayerEvent::Error {
                    generation,
                    message: format!("{e:#}"),
                });
            }
            let _ = tx.try_send(PlayerEvent::Stopped { generation });
        });
    }
}

struct LayerDecoder { child:Child, stream:ChildStdout, width:u32,height:u32,last:Option<image::RgbaImage> }
#[cfg(test)]
pub(crate) fn decode_test_frame(p:&crate::model::Project,s:&Settings,c:&crate::composition::LayerClip,local:u64)->anyhow::Result<image::RgbaImage> {
    LayerDecoder::new(p,s,c,local)?.read().ok_or_else(||anyhow::anyhow!("Preview decode returned no frame"))
}
impl Drop for LayerDecoder {fn drop(&mut self){let _=self.child.kill();let _=self.child.wait();}}
impl LayerDecoder {
    fn new(p:&crate::model::Project,s:&Settings,c:&crate::composition::LayerClip,local:u64)->anyhow::Result<Self> {
        let (ow,oh)=p.output.dimensions();let ratio=s.preview_long_edge.clamp(240,1280) as f64/oh.max(ow) as f64;
        let (w,h)=(((ow as f64*ratio).round() as u32/2*2).max(2),((oh as f64*ratio).round() as u32/2*2).max(2));
        let (input,mut filter)=crate::layer_media::visual_input(p,c,local,w,h,24,true)?;
        // Pad decoded media to a fixed transparent allocation; texture UV is
        // cropped below so geometry remains independent of decoder resolution.
        let (width,height)=if c.media.is_some()&&c.track!=crate::composition::BACKGROUND {
            let path=c.proxy.as_ref().filter(|v|v.is_file()).or(c.media.as_ref()).unwrap();
            let info=media::probe(s,path,&process::cancel_token())?;
            let f=(w as f64/info.width.max(1) as f64).min(h as f64/info.height.max(1) as f64);
            let dw=(info.width as f64*f).round() as u32;let dh=(info.height as f64*f).round() as u32;
            // Explicit dimensions avoid ffmpeg rounding differing from allocation.
            filter=format!("{}{},scale={dw}:{dh},setsar=1,fps=24,format=rgba",c.pts(),crate::visual_effects::filters(p,c)?);(dw,dh)
        }else{(w,h)};
        let mut args=vec!["-v".into(),"error".into(),"-threads".into(),"1".into(),"-filter_threads".into(),"1".into()];args.extend(input);
        args.extend(["-an".into(),"-vf".into(),filter,"-t".into(),format!("{:.6}",c.duration_ms.saturating_sub(local) as f64/1000.),"-pix_fmt".into(),"rgba".into(),"-f".into(),"rawvideo".into(),"pipe:1".into()]);
        let mut child=process::command(&s.ffmpeg).args(args).stdout(Stdio::piped()).stderr(Stdio::null()).stdin(Stdio::null()).spawn()?;
        let stream=child.stdout.take().ok_or_else(||anyhow::anyhow!("Layer pipe"))?;
        Ok(Self{child,stream,width,height,last:None})
    }
    fn read(&mut self)->Option<image::RgbaImage> {
        let mut data=vec![0;(self.width*self.height*4) as usize];
        if self.stream.read_exact(&mut data).is_ok(){self.last=image::RgbaImage::from_raw(self.width,self.height,data);}
        self.last.clone()
    }
}
type AudioControls=std::sync::Arc<std::sync::Mutex<Vec<(String,f32,bool)>>>;

struct ControlledAudio {
    source:Box<dyn AudioSource<Item=i16>+Send>,controls:AudioControls,id:String,
    samples:u64,offset:u64,duration:u64,fade_in:u64,fade_out:u64,gain:f32,
}
impl Iterator for ControlledAudio {
    type Item=i16;
    fn next(&mut self)->Option<i16>{
        let sample=self.source.next()?;
        if self.samples % 960 == 0 {
            self.gain=self.controls.lock().ok().and_then(|c|c.iter().find(|c|c.0==self.id).map(|c|if c.2{c.1}else{0.})).unwrap_or(0.);
        }
        let ms=self.offset+self.samples*1000/(self.source.sample_rate() as u64*self.source.channels() as u64).max(1);
        let fi=if self.fade_in>0 {(ms as f32/self.fade_in as f32).min(1.)}else{1.};
        let fo=if self.fade_out>0 {(self.duration.saturating_sub(ms) as f32/self.fade_out as f32).min(1.)}else{1.};
        self.samples+=1;Some((sample as f32*self.gain*fi*fo).clamp(i16::MIN as f32,i16::MAX as f32) as i16)
    }
}
impl AudioSource for ControlledAudio {
    fn current_frame_len(&self)->Option<usize>{self.source.current_frame_len()}
    fn channels(&self)->u16{self.source.channels()}
    fn sample_rate(&self)->u32{self.source.sample_rate()}
    fn total_duration(&self)->Option<Duration>{self.source.total_duration()}
}
fn replace_sfx(comp:&mut crate::composition::Composition,new:Vec<crate::composition::LayerClip>,now:u64)->(Vec<String>,Vec<String>) {
    let fingerprint=|c:&crate::composition::LayerClip|serde_json::to_vec(&(&c.media,c.media_in_ms,c.start_ms,c.duration_ms,c.playback_rate,c.fade_in_ms,c.fade_out_ms)).unwrap_or_default();
    let old:Vec<_>=comp.clips.iter().filter(|c|c.track==crate::timeline::SFX).collect();
    let stop:Vec<_>=old.iter().filter(|c|new.iter().find(|n|n.id==c.id).is_none_or(|n|fingerprint(n)!=fingerprint(c))).map(|c|c.id.clone()).collect();
    let retired=new.iter().filter(|n|old.iter().find(|c|c.id==n.id).map_or(n.start_ms<now,|c|c.end()<=now)).map(|n|n.id.clone()).collect();
    comp.clips.retain(|c|c.track!=crate::timeline::SFX);comp.clips.extend(new);(stop,retired)
}
#[allow(clippy::too_many_arguments)]
fn composition_worker(p:&crate::model::Project,s:&Settings,ms:u64,play:bool,cancel:&Cancel,tx:&SyncSender<PlayerEvent>,generation:u64,controls:AudioControls,sfx_updates:std::sync::Arc<std::sync::Mutex<Option<Vec<crate::composition::LayerClip>>>>)->anyhow::Result<()> {
    let count=crate::evidence::pending_backgrounds(p);
    if count>0 {
        let _=tx.try_send(PlayerEvent::Backgrounds{generation,count});
        crate::evidence::prepare_backgrounds(p,s,cancel).context("Không cập nhật được nền dẫn chứng; kiểm tra video nguồn và thử lại")?;
    }
    if play {crate::background_music::prepare(p,s,cancel)?;}
    let mut comp=p.composition_snapshot();let end=comp.duration_ms();let mut decoders=std::collections::BTreeMap::<String,LayerDecoder>::new();
    let mut retired_sfx=std::collections::BTreeSet::new();
    let mut stills=std::collections::BTreeMap::<String,image::RgbaImage>::new();
    let mut delivered=std::collections::BTreeSet::new();
    let audio_stream=if play {rodio::OutputStream::try_default().ok()}else{None};
    let mut sinks=std::collections::BTreeMap::<String,rodio::Sink>::new();
    let mut clock:Option<std::time::Instant>=None;let mut index=0u64;
    while !cancel.load(Ordering::Relaxed) {
        let time=ms+index*1000/24;if time>=end {break;}
        let pending=sfx_updates.lock().ok().and_then(|mut p|p.take());
        if let Some(new)=pending {
            let now=clock.map(|clock|ms+clock.elapsed().as_millis() as u64).unwrap_or(ms);
            let (stop,retired)=replace_sfx(&mut comp,new,now);retired_sfx.extend(retired);
            for id in stop{if let Some(sink)=sinks.remove(&id){sink.stop();}}
        }
        sinks.retain(|id,_|comp.clips.iter().any(|c|&c.id==id&&c.end()>time));
        if let Some((_,handle))=&audio_stream {
            for c in comp.clips.iter().filter(|c|c.audio()&&!retired_sfx.contains(&c.id)&&c.end()>time&&c.start_ms<=time+2000) {
                if sinks.contains_key(&c.id){continue;}
                let Some(path)=c.media.as_ref().filter(|p|p.is_file()) else{continue;};
                let audio_time=clock.map(|clock|ms+clock.elapsed().as_millis() as u64).unwrap_or(ms);
                let offset=audio_time.saturating_sub(c.start_ms).min(c.duration_ms);let input_ms=c.source_at(offset);
                let remaining=Duration::from_millis(c.duration_ms-offset);
                let source:Box<dyn AudioSource<Item=i16>+Send>=if (c.playback_rate-1.).abs()<0.000001&&path.extension().is_some_and(|e|e.eq_ignore_ascii_case("wav")) {
                    let decoder=rodio::Decoder::new(BufReader::new(std::fs::File::open(path)?))?;
                    Box::new(decoder.skip_duration(Duration::from_millis(input_ms)).take_duration(remaining))
                }else{
                    let mut child=process::command(&s.ffmpeg).args(["-v","error","-threads","1","-ss",&format!("{:.6}",input_ms as f64/1000.),"-i",&path.to_string_lossy(),"-t",&format!("{:.6}",remaining.as_secs_f64()),"-af",&c.tempo(),"-vn","-f","s16le","-ac","2","-ar","48000","pipe:1"]).stdout(Stdio::piped()).stderr(Stdio::null()).stdin(Stdio::null()).spawn()?;
                    let stdout=child.stdout.take().ok_or_else(||anyhow::anyhow!("Audio pipe"))?;
                    Box::new(PcmSource{reader:BufReader::new(stdout),child,cancel:cancel.clone()})
                };
                let source=ControlledAudio{source,controls:controls.clone(),id:c.id.clone(),samples:0,offset,duration:c.duration_ms,fade_in:c.fade_in_ms,fade_out:c.fade_out_ms,gain:c.effective_gain(p)};
                let sink=rodio::Sink::try_new(handle)?;sink.pause();
                sink.append(source.delay(Duration::from_millis(c.start_ms.saturating_sub(audio_time))));
                if clock.is_some(){sink.play();}sinks.insert(c.id.clone(),sink);
            }
        }
        let active:Vec<_>=comp.clips.iter().filter(|c|!c.audio()&&c.enabled(p)&&c.active(time)).collect();
        decoders.retain(|id,_|comp.clips.iter().any(|c|&c.id==id&&c.end()>time));
        let mut frames=vec![];
        for c in active {
            if c.media.as_ref().is_some_and(|v|!v.is_file()){continue;}
            if c.still(){
                if !stills.contains_key(&c.id){let img=if crate::visual_effects::selected(p,c).is_some_and(|e|e.active()) {LayerDecoder::new(p,s,c,0)?.read().context("Không đọc được ảnh với hiệu ứng")?}else{image::open(c.media.as_ref().unwrap())?.to_rgba8()};let edge=s.preview_long_edge.clamp(240,1280);let ratio=(edge as f32/img.width().max(img.height()) as f32).min(1.);let img=image::imageops::resize(&img,(img.width() as f32*ratio).round().max(1.) as u32,(img.height() as f32*ratio).round().max(1.) as u32,image::imageops::FilterType::Triangle);stills.insert(c.id.clone(),img);}
                if !delivered.contains(&c.id){frames.push((c.id.clone(),stills[&c.id].clone()));}
                if c.motion.backdrop_ms.is_some(){
                    let key=format!("{}#blur",c.id);
                    if !stills.contains_key(&key){let img=&stills[&c.id];let blurred=image::imageops::blur(img,8.*img.width() as f32/800.);stills.insert(key.clone(),blurred);}
                    if !delivered.contains(&key){frames.push((key.clone(),stills[&key].clone()));}
                }
                continue;
            }
            if !decoders.contains_key(&c.id){decoders.insert(c.id.clone(),LayerDecoder::new(p,s,c,time-c.start_ms)?);}
            if let Some(image)=decoders.get_mut(&c.id).unwrap().read(){frames.push((c.id.clone(),image));}
            process::check_cancel(cancel)?;
        }
        let origin=clock.get_or_insert_with(std::time::Instant::now);
        if index==0 {for sink in sinks.values(){sink.play();}}
        let target=Duration::from_secs_f64(index as f64/24.);
        while play && origin.elapsed()<target && !cancel.load(Ordering::Relaxed) {thread::sleep(Duration::from_millis(2));}
        // Bounded UI queue: a slow UI drops display frames, never builds a backlog.
        let static_ids:Vec<_>=frames.iter().filter(|(id,_)|stills.contains_key(id)).map(|(id,_)|id.clone()).collect();
        if tx.try_send(PlayerEvent::Layers{generation,ms:time,frames}).is_ok(){delivered.extend(static_ids);}
        if !play{break;}
        // Prefetch through OS pipes. They apply backpressure; no unbounded queue.
        for c in comp.clips.iter().filter(|c|!c.audio()&&c.enabled(p)&&c.start_ms>time&&c.start_ms<=time+750) {
            if c.still(){continue;}
            if !decoders.contains_key(&c.id) && c.media.as_ref().is_none_or(|m|m.is_file()) {
                decoders.insert(c.id.clone(),LayerDecoder::new(p,s,c,0)?);
            }
        }
        index+=1;
        stills.retain(|id,_|comp.clips.iter().any(|c|c.id==id.trim_end_matches("#blur")&&c.end()>time&&c.start_ms<time+750));
        delivered.retain(|id|stills.contains_key(id));
    }
    for sink in sinks.values(){sink.stop();}
    Ok(())
}
impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
    }
}

struct PcmSource {
    reader: BufReader<ChildStdout>,
    child: Child,
    cancel: Cancel,
}
impl Iterator for PcmSource {
    type Item = i16;
    fn next(&mut self) -> Option<i16> {
        if self.cancel.load(Ordering::Relaxed) {
            return None;
        }
        let mut b = [0; 2];
        self.reader.read_exact(&mut b).ok()?;
        Some(i16::from_le_bytes(b))
    }
}
impl AudioSource for PcmSource {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        48000
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}
impl Drop for PcmSource {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[allow(clippy::too_many_arguments)]
fn play_worker(
    s: &Settings,
    path: &std::path::Path,
    ms: u64,
    end_ms: u64,
    cancel: &Cancel,
    tx: &SyncSender<PlayerEvent>,
    generation: u64,
    gain: f32,
) -> anyhow::Result<()> {
    let info = media::probe(s, path, cancel)?;
    let (width, height) = if info.width >= info.height {
        (
            640u32,
            (640.0 * info.height as f64 / info.width as f64).round() as u32 / 2 * 2,
        )
    } else {
        (
            (360.0 * info.width as f64 / info.height as f64).round() as u32 / 2 * 2,
            360u32,
        )
    };
    let start = format!("{:.3}", ms as f64 / 1000.0);
    let length = format!("{:.3}", end_ms.saturating_sub(ms).max(1) as f64 / 1000.0);
    let path = path.to_string_lossy();
    let audio_stream = rodio::OutputStream::try_default().ok();
    let sink = audio_stream
        .as_ref()
        .and_then(|(_, h)| rodio::Sink::try_new(h).ok());
    if let Some(sink) = &sink {
        sink.set_volume(gain);
        if let Ok(mut child) = process::command(&s.ffmpeg)
            .args([
                "-v", "error", "-ss", &start, "-i", &path, "-t", &length, "-vn", "-f", "s16le",
                "-ac", "2", "-ar", "48000", "pipe:1",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn()
        {
            if let Some(stdout) = child.stdout.take() {
                sink.append(PcmSource {
                    reader: BufReader::new(stdout),
                    child,
                    cancel: cancel.clone(),
                });
            }
        }
    }
    let filter = format!("scale={width}:{height},fps=24");
    let mut child = process::command(&s.ffmpeg)
        .args([
            "-v", "error", "-re", "-ss", &start, "-i", &path, "-t", &length, "-an", "-vf", &filter,
            "-pix_fmt", "rgba", "-f", "rawvideo", "pipe:1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()?;
    let mut stream = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("Không có video pipe"))?;
    let mut index = 0u64;
    while !cancel.load(Ordering::Relaxed) {
        let mut bytes = vec![0; (width * height * 4) as usize];
        if stream.read_exact(&mut bytes).is_err() {
            break;
        }
        if let Some(image) = image::RgbaImage::from_raw(width, height, bytes) {
            let _ = tx.try_send(PlayerEvent::Frame {
                generation,
                ms: ms + index * 1000 / 24,
                image,
            });
        }
        index += 1;
    }
    process::stop(&mut child);
    if let Some(sink) = sink {
        sink.stop();
    }
    Ok(())
}

#[cfg(test)]
mod original_audio_tests {
    use super::*;
    use crate::{model::*,timeline};
    #[test]
    fn live_sound_style_swap_preserves_voice_and_retires_finished_effects() {
        let mut p=Project{segments:vec![Segment{id:"host".into(),kind:"host".into(),audio:Some("voice.wav".into()),duration_ms:3000,graphics:vec![GraphicCue{start_ms:0,end_ms:1000,..Default::default()},GraphicCue{start_ms:2000,end_ms:2900,..Default::default()}],..Default::default()}],..Default::default()};
        p.card_sound.style="pop".into();p.ensure_composition();let mut running=p.composition_snapshot();
        let pictures=serde_json::to_vec(&running.clips.iter().filter(|c|c.track!=timeline::SFX).collect::<Vec<_>>()).unwrap();
        let mut player=Player::default();player.playing=true;let generation=player.generation;
        p.card_sound.style="whoosh".into();crate::evidence_sfx::apply_settings(&mut p,crate::evidence_sfx::Scope::Cards,None);
        player.refresh_sfx(&p);assert!(player.playing);assert_eq!(player.generation,generation);
        let pending=player.sfx_updates.lock().unwrap().take().unwrap();
        let (stopped,retired)=replace_sfx(&mut running,pending,500);
        assert_eq!(stopped.len(),2);assert_eq!(retired,vec!["host:gfx:0:9"]);
        assert!(stopped.iter().all(|id|id.contains(":gfx:")));
        assert_eq!(pictures,serde_json::to_vec(&running.clips.iter().filter(|c|c.track!=timeline::SFX).collect::<Vec<_>>()).unwrap());
        // Gain-only changes keep the current sink; audio samples use live controls.
        p.card_sound.gain=0.5;player.refresh_sfx(&p);
        let pending=player.sfx_updates.lock().unwrap().take().unwrap();
        assert!(replace_sfx(&mut running,pending,600).0.is_empty());
    }
    #[test]
    fn real_evidence_whoosh_reaches_native_preview_with_gain_and_mute() {
        let mut p=Project{segments:vec![Segment{id:"host".into(),kind:"host".into(),duration_ms:2000,
            evidence_items:vec![crate::evidence::EvidenceItem{id:"photo".into(),kind:"fact".into(),asset:Some("photo.png".into()),..Default::default()}],..Default::default()}],..Default::default()};
        p.ensure_composition();let clip=p.composition.as_ref().unwrap().clips.iter().find(|c|c.track==timeline::SFX).unwrap().clone();
        assert!(clip.media.as_ref().unwrap().file_name().unwrap().to_string_lossy().starts_with("whoosh-"));
        let player=Player::default();
        let decode=|p:&Project| {
            player.update_audio(p);
            let decoder=rodio::Decoder::new(BufReader::new(std::fs::File::open(clip.media.as_ref().unwrap()).unwrap())).unwrap();
            let audio=ControlledAudio{source:Box::new(decoder),controls:player.audio_mix.clone(),id:clip.id.clone(),samples:0,offset:0,duration:clip.duration_ms,fade_in:clip.fade_in_ms,fade_out:clip.fade_out_ms,gain:0.};
            let samples:Vec<_>=audio.collect();
            (samples.iter().map(|s|(*s as f64/32768.).powi(2)).sum::<f64>()/samples.len() as f64).sqrt()
        };
        let normal=decode(&p);assert!(normal>0.03,"Preview whoosh must survive fades and default gain");
        p.composition.as_mut().unwrap().clips.iter_mut().find(|c|c.id==clip.id).unwrap().gain*=0.5;
        let quieter=decode(&p);assert!((0.49..0.51).contains(&(quieter/normal)));
        p.timeline.tracks[timeline::SFX].muted=true;assert_eq!(decode(&p),0.);
    }
    #[test]
    fn live_card_sound_gain_and_mute_change_samples_without_restart() {
        let mut p=Project{segments:vec![Segment{id:"host".into(),kind:"host".into(),duration_ms:1000,graphics:vec![GraphicCue{end_ms:900,..Default::default()}],..Default::default()}],..Default::default()};
        p.ensure_composition();let id=p.composition.as_ref().unwrap().clips.iter().find(|c|c.track==timeline::SFX).unwrap().id.clone();
        let player=Player::default();player.update_audio(&p);
        let source=rodio::buffer::SamplesBuffer::new(2,48000,vec![10000_i16;10000]);
        let mut audio=ControlledAudio{source:Box::new(source),controls:player.audio_mix.clone(),id,samples:0,offset:0,duration:1000,fade_in:0,fade_out:0,gain:0.};
        for (enabled,gain) in [(true,0.25),(true,0.5),(false,0.5),(true,0.),(true,1.)] {
            p.card_sound.enabled=enabled;p.card_sound.gain=gain;player.update_audio(&p);
            while audio.samples%960!=0{audio.next();}
            assert_eq!(audio.next(),Some(if enabled{(10000.*gain) as i16}else{0}));
        }
        p.timeline.tracks[timeline::GRAPHICS].muted=true;player.update_audio(&p);
        while audio.samples%960!=0{audio.next();}assert_eq!(audio.next(),Some(0));
    }
    #[test]
    fn live_background_controls_change_samples_without_restarting_preview() {
        let mut p=Project{sources:vec![crate::model::Source{id:"s".into(),has_audio:true,..Default::default()}],segments:vec![Segment{id:"review".into(),kind:"review".into(),source_id:"s".into(),out_ms:1000,duration_ms:1000,..Default::default()}],..Default::default()};
        p.ensure_composition();let id=p.composition.as_ref().unwrap().clips.iter().find(|c|c.track==timeline::ORIGINAL).unwrap().id.clone();
        let player=Player::default();player.update_audio(&p);
        let source=rodio::buffer::SamplesBuffer::new(2,48000,vec![10000_i16;5000]);
        let mut controlled=ControlledAudio{source:Box::new(source),controls:player.audio_mix.clone(),id,samples:0,offset:0,duration:1000,fade_in:0,fade_out:0,gain:0.};
        assert_eq!(controlled.next(),Some(0));
        for gain in [0.15,0.5,2.] {
            p.output.original_audio_enabled=true;p.output.original_audio_gain=gain;player.update_audio(&p);
            while controlled.samples%960!=0 {controlled.next();}
            assert_eq!(controlled.next(),Some((10000.*gain) as i16));
        }
        p.timeline.tracks[timeline::ORIGINAL].muted=true;player.update_audio(&p);
        while controlled.samples%960!=0 {controlled.next();}assert_eq!(controlled.next(),Some(0));
    }
}
