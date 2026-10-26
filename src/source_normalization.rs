//! Preserve original source media; normalize only in the project cache.
use crate::{media,model::{Settings as Runtime,Source,write_json},process::{self,Cancel}};
use anyhow::{ensure,Context,Result};
use serde::{Serialize,Deserialize};
use std::{fs,path::Path,time::Duration};
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(default)]
pub struct Settings{pub mode:String,pub proxy:bool}
impl Default for Settings{fn default()->Self{Self{mode:"auto".into(),proxy:true}}}
impl Settings{pub fn validate(&self)->Result<()>{ensure!(matches!(self.mode.as_str(),"auto"|"keep"|"remux"|"transcode"),"Chế độ chuẩn hóa đầu vào không hợp lệ");Ok(())}}
pub fn prepare(cfg:&Settings,s:&Runtime,source:&mut Source,root:&Path,cancel:&Cancel)->Result<()> {
    cfg.validate()?;let meta=fs::metadata(&source.path)?;let key=media::hash(format!("normalize-v2:{}:{}:{:?}:{}",source.path.display(),meta.len(),meta.modified().ok(),cfg.mode));let folder=root.join("cache/normalized");fs::create_dir_all(&folder)?;
    let report=folder.join(format!("{key}.json"));let destination=folder.join(format!("{key}.mp4"));
    let raw=process::run(&s.ffprobe,&process::args(&["-v","error","-show_streams","-show_format","-of","json",&source.path.to_string_lossy()]),None,cancel,Duration::from_secs(60),None)?;
    let metadata:serde_json::Value=serde_json::from_slice(&raw.stdout)?;
    let hdr=metadata["streams"].as_array().into_iter().flatten().any(|v|matches!(v["color_transfer"].as_str(),Some("smpte2084"|"arib-std-b67")));
    let mut method=cfg.mode.clone();
    if method=="auto" {
        let decode=process::run(&s.ffmpeg,&process::args(&["-v","error","-i",&source.path.to_string_lossy(),"-map","0:v:0","-frames:v","1","-an","-f","null","-"]),None,cancel,Duration::from_secs(60),None);
        process::check_cancel(cancel)?;
        let start=metadata["format"]["start_time"].as_str().and_then(|v|v.parse::<f64>().ok()).unwrap_or(0.);
        method=if decode.is_err()||hdr{"transcode"}else if start.abs()>0.1{"remux"}else{"keep"}.into();
    }
    source.normalized=Default::default();
    if method!="keep" {
        if !(destination.is_file()&&media::probe(s,&destination,cancel).is_ok_and(|v|v.width>0&&v.duration_ms.abs_diff(source.duration_ms)<=100)) {
            let scratch=tempfile::Builder::new().prefix("normalize-").tempdir_in(&folder)?;let pending=scratch.path().join("source.mp4");
            let mut args=process::args(&["-y","-v","error","-fflags","+genpts","-i",&source.path.to_string_lossy(),"-map","0:v:0","-map","0:a?","-sn","-dn"]);
            if method=="remux" {args.extend(process::args(&["-c","copy"]));}else{
                let filter=if hdr{"zscale=t=linear:npl=100,format=gbrpf32le,tonemap=tonemap=hable:desat=0,zscale=p=bt709:t=bt709:m=bt709:r=tv,scale=ceil(iw/2)*2:ceil(ih/2)*2,setsar=1"}else{"scale=ceil(iw/2)*2:ceil(ih/2)*2,setsar=1"};
                args.extend(process::args(&["-vf",filter,"-c:v","libx264","-preset","veryfast","-crf","18","-pix_fmt","yuv420p","-c:a","aac","-b:a","192k"]));
                if hdr{args.extend(process::args(&["-color_primaries","bt709","-color_trc","bt709","-colorspace","bt709","-bsf:v","h264_metadata=colour_primaries=1:transfer_characteristics=1:matrix_coefficients=1"]));}
            }
            args.extend(process::args(&["-movflags","+faststart",&pending.to_string_lossy()]));
            process::run(&s.ffmpeg,&args,None,cancel,Duration::from_secs(s.timeout_seconds),None).context("Không chuẩn hóa được nguồn; chọn Giữ nguyên hoặc Chuyển mã trong thiết lập nguồn")?;
            let info=media::probe(s,&pending,cancel)?;ensure!(info.width>0&&info.duration_ms.abs_diff(source.duration_ms)<=100,"Nguồn chuẩn hóa lệch thời lượng hơn 100 ms; giữ nguyên video gốc");process::check_cancel(cancel)?;fs::rename(pending,&destination)?;
        }
        source.normalized=destination.clone();
    }
    write_json(&report,&serde_json::json!({"source":source.path,"normalized":source.normalized,"method":method,"probe":metadata,"original_unchanged":true}))?;
    if cfg.proxy {let mut input=source.clone();input.path=source.input_path().to_owned();source.proxy=media::proxy(s,&input,&root.join("cache/proxy"),cancel)?;}else{source.proxy=Default::default();}Ok(())
}
