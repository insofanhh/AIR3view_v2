//! Child processes never run through a command shell. User text is passed as stdin or argv.
use anyhow::{bail, Context, Result};
use std::{
    io::{Read, Write},
    path::{Path,PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

pub type Cancel = Arc<AtomicBool>;
#[derive(Debug)]
pub struct Timeout { pub seconds: u64 }
impl std::fmt::Display for Timeout {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {write!(f,"vượt thời gian chờ {}s",self.seconds)}
}
impl std::error::Error for Timeout {}
pub fn cancel_token() -> Cancel {
    Arc::new(AtomicBool::new(false))
}
pub fn check_cancel(c: &Cancel) -> Result<()> {
    if c.load(Ordering::Relaxed) {
        bail!("Đã hủy tác vụ")
    }
    Ok(())
}
pub fn command(program: &str) -> Command {
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000);
    }
    c
}
pub fn stop(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        let _ = command("taskkill.exe")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}
#[derive(Debug)]
pub struct Output {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
fn read_bounded(mut r: impl Read, completed: Option<Cancel>, mut trace:Option<std::fs::File>) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut line = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match r.read(&mut buf) {
            Ok(0) | Err(_) => {
                mark_result(&line, completed.as_deref());
                break;
            },
            Ok(n) => {
                if bytes.len() + n < 16 * 1024 * 1024 {
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(file)=&mut trace {let _=file.write_all(&buf[..n]);}
                }
                if completed.is_some() {
                    for part in buf[..n].split_inclusive(|b| *b==b'\n') {
                        if line.len()+part.len()<=16*1024*1024 {line.extend_from_slice(part);}
                        if part.last()==Some(&b'\n') {
                            mark_result(&line,completed.as_deref());line.clear();
                        }
                    }
                    // Some wrappers flush the final JSON without a trailing newline
                    // and retain their stdout handle while shutting down.
                    mark_result(&line,completed.as_deref());
                }
            }
        }
    }
    bytes
}
fn mark_result(line:&[u8],completed:Option<&AtomicBool>) {
    if let Some(completed)=completed {
        if serde_json::from_slice::<serde_json::Value>(line).ok()
            .is_some_and(|v|v["event"]=="result" && v["result"].is_object()) {
            completed.store(true,Ordering::Release);
        }
    }
}
pub fn run(
    program: &str,
    args: &[String],
    cwd: Option<&Path>,
    cancel: &Cancel,
    timeout: Duration,
    input: Option<Vec<u8>>,
) -> Result<Output> {
    run_inner(program,args,cwd,cancel,timeout,input,RunMode::default())
}
/// Stream-json CLIs may publish a terminal result then hang during service shutdown.
/// Only the provider should use this: it must still validate terminal status/payload.
pub fn run_json_result(
    program:&str,args:&[String],cwd:Option<&Path>,cancel:&Cancel,
    timeout:Duration,input:Option<Vec<u8>>,
)->Result<Output> {
    run_inner(program,args,cwd,cancel,timeout,input,RunMode{terminal_json:true,..Default::default()})
}
pub fn run_json_result_trace(
    program:&str,args:&[String],cwd:Option<&Path>,cancel:&Cancel,
    timeout:Duration,input:Option<Vec<u8>>,trace:PathBuf,
)->Result<Output> {
    run_inner(program,args,cwd,cancel,timeout,input,RunMode{terminal_json:true,trace:Some(trace)})
}
#[derive(Default)]
struct RunMode {terminal_json:bool,trace:Option<PathBuf>}
fn run_inner(
    program:&str,args:&[String],cwd:Option<&Path>,cancel:&Cancel,
    timeout:Duration,input:Option<Vec<u8>>,mode:RunMode,
)->Result<Output> {
    check_cancel(cancel)?;
    if program.to_lowercase().ends_with(".cmd") || program.to_lowercase().ends_with(".bat") {
        bail!("Chọn executable .exe hoặc node.exe + script CLI, không dùng wrapper .cmd/.bat");
    }
    let trace=mode.trace.map(std::fs::File::create).transpose()?;
    let mut cmd = command(program);
    cmd.args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(p) = cwd {
        cmd.current_dir(p);
    }
    let mut child = cmd.spawn().with_context(|| {
        format!("Không khởi chạy được {program}. Kiểm tra đường dẫn trong Cài đặt.")
    })?;
    let stdout = child.stdout.take().context("stdout")?;
    let stderr = child.stderr.take().context("stderr")?;
    let completed=cancel_token();let completion=mode.terminal_json.then(||completed.clone());
    let out = thread::spawn(move || read_bounded(stdout,completion,trace));
    let err = thread::spawn(move || read_bounded(stderr,None,None));
    let writer = if let Some(data) = input {
        let mut stdin = child.stdin.take().context("stdin")?;
        Some(thread::spawn(move || stdin.write_all(&data)))
    } else {
        None
    };
    let start = Instant::now();
    let mut interrupted = false;
    let mut result_at=None;
    let mut finished_by_result=false;
    let status = loop {
        if cancel.load(Ordering::Relaxed) || (start.elapsed() > timeout && !completed.load(Ordering::Acquire)) {
            stop(&mut child);
            interrupted = true;
            break None;
        }
        if let Some(s) = child.try_wait()? {
            break Some(s);
        }
        if completed.load(Ordering::Acquire) {
            let at=result_at.get_or_insert_with(Instant::now);
            if at.elapsed()>=Duration::from_secs(2) || start.elapsed()>timeout {
                stop(&mut child);finished_by_result=true;break None;
            }
        }
        thread::sleep(Duration::from_millis(40));
    };
    if let Some(w) = writer {
        let _ = w.join();
    }
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    if interrupted {
        check_cancel(cancel)?;
        return Err(anyhow::Error::new(Timeout{seconds:timeout.as_secs()}).context(program.to_owned()));
    }
    if !finished_by_result && !status.context("Không có exit status")?.success() {
        let details = String::from_utf8_lossy(&stderr);
        let tail: String = details
            .chars()
            .rev()
            .take(1800)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        bail!("{program} thất bại: {tail}");
    }
    Ok(Output { stdout, stderr })
}
pub fn version(program: &str) -> String {
    match run(
        program,
        &["-version".into()],
        None,
        &cancel_token(),
        Duration::from_secs(10),
        None,
    ) {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .lines()
            .next()
            .unwrap_or("OK")
            .to_owned(),
        Err(e) => format!("{e:#}"),
    }
}
pub fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| s.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_json_before_deadline_survives_a_hanging_cli_shutdown() {
        let node=Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/node.exe");
        if !cfg!(windows) {return;}
        let output=run_json_result(&node.to_string_lossy(),&args(&["-e",
            "process.stdout.write(JSON.stringify({event:'result',result:{status:'SUCCESS',output:{verified:true}}})+'\\n');setInterval(()=>{},1000)"]),
            None,&cancel_token(),Duration::from_millis(1200),None).unwrap();
        let value:serde_json::Value=serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["result"]["output"]["verified"],true);
    }
    #[test]
    fn a_live_cli_without_a_terminal_result_returns_a_typed_timeout() {
        let node=Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/node.exe");
        if !cfg!(windows) {return;}
        let error=run_json_result(&node.to_string_lossy(),&args(&["-e","setInterval(()=>{},1000)"]),
            None,&cancel_token(),Duration::from_millis(300),None).unwrap_err();
        assert!(error.downcast_ref::<Timeout>().is_some());
    }
}
