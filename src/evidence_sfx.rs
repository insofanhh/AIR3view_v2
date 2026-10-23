//! Embedded CC0 reveal sounds. One local copy per content version; no network/provider.
use std::{path::PathBuf,sync::OnceLock};
pub fn sound(index:usize,kind:&str)->Option<(PathBuf,u64)> {
    static LIBRARY:OnceLock<Vec<Option<(PathBuf,u64)>>>=OnceLock::new();
    let library=LIBRARY.get_or_init(||{
        let root=crate::model::data_dir().join("assets/evidence-sfx-v1");
        let _=std::fs::create_dir_all(&root);
        let _=std::fs::write(root.join("CREDITS.md"),include_bytes!("../resources/sfx/CREDITS.md"));
        [ ("whoosh",include_bytes!("../resources/sfx/whoosh.wav").as_slice()),
          ("pop",include_bytes!("../resources/sfx/pop.wav").as_slice()),
          ("ting",include_bytes!("../resources/sfx/ting.wav").as_slice()) ].iter().map(|(name,bytes)|{
            let path=root.join(format!("{name}-{}.wav",&crate::media::hash(bytes)[..12]));
            if !path.is_file() {let pending=path.with_extension("pending.wav");std::fs::write(&pending,bytes).ok()?;std::fs::rename(pending,&path).ok()?;}
            // All embedded files are PCM16 mono 48 kHz produced by the asset import.
            let data=bytes.windows(4).position(|w|w==b"data")?;
            let length=u32::from_le_bytes(bytes.get(data+4..data+8)?.try_into().ok()?);
            Some((path,length as u64*1000/96000))
        }).collect()
    });
    library[if matches!(kind,"stat"|"fact")||index%3==2{2}else if index==0{0}else{1}].clone()
}
