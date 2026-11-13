use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

use super::{Platform, stage};
use crate::proc::{run, utf8};

const NAME: &str = "sdrmm-app";
const PRIVATE: &str = "usr/lib/sdrmm-app";

/// Repackages the `deb` that `cargo packager` built as an RPM, so both carry the same files.
/// `rpmbuild` derives the system libraries it requires from the binaries.
pub fn from_deb(root: &Path, platform: &Platform, packages: &Path) -> Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    let deb = find_deb(packages, version)?;
    let work = stage::fresh(&packages.join("rpm-build"))?;
    let tree = stage::fresh(&work.join("SOURCES/root"))?;
    run("dpkg-deb", &["-x", utf8(&deb)?, utf8(&tree)?], root)?;
    let spec = work.join(format!("{NAME}.spec"));
    let text = spec_text(version, &tree, &files(&tree)?, &private(&tree)?);
    std::fs::write(&spec, text)?;
    let defines = [
        format!("_topdir {}", utf8(&work)?),
        format!("_rpmdir {}", utf8(packages)?),
        "_build_name_fmt %%{NAME}-%%{VERSION}-%%{RELEASE}.%%{ARCH}.rpm".to_owned(),
    ];
    let mut args = vec!["-bb", "--target", platform.arch];
    for define in &defines {
        args.extend(["--define", define]);
    }
    args.push(utf8(&spec)?);
    run("rpmbuild", &args, root)
}

fn find_deb(packages: &Path, version: &str) -> Result<PathBuf> {
    let prefix = format!("{NAME}_{version}_");
    std::fs::read_dir(packages)
        .with_context(|| format!("read {}", packages.display()))?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&prefix) && name.ends_with(".deb"))
        })
        .with_context(|| {
            format!(
                "no {prefix}*.deb in {}: build `deb` with `rpm`",
                packages.display()
            )
        })
}

fn files(tree: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    walk(tree, tree, &mut files)?;
    files.sort();
    ensure!(!files.is_empty(), "{} holds no files", tree.display());
    Ok(files)
}

fn walk(tree: &Path, dir: &Path, files: &mut Vec<String>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            walk(tree, &path, files)?;
        } else if let Ok(relative) = path.strip_prefix(tree) {
            files.push(format!("/{}", relative.to_string_lossy()));
        }
    }
    Ok(())
}

/// The shared libraries the package carries for itself, which it neither offers the system nor
/// asks the system for.
fn private(tree: &Path) -> Result<Vec<String>> {
    let dir = tree.join(PRIVATE);
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .with_context(|| format!("read {}", dir.display()))?
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".so"))
        .collect();
    names.sort();
    Ok(names)
}

fn spec_text(version: &str, tree: &Path, files: &[String], private: &[String]) -> String {
    let excluded = private
        .iter()
        .map(|name| regex_escape(name))
        .collect::<Vec<_>>()
        .join("|");
    let files = files
        .iter()
        .map(|file| format!("\"{file}\""))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Name: {NAME}\n\
         Version: {}\n\
         Release: 1\n\
         Summary: Modular software-defined radio\n\
         License: AGPL-3.0-or-later\n\
         URL: https://sdrmm.com\n\
         %global __os_install_post %{{nil}}\n\
         %global _build_id_links none\n\
         %global __provides_exclude_from ^/{PRIVATE}/.*$\n\
         %global __requires_exclude ^({excluded})\n\n\
         %description\n\
         Modular, client-server software-defined radio receiver.\n\n\
         %install\n\
         cp -a \"{}/.\" \"%{{buildroot}}/\"\n\n\
         %files\n\
         {files}\n",
        version.replace('-', "~"),
        tree.display()
    )
}

fn regex_escape(name: &str) -> String {
    name.chars()
        .flat_map(|c| match c {
            '.' | '+' | '(' | ')' | '[' | ']' => vec!['\\', c],
            c => vec![c],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spec_keeps_bundled_libraries_out_of_the_dependency_graph() {
        let spec = spec_text(
            "1.2.3-rc.1",
            Path::new("/work/SOURCES/root"),
            &["/usr/bin/sdrmm-app".to_owned()],
            &["libcef.so".to_owned(), "libavcodec.so.62".to_owned()],
        );
        assert!(spec.contains("Version: 1.2.3~rc.1\n"));
        assert!(spec.contains("%global __requires_exclude ^(libcef\\.so|libavcodec\\.so\\.62)\n"));
        assert!(spec.contains("cp -a \"/work/SOURCES/root/.\" \"%{buildroot}/\"\n"));
        assert!(spec.contains("%files\n\"/usr/bin/sdrmm-app\"\n"));
    }

    #[test]
    fn lists_every_file_of_the_tree() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(PRIVATE).join("locales")).unwrap();
        std::fs::write(dir.path().join(PRIVATE).join("locales/en-US.pak"), b"").unwrap();
        std::fs::write(dir.path().join(PRIVATE).join("libcef.so"), b"").unwrap();
        assert_eq!(
            files(dir.path()).unwrap(),
            [
                "/usr/lib/sdrmm-app/libcef.so",
                "/usr/lib/sdrmm-app/locales/en-US.pak"
            ]
        );
        assert_eq!(private(dir.path()).unwrap(), ["libcef.so"]);
    }
}
