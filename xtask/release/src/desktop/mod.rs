//! The desktop app: Chromium Embedded Framework around the embedded server, packaged by
//! `cargo packager`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};

use crate::dist::release_features;
use crate::media::{ensure_target, host_triple, media_dir, run_against_media_with};
use crate::proc::{run_with_env, utf8};
use crate::web::web_build;

mod config;
mod mac;
mod rpm;
mod stage;

pub const IDENTIFIER: &str = "dev.newspicel.sdrmm";
const CEF_SYS: &str = "cef-dll-sys";
/// Formats `cargo packager` builds; `rpm` is made here from the `deb`.
const PACKAGER_FORMATS: &[&str] = &["app", "dmg", "deb", "appimage", "nsis", "wix"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Mac,
    Linux,
    Windows,
}

#[derive(Clone, Debug)]
pub struct Platform {
    pub triple: String,
    pub os: Os,
    pub arch: &'static str,
}

impl Platform {
    pub fn of(triple: &str) -> Result<Self> {
        let os = if triple.contains("apple-darwin") {
            Os::Mac
        } else if triple.contains("linux") {
            Os::Linux
        } else if triple.contains("windows") {
            Os::Windows
        } else {
            bail!("no desktop build for {triple}");
        };
        let arch = match triple.split('-').next() {
            Some("x86_64") => "x86_64",
            Some("aarch64") => "aarch64",
            _ => bail!("no desktop build for {triple}"),
        };
        Ok(Self {
            triple: triple.to_owned(),
            os,
            arch,
        })
    }

    /// The installed name of the app: Linux packages have always been `sdrmm-app`.
    pub fn product(&self) -> &'static str {
        match self.os {
            Os::Linux => "sdrmm-app",
            Os::Mac | Os::Windows => "SDRmm",
        }
    }

    pub fn exe(&self, name: &str) -> String {
        match self.os {
            Os::Windows => format!("{name}.exe"),
            Os::Mac | Os::Linux => name.to_owned(),
        }
    }

    fn cef_os(&self) -> &'static str {
        match self.os {
            Os::Mac => "macos",
            Os::Linux => "linux",
            Os::Windows => "windows",
        }
    }
}

/// Builds the app and hands it to `cargo packager` for each of `formats`.
pub fn package(root: &Path, target: Option<&str>, formats: &str) -> Result<()> {
    let formats: Vec<&str> = formats.split(',').map(str::trim).collect();
    if let Some(unknown) = formats
        .iter()
        .find(|format| **format != "rpm" && !PACKAGER_FORMATS.contains(format))
    {
        bail!("`{unknown}` is no desktop package format");
    }
    let platform = build(root, target)?;
    let release = release_dir(root, target);
    let stage = stage::fresh(&release.join("desktop"))?;
    let staged = stage::stage(root, &platform, &release, &stage)?;
    let config = config::write(root, &platform, &staged, &stage, &release)?;
    let packaged: Vec<&str> = formats.iter().copied().filter(|f| *f != "rpm").collect();
    if !packaged.is_empty() {
        cargo_packager(root, &config, &packaged.join(","))?;
    }
    if formats.contains(&"rpm") {
        rpm::from_deb(root, &platform, &release.join("packages"))?;
    }
    Ok(())
}

/// Builds the app as a runnable bundle and starts it, the way `cargo run` would.
pub fn run(root: &Path) -> Result<()> {
    let platform = build(root, None)?;
    let release = release_dir(root, None);
    let stage = stage::fresh(&release.join("desktop"))?;
    let exe = stage::runnable(root, &platform, &release, &stage)?;
    run_with_env(utf8(&exe)?, &[], root, &[])
}

/// Where `cef-dll-sys` downloads Chromium, shared by every build of the workspace.
pub fn cef_path(root: &Path) -> PathBuf {
    root.join("target").join("cef")
}

