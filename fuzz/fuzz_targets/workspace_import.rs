#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use sdrmm_wire::{ChannelDescriptor, WorkspaceExport};

static DESCRIPTORS: OnceLock<Vec<ChannelDescriptor>> = OnceLock::new();

fuzz_target!(|data: &[u8]| {
    let Ok(mut export) = serde_json::from_slice::<WorkspaceExport>(data) else {
        return;
    };
    let _ = export.validate();
    export.snapshot.graph.settle_arrays();
    let _ = export.validate_against(DESCRIPTORS.get_or_init(sdrmm_channels::descriptors));
    export.forget_absent_nodes();
    let Ok(round) = serde_json::to_vec(&export) else {
        return;
    };
    let Ok(again) = serde_json::from_slice::<WorkspaceExport>(&round) else {
        panic!("an imported workspace did not survive re-encoding");
    };
    assert_eq!(
        export, again,
        "an imported workspace changed when it was re-encoded"
    );
});
