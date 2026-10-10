use std::{
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
    time::Duration,
};

use sdrmm_wire::DenoiseModel;
use sha2::{Digest, Sha256};

static FETCHING: Mutex<()> = Mutex::new(());

pub fn denoise_model(model: DenoiseModel) -> Result<Vec<u8>, String> {
    let _guard = FETCHING.lock().unwrap_or_else(PoisonError::into_inner);
    let path = cache_dir().join(model.file_name());
    if let Ok(bytes) = std::fs::read(&path)
        && matches_catalog(model, &bytes)
    {
        return Ok(bytes);
    }
    let bytes = download(model)?;
    if !matches_catalog(model, &bytes) {
        return Err(format!("{} does not match its catalog entry", model.url()));
    }
    store(&path, &bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(bytes)
}

fn cache_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(
            || PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target")),
            PathBuf::from,
        )
        .join("test-models")
}

fn matches_catalog(model: DenoiseModel, bytes: &[u8]) -> bool {
    let expected = model.artifact();
    let digest: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    bytes.len() as u64 == expected.bytes && digest == expected.sha256
}

fn download(model: DenoiseModel) -> Result<Vec<u8>, String> {
    let url = model.url();
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .and_then(|client| client.get(&url).send())
        .and_then(reqwest::blocking::Response::error_for_status)
        .and_then(reqwest::blocking::Response::bytes)
        .map(|bytes| bytes.to_vec())
        .map_err(|error| format!("could not download {url}: {error}"))
}

fn store(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let partial = path.with_extension(format!("{}.part", std::process::id()));
    std::fs::write(&partial, bytes)?;
    std::fs::rename(&partial, path)
}
