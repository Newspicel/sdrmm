use std::path::Path;

use anyhow::{Context, Result};

use crate::bundled;
use crate::media::{ensure_target, host_triple, media_dir, run_against_media};
use crate::pack::{archive, copy_tree};
use crate::proc::run;
use crate::web::{assert_web_dist, web_build};

pub fn release_features() -> [String; 3] {
    [
        "--no-default-features".to_string(),
        "--features".to_string(),
        "soapy,sdrplay,rtlsdr,hackrf,airspy,airspyhf,funcube,espsdr,ad936x,net-client,gpu-fft"
            .to_string(),
    ]
}

pub fn dist(root: &Path, target: Option<&str>) -> Result<()> {
    web_build(root)?;
    assert_web_dist(root)?;
    ensure_target(root, target)?;

    let features = release_features();
    let mut args = vec![
        "build",
        "--release",
        "--locked",
        "-p",
        "sdrmm",
        &features[0],
        &features[1],
        &features[2],
    ];
    if let Some(triple) = target {
        args.push("--target");
        args.push(triple);
    }
    run_against_media(&args, root, media_dir(root, target)?.as_deref())?;

    let triple = match target {
        Some(triple) => triple.to_string(),
        None => host_triple()?,
    };
    let windows = triple.contains("windows");
    let exe = if windows { "sdrmm.exe" } else { "sdrmm" };
    let built = match target {
        Some(triple) => root.join("target").join(triple).join("release").join(exe),
        None => root.join("target").join("release").join(exe),
    };

    let out = root.join("dist");
    let name = format!("sdrmm-{}-{triple}", env!("CARGO_PKG_VERSION"));
    let staged = out.join(&name);
    if staged.exists() {
        std::fs::remove_dir_all(&staged)
            .with_context(|| format!("cannot clear {}", staged.display()))?;
    }
    std::fs::create_dir_all(&staged)
        .with_context(|| format!("cannot create {}", staged.display()))?;

    std::fs::copy(&built, staged.join(exe))
        .with_context(|| format!("cannot stage {}", built.display()))?;
    bundled::stage(&bundled::libraries(root, target)?, &staged)?;
    for doc in ["README.md", "LICENSE", "THIRD_PARTY_NOTICES.md"] {
        std::fs::copy(root.join(doc), staged.join(doc))
            .with_context(|| format!("cannot stage {doc}"))?;
    }

    if triple.contains("linux") {
        run(
            "strip",
            &[staged.join(exe).to_str().expect("utf8 path")],
            root,
        )?;
    }

    let archive = archive(root, &out, &name, windows)?;
    println!("dist: {}", archive.display());
    Ok(())
}

pub fn source_dist(root: &Path) -> Result<()> {
    web_build(root)?;
    assert_web_dist(root)?;

    let out = root.join("dist");
    let name = format!("sdrmm-{}-src", env!("CARGO_PKG_VERSION"));
    let staged = out.join(&name);
    if staged.exists() {
        std::fs::remove_dir_all(&staged)
            .with_context(|| format!("cannot clear {}", staged.display()))?;
    }
    std::fs::create_dir_all(&staged)
        .with_context(|| format!("cannot create {}", staged.display()))?;

    let tar = out.join(format!("{name}.tar"));
    run(
        "git",
        &[
            "archive",
            "--format=tar",
            "--output",
            tar.to_str().expect("utf8 path"),
            "HEAD",
        ],
        root,
    )?;
    run(
        "tar",
        &[
            "-xf",
            tar.to_str().expect("utf8 path"),
            "-C",
            staged.to_str().expect("utf8 path"),
        ],
        root,
    )?;
    std::fs::remove_file(&tar).with_context(|| format!("cannot clear {}", tar.display()))?;

    // The stamped manifests and the built UI are what the commit cannot carry: one names the
    // release, the other is embedded by `crates/server` at compile time.
    for manifest in ["Cargo.toml", "Cargo.lock"] {
        std::fs::copy(root.join(manifest), staged.join(manifest))
            .with_context(|| format!("cannot stage {manifest}"))?;
    }
    copy_tree(&root.join("web/dist"), &staged.join("web/dist"))?;

    let archive = archive(root, &out, &name, false)?;
    println!("source dist: {}", archive.display());
    Ok(())
}
