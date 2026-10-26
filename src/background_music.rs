//! Cached, seekable music bed. Only audio is rebuilt when the bed changes.
use crate::{composition::{LayerClip,Transform},model::*,process::{self,Cancel},timeline};
use anyhow::{ensure,Context,Result};
use serde::{Deserialize,Serialize};
use std::{fs,path::{Path,PathBuf},time::Duration};
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(default)]
pub struct Settings {pub enabled:bool,pub file:PathBuf,pub name:String,pub offset_ms:u64,pub gain:f32,pub looping:bool,pub fade_in_ms:u64,pub fade_out_ms:u64,pub duck:bool,pub duck_gain:f32,pub mute_source:bool}
impl Default for Settings{fn default()->Self{Self{enabled:false,file:PathBuf::new(),name:String::new(),offset_ms:0,gain:0.12,looping:true,fade_in_ms:800,fade_out_ms:1200,duck:true,duck_gain:0.3,mute_source:true}}}
impl Settings{pub fn validate(&self)->Result<()>{ensure!([self.gain,self.duck_gain].iter().all(|v|v.is_finite()&&(0.0..=1.).contains(v)),"Âm lượng nhạc nền phải từ 0–100%");ensure!(self.offset_ms<=86_400_000&&self.fade_in_ms<=60_000&&self.fade_out_ms<=60_000,"Điểm vào / fade nhạc không hợp lệ");if self.enabled{ensure!(self.file.is_file(),"Thiếu file nhạc nền: {}",self.file.display());}Ok(())}}
pub fn import(s:&crate::model::Settings,source:&Path,folder:&Path,cancel:&Cancel)->Result<PathBuf>{
    process::check_cancel(cancel)?;ensure!(fs::metadata(source)?.len()<=256*1024*1024,"Nhạc nền vượt 256 MB");
    let info=crate::media::probe(s,source,cancel)?;ensure!(info.has_audio&&info.duration_ms>=1000,"Chọn file nhạc có âm thanh dài ít nhất 1 giây");
    fs::create_dir_all(folder)?;let ext=source.extension().and_then(|e|e.to_str()).unwrap_or("mp3").to_ascii_lowercase();ensure!(matches!(ext.as_str(),"mp3"|"wav"|"m4a"|"flac"|"ogg"),"Định dạng nhạc chưa hỗ trợ");
    let output=folder.join(format!("{}.{}",crate::visual_effects::fingerprint(source),ext));if !output.is_file(){fs::copy(source,&output)?;}Ok(output)
}
fn end(p:&Project)->u64 {p.composition.as_ref().map(|c|c.clips.iter().filter(|c|c.track!=timeline::MUSIC).map(LayerClip::end).max().unwrap_or(0)).unwrap_or_else(||p.duration_ms())}
fn windows(p:&Project)->Vec<(u64,u64,f32)>{
    let mut result=vec![];let cfg=&p.music;
    if cfg.duck&&!p.timeline.tracks[timeline::VOICE].muted {
        if let Some(comp)=&p.composition {for c in comp.clips.iter().filter(|c|c.track==timeline::VOICE&&c.visible&&c.gain>0.){result.push((c.start_ms,c.end(),cfg.duck_gain));}}
        else{let mut start=0;for s in &p.segments{if s.kind!="source"{result.push((start,start+s.duration_ms,cfg.duck_gain));}start+=s.duration_ms;}}
    }
    if cfg.mute_source {
        if let Some(comp)=&p.composition {
            for c in comp.clips.iter().filter(|c|matches!(c.track,timeline::VIDEO|timeline::ORIGINAL)&&c.visible&&p.segments.iter().any(|s|s.id==c.scene_id&&s.kind=="source")) {result.push((c.start_ms,c.end(),0.));}
        }else{let mut start=0;for s in &p.segments{if s.kind=="source"{result.push((start,start+s.duration_ms,0.));}start+=s.duration_ms;}}
    }
    result.sort_unstable_by_key(|w|(w.0,w.1));result.dedup();
    result
}
pub fn prepared_path(p:&Project)->PathBuf {
    // Called while laying out the native timeline: never hash/read a song on the UI thread.
    // Managed imports are content-addressed; metadata also invalidates an edited external file.
    let metadata=fs::metadata(&p.music.file).ok();
    let identity=format!("{}:{:?}:{:?}",p.music.file.display(),metadata.as_ref().map(|m|m.len()),metadata.and_then(|m|m.modified().ok()));
    let key=crate::media::hash(serde_json::to_vec(&("music-v3",identity,p.music.offset_ms,p.music.looping,end(p),windows(p))).unwrap_or_default());
    p.root.join("cache/music").join(format!("{key}.wav"))
}
pub fn clip(p:&Project)->Option<LayerClip>{
    if !p.music.enabled||end(p)==0{return None;}
    Some(LayerClip{id:"music-bed".into(),scene_id:p.segments.first()?.id.clone(),track:timeline::MUSIC,start_ms:0,duration_ms:end(p),media_in_ms:0,playback_rate:1.,cue_in_ms:0,media:Some(prepared_path(p)),proxy:None,transform:Transform::default(),z:0,visible:true,locked:false,linked:false,gain:p.music.gain,fade_in_ms:p.music.fade_in_ms,fade_out_ms:p.music.fade_out_ms,evidence_id:None,group:None,motion:Default::default(),timing_manual:false})
}
pub fn prepare(p:&Project,s:&crate::model::Settings,cancel:&Cancel)->Result<()> {
    if !p.music.enabled{return Ok(());}process::check_cancel(cancel)?;p.music.validate()?;let output=prepared_path(p);if output.is_file(){return Ok(());}
    let info=crate::media::probe(s,&p.music.file,cancel)?;ensure!(info.has_audio&&info.duration_ms>p.music.offset_ms+200,"Điểm bắt đầu vượt thời lượng bài nhạc");
    let duration=(info.duration_ms-p.music.offset_ms) as f64/1000.;let offset=p.music.offset_ms as f64/1000.;let total=end(p) as f64/1000.;if total==0.{return Ok(());}
    fs::create_dir_all(output.parent().unwrap())?;
    let mut graph=format!("[0:a]atrim=start={offset:.6},asetpts=PTS-STARTPTS,aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo");
    if p.music.looping&&duration>0.6 {
        let fade=0.25_f64.min(duration/4.);let body_end=duration-fade;let samples=((duration-fade)*48000.).round() as u64;
        graph.push_str(&format!(",asplit=4[prefix][head][body][tail];[prefix]atrim=duration={fade:.6},asetpts=PTS-STARTPTS[p];[head]atrim=duration={fade:.6},asetpts=PTS-STARTPTS[h];[body]atrim=start={fade:.6}:end={body_end:.6},asetpts=PTS-STARTPTS[b];[tail]atrim=start={body_end:.6},asetpts=PTS-STARTPTS[t];[t][h]acrossfade=d={fade:.6}:c1=tri:c2=tri[x];[b][x]concat=n=2:v=0:a=1,aloop=loop=-1:size={samples}[loop];[p][loop]concat=n=2:v=0:a=1"));
    }else if p.music.looping {graph.push_str(&format!(",aloop=loop=-1:size={}",(duration*48000.).round() as u64));}
    let mut envelope="1".to_owned();for (a,b,gain) in windows(p){let (a,b)=(a as f64/1000.,b as f64/1000.);let value=if gain==0.{format!("if(between(t,{a:.6},{b:.6}),0,1)")}else{format!("1-{}*max(0,min(1,min((t-{a:.6})/0.12,({b:.6}-t)/0.35)))",1.-gain)};envelope=format!("min({envelope},{value})");}
    graph.push_str(&format!(",apad,atrim=duration={total:.6},volume='{envelope}':eval=frame[out]"));
    let scratch=tempfile::Builder::new().prefix("music-").tempdir_in(output.parent().unwrap())?;let pending=scratch.path().join("bed.wav");let script=scratch.path().join("graph.txt");fs::write(&script,graph)?;
    process::run(&s.ffmpeg,&vec!["-y".into(),"-v".into(),"error".into(),"-i".into(),p.music.file.to_string_lossy().into_owned(),"-/filter_complex".into(),script.to_string_lossy().into_owned(),"-map".into(),"[out]".into(),"-c:a".into(),"pcm_s16le".into(),pending.to_string_lossy().into_owned()],None,cancel,Duration::from_secs(s.timeout_seconds),None).context("Không chuẩn bị được nhạc nền")?;
    let verified=crate::media::probe(s,&pending,cancel)?;ensure!(verified.has_audio&&verified.duration_ms.abs_diff(end(p))<60,"Nhạc nền lệch thời lượng timeline");
    process::check_cancel(cancel)?;
    if !output.exists(){fs::rename(pending,&output)?;}Ok(())
}
