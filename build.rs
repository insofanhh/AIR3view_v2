use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=resources/air3view.rc");
    println!("cargo:rerun-if-changed=assets/air3view.ico");
    println!("cargo:rerun-if-env-changed=RC");
    if !env::var("TARGET")
        .unwrap_or_default()
        .contains("windows-msvc")
    {
        return;
    }
    let compiler = env::var_os("RC")
        .map(PathBuf::from)
        .or_else(|| {
            let sdk = PathBuf::from(env::var_os("ProgramFiles(x86)")?).join("Windows Kits/10/bin");
            let mut versions: Vec<_> = std::fs::read_dir(sdk)
                .ok()?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.join("x64/rc.exe").is_file())
                .collect();
            versions.sort();
            versions.pop().map(|path| path.join("x64/rc.exe"))
        })
        .unwrap_or_else(|| PathBuf::from("rc.exe"));
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("air3view.res");
    let status = Command::new(compiler)
        .arg("/nologo")
        .arg("/fo")
        .arg(&output)
        .arg("resources/air3view.rc")
        .status()
        .expect("Windows SDK Resource Compiler (rc.exe) is required");
    assert!(
        status.success(),
        "AIR3view Windows icon/version resource compilation failed"
    );
    println!(
        "cargo:rustc-link-arg-bin=reaction-studio={}",
        output.display()
    );
}
