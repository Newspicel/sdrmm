//! A plugin taking a built-in's type, or one another plugin took, is refused alone with an error;
//! the rest of the directory loads.

use std::{path::PathBuf, sync::Arc};

use sdrmm_channels::{array_processor::processor_plugin, descriptors, registry};
use sdrmm_plugins::{PluginDir, PluginError};

mod common;

fn write(dir: &std::path::Path, file: &str, type_id: &str, processor: Option<&str>) {
    let processor = processor.map_or_else(String::new, |id| {
        format!(r#","processor":{{"type_id":"{id}","name":"{id}"}}"#)
    });
    let manifest = format!(
        r#"{{"abi":1,"descriptor":{{"type_id":"{type_id}","name":"{file}","bandwidth_hz":1000.0,"input_rate_hz":48000.0}}{processor}}}"#
    );
    let text = common::wat_text(1, &manifest, "i32.const 0", common::ARRAY_EXPORTS);
    std::fs::write(dir.join(format!("{file}.wasm")), text)
        .unwrap_or_else(|error| panic!("{error}"));
}

fn refused(errors: &[PluginError]) -> Vec<(String, String)> {
    errors
        .iter()
        .map(|error| match error {
            PluginError::Load { plugin, error } => (plugin.clone(), error.clone()),
            other => panic!("{other}"),
        })
        .collect()
}

#[test]
fn a_clashing_plugin_is_refused_alone() {
    let built_in = descriptors()[0].type_id.clone();
    let before = registry().find(&built_in).cloned().expect("a built-in");
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("clash");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|error| panic!("{error}"));
    write(&dir, "a_good", "clash_good", Some("clash_meter"));
    write(&dir, "b_built_in", &built_in, None);
    write(&dir, "c_twin", "clash_twin", None);
    write(&dir, "d_twin", "clash_twin", None);
    write(&dir, "e_processor", "clash_processor", Some("df"));
    write(&dir, "f_meter", "clash_other", Some("clash_meter"));

    let reload = PluginDir::new(dir).with_cache(None).reload();

    let refused = refused(&reload.errors);
    let names: Vec<&str> = refused.iter().map(|(plugin, _)| plugin.as_str()).collect();
    assert_eq!(names, ["b_built_in", "d_twin", "e_processor", "f_meter"]);
    assert!(refused[0].1.contains("built in"), "{}", refused[0].1);
    assert!(refused[1].1.contains("c_twin"), "{}", refused[1].1);
    assert!(refused[3].1.contains("a_good"), "{}", refused[3].1);

    let current = registry();
    assert!(current.find("clash_good").is_some());
    let twin = current.find("clash_twin").expect("the first twin loads");
    assert_eq!(twin.descriptor().name, "c_twin");
    assert!(current.find("clash_processor").is_none());
    assert!(current.find("clash_other").is_none());
    let kept = current.find(&built_in).expect("the built-in stays");
    assert!(Arc::ptr_eq(kept, &before));
    let meter = processor_plugin("clash_meter").expect("the first meter loads");
    assert_eq!(meter.kind().name, "clash_meter");
}