/// The extracted CEF distribution the build linked against.
pub fn cef_dir(root: &Path, platform: &Platform) -> Result<PathBuf> {
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).context("read Cargo.lock")?;
    let version = cef_version(&lock)?;
    let dir =
        cef_path(root)
            .join(version)
            .join(format!("cef_{}_{}", platform.cef_os(), platform.arch));
    ensure!(
        dir.is_dir(),
        "{} is missing: the desktop build did not fetch Chromium",
        dir.display()
    );
    Ok(dir)
}

fn cef_version(lock: &str) -> Result<&str> {
    let mut lines = lock.lines();
    lines
        .find(|line| *line == format!("name = \"{CEF_SYS}\""))
        .with_context(|| format!("Cargo.lock has no {CEF_SYS}"))?;
    let version = lines
        .next()
        .and_then(|line| line.strip_prefix("version = \""))
        .and_then(|line| line.strip_suffix('"'))
        .with_context(|| format!("Cargo.lock names no version for {CEF_SYS}"))?;
    version
        .split_once('+')
        .map(|(_, cef)| cef)
        .with_context(|| format!("{CEF_SYS} {version} names no CEF build"))
}

fn build(root: &Path, target: Option<&str>) -> Result<Platform> {
    web_build(root)?;
    ensure_target(root, target)?;
    let triple = target.map_or_else(host_triple, |triple| Ok(triple.to_owned()))?;
    let platform = Platform::of(&triple)?;
    let features = release_features();
    let mut args = vec![
        "build",
        "--release",
        "--locked",
        "-p",
        "sdrmm-desktop",
        "--bins",
        &features[0],
        &features[1],
        &features[2],
    ];
    if let Some(triple) = target {
        args.extend(["--target", triple]);
    }
    let cef = cef_path(root);
    let media = media_dir(root, target)?;
    run_against_media_with(&args, root, media.as_deref(), &[("CEF_PATH", utf8(&cef)?)])?;
    Ok(platform)
}

fn release_dir(root: &Path, target: Option<&str>) -> PathBuf {
    match target {
        Some(triple) => root.join("target").join(triple).join("release"),
        None => root.join("target").join("release"),
    }
}

/// The config goes inline: with a file, `cargo packager` 0.11 resolves it as a directory.
fn cargo_packager(root: &Path, config: &Path, formats: &str) -> Result<()> {
    let installed = std::process::Command::new("cargo")
        .args(["packager", "--version"])
        .output()
        .is_ok_and(|out| out.status.success());
    ensure!(
        installed,
        "cargo-packager is missing: `cargo install --locked cargo-packager`"
    );
    if std::env::var_os("CARGO_PACKAGER_SIGN_PRIVATE_KEY").is_none() {
        println!(
            "note: CARGO_PACKAGER_SIGN_PRIVATE_KEY is unset: these packages carry no updater \
             signature."
        );
    }
    let config =
        std::fs::read_to_string(config).with_context(|| format!("read {}", config.display()))?;
    run_with_env(
        "cargo",
        &["packager", "--config", config.trim(), "--formats", formats],
        root,
        &[],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_cef_build_the_bindings_were_made_for() {
        let lock = "[[package]]\nname = \"cef-dll-sys\"\nversion = \"156.3.0+156.0.15\"\n";
        assert_eq!(cef_version(lock).unwrap(), "156.0.15");
    }

    #[test]
    fn names_each_platform_the_way_its_packages_always_have() {
        let linux = Platform::of("x86_64-unknown-linux-gnu").unwrap();
        assert_eq!(linux.product(), "sdrmm-app");
        assert_eq!(linux.cef_os(), "linux");
        let windows = Platform::of("aarch64-pc-windows-msvc").unwrap();
        assert_eq!(windows.exe("SDRmm"), "SDRmm.exe");
        assert_eq!(windows.arch, "aarch64");
        assert_eq!(Platform::of("aarch64-apple-darwin").unwrap().os, Os::Mac);
        assert!(Platform::of("wasm32-unknown-unknown").is_err());
    }
}
