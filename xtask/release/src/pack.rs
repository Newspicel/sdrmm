use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::proc::{run, run_with_env, utf8};

pub fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).with_context(|| format!("cannot create {}", to.display()))?;
    for entry in
        std::fs::read_dir(from).with_context(|| format!("cannot read {}", from.display()))?
    {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)
                .with_context(|| format!("cannot copy {}", entry.path().display()))?;
        }
    }
    Ok(())
}

pub fn archive(root: &Path, out: &Path, name: &str, windows: bool) -> Result<PathBuf> {
    let ext = if windows { "zip" } else { "tar.gz" };
    let path = out.join(format!("{name}.{ext}"));
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err).with_context(|| format!("replace {}", path.display())),
    }

    if windows {
        run(
            "powershell",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!(
                    "Add-Type -AssemblyName System.IO.Compression.FileSystem; \
                     [System.IO.Compression.ZipFile]::CreateFromDirectory('{}', '{}', \
                     [System.IO.Compression.CompressionLevel]::Optimal, $true)",
                    out.join(name).display(),
                    path.display()
                ),
            ],
            root,
        )?;
    } else {
        run_with_env(
            "tar",
            &["-C", utf8(out)?, "-czf", utf8(&path)?, name],
            root,
            &[("COPYFILE_DISABLE", "1")],
        )?;
    }
    Ok(path)
}
