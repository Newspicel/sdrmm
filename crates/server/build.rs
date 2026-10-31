#![allow(clippy::expect_used)]

use std::{
    fmt::Write as _,
    fs,
    io::Write as _,
    path::{Path, PathBuf},
};

use flate2::{Compression, write::GzEncoder};

const WORTHWHILE_RATIO: f64 = 0.9;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let dist = manifest.join("../../web/dist");
    if let Err(error) = ensure_watchable(&dist) {
        println!(
            "cargo:warning=cannot prepare {} for watching: {error}",
            dist.display()
        );
    }
    println!("cargo:rerun-if-changed=../../web/dist");
    println!("cargo:rerun-if-changed=data");

    write_web_table(
        built_ui(&dist),
        &out.join("web"),
        &out.join("web_assets.rs"),
    );
    compress_tree(&manifest.join("data"), &out.join("data"));
}

/// Cargo treats a watched path that does not exist as changed, so a checkout without a web
/// build would rebuild the server and everything above it on every cargo run. An empty
/// directory dated to the epoch is watchable and never newer than the last run; the first
/// file `pnpm build` writes into it still triggers exactly one rerun.
fn ensure_watchable(dist: &Path) -> std::io::Result<()> {
    if dist.exists() {
        return Ok(());
    }
    fs::create_dir_all(dist)?;
    open_dir(dist)?.set_modified(std::time::SystemTime::UNIX_EPOCH)
}

#[cfg(not(windows))]
fn open_dir(dir: &Path) -> std::io::Result<fs::File> {
    fs::File::open(dir)
}

/// Windows opens a directory only with backup semantics, and dating it needs attribute access.
#[cfg(windows)]
fn open_dir(dir: &Path) -> std::io::Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt as _;
    const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    fs::OpenOptions::new()
        .access_mode(FILE_WRITE_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(dir)
}

fn built_ui(dist: &Path) -> Option<&Path> {
    let index = dist.join("index.html");
    if index.is_file() {
        println!("cargo:rustc-env=SDRMM_WEB_UI=embedded");
        return Some(dist);
    }
    println!(
        "cargo:warning={} is missing, so this build serves no interface: run `pnpm --dir web \
         build` for one",
        index.display()
    );
    println!("cargo:rustc-env=SDRMM_WEB_UI=empty");
    None
}

fn write_web_table(dist: Option<&Path>, out: &Path, table: &Path) {
    let mut source = String::from("&[\n");
    if let Some(dist) = dist {
        for relative in sorted_files(dist) {
            write_row(&mut source, dist, out, &relative);
        }
    }
    source.push(']');
    fs::write(table, source).expect("write asset table");
}

fn sorted_files(dist: &Path) -> Vec<String> {
    let mut files = Vec::new();
    collect(dist, dist, &mut files);
    files.sort();
    files
}

fn write_row(source: &mut String, dist: &Path, out: &Path, relative: &str) {
    let raw = fs::read(dist.join(relative)).expect("read web asset");
    let packed = gzip(&raw);
    let gzipped = (packed.len() as f64) < raw.len() as f64 * WORTHWHILE_RATIO;
    let stored = out.join(relative);
    fs::create_dir_all(stored.parent().expect("asset parent")).expect("create asset dir");
    fs::write(&stored, if gzipped { &packed } else { &raw }).expect("write asset");
    let mime = mime_guess::from_path(relative).first_or_octet_stream();
    writeln!(
        source,
        "    Asset {{ path: {relative:?}, mime: {mime:?}, gzipped: {gzipped}, body: include_bytes!({stored:?}) }},",
        mime = mime.as_ref(),
    )
    .expect("write table row");
}

fn compress_tree(source: &Path, out: &Path) {
    let mut files = Vec::new();
    collect(source, source, &mut files);
    for relative in files {
        let raw = fs::read(source.join(&relative)).expect("read data file");
        let stored = out.join(format!("{relative}.gz"));
        fs::create_dir_all(stored.parent().expect("data parent")).expect("create data dir");
        fs::write(stored, gzip(&raw)).expect("write data file");
    }
}

fn collect(root: &Path, dir: &Path, files: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect(root, &path, files);
        } else {
            let relative = path.strip_prefix(root).expect("inside root");
            files.push(relative.to_str().expect("utf-8 path").replace('\\', "/"));
        }
    }
}

fn gzip(raw: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(raw).expect("gzip");
    encoder.finish().expect("gzip")
}
