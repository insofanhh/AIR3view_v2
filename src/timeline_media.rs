//! Bounded background thumbnail/waveform extraction. No decoding on the UI thread.
use crate::{media, model::Settings, process};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    sync::{atomic::Ordering, mpsc},
    thread,
    time::Duration,
};

pub enum Data {
    Thumbnail(image::RgbaImage),
    Waveform(Vec<f32>),
    Failed,
}
pub struct Request {
    pub key: String,
    pub path: PathBuf,
    pub start: u64,
    pub duration: u64,
    pub waveform: bool,
    pub folder: PathBuf,
    pub settings: Settings,
}
pub struct MediaCache {
    tx: mpsc::Sender<Request>,
    rx: mpsc::Receiver<(String, Data)>,
    cancel: process::Cancel,
    requested: BTreeSet<String>,
    pub ready: BTreeMap<String, Data>,
}
impl Default for MediaCache {
    fn default() -> Self {
        let (tx, jobs) = mpsc::channel::<Request>();
        let (results, rx) = mpsc::channel();
        let cancel = process::cancel_token();
        let token = cancel.clone();
        thread::spawn(move || {
            while let Ok(job) = jobs.recv() {
                if token.load(Ordering::Relaxed) {
                    break;
                }
                let data = extract(&job, &token).unwrap_or(Data::Failed);
                if results.send((job.key, data)).is_err() {
                    break;
                }
            }
        });
        Self {
            tx,
            rx,
            cancel,
            requested: Default::default(),
            ready: Default::default(),
        }
    }
}
impl Drop for MediaCache {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl MediaCache {
    pub fn request(&mut self, job: Request) {
        if self.requested.insert(job.key.clone()) {
            let _ = self.tx.send(job);
        }
    }
    pub fn poll(&mut self) {
        while let Ok((key, data)) = self.rx.try_recv() {
            self.ready.insert(key, data);
        }
    }
}
pub fn identity(path: &std::path::Path, start: u64, duration: u64, wave: bool) -> String {
    let metadata = fs::metadata(path).ok();
    media::hash(format!(
        "timeline-media-v1:{}:{}:{:?}:{start}:{duration}:{wave}",
        path.display(),
        metadata.as_ref().map(|m| m.len()).unwrap_or(0),
        metadata.and_then(|m| m.modified().ok())
    ))
}
fn extract(j: &Request, cancel: &process::Cancel) -> anyhow::Result<Data> {
    fs::create_dir_all(&j.folder)?;
    if !j.waveform {
        let file = j.folder.join(format!("{}.png", j.key));
        if file.is_file() {
            return Ok(Data::Thumbnail(image::open(file)?.to_rgba8()));
        }
        let still=j.path.extension().is_some_and(|e|e.eq_ignore_ascii_case("png")||e.eq_ignore_ascii_case("jpg"));
        let frame=if still{image::open(&j.path)?.to_rgba8()}else{media::frame(&j.settings,&j.path,j.start+j.duration/2,cancel)?};
        let source_view=j.folder.file_name().is_some_and(|v|v=="evidence-ui-source");
        let frame = image::DynamicImage::ImageRgba8(frame)
            .thumbnail(if source_view{1280}else{112}, if source_view{720}else{64})
            .to_rgba8();
        frame.save(file)?;
        return Ok(Data::Thumbnail(frame));
    }
    let file = j.folder.join(format!("{}.peaks.json", j.key));
    if file.is_file() {
        return Ok(Data::Waveform(serde_json::from_slice(&fs::read(file)?)?));
    }
    let raw = j.folder.join(format!("{}.pcm", j.key));
    process::run(
        &j.settings.ffmpeg,
        &process::args(&[
            "-y",
            "-v",
            "error",
            "-ss",
            &format!("{:0.3}", j.start as f64 / 1000.),
            "-i",
            &j.path.to_string_lossy(),
            "-t",
            &format!("{:0.3}", j.duration as f64 / 1000.),
            "-vn",
            "-ac",
            "1",
            "-ar",
            "2000",
            "-f",
            "s16le",
            &raw.to_string_lossy(),
        ]),
        None,
        cancel,
        Duration::from_secs(45),
        None,
    )?;
    let bytes = fs::read(&raw)?;
    let _ = fs::remove_file(raw);
    let peaks = waveform(&bytes, 256);
    fs::write(file, serde_json::to_vec(&peaks)?)?;
    Ok(Data::Waveform(peaks))
}
pub fn waveform(bytes: &[u8], bins: usize) -> Vec<f32> {
    let samples: Vec<_> = bytes
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.)
        .collect();
    (0..bins)
        .map(|i| {
            let start = samples.len() * i / bins;
            let end = samples.len() * (i + 1) / bins;
            samples[start..end]
                .iter()
                .map(|x| x.abs())
                .fold(0., f32::max)
        })
        .collect()
}
#[cfg(test)]
mod tests {
    #[test]
    fn waveform_uses_real_samples_and_retains_silence() {
        let samples: Vec<u8> = [0i16, 0, 16000, -32000, 0, 0, 8000, 4000]
            .iter()
            .flat_map(|n| n.to_le_bytes())
            .collect();
        let peaks = super::waveform(&samples, 4);
        assert_eq!(peaks[0], 0.);
        assert!(peaks[1] > 0.9);
        assert_eq!(peaks[2], 0.);
        assert!(peaks[3] > 0.2 && peaks[3] < 0.3);
    }
}
