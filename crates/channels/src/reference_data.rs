use std::path::{Path, PathBuf};

pub fn xng_bench_data() -> Option<PathBuf> {
    directory(
        "XNG_BENCH_DATA",
        "a directory of dumpvdl2/dumphfdl bench captures",
    )
}

pub fn stanag_sigid_dir() -> Option<PathBuf> {
    directory(
        "STANAG_SIGID_DIR",
        "a directory of SigIDWiki samples as 9.6 kHz mono f32le, one <stem>.f32 each",
    )
}

pub fn dect_sigid_wav() -> Option<PathBuf> {
    file("DECT_SIGID_WAV", "the dect.wav from the SigIDWiki Dect.zip")
}

pub fn g726_vectors() -> Option<PathBuf> {
    directory(
        "G726_VECTORS",
        "the ITU-T G.726 Appendix II DISK2 directory, with RESET/ and INPUT/ inside",
    )
}

pub fn ft8_corpus() -> Option<PathBuf> {
    directory(
        "FT8_CORPUS",
        "a directory of WSJT-X reference wav files, each beside its published .txt",
    )
}

pub fn directory(var: &str, needs: &str) -> Option<PathBuf> {
    let path = provided(var, needs)?;
    if path.is_dir() {
        return Some(path);
    }
    eprintln!(
        "skipping: {var}={} is not a directory; it needs {needs}",
        path.display()
    );
    None
}

pub fn file(var: &str, needs: &str) -> Option<PathBuf> {
    let path = provided(var, needs)?;
    if path.is_file() {
        return Some(path);
    }
    eprintln!(
        "skipping: {var}={} is not a file; it needs {needs}",
        path.display()
    );
    None
}

fn provided(var: &str, needs: &str) -> Option<PathBuf> {
    match std::env::var_os(var) {
        Some(value) if !value.is_empty() => Some(PathBuf::from(value)),
        _ => {
            eprintln!("skipping: {var} is unset; it needs {needs}");
            None
        }
    }
}

pub fn read(path: &Path) -> Vec<u8> {
    let found = std::fs::read(path)
        .unwrap_or_else(|error| panic!("{} is not readable: {error}", path.display()));
    assert!(!found.is_empty(), "{} is empty", path.display());
    found
}

pub fn read_text(path: &Path) -> String {
    let found = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{} is not readable: {error}", path.display()));
    assert!(!found.is_empty(), "{} is empty", path.display());
    found
}

pub fn bytes(root: &Path, name: &str) -> Vec<u8> {
    read(&root.join(name))
}

pub fn text(root: &Path, name: &str) -> String {
    read_text(&root.join(name))
}
