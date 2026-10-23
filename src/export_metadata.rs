//! Export-only metadata. Never part of picture/audio cache keys.
use crate::{model::{Project, Settings}, process::{self, Cancel}};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs, path::{Path, PathBuf}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

pub const PROFILE_VERSION: &str = "metadata-v1";
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum Preset { #[default] Air3view, CapcutDesktop, Custom }
impl Preset {
    pub fn label(self) -> &'static str { match self { Self::Air3view=>"AIR3view", Self::CapcutDesktop=>"Mô phỏng CapCut Desktop", Self::Custom=>"Tùy chỉnh" } }
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum Scope { #[default] Container, Video, Audio }
impl Scope {
    pub fn label(self) -> &'static str {match self { Self::Container=>"File",Self::Video=>"Video",Self::Audio=>"Audio" }}
    fn argument(self) -> &'static str {match self {Self::Container=>"-metadata",Self::Video=>"-metadata:s:v:0",Self::Audio=>"-metadata:s:a:0"}}
}
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Tag { pub scope: Scope, pub key: String, pub value: String }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MetadataSettings {
    pub enabled: bool,
    pub preset: Preset,
    /// Empty means use the current project title.
    pub title: String,
    pub author: String,
    pub description: String,
    pub write_creation_time: bool,
    /// Empty means actual export time. Overrides use UTC RFC3339, second precision.
    pub creation_time: String,
    pub custom_tags: Vec<Tag>,
}
impl Default for MetadataSettings {
    fn default()->Self { Self {enabled:true,preset:Preset::Air3view,title:String::new(),author:String::new(),description:String::new(),write_creation_time:true,creation_time:String::new(),custom_tags:vec![]} }
}
pub fn legacy_settings()->MetadataSettings {MetadataSettings{enabled:false,..Default::default()}}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Prepared {
    pub enabled: bool, pub preset: Preset, pub export_id: String,
    pub tags: Vec<Tag>, pub warnings: Vec<String>,
}
impl Prepared {
    pub fn args(&self)->Vec<String> {
        if !self.enabled {return vec![];}
        let mut args=vec!["-map_metadata".into(),"-1".into(),"-map_metadata:s".into(),"-1".into(),"-movflags".into(),"+faststart+use_metadata_tags".into()];
        for t in &self.tags {args.extend([t.scope.argument().into(),format!("{}={}",t.key,t.value)]);}
        args
    }
}
fn utc(seconds:u64)->String {
    // Civil date from days since 1970, Gregorian calendar.
    let z=(seconds/86400) as i64+719468;
    let era=z/146097;let doe=z-era*146097;
    let yoe=(doe-doe/1460+doe/36524-doe/146096)/365;
    let mut year=yoe+era*400;let doy=doe-(365*yoe+yoe/4-yoe/100);
    let mp=(5*doy+2)/153;let day=doy-(153*mp+2)/5+1;
    let month=mp+if mp<10 {3}else{-9};year+=i64::from(month<=2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",seconds/3600%24,seconds/60%60,seconds%60)
}
fn validate_time(value:&str)->Result<()> {
    let b=value.as_bytes();
    ensure!(b.len()==20 && [4,7].iter().all(|&i|b[i]==b'-') && b[10]==b'T' && b[13]==b':' && b[16]==b':' && b[19]==b'Z'
        && b.iter().enumerate().all(|(i,c)|[4,7,10,13,16,19].contains(&i)||c.is_ascii_digit()),"Thời gian metadata cần dạng UTC: 2026-10-23T08:00:00Z");
    let n=|a,b|value[a..b].parse::<u32>().unwrap_or(0);
    let (y,m,d)=(n(0,4),n(5,7),n(8,10));
    let leap=y%4==0 && (y%100!=0 || y%400==0);
    let days=match m {1|3|5|7|8|10|12=>31,4|6|9|11=>30,2=>if leap {29}else{28},_=>0};
    ensure!(y>=1970 && d>0 && d<=days && n(11,13)<24 && n(14,16)<60 && n(17,19)<60,"Ngày/giờ metadata không hợp lệ");Ok(())
}
pub fn prepare(p:&Project,s:&Settings)->Result<Prepared> {
    let c=&p.export_metadata;
    let mut out=Prepared{enabled:c.enabled,preset:c.preset,export_id:uuid::Uuid::new_v4().to_string(),tags:vec![],warnings:vec![]};
    if !c.enabled {return Ok(out);}
    let mut add=|scope,key:&str,value:String| {out.tags.push(Tag{scope,key:key.into(),value});};
    add(Scope::Container,"title",if c.title.trim().is_empty(){p.title.clone()}else{c.title.clone()});
    if !c.author.is_empty(){add(Scope::Container,"artist",c.author.clone());}
    if !c.description.is_empty(){add(Scope::Container,"description",c.description.clone());}
    if c.write_creation_time {
        let value=if c.creation_time.trim().is_empty(){utc(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())}else{c.creation_time.trim().to_owned()};
        validate_time(&value)?;add(Scope::Container,"creation_time",value);
    }
    if c.preset==Preset::Air3view {add(Scope::Container,"software","AIR3view".into());}
    if c.preset==Preset::CapcutDesktop {
        // Reference, not an authenticated CapCut export. Leave encoder at the
        // real muxing library rather than inventing a CapCut/Lavf version.
        add(Scope::Container,"te_is_reencode","1".into());
        // Hw describes encoding during this render, not a claim about original footage.
        add(Scope::Container,"Hw",if s.encoder.contains("nvenc")||s.encoder.contains("qsv")||s.encoder.contains("amf"){ "1" }else{"0"}.into());
        let production=crate::workflow::effective_settings(p,s);
        let manifest=json!({"data":{"editType":"default","videoId":out.export_id,"os":"windows","product":"vicut",
            "is_use_voice_clone":if production.voice_reference_enabled && p.segments.iter().any(|s|s.audio.is_some()&&!s.narration.is_empty()) {"1"}else{"0"}},"source_platform":"desktop","source_type":"vicut"});
        out.tags.push(Tag{scope:Scope::Container,key:"com.apple.quicktime.artwork".into(),value:serde_json::to_string(&manifest)?});
        out.warnings.push("Preset tham khảo CapCut Desktop; key/JSON đã kiểm tra đọc lại, chưa có MP4 CapCut gốc để đối chiếu cấu trúc atom. Không tạo chữ ký CapCut.".into());
        out.warnings.push("FFmpeg có thể báo Unknown cover type: 0x1 khi đọc JSON artwork. Đây là trường văn bản, không phải ảnh bìa; AIR3view đọc trực tiếp mdta. Preset AIR3view không dùng trường này.".into());
    }
    if c.preset==Preset::Custom {out.tags.extend(c.custom_tags.clone());}
    ensure!(out.tags.len()<=40,"Tối đa 40 trường metadata");
    ensure!(out.tags.iter().map(|t|t.key.len()+t.value.len()).sum::<usize>()<=128*1024,"Tổng metadata tối đa 128 KiB");
    let mut seen=BTreeSet::new();
    for t in &out.tags {
        ensure!(!t.key.is_empty() && t.key.len()<=128 && t.key.bytes().all(|c|c.is_ascii_alphanumeric()||b"._-".contains(&c)),"Tên tag chỉ gồm chữ Latin, số, dấu chấm, gạch dưới/gạch ngang");
        ensure!(t.value.len()<=16384 && !t.value.contains('\0'),"Giá trị metadata quá dài hoặc có ký tự NUL");
        ensure!(!["major_brand","minor_version","compatible_brands","duration","bit_rate","nb_frames","codec_name"].contains(&t.key.to_ascii_lowercase().as_str()),"{} là thông số kỹ thuật, không phải tag tùy chỉnh",t.key);
        ensure!(t.scope!=Scope::Container || !t.key.eq_ignore_ascii_case("encoder"),"Encoder cấp File do thư viện mux ghi tự động; dùng tag software để đặt tên ứng dụng");
        ensure!(seen.insert(format!("{:?}:{}",t.scope,t.key.to_ascii_lowercase())),"Trùng tag {} ở cấp {}",t.key,t.scope.label());
        if t.key.eq_ignore_ascii_case("creation_time") {validate_time(&t.value)?;}
    }
    Ok(out)
}
pub fn inspect(s:&Settings,path:&Path,cancel:&Cancel)->Result<Value> {
    let o=process::run(&s.ffprobe,&["-v".into(),"error".into(),"-show_format".into(),"-show_streams".into(),"-of".into(),"json".into(),path.to_string_lossy().into_owned()],None,cancel,Duration::from_secs(60),None)?;
    let mut actual:Value=serde_json::from_slice(&o.stdout)?;
    let mdta=read_mdta(path)?;
    // ffprobe interprets com.apple.quicktime.artwork as cover artwork and skips
    // its UTF-8 value. Read the actual keys/ilst boxes, not the requested config.
    if let Some(format)=actual.get_mut("format").and_then(Value::as_object_mut) {
        let tags=format.entry("tags").or_insert_with(||json!({}));
        if let Some(tags)=tags.as_object_mut() {for (key,value) in &mdta {tags.entry(key.clone()).or_insert_with(||json!(value));}}
    }
    actual["mdta_text_tags"]=json!(mdta);
    actual["metadata_readers"]=json!({"streams_and_format":"ffprobe","mdta_text":"AIR3view bounded MP4 parser"});
    Ok(actual)
}

#[derive(Clone,Copy)]
struct Atom {kind:[u8;4],start:u64,end:u64}
fn atoms(file:&mut fs::File,start:u64,end:u64)->Result<Vec<Atom>> {
    use std::io::{Read,Seek,SeekFrom};
    let mut at=start;let mut out=vec![];
    while at<end {
        ensure!(end-at>=8,"MP4 có atom header bị cắt");
        file.seek(SeekFrom::Start(at))?;let mut header=[0;8];file.read_exact(&mut header)?;
        let len=u32::from_be_bytes(header[..4].try_into()?) as u64;
        let (size,head)=if len==1 {let mut ext=[0;8];file.read_exact(&mut ext)?;(u64::from_be_bytes(ext),16)}else if len==0 {(end-at,8)}else{(len,8)};
        ensure!(size>=head && size<=end-at,"MP4 có kích thước atom không hợp lệ");
        out.push(Atom{kind:header[4..8].try_into()?,start:at+head,end:at+size});
        ensure!(out.len()<=10000,"MP4 có quá nhiều atom metadata");at+=size;
    }
    Ok(out)
}
fn atom_bytes(file:&mut fs::File,a:Atom,limit:u64)->Result<Vec<u8>> {
    use std::io::{Read,Seek,SeekFrom};
    ensure!(a.end-a.start<=limit,"Khối metadata MP4 vượt giới hạn đọc");
    file.seek(SeekFrom::Start(a.start))?;let mut bytes=vec![0;(a.end-a.start) as usize];file.read_exact(&mut bytes)?;Ok(bytes)
}
/// Read UTF-8 mdta values without reading mdat/video payloads. Nested boxes use
/// validated parent bounds and each metadata payload is limited to 1 MiB.
pub fn read_mdta(path:&Path)->Result<std::collections::BTreeMap<String,String>> {
    let mut file=fs::File::open(path)?;let size=file.metadata()?.len();let mut result=std::collections::BTreeMap::new();
    let mut remaining=2*1024*1024usize;
    for moov in atoms(&mut file,0,size)?.into_iter().filter(|a|a.kind==*b"moov") {
        let children=atoms(&mut file,moov.start,moov.end)?;
        let mut metas:Vec<Atom>=children.iter().copied().filter(|a|a.kind==*b"meta").collect();
        for udta in children.iter().filter(|a|a.kind==*b"udta") {metas.extend(atoms(&mut file,udta.start,udta.end)?.into_iter().filter(|a|a.kind==*b"meta"));}
        for meta in metas {
            ensure!(meta.end-meta.start>=4,"Thiếu MP4 metadata FullBox header");
            let boxes=atoms(&mut file,meta.start+4,meta.end)?;
            let Some(keys)=boxes.iter().find(|a|a.kind==*b"keys") else {continue;};
            ensure!(keys.end-keys.start<=remaining as u64,"Tổng metadata MP4 vượt 2 MiB");
            let data=atom_bytes(&mut file,*keys,1024*1024)?;remaining-=data.len();
            ensure!(data.len()>=8,"Thiếu MP4 metadata key count");
            let count=u32::from_be_bytes(data[4..8].try_into()?) as usize;ensure!(count<=10000,"Quá nhiều metadata keys");
            let mut names=vec![];let mut offset=8;
            for _ in 0..count {
                ensure!(offset+8<=data.len(),"Metadata key bị cắt");
                let len=u32::from_be_bytes(data[offset..offset+4].try_into()?) as usize;
                ensure!(len>=8 && len<=data.len()-offset,"Metadata key sai kích thước");
                names.push(if &data[offset+4..offset+8]==b"mdta" {Some(String::from_utf8(data[offset+8..offset+len].to_vec())?)}else{None});offset+=len;
            }
            if let Some(ilst)=boxes.iter().find(|a|a.kind==*b"ilst") {
                for item in atoms(&mut file,ilst.start,ilst.end)? {
                    let index=u32::from_be_bytes(item.kind) as usize;
                    let Some(Some(name))=index.checked_sub(1).and_then(|i|names.get(i)) else {continue;};
                    for data in atoms(&mut file,item.start,item.end)?.into_iter().filter(|a|a.kind==*b"data") {
                        ensure!(data.end-data.start<=remaining as u64,"Tổng metadata MP4 vượt 2 MiB");remaining-=(data.end-data.start) as usize;
                        let bytes=atom_bytes(&mut file,data,1024*1024)?;
                        if bytes.len()>=8 && bytes[..4]==[0,0,0,1] {result.insert(name.clone(),String::from_utf8(bytes[8..].to_vec())?);}
                    }
                }
            }
        }
    }
    Ok(result)
}
fn tags_for(actual:&Value,scope:Scope)->Option<&Value> {
    if scope==Scope::Container {return actual.get("format")?.get("tags");}
    actual.get("streams")?.as_array()?.iter().find(|s|s["codec_type"]==if scope==Scope::Video {"video"}else{"audio"})?.get("tags")
}
pub fn verify(prepared:&Prepared,actual:&Value)->Result<()> {
    for t in &prepared.tags {
        let value=tags_for(actual,t.scope).and_then(Value::as_object).and_then(|tags|tags.iter().find(|(k,_)|k.eq_ignore_ascii_case(&t.key))).and_then(|(_,v)|v.as_str());
        // FFprobe combines mvhd and mdta creation_time values with a semicolon.
        let same=value.is_some_and(|v|v==t.value || (t.key.eq_ignore_ascii_case("creation_time") && v.split(';').all(|part|part.replace(".000000Z","Z")==t.value)));
        ensure!(same,"Metadata {} / {} không được MP4 ghi đúng. Kiểm tra tên tag/cấp lưu trữ; file đích chưa được thay thế.",t.scope.label(),t.key);
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub profile_version:String, pub preset:Preset, pub enabled:bool, pub output:PathBuf,
    pub export_id:String, pub requested:Vec<Tag>, pub actual:Value, pub warnings:Vec<String>,
    pub mux_ms:u128, pub operation:String,
}
/// Stage on the destination volume, then replace in a single rename. A full disk,
/// cancellation or locked destination never truncates an existing output.
pub fn publish(pending:&Path,output:&Path,cancel:&Cancel)->Result<()> {
    process::check_cancel(cancel)?;
    let parent=output.parent().filter(|p|!p.as_os_str().is_empty()).unwrap_or_else(||Path::new("."));fs::create_dir_all(parent)?;
    let mut staged=tempfile::NamedTempFile::new_in(parent)?;
    let mut input=fs::File::open(pending)?;
    use std::io::{Read,Write};
    let mut buffer=vec![0u8;1024*1024];
    loop {process::check_cancel(cancel)?;let n=input.read(&mut buffer)?;if n==0{break;}staged.write_all(&buffer[..n])?;}
    staged.as_file().sync_all()?;process::check_cancel(cancel)?;
    staged.persist(output).map_err(|e|e.error).with_context(||format!("Không công bố được MP4 tại {}. File cũ được giữ nguyên.",output.display()))?;Ok(())
}
pub fn finish(p:&Project,s:&Settings,pending:&Path,output:&Path,prepared:Prepared,timing:(u128,&str),cancel:&Cancel)->Result<Report> {
    let (mux_ms,operation)=timing;
    let actual=inspect(s,pending,cancel)?;verify(&prepared,&actual)?;
    // Do not expose the temporary path as the exported file's metadata.
    let mut actual=actual;if let Some(format)=actual.get_mut("format").and_then(Value::as_object_mut){format.insert("filename".into(),json!(output));}
    let report=Report{profile_version:PROFILE_VERSION.into(),preset:prepared.preset,enabled:prepared.enabled,output:output.into(),export_id:prepared.export_id,requested:prepared.tags,actual,warnings:prepared.warnings,mux_ms,operation:operation.into()};
    // Prepare the report before replacing the video. Private staging also keeps
    // simultaneous exports from sharing a .tmp file.
    fs::create_dir_all(&p.root)?;
    let mut staged=tempfile::NamedTempFile::new_in(&p.root)?;
    serde_json::to_writer_pretty(staged.as_file_mut(),&report)?;staged.as_file().sync_all()?;
    publish(pending,output,cancel)?;
    staged.persist(p.root.join("metadata-report.json")).map_err(|e|e.error).context("MP4 đã lưu nhưng không ghi được metadata-report.json")?;
    Ok(report)
}
pub fn apply_to_mp4(p:&Project,s:&Settings,input:&Path,output:&Path,cancel:&Cancel)->Result<Report> {
    ensure!(p.export_metadata.enabled,"Bật Metadata video trước khi cập nhật MP4");
    ensure!(input.is_file(),"MP4 nguồn không tồn tại");
    ensure!(!output.exists(),"File đích đã tồn tại; chọn tên khác để giữ video cũ");
    let mut prepared=prepare(p,s)?;
    if prepared.preset==Preset::CapcutDesktop {
        prepared.tags.retain(|t|t.key!="Hw");
        prepared.warnings.push("Metadata-only không mã hóa hình/tiếng; bỏ Hw vì không xác minh được bộ mã hóa của file nguồn.".into());
    }
    let source=inspect(s,input,cancel)?;
    let parent=output.parent().filter(|p|!p.as_os_str().is_empty()).unwrap_or_else(||Path::new("."));fs::create_dir_all(parent)?;
    let scratch=tempfile::Builder::new().prefix(".air3view-metadata-").tempdir_in(parent)?;
    let pending=scratch.path().join("pending.mp4");let clock=Instant::now();
    let mut args=vec!["-y".into(),"-v".into(),"error".into(),"-i".into(),input.to_string_lossy().into_owned(),"-map".into(),"0".into(),"-c".into(),"copy".into()];
    args.extend(prepared.args());args.push(pending.to_string_lossy().into_owned());
    process::run(&s.ffmpeg,&args,None,cancel,Duration::from_secs(s.timeout_seconds),None)?;
    let mux_ms=clock.elapsed().as_millis();
    let actual=inspect(s,&pending,cancel)?;
    let shape=|v:&Value|->Vec<Value>{v["streams"].as_array().into_iter().flatten().map(|s|json!([s["codec_type"],s["codec_name"],s["width"],s["height"],s["sample_rate"],s["channels"]])).collect()};
    ensure!(shape(&source)==shape(&actual),"MP4 đổi thông số stream sau khi ghi metadata");
    let duration=|v:&Value|v["format"]["duration"].as_str().and_then(|v|v.parse::<f64>().ok()).unwrap_or(0.);
    ensure!((duration(&source)-duration(&actual)).abs()<0.1,"MP4 đổi thời lượng sau khi ghi metadata");
    finish(p,s,&pending,output,prepared,(mux_ms,"metadata_only"),cancel)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn dates_validation_and_old_project_defaults() {
        assert_eq!(utc(0),"1970-01-01T00:00:00Z");assert_eq!(utc(1792744710),"2026-10-23T08:38:30Z");
        for s in ["2024-02-29T12:00:00Z","2026-10-23T08:00:00Z"] {validate_time(s).unwrap();}
        for s in ["2025-02-29T00:00:00Z","2026-13-01T00:00:00Z","2026-10-00T24:00:00Z","💥"] {assert!(validate_time(s).is_err());}
        let p:Project=serde_json::from_str("{}").unwrap();assert!(!p.export_metadata.enabled);assert!(Project::default().export_metadata.enabled);
        let old:crate::model::ProjectDefaults=serde_json::from_str("{}").unwrap();assert!(!old.export_metadata.enabled);
        let mut p=Project::default();p.export_metadata.author="Kênh của tôi".into();p.export_metadata.preset=Preset::CapcutDesktop;
        let prefs=crate::model::ProjectDefaults::from_project(&p);let loaded=serde_json::from_slice::<crate::model::ProjectDefaults>(&serde_json::to_vec(&prefs).unwrap()).unwrap();
        let mut next=Project::default();loaded.apply(&mut next);assert_eq!(next.export_metadata.author,"Kênh của tôi");assert_eq!(next.export_metadata.preset,Preset::CapcutDesktop);
        p.export_metadata.preset=Preset::Custom;
        p.export_metadata.custom_tags=(0..9).map(|i|Tag{scope:Scope::Container,key:format!("tag_{i}"),value:"x".repeat(16000)}).collect();
        assert!(prepare(&p,&Settings::default()).unwrap_err().to_string().contains("128 KiB"));
    }
    #[test] fn malformed_atoms_are_bounded_and_rejected() {
        let temp=tempfile::tempdir().unwrap();let path=temp.path().join("invalid.mp4");
        for bytes in [vec![0,0,0,20,b'm',b'o',b'o',b'v'],vec![0,0,0,1,b'm',b'o',b'o',b'v'],vec![0,0,0,7,b'f',b'r',b'e',b'e'],vec![0,0,0]] {
            fs::write(&path,bytes).unwrap();assert!(read_mdta(&path).is_err());
        }
        // A valid large-size empty moov and a size-zero empty free box.
        fs::write(&path,[&[0,0,0,1,b'm',b'o',b'o',b'v'][..],&16u64.to_be_bytes()].concat()).unwrap();assert!(read_mdta(&path).unwrap().is_empty());
        fs::write(&path,[0,0,0,0,b'f',b'r',b'e',b'e']).unwrap();assert!(read_mdta(&path).unwrap().is_empty());
        fn atom(kind:[u8;4],payload:Vec<u8>)->Vec<u8> {[(payload.len() as u32+8).to_be_bytes().to_vec(),kind.to_vec(),payload].concat()}
        let key=atom(*b"mdta",b"comment".to_vec());
        let keys=atom(*b"keys",[vec![0;4],1u32.to_be_bytes().to_vec(),key].concat());
        let value=atom(*b"data",[vec![0,0,0,1,0,0,0,0],vec![b'x';800000]].concat());
        let item=atom(1u32.to_be_bytes(),[value.clone(),value.clone(),value].concat());
        let ilst=atom(*b"ilst",item);let meta=atom(*b"meta",[vec![0;4],keys,ilst].concat());
        fs::write(&path,atom(*b"moov",atom(*b"udta",meta))).unwrap();
        assert!(read_mdta(&path).unwrap_err().to_string().contains("2 MiB"));
    }
}
