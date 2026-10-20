use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, ensure};

use crate::proc::{retry, run, run_with_env};

const WINDOWS_LIBCLANG_RELOAD_ATTEMPTS: usize = 3;

const LIBRARY_PATH_VAR: Option<&str> = if cfg!(target_os = "linux") {
    Some("LD_LIBRARY_PATH")
} else if cfg!(target_os = "macos") {
    Some("DYLD_LIBRARY_PATH")
} else {
    None
};

pub fn ensure_target(root: &Path, target: Option<&str>) -> Result<()> {
    let Some(triple) = target else {
        return Ok(());
    };
    if triple == host_triple()? {
        return Ok(());
    }
    run("rustup", &["target", "add", triple], root)
}

pub fn host_triple() -> Result<String> {
    let out = Command::new("rustc")
        .arg("-vV")
        .output()
        .context("failed to spawn `rustc`")?;
    ensure!(out.status.success(), "`rustc -vV` failed");
    let stdout = String::from_utf8(out.stdout).context("`rustc -vV` printed non-utf8")?;
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_string)
        .context("`rustc -vV` printed no host line")
}

// Every codec build lands in its own `.media/<triple>`, so a cross build has to be pointed at the
// tree for the target rather than the one the host happens to have.
pub fn media_dir(root: &Path, target: Option<&str>) -> Result<Option<PathBuf>> {
    let triple = target.map(str::to_owned).map_or_else(host_triple, Ok)?;
    let dir = root.join(".media").join(triple);
    Ok(dir.join("sdrmm-build.txt").is_file().then_some(dir))
}

pub fn run_against_media(args: &[&str], cwd: &Path, media: Option<&Path>) -> Result<()> {
    let attempts = if cfg!(windows) {
        WINDOWS_LIBCLANG_RELOAD_ATTEMPTS
    } else {
        1
    };
    retry(attempts, || match media {
        Some(dir) => {
            let env = media_env(dir);
            let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
            run_with_env("cargo", args, cwd, &env)
        }
        None => run("cargo", args, cwd),
    })
}

pub fn media_env(dir: &Path) -> Vec<(&'static str, String)> {
    let mut env = vec![("FFMPEG_DIR", dir.to_string_lossy().into_owned())];
    if let Some(var) = LIBRARY_PATH_VAR {
        env.push((var, library_path(var, dir)));
    }
    env
}

fn library_path(var: &str, media: &Path) -> String {
    let lib = media.join("lib").to_string_lossy().into_owned();
    match std::env::var(var) {
        Ok(existing) if !existing.is_empty() => format!("{lib}:{existing}"),
        _ => lib,
    }
}
