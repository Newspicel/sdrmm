#![no_main]

use libfuzzer_sys::fuzz_target;
use sdrmm_wire::WorkspaceExport;

fuzz_target!(|data: &[u8]| {
    let Ok(mut export) = serde_json::from_slice::<WorkspaceExport>(data) else {
        return;
    };
    let _ = export.validate();
    export.snapshot.graph.settle_arrays();
    let _ = export.validate_against(sdrmm_channels::descriptor_table());
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
