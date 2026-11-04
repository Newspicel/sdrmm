use std::{
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use sdrmm_channels::{Decoder, install_plugins};

use crate::{PluginError, decoder::WasmDecoder, module::PluginModule};

/// Quiet time after the last change before the directory is read again, so a plugin still
/// being written is not loaded half done.
const SETTLE: Duration = Duration::from_millis(300);

/// Every `.wasm` in `dir` as a decoder, with the reason for each file that would not load.
#[must_use]
pub fn load_dir(dir: &Path) -> (Vec<Arc<dyn Decoder>>, Vec<PluginError>) {
    let mut decoders: Vec<Arc<dyn Decoder>> = Vec::new();
    let mut errors = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (decoders, errors),
        Err(error) => {
            errors.push(PluginError::Dir(error.to_string()));
            return (decoders, errors);
        }
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "wasm"))
        .collect();
    paths.sort();
    for path in paths {
        match PluginModule::load(&path) {
            Ok(module) => decoders.push(Arc::new(WasmDecoder::new(module))),
            Err(error) => errors.push(error),
        }
    }
    (decoders, errors)
}

/// Loads `dir` into the decoder registry, logging every plugin that failed.
pub fn reload(dir: &Path) -> Vec<PluginError> {
    let (decoders, mut errors) = load_dir(dir);
    let count = decoders.len();
    match install_plugins(decoders) {
        Ok(()) => tracing::info!(count, dir = %dir.display(), "plugins loaded"),
        Err(error) => errors.push(PluginError::Install(error.to_string())),
    }
    for error in &errors {
        tracing::error!(%error, "plugin not loaded");
    }
    errors
}

/// Keeps the registry in step with the plugins directory for as long as it is held.
pub struct PluginWatcher {
    _watcher: RecommendedWatcher,
}

impl PluginWatcher {
    /// Loads `dir` now and again whenever a file in it changes.
    pub fn start(dir: PathBuf) -> Result<Self, PluginError> {
        std::fs::create_dir_all(&dir).map_err(|error| PluginError::Dir(error.to_string()))?;
        reload(&dir);
        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = tx.send(event);
        })
        .map_err(|error| PluginError::Dir(error.to_string()))?;
        watcher
            .watch(&dir, RecursiveMode::NonRecursive)
            .map_err(|error| PluginError::Dir(error.to_string()))?;
        thread::Builder::new()
            .name("plugin-watch".to_owned())
            .spawn(move || settle_and_reload(&dir, &rx))
            .map_err(|error| PluginError::Dir(error.to_string()))?;
        Ok(Self { _watcher: watcher })
    }
}

fn settle_and_reload(dir: &Path, rx: &mpsc::Receiver<notify::Result<notify::Event>>) {
    while rx.recv().is_ok() {
        while rx.recv_timeout(SETTLE).is_ok() {}
        reload(dir);
    }
}
