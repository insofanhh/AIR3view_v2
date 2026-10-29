use crate::{
    model::*,
    process::{self, Cancel},
    subtitle,
};
use anyhow::{bail, ensure, Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

pub fn hash(data: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(data.as_ref()))
}
fn strpath(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}
pub fn probe(settings: &Settings, path: &Path, cancel: &Cancel) -> Result<Source> {
    let o = process::run(
        &settings.ffprobe,
        &[
            "-v".into(),
            "error".into(),
            "-show_format".into(),
            "-show_streams".into(),
            "-of".into(),
            "json".into(),
            strpath(path),
        ],
        None,
        cancel,
        Duration::from_secs(60),
        None,
    )?;
    let v: Value = serde_json::from_slice(&o.stdout)?;
    let streams = v["streams"].as_array().context("Video không có stream")?;
    let visual = streams.iter().find(|s| s["codec_type"] == "video");
    let rotation=visual.and_then(|v|v["side_data_list"].as_array()).and_then(|v|v.iter().find_map(|v|v["rotation"].as_f64())).unwrap_or(0.);
    let rotated=(rotation.abs().round() as i32).rem_euclid(180)==90;
    let width=visual.and_then(|v|v["width"].as_u64()).unwrap_or(0) as u32;
    let height=visual.and_then(|v|v["height"].as_u64()).unwrap_or(0) as u32;
    let seconds = v["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);
    ensure!(
        seconds.is_finite() && seconds > 0.0,
        "Không đọc được thời lượng media"
    );
    Ok(Source {
        path: path.to_owned(),
        duration_ms: (seconds * 1000.0).round() as u64,
        width: if rotated{height}else{width},
        height: if rotated{width}else{height},
        has_audio: streams.iter().any(|s| s["codec_type"] == "audio"),
        ..Default::default()
    })
}
pub fn import_local(
    settings: &Settings,
    path: &Path,
    id: String,
    cancel: &Cancel,
) -> Result<Source> {
    let absolute = fs::canonicalize(path)?;
    let mut s = probe(settings, &absolute, cancel)?;
    ensure!(s.width > 0, "File không có video");
    s.id = id;
    s.title = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    Ok(s)
}
pub fn download(
    settings: &Settings,
    url: &str,
    folder: &Path,
    id: String,
    cancel: &Cancel,
) -> Result<Source> {
    let url = validate_youtube_url(url)?;
    fs::create_dir_all(folder).with_context(||format!("Không tạo/ghi được thư mục tải video nguồn: {}",folder.display()))?;
    let cache = folder.join(format!("{}.json", hash(&url)));
    if cache.is_file() {
        if let Ok(s) = serde_json::from_slice::<Source>(&fs::read(&cache)?) {
            if s.path.is_file() {
                return Ok(Source { id, ..s });
            }
        }
    }
    let template = folder.join(format!("{}.%(ext)s", hash(&url)));
    let cookies = crate::youtube_cookies::DownloadCookies::from_setting(&settings.youtube_cookie_path)?;
    let mut args = vec![
        "--ignore-config".into(),
        "--js-runtimes".into(),
        youtube_js_runtime(settings),
        "--no-playlist".into(),
        "--no-mtime".into(),
        "--no-progress".into(),
        "--restrict-filenames".into(),
        "--write-info-json".into(),
        "--format".into(),
        "bv*[height<=1080]+ba/b[height<=1080]/best".into(),
        "--merge-output-format".into(),
        "mp4".into(),
        "--print".into(),
        "after_move:filepath".into(),
        "-o".into(),
        strpath(&template),
        "--".into(),
        url.clone(),
    ];
    // yt-dlp treats --ffmpeg-location as a filesystem path, not a PATH command.
    if Path::new(&settings.ffmpeg).is_file() {
        args.splice(0..0, ["--ffmpeg-location".into(), settings.ffmpeg.clone()]);
    }
    if let Some(cookies) = &cookies {
        args.splice(0..0, ["--cookies".into(), strpath(cookies.path())]);
    }
    let o = process::run(
        &settings.ytdlp,
        &args,
        None,
        cancel,
        Duration::from_secs(settings.timeout_seconds.max(60)),
        None,
    ).map_err(|error| {
        let message = format!("{error:#}").to_ascii_lowercase();
        if ["challenge solving failed", "no supported javascript runtime", "javascript runtime not found", "could not find a supported javascript runtime"].iter()
            .any(|text| message.contains(text)) {
            return error.context("yt-dlp không giải được JavaScript challenge của YouTube. Kiểm tra Node.js (từ 22) tại Cài đặt kết nối → Nâng cao: JavaScript YouTube; đây là lỗi bộ tải, chưa phải kết luận cookie sai. Bản yt-dlp chính thức cần kèm EJS phù hợp.");
        }
        if ["confirm your age", "age-restricted", "age restricted", "sign in to confirm", "cookies are no longer valid"].iter()
            .any(|text| message.contains(text)) {
            error.context(if cookies.is_some() {
                "YouTube chưa chấp nhận phiên cookie hoặc tài khoản chưa có quyền xem video. Đăng nhập tài khoản xem được video, xuất cookie Netscape mới rồi chọn lại trong Cài đặt kết nối và Tiếp tục / sửa phần lỗi."
            } else {
                "Video yêu cầu đăng nhập/xác nhận tuổi. Chọn file cookie YouTube (Netscape .txt) trong Cài đặt kết nối, Lưu cấu hình rồi Tiếp tục / sửa phần lỗi."
            })
        } else { error }
    })?;
    let file = String::from_utf8_lossy(&o.stdout)
        .lines()
        .rev()
        .find(|l| Path::new(l.trim()).is_file())
        .map(|s| PathBuf::from(s.trim()))
        .context("yt-dlp không trả đường dẫn video")?;
    let mut source = probe(settings, &file, cancel)?;
    source.id = id;
    source.url = url;
    source.title = file
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let info = folder.join(format!("{}.info.json", hash(&source.url)));
    if let Ok(text) = fs::read(&info) {
        if let Ok(v) = serde_json::from_slice::<Value>(&text) {
            source.title = v["title"].as_str().unwrap_or(&source.title).into();
        }
    }
    write_json(&cache, &source)?;
    Ok(source)
}

pub fn youtube_js_runtime(settings: &Settings) -> String {
    let node = settings.youtube_node.trim();
    if node.is_empty() || node == "node" { "node".into() }
    else { format!("node:{node}") }
}
pub fn proxy(
    settings: &Settings,
    source: &Source,
    folder: &Path,
    cancel: &Cancel,
) -> Result<PathBuf> {
    fs::create_dir_all(folder)?;
    let key = hash(format!(
        "{}:{}:{}:{:?}",
        source.path.display(),
        source.duration_ms,
        fs::metadata(&source.path)?.len(),fs::metadata(&source.path)?.modified().ok()
    ));
    let path = folder.join(format!("{key}.mp4"));
    if path.is_file() && probe(settings, &path, cancel).is_ok() {
        return Ok(path);
    }
    let args = vec![
        "-y".into(),
        "-v".into(),
        "error".into(),
        "-i".into(),
        strpath(&source.path),
        "-map".into(),
        "0:v:0".into(),
        "-map".into(),
        "0:a?".into(),
        "-vf".into(),
        "scale=640:640:force_original_aspect_ratio=decrease:force_divisible_by=2,fps=24".into(),
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "ultrafast".into(),
        "-crf".into(),
        "28".into(),
        "-c:a".into(),
        "aac".into(),
        "-movflags".into(),
        "+faststart".into(),
        strpath(&path),
    ];
    process::run(
        &settings.ffmpeg,
        &args,
        None,
        cancel,
        Duration::from_secs(settings.timeout_seconds),
        None,
    )?;
    Ok(path)
}
pub fn frame(
    settings: &Settings,
    path: &Path,
    ms: u64,
    cancel: &Cancel,
) -> Result<image::RgbaImage> {
    let args = vec![
        "-v".into(),
        "error".into(),
        "-ss".into(),
        format!("{:.3}", ms as f64 / 1000.0),
        "-i".into(),
        strpath(path),
        "-frames:v".into(),
        "1".into(),
        "-vf".into(),
        "scale=720:720:force_original_aspect_ratio=decrease".into(),
        "-f".into(),
        "image2pipe".into(),
        "-vcodec".into(),
        "png".into(),
        "-pix_fmt".into(),
        "rgba".into(),
        "pipe:1".into(),
    ];
    let o = process::run(
        &settings.ffmpeg,
        &args,
        None,
        cancel,
        Duration::from_secs(30),
        None,
    )?;
    Ok(image::load_from_memory(&o.stdout)?.to_rgba8())
}
pub fn snapshot(
    settings: &Settings,
    path: &Path,
    ms: u64,
    output: &Path,
    cancel: &Cancel,
) -> Result<()> {
    frame(settings, path, ms, cancel)?.save(output)?;
    Ok(())
}

fn media_identity(path: &Path) -> String {
    match fs::metadata(path) {
        Ok(m) => format!("{}:{}:{:?}", path.display(), m.len(), m.modified().ok()),
        Err(_) => path.display().to_string(),
    }
}
pub fn segment_key(p: &Project, s: &Segment, settings: &Settings) -> String {
    let source = p
        .source(&s.source_id)
        .ok()
        .map(|s| media_identity(&s.path))
        .unwrap_or_default();
    hash(format!(
        "render-v9-footage-speed:{}:{}:{}:{}:{}:{}:{}:{}",
        serde_json::to_string(s).unwrap_or_default(),
        serde_json::to_string(&p.output).unwrap_or_default(),
        serde_json::to_string(&p.subtitles).unwrap_or_default(),
        serde_json::to_string(&p.timeline.tracks.iter().map(|t|t.muted).collect::<Vec<_>>()).unwrap_or_default(),
        settings.encoder,
        source,
        s.audio
            .as_ref()
            .map(|p| media_identity(p))
            .unwrap_or_default(),
        s.avatar
            .as_ref()
            .map(|p| media_identity(p))
            .unwrap_or_default()
    ))
}
pub fn render_segment(
    p: &Project,
    s: &Segment,
    settings: &Settings,
    folder: &Path,
    cancel: &Cancel,
) -> Result<PathBuf> {
    fs::create_dir_all(folder)?;
    let key = segment_key(p, s, settings);
    let work = folder.join(&key);
    fs::create_dir_all(&work)?;
    let output = work.join("segment.mp4");
    if output.is_file() && probe(settings, &output, cancel).is_ok() {
        return Ok(output);
    }
    if (s.source_rate-1.).abs()>0.00001 || !s.footage_spans.is_empty() {
        let mut scene=p.clone();scene.segments=vec![s.clone()];scene.composition=None;scene.ensure_composition();
        crate::layer_media::export(&scene,settings,&output,cancel)?;
        return Ok(output);
    }
    let (w, h) = p.output.dimensions();
    let fps = p.output.fps;
    let seconds = s.duration_ms as f64 / 1000.0;
    let mut args = vec!["-y".into(), "-v".into(), "error".into()];
    let mut filters = vec![];
    let mut audio_map: String;
    let mut mixed_original=false;
    let video_index: usize;
    let mut avatar_index = None;
    if s.kind == "host" {
        let av = s.avatar.as_ref().context(
            "Cảnh host cần nhân vật. Chọn 2D local hoặc HeyGen rồi tạo tài nguyên trước.",
        )?;
        args.extend(["-ss".into(), format!("{:.3}", s.avatar_start_ms as f64/1000.), "-i".into(), strpath(av)]);
        video_index = 0;
        let audio = s.audio.as_ref().context("Cảnh host thiếu audio")?;
        args.extend(["-ss".into(), format!("{:.3}", s.asset_start_ms as f64/1000.), "-i".into(), strpath(audio)]);
        audio_map = "1:a:0".into();
    } else {
        let source = p.source(&s.source_id)?;
        let length = (s.out_ms - s.in_ms) as f64 / 1000.0;
        ensure!(
            seconds <= length + 2.0,
            "Cảnh {} thiếu {:.1}s hình ảnh cho thoại; kéo dài cảnh hoặc rút ngắn thoại",
            s.id,
            seconds - length
        );
        args.extend([
            "-ss".into(),
            format!("{:.3}", s.in_ms as f64 / 1000.0),
            "-t".into(),
            format!("{length:.3}"),
            "-i".into(),
            strpath(&source.path),
        ]);
        video_index = 0;
        if s.kind == "review" {
            let audio = s.audio.as_ref().context("Cảnh review chưa tạo giọng")?;
            args.extend(["-ss".into(), format!("{:.3}", s.asset_start_ms as f64/1000.), "-i".into(), strpath(audio)]);
            audio_map = "1:a:0".into();
            if let Some(av) = s.avatar.as_ref().filter(|_| !p.timeline.tracks[crate::timeline::AVATAR].muted) {
                args.extend(["-ss".into(), format!("{:.3}", s.avatar_start_ms as f64/1000.), "-i".into(), strpath(av)]);
                avatar_index = Some(2);
            }
        } else if source.has_audio {
            audio_map = "0:a:0".into();
        } else {
            args.extend([
                "-f".into(),
                "lavfi".into(),
                "-i".into(),
                "anullsrc=r=48000:cl=stereo".into(),
            ]);
            audio_map = "1:a:0".into();
        }
    }
    let mode = if p.output.fit == "cover" {
        "increase"
    } else {
        "decrease"
    };
    let sw = (w as f32 * p.output.scale).round() as u32 / 2 * 2;
    let sh = (h as f32 * p.output.scale).round() as u32 / 2 * 2;
    let background = if p.output.editorial {
        "0xF4F0E6"
    } else {
        "0x0B1220"
    };
    let hidden = p.timeline.tracks[if s.kind == "host" {crate::timeline::AVATAR} else {crate::timeline::VIDEO}].muted;
    let foreground_input = if p.output.blur_background && s.kind != "host" && !hidden {
        // Split the same decoded clip: background stays synchronized with footage.
        // Blur at reduced resolution, then upscale to keep the CPU cost bounded.
        filters.push(format!("[{video_index}:v]split=2[bgsrc][fgsrc]"));
        let bw = (w / 4 / 2 * 2).max(2);
        let bh = (h / 4 / 2 * 2).max(2);
        filters.push(format!("[bgsrc]setpts=PTS-STARTPTS,scale={bw}:{bh}:force_original_aspect_ratio=increase,crop={bw}:{bh},gblur=sigma=12,eq=brightness=-0.12,scale={w}:{h},setsar=1,fps={fps},tpad=stop_mode=clone:stop_duration=2[bg]"));
        "fgsrc".to_string()
    } else {
        filters.push(format!("color=c={background}:s={w}x{h}:r={fps}:d={seconds:.3}[bg]"));
        format!("{video_index}:v")
    };
    let motion = if s.motion == "push_in" {
        if p.output.blur_background && mode == "decrease" && s.kind != "host" {
            let src = p.source(&s.source_id)?;
            let ratio = (sw as f64 / src.width.max(1) as f64)
                .min(sh as f64 / src.height.max(1) as f64);
            let fw = ((src.width as f64 * ratio) as u32 / 2 * 2).max(2);
            let fh = ((src.height as f64 * ratio) as u32 / 2 * 2).max(2);
            // Keep the zoom inside the fitted source rectangle, so opaque
            // portrait padding cannot hide the synchronized blurred background.
            format!(",zoompan=z='1+0.05*min(on/({seconds:.3}*{fps}),1)':x='iw/2-iw/zoom/2':y='ih/2-ih/zoom/2':d=1:s={fw}x{fh}:fps={fps}")
        } else {
        let frame = if mode == "increase" {
            format!("crop={sw}:{sh}")
        } else {
            format!("pad={sw}:{sh}:(ow-iw)/2:(oh-ih)/2:color={background}")
        };
        format!(",{frame},zoompan=z='1+0.05*min(on/({seconds:.3}*{fps}),1)':x='iw/2-iw/zoom/2':y='ih/2-ih/zoom/2':d=1:s={sw}x{sh}:fps={fps}")
        }
    } else {
        String::new()
    };
    if hidden {
        filters.push(format!("color=c={background}:s={sw}x{sh}:r={fps}:d={seconds:.3}[fg]"));
    } else {
        filters.push(format!("[{foreground_input}]setpts=PTS-STARTPTS,scale={sw}:{sh}:force_original_aspect_ratio={mode}:force_divisible_by=2,setsar=1,fps={fps}{motion},tpad=stop_mode=clone:stop_duration=2[fg]"));
    }
    let shake = if s.motion == "reaction_shake" {
        "+if(lt(t,0.45),5*sin(55*t),0)"
    } else {
        ""
    };
    filters.push(format!(
        "[bg][fg]overlay=x='(W-w)*{:.5}{shake}':y='(H-h)*{:.5}':shortest=1[base]",
        p.output.x, p.output.y,
    ));
    let mut label = "base";
    if let Some(i) = avatar_index {
        let avatar_scale = if s.avatar_size == "large" {
            (p.output.avatar_scale * 1.6).min(0.8)
        } else {
            p.output.avatar_scale
        };
        let aw = (w as f32 * avatar_scale).round() as u32 / 2 * 2;
        filters.push(format!("[{i}:v]setpts=PTS-STARTPTS,scale={aw}:-2,setsar=1,fps={fps},tpad=stop_mode=clone:stop_duration=2[av]"));
        filters.push(format!(
            "[base][av]overlay=x=(W-w)*{:.5}:y=(H-h)*{:.5}:eof_action=pass[composed]",
            p.output.avatar_x, p.output.avatar_y
        ));
        label = "composed";
    }
    let mut subproject = p.clone();
    subproject.segments = vec![s.clone()];
    subtitle::write(&subproject, &work)?;
    if !p.timeline.tracks[crate::timeline::GRAPHICS].muted && crate::motion::write(&subproject, &work)? {
        filters.push(format!(
            "[{label}]subtitles=filename=graphics.ass[graphics]"
        ));
        label = "graphics";
    }
    if p.subtitles.enabled && !p.timeline.tracks[crate::timeline::CAPTIONS].muted {
        filters.push(format!("[{label}]subtitles=filename=captions.ass[outv]"));
    } else {
        filters.push(format!("[{label}]null[outv]"));
    }
    if s.kind=="review" && p.output.original_audio_enabled && p.source(&s.source_id)?.has_audio {
        let voice=if p.timeline.tracks[crate::timeline::VOICE].muted {0.}else{s.audio_gain};
        let original=if p.timeline.tracks[crate::timeline::ORIGINAL].muted {0.}else{p.output.original_audio_gain};
        filters.push(format!("[1:a]asetpts=PTS-STARTPTS,volume={voice}[voice_mix];[0:a]asetpts=PTS-STARTPTS,volume={original}[original_mix];[voice_mix][original_mix]amix=inputs=2:duration=longest:normalize=0,apad,alimiter=limit=0.95:latency=1[outa]"));
        audio_map="[outa]".into();mixed_original=true;
    }
    // Keep the filter graph for inspection; pass it as a shell-free argument (under 4 KB).
    fs::write(work.join("filters.txt"), filters.join(";\n"))?;
    args.extend([
        "-filter_complex".into(),
        fs::read_to_string(work.join("filters.txt"))?,
        "-map".into(),
        "[outv]".into(),
        "-map".into(),
        audio_map,
        "-t".into(),
        format!("{seconds:.3}"),
        "-c:v".into(),
        settings.encoder.clone(),
    ]);
    match settings.encoder.as_str() {
        "libx264" => args.extend([
            "-preset".into(),
            "veryfast".into(),
            "-crf".into(),
            "21".into(),
        ]),
        "h264_nvenc" => args.extend(["-preset".into(), "p4".into(), "-cq".into(), "23".into()]),
        "h264_qsv" => {}
        _ => bail!("Encoder chưa hỗ trợ"),
    }
    args.extend([
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-r".into(),
        fps.to_string(),
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        "160k".into(),
        "-ar".into(),
        "48000".into(),
        "-ac".into(),
        "2".into(),
        "-video_track_timescale".into(),
        "90000".into(),
        "-movflags".into(),
        "+faststart".into(),
        "segment.pending.mp4".into(),
    ]);
    if !mixed_original {
        let last=args.len()-1;
        args.splice(last..last,["-af".into(),format!("volume={:.4},apad,alimiter=limit=0.95",if p.timeline.tracks[if s.kind=="source" {crate::timeline::ORIGINAL}else{crate::timeline::VOICE}].muted {0.}else{s.audio_gain})]);
    }
    let result = process::run(
        &settings.ffmpeg,
        &args,
        Some(&work),
        cancel,
        Duration::from_secs(settings.timeout_seconds),
        None,
    );
    if let Err(e) = result {
        let _ = fs::remove_file(work.join("segment.pending.mp4"));
        return Err(e);
    }
    fs::rename(work.join("segment.pending.mp4"), &output)?;
    Ok(output)
}
pub fn render<F: FnMut(usize, usize)>(
    p: &Project,
    settings: &Settings,
    output: &Path,
    cancel: &Cancel,
    mut progress: F,
) -> Result<()> {
    p.validate(true)?;
    p.check_duration()?;
    if p.music.enabled || p.effects.footage.active() || !p.effects.layers.is_empty() {
        let mut prepared=p.clone();prepared.ensure_composition();
        crate::layer_media::export_with_progress(&prepared,settings,output,cancel,progress)?;return Ok(());
    }
    if p.composition.is_some() || p.segments.iter().any(|v|v.source_rate!=1.||!v.footage_spans.is_empty()) {
        crate::layer_media::export_with_progress(p,settings,output,cancel,progress)?;
        return Ok(());
    }
    let metadata=crate::export_metadata::prepare(p,settings)?;
    let folder = p.root.join("cache/render");
    fs::create_dir_all(&folder)?;
    let mut clips = vec![];
    for (i, s) in p.segments.iter().enumerate() {
        process::check_cancel(cancel)?;
        clips.push(render_segment(p, s, settings, &folder, cancel)?);
        progress(i + 1, p.segments.len());
    }
    let work = p.root.join("export");
    fs::create_dir_all(&work)?;
    let mut list = String::new();
    for clip in clips {
        let normalized = fs::canonicalize(clip)?
            .to_string_lossy()
            .trim_start_matches("\\\\?\\")
            .replace('\\', "/")
            .replace('\'', "'\\''");
        list.push_str(&format!("file '{normalized}'\n"));
    }
    fs::write(work.join("concat.txt"), list)?;
    subtitle::write(p, &work)?;
    crate::motion::write(p, &work)?;
    let mut args = process::args(&[
        "-y",
        "-v",
        "error",
        "-f",
        "concat",
        "-safe",
        "0",
        "-i",
        "concat.txt",
        "-c",
        "copy",
        "-movflags",
        "+faststart",
    ]);
    args.extend(crate::content_language::mux_args(p));
    args.extend(metadata.args());args.push("final.pending.mp4".into());
    let mux_clock=std::time::Instant::now();
    process::run(
        &settings.ffmpeg,
        &args,
        Some(&work),
        cancel,
        Duration::from_secs(settings.timeout_seconds),
        None,
    )?;
    let result = probe(settings, &work.join("final.pending.mp4"), cancel)?;
    let (w, h) = p.output.dimensions();
    ensure!(
        result.width == w && result.height == h,
        "Output sai khung hình"
    );
    ensure!(
        result.duration_ms.abs_diff(p.duration_ms()) <= 500,
        "Output lệch thời lượng hơn 500ms"
    );
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    crate::export_metadata::finish(p,settings,&work.join("final.pending.mp4"),output,metadata,(mux_clock.elapsed().as_millis(),"legacy_render"),cancel)?;
    fs::remove_file(work.join("final.pending.mp4"))?;
    Ok(())
}
