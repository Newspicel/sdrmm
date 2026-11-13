use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const SKIPPED_DIRS: [&str; 3] = ["target", "node_modules", "generated"];

/// Our own source files under `dir` with one of `extensions`, sorted. Build output, installed
/// packages and generated code are not ours to lint. A missing `dir` has no sources.
pub(crate) fn files(dir: &Path, extensions: &[&str]) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let path = entry
            .with_context(|| format!("read {}", dir.display()))?
            .path();
        if path.is_dir() {
            if !is_skipped(&path) {
                out.extend(files(&path, extensions)?);
            }
        } else if path
            .extension()
            .is_some_and(|extension| extensions.iter().any(|wanted| extension == *wanted))
        {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

fn is_skipped(dir: &Path) -> bool {
    dir.file_name()
        .is_some_and(|name| SKIPPED_DIRS.iter().any(|skipped| name == *skipped))
}
