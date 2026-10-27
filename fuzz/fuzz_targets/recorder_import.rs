#![no_main]

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use libfuzzer_sys::fuzz_target;
use sdrmm_recorder::{Datatype, SigmfMeta, import_archive};

const MAX_SAMPLES: usize = 4_096;

static SCRATCH: OnceLock<PathBuf> = OnceLock::new();

fn scratch() -> &'static Path {
    SCRATCH.get_or_init(|| {
        std::env::temp_dir().join(format!("sdrmm-fuzz-recorder-{}", std::process::id()))
    })
}

fn walk_samples(meta: &SigmfMeta, bytes: &[u8]) {
    let Some(datatype) = Datatype::parse(&meta.global.datatype) else {
        return;
    };
    let stride = datatype.bytes_per_sample();
    for frame in bytes.chunks_exact(stride).take(MAX_SAMPLES) {
        std::hint::black_box(datatype.sample(frame));
    }
}

fuzz_target!(|data: &[u8]| {
    if let Ok(meta) = serde_json::from_slice::<SigmfMeta>(data) {
        walk_samples(&meta, data);
        let Ok(round) = serde_json::to_vec(&meta) else {
            return;
        };
        assert!(
            serde_json::from_slice::<SigmfMeta>(&round).is_ok(),
            "SigMF metadata did not survive re-encoding"
        );
    }
    let dir = scratch();
    let _ = import_archive(dir, data);
    let _ = std::fs::remove_dir_all(dir);
});
