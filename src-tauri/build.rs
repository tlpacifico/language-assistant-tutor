fn main() {
    use std::process::Command;
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let revision = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let dirty = git(&["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    let built_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    println!("cargo:rustc-env=TUTOR_REVISION={revision}");
    println!("cargo:rustc-env=TUTOR_DIRTY={dirty}");
    println!("cargo:rustc-env=TUTOR_BUILT_AT={built_at}");
    for path in [
        "src",
        "build.rs",
        "Cargo.toml",
        "../src",
        "../package.json",
        "../.git/HEAD",
        "../.git/index",
        "../.git/refs",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    tauri_build::build()
}
