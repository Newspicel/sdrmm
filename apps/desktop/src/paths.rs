use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub const IDENTIFIER: &str = "dev.newspicel.sdrmm";
/// Where Linux packages install the Chromium runtime, relative to the launcher in `bin`.
#[cfg(not(target_os = "macos"))]
const LINUX_RUNTIME: &str = "../lib/sdrmm-app";
#[cfg(not(target_os = "macos"))]
const RUNTIME_MARKER: &str = "icudtl.dat";
#[cfg(target_os = "windows")]
const HELPER: &str = "sdrmm-desktop-helper.exe";
#[cfg(target_os = "linux")]
const HELPER: &str = "sdrmm-desktop-helper";

pub fn data_dir() -> Result<PathBuf> {
    Ok(dirs::data_dir()
        .context("this system names no data directory")?
        .join(IDENTIFIER))
}

pub fn cache_dir() -> Result<PathBuf> {
    Ok(dirs::cache_dir()
        .context("this system names no cache directory")?
        .join(IDENTIFIER))
}

/// The files Chromium loads beside `libcef`: on Windows and Linux they are not found through
/// a bundle, so the shell points CEF at them.
#[cfg(not(target_os = "macos"))]
#[derive(Debug, PartialEq, Eq)]
pub struct Runtime {
    pub resources: PathBuf,
    pub locales: PathBuf,
    pub helper: PathBuf,
}

#[cfg(not(target_os = "macos"))]
pub fn runtime(exe: &Path) -> Result<Runtime> {
    let dir = exe.parent().context("the executable has no directory")?;
    let resources = [dir.to_path_buf(), dir.join(LINUX_RUNTIME)]
        .into_iter()
        .find(|candidate| candidate.join(RUNTIME_MARKER).is_file())
        .with_context(|| {
            format!(
                "no Chromium runtime beside {}: build the app with `cargo xtask desktop --run`",
                exe.display()
            )
        })?;
    Ok(Runtime {
        locales: resources.join("locales"),
        helper: resources.join(HELPER),
        resources,
    })
}

/// The CEF framework a bundle carries in `Contents/Frameworks`; `cargo run` has none.
#[cfg(target_os = "macos")]
pub fn framework(exe: &Path) -> Result<()> {
    let framework = exe
        .parent()
        .context("the executable has no directory")?
        .join("../Frameworks/Chromium Embedded Framework.framework");
    anyhow::ensure!(
        framework.is_dir(),
        "{} is not inside an app bundle: build it with `cargo xtask desktop --run`",
        exe.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn finds_the_runtime_beside_the_executable_or_in_the_package_library() {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin");
        let lib = root.path().join("lib/sdrmm-app");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&lib).unwrap();
        let exe = bin.join("sdrmm-app");
        assert!(runtime(&exe).is_err());
        std::fs::write(lib.join(RUNTIME_MARKER), b"").unwrap();
        assert_eq!(
            runtime(&exe).unwrap().helper,
            bin.join(LINUX_RUNTIME).join(HELPER)
        );
        std::fs::write(bin.join(RUNTIME_MARKER), b"").unwrap();
        assert_eq!(runtime(&exe).unwrap().resources, bin);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_bare_executable_is_no_bundle() {
        let dir = tempfile::tempdir().unwrap();
        assert!(framework(&dir.path().join("sdrmm-desktop")).is_err());
    }
}
