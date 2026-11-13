//! A compiled plugin is kept on disk and loaded from there next time; a damaged or outdated
//! build is replaced, never run.

use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use sdrmm_channels::Decoder;
use sdrmm_plugins::{ModuleCache, PluginModule, WasmDecoder};

mod common;

fn manifest(name: &str) -> String {
    format!(
        r#"{{"abi":1,"descriptor":{{"type_id":"cached","name":"{name}","bandwidth_hz":1000.0,"input_rate_hz":48000.0}}}}"#
    )
}

fn load(cache: &ModuleCache, name: &str) -> String {
    let text = common::wat_text(1, &manifest(name), "i32.const 0", "");
    let module = PluginModule::from_bytes("cached".to_owned(), text.as_bytes(), Some(cache))
        .unwrap_or_else(|error| panic!("{error}"));
    WasmDecoder::new(module).descriptor().name.clone()
}

fn builds(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("{error}"))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "cwasm"))
        .collect()
}

fn modified(path: &Path) -> SystemTime {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .unwrap_or_else(|error| panic!("{error}"))
}

#[test]
fn a_plugin_compiles_once_and_a_bad_build_is_replaced() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("module-cache");
    let _ = fs::remove_dir_all(&dir);
    let cache = ModuleCache::new(dir.clone());

    assert_eq!(load(&cache, "First"), "First");
    let [build] = builds(&dir).try_into().expect("one build");
    let written = modified(&build);

    assert_eq!(load(&cache, "First"), "First");
    assert_eq!(
        modified(&build),
        written,
        "a cached build is not compiled again"
    );

    let mut bytes = fs::read(&build).unwrap_or_else(|error| panic!("{error}"));
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    fs::write(&build, &bytes).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(load(&cache, "First"), "First");
    assert_ne!(
        fs::read(&build).unwrap_or_else(|error| panic!("{error}")),
        bytes,
        "the damaged build is rewritten"
    );

    assert_eq!(load(&cache, "Second"), "Second");
    let [newer] = builds(&dir).try_into().expect("one build per plugin");
    assert_ne!(newer, build);
}
