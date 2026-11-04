//! A reload rebuilds only the plugins whose file changed and names them, so their running
//! channels and array processors can reopen.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use sdrmm_channels::{Decoder, array_processor::processor_plugin, registry};
use sdrmm_plugins::PluginDir;

mod common;

const TYPE_ID: &str = "reload_watch";
const PROCESSOR_ID: &str = "reload_meter";

fn write(dir: &Path, name: &str) {
    let manifest = format!(
        r#"{{"abi":1,"descriptor":{{"type_id":"{TYPE_ID}","name":"{name}","bandwidth_hz":1000.0,"input_rate_hz":48000.0}},"processor":{{"type_id":"{PROCESSOR_ID}","name":"{name}"}}}}"#
    );
    let text = common::wat_text(1, &manifest, "i32.const 0", common::ARRAY_EXPORTS);
    std::fs::write(dir.join("watch.wasm"), text).unwrap_or_else(|error| panic!("{error}"));
}

fn installed() -> Option<Arc<dyn Decoder>> {
    registry().find(TYPE_ID).cloned()
}

#[test]
fn a_reload_names_the_plugins_a_changed_file_rebuilt() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("reload");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|error| panic!("{error}"));
    write(&dir, "First");
    let mut plugins = PluginDir::new(dir.clone());

    let first = plugins.reload();
    assert!(first.errors.is_empty(), "{:?}", first.errors);
    assert!(first.replaced.is_empty());
    let before = installed().expect("the plugin is installed");

    let unchanged = plugins.reload();
    assert!(unchanged.replaced.is_empty());
    let kept = installed().expect("still installed");
    assert!(
        Arc::ptr_eq(&before, &kept),
        "an unchanged file keeps its decoder"
    );

    write(&dir, "Second");
    let changed = plugins.reload();
    assert_eq!(changed.replaced, [TYPE_ID, PROCESSOR_ID]);
    let after = installed().expect("still installed");
    assert_eq!(after.descriptor().name, "Second");
    let meter = processor_plugin(PROCESSOR_ID).expect("the processor is loaded");
    assert_eq!(meter.kind().name, "Second");

    std::fs::remove_file(dir.join("watch.wasm")).unwrap_or_else(|error| panic!("{error}"));
    let removed = plugins.reload();
    assert!(removed.replaced.is_empty());
    assert!(installed().is_none());
    assert!(processor_plugin(PROCESSOR_ID).is_none());
}
