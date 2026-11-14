use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::{Os, Platform, cef_dir, mac};
use crate::bundled;
use crate::pack::copy_tree;

const APP: &str = "sdrmm-desktop";
const HELPER: &str = "sdrmm-desktop-helper";
const NOTICES: &str = "THIRD_PARTY_NOTICES.md";
/// Chromium's own license list, shipped with it.
const CREDITS: &str = "CREDITS.html";
const LOCALES: &str = "locales";

/// What `cargo packager` gets beyond the main binary.
#[derive(Debug)]
pub struct Staged {
    pub bin_dir: PathBuf,
    pub main: String,
    /// Windows and Linux: everything that sits beside `libcef`, installed as resources.
    pub runtime: Option<PathBuf>,
    /// macOS: Chromium, its helper apps and the FFmpeg libraries, in `Contents/Frameworks`.
    pub frameworks: Vec<PathBuf>,
    /// macOS: files for `Contents/Resources`.
    pub resources: Vec<PathBuf>,
}

pub fn fresh(dir: &Path) -> Result<PathBuf> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).with_context(|| format!("clear {}", dir.display()))?;
    }
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    Ok(dir.to_path_buf())
}

pub fn stage(root: &Path, platform: &Platform, release: &Path, stage: &Path) -> Result<Staged> {
    let cef = cef_dir(root, platform)?;
    let bin_dir = stage.join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    let main = platform.product().to_owned();
    copy(
        &release.join(platform.exe(APP)),
        &bin_dir.join(platform.exe(&main)),
    )?;
    if platform.os == Os::Mac {
        let mut frameworks = vec![cef.join(mac::FRAMEWORK)];
        frameworks.extend(mac::helpers(&release.join(HELPER), stage, &main)?);
        frameworks.extend(mac::presigned(
            &bundled::libraries(root, Some(&platform.triple))?,
            stage,
        )?);
        return Ok(Staged {
            bin_dir,
            main,
            runtime: None,
            frameworks,
            resources: vec![root.join(NOTICES), cef.join(CREDITS)],
        });
    }
    Ok(Staged {
        bin_dir,
        main,
        runtime: Some(runtime(root, platform, release, &cef, stage)?),
        frameworks: Vec::new(),
        resources: Vec::new(),
    })
}

/// Stages the app so it starts from `target`, and returns the executable to launch.
pub fn runnable(root: &Path, platform: &Platform, release: &Path, stage: &Path) -> Result<PathBuf> {
    let cef = cef_dir(root, platform)?;
    if platform.os == Os::Mac {
        return mac::dev_app(release, &cef, stage, platform.product());
    }
    let dir = runtime(root, platform, release, &cef, stage)?;
    let exe = dir.join(platform.exe(APP));
    copy(&release.join(platform.exe(APP)), &exe)?;
    Ok(exe)
}

fn runtime(
    root: &Path,
    platform: &Platform,
    release: &Path,
    cef: &Path,
    stage: &Path,
) -> Result<PathBuf> {
    let dir = stage.join("runtime");
    std::fs::create_dir_all(&dir)?;
    for entry in std::fs::read_dir(cef).with_context(|| format!("read {}", cef.display()))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type()?.is_file() && is_runtime(&name) {
            copy(&entry.path(), &dir.join(&name))?;
        }
    }
    copy_tree(&cef.join(LOCALES), &dir.join(LOCALES))?;
    copy(
        &release.join(platform.exe(HELPER)),
        &dir.join(platform.exe(HELPER)),
    )?;
    copy(&root.join(NOTICES), &dir.join(NOTICES))?;
    bundled::stage(&bundled::libraries(root, Some(&platform.triple))?, &dir)?;
    Ok(dir)
}

/// The files Chromium loads at runtime, as opposed to the headers, build scripts and static
/// libraries the distribution also carries.
fn is_runtime(name: &str) -> bool {
    const LOADED: &[&str] = &[".pak", ".dat", ".bin", ".dll", ".json"];
    name == CREDITS
        || name != "archive.json"
            && (LOADED.iter().any(|ext| name.ends_with(ext)) || name.contains(".so"))
}

pub fn copy(from: &Path, to: &Path) -> Result<()> {
    std::fs::copy(from, to)
        .map(drop)
        .with_context(|| format!("copy {} to {}", from.display(), to.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ships_what_chromium_loads_and_leaves_the_sdk_behind() {
        for loaded in [
            "libcef.so",
            "libvulkan.so.1",
            "libcef.dll",
            "icudtl.dat",
            "v8_context_snapshot.bin",
            "resources.pak",
            "vk_swiftshader_icd.json",
            "CREDITS.html",
        ] {
            assert!(is_runtime(loaded), "{loaded}");
        }
        for left in [
            "archive.json",
            "libcef.lib",
            "cef_sandbox.lib",
            "bootstrap.exe",
            "chrome-sandbox",
            "CMakeLists.txt",
        ] {
            assert!(!is_runtime(left), "{left}");
        }
    }
}
