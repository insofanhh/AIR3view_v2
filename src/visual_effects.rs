//! Non-destructive picture effects shared by preview and the export graph.
use crate::{composition::LayerClip,model::*,timeline};
use anyhow::{ensure,Context,Result};
use serde::{Deserialize,Serialize};
use std::{collections::BTreeMap,fs,path::{Path,PathBuf},sync::{Mutex,OnceLock}};

#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
#[serde(default)]
pub struct Effect {pub mirror_x:bool,pub mirror_y:bool,pub lut:String,pub strength:f32}
impl Default for Effect {fn default()->Self{Self{mirror_x:false,mirror_y:false,lut:"off".into(),strength:0.35}}}
#[derive(Clone,Debug,Default,Serialize,Deserialize)]
#[serde(default)]
pub struct Settings {pub footage:Effect,pub layers:BTreeMap<String,Effect>}
impl Effect {
    pub fn validate(&self)->Result<()> {
        ensure!(self.strength.is_finite()&&(0.0..=1.).contains(&self.strength),"Cường độ LUT phải từ 0–100%");
        if !matches!(self.lut.as_str(),"off"|"warm"|"cool"|"documentary") {cube(Path::new(&self.lut))?;}
        Ok(())
    }
    pub fn active(&self)->bool {self.mirror_x||self.mirror_y||(self.lut!="off"&&self.strength>0.)}
}
pub fn selected<'a>(p:&'a Project,c:&LayerClip)->Option<&'a Effect> {
    p.effects.layers.get(&c.id).or_else(||if matches!(c.track,timeline::VIDEO|timeline::BACKGROUND)&&!c.still(){Some(&p.effects.footage)}else{None})
}
pub fn fingerprint(path:&Path)->String {
    type Cache=BTreeMap<PathBuf,(u64,Option<std::time::SystemTime>,String)>;
    static CACHE:OnceLock<Mutex<Cache>>=OnceLock::new();
    let Ok(m)=fs::metadata(path) else{return format!("missing:{}",path.display());};
    let modified=m.modified().ok();let cache=CACHE.get_or_init(Default::default);
    if let Ok(cache)=cache.lock(){if let Some((len,time,key))=cache.get(path){if *len==m.len()&&*time==modified{return key.clone();}}}
    let key=crate::input_assets::file_fingerprint(&path.to_string_lossy());
    if let Ok(mut cache)=cache.lock(){if cache.len()>128{cache.clear();}cache.insert(path.to_owned(),(m.len(),modified,key.clone()));}key
}
pub fn identity(p:&Project,c:&LayerClip)->String {
    selected(p,c).map(|e|crate::media::hash(format!("{e:?}:{}",if matches!(e.lut.as_str(),"off"|"warm"|"cool"|"documentary"){String::new()}else{fingerprint(Path::new(&e.lut))}))).unwrap_or_default()
}
fn cube(path:&Path)->Result<(usize,Vec<[f32;3]>)> {
    ensure!(fs::metadata(path).with_context(||format!("Không tìm thấy LUT: {}",path.display()))?.len()<=16*1024*1024,"LUT vượt 16 MB");
    let text=fs::read_to_string(path).context("LUT cần là file .cube dạng văn bản")?;
    let mut size=0;let mut values=vec![];
    for line in text.lines(){let line=line.split('#').next().unwrap_or("").trim();if line.is_empty()||line.starts_with("TITLE"){continue;}
        let fields:Vec<_>=line.split_whitespace().collect();
        match fields[0]{
            "LUT_3D_SIZE"=>{size=fields.get(1).context("LUT thiếu kích thước")?.parse()?;ensure!((2..=65).contains(&size),"LUT 3D cần kích thước 2–65");},
            "DOMAIN_MIN"|"DOMAIN_MAX"=>{ensure!(fields.len()==4,"DOMAIN LUT không hợp lệ");let expected=if fields[0]=="DOMAIN_MIN"{0.}else{1.};for v in &fields[1..]{ensure!((v.parse::<f32>()?-expected).abs()<0.00001,"Bản này hỗ trợ LUT miền RGB 0–1");}},
            _=>{ensure!(fields.len()==3,"LUT có dòng không hợp lệ");let v=[fields[0].parse::<f32>()?,fields[1].parse()?,fields[2].parse()?];ensure!(v.iter().all(|v|v.is_finite()),"LUT có số không hợp lệ");values.push(v);}
        }
    }
    ensure!(size>=2&&values.len()==size*size*size,"LUT thiếu hoặc thừa dữ liệu màu");Ok((size,values))
}
pub fn import(source:&Path,folder:&Path)->Result<PathBuf>{cube(source)?;fs::create_dir_all(folder)?;let path=folder.join(format!("{}.cube",fingerprint(source)));if !path.exists(){fs::copy(source,&path)?;}Ok(path)}
fn lut_file(p:&Project,e:&Effect)->Result<PathBuf> {
    e.validate()?;
    let id=if matches!(e.lut.as_str(),"warm"|"cool"|"documentary"){e.lut.clone()}else{fingerprint(Path::new(&e.lut))};
    let folder=p.root.join("cache/luts");fs::create_dir_all(&folder)?;
    let path=folder.join(format!("{}.cube",crate::media::hash(format!("lut-v1:{id}:{}",e.strength))));if path.is_file(){return Ok(path);}
    let (size,values)=if matches!(e.lut.as_str(),"warm"|"cool"|"documentary"){(17,vec![])}else{cube(Path::new(&e.lut))?};
    let mut text=format!("LUT_3D_SIZE {size}\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 1 1 1\n");
    for b in 0..size{for g in 0..size{for r in 0..size{
        let base=[r as f32/(size-1) as f32,g as f32/(size-1) as f32,b as f32/(size-1) as f32];
        let target=match e.lut.as_str(){"warm"=>[base[0]*1.04+0.015,base[1],base[2]*0.94],"cool"=>[base[0]*0.96,base[1],base[2]*1.05+0.01],"documentary"=>base.map(|v|(v-0.5)*1.08+0.5),_=>values[(b*size+g)*size+r]};
        let v=std::array::from_fn::<_,3,_>(|i|base[i]+(target[i]-base[i])*e.strength);text.push_str(&format!("{:.7} {:.7} {:.7}\n",v[0],v[1],v[2]));
    }}}
    let temp=tempfile::NamedTempFile::new_in(&folder)?;fs::write(temp.path(),text)?;temp.persist(&path).map_err(|e|e.error)?;Ok(path)
}
pub fn filters(p:&Project,c:&LayerClip)->Result<String>{
    let Some(e)=selected(p,c) else{return Ok(String::new());};e.validate()?;let mut list=vec![];
    if e.mirror_x{list.push("hflip".into());}
    if e.mirror_y{list.push("vflip".into());}
    if e.lut!="off"&&e.strength>0. {let path=lut_file(p,e)?.to_string_lossy().replace('\\',"/").replace(':',"\\:").replace('\'',"'\\''");list.push(format!("lut3d=file='{path}':interp=trilinear"));}
    Ok(if list.is_empty(){String::new()}else{format!(",{}",list.join(","))})
}
