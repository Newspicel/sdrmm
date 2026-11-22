use std::process::Command;

/// Stamps the sdr-- revision this crate was built from, shown beside a plugin in the doctor. A
/// git dependency is checked out per revision, so a plugin's stamp names exactly what it used.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SDRMM_SDK_BUILD");
    let build = std::env::var("SDRMM_SDK_BUILD")
        .ok()
        .or_else(git_rev)
        .unwrap_or_default();
    println!("cargo:rustc-env=SDRMM_SDK_BUILD={build}");
}

fn git_rev() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
