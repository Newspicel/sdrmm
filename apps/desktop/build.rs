include!("../bundled_rpath.rs");

fn main() -> std::io::Result<()> {
    println!("cargo::rerun-if-env-changed=SDRMM_RELEASE");
    println!("cargo::rerun-if-changed=windows/sdrmm.exe.manifest");
    println!("cargo::rerun-if-changed=icons/icon.ico");
    bundled_rpath();
    cef_rpath();
    windows_resources()
}

/// Linux packages install the Chromium runtime beside the helper in `/usr/lib/sdrmm-app`, while
/// the launcher sits in `/usr/bin`, so the loader has to look there for `libcef.so`. A media
/// build already gets both directories from `bundled_rpath`.
fn cef_rpath() {
    let linux = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux");
    if !linux || std::env::var_os("FFMPEG_DIR").is_some() {
        return;
    }
    println!("cargo::rustc-link-arg-bins=-Wl,--disable-new-dtags");
    for dir in ["$ORIGIN", "$ORIGIN/../lib/sdrmm-app"] {
        println!("cargo::rustc-link-arg-bins=-Wl,-rpath,{dir}");
    }
}

fn windows_resources() -> std::io::Result<()> {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return Ok(());
    }
    winresource::WindowsResource::new()
        .set_icon("icons/icon.ico")
        .set_manifest_file("windows/sdrmm.exe.manifest")
        .compile()
}
