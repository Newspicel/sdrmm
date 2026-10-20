use std::path::Path;

use anyhow::{Result, ensure};

use crate::proc::run;

#[cfg(windows)]
pub const PNPM: &str = "pnpm.cmd";
#[cfg(not(windows))]
pub const PNPM: &str = "pnpm";

pub fn ensure_web_deps(root: &Path) -> Result<()> {
    for dir in ["web", "site"] {
        if !root.join(dir).join("node_modules").is_dir() {
            run(PNPM, &["--dir", dir, "install", "--frozen-lockfile"], root)?;
        }
    }
    Ok(())
}

pub fn web_build(root: &Path) -> Result<()> {
    ensure_web_deps(root)?;
    run(PNPM, &["--dir", "web", "build"], root)
}

pub fn assert_web_dist(root: &Path) -> Result<()> {
    let index = root.join("web/dist/index.html");
    ensure!(
        index.exists(),
        "{} is missing after the web build: the artifact would embed an empty UI",
        index.display()
    );
    Ok(())
}
