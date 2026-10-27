use std::{
    path::Path,
    thread,
    time::{Duration, Instant},
};

use crate::Engine;
use sdrmm_recorder::SigmfReader;

const NODE: &str = "capture-health-history";

fn pre_roll_s() -> u32 {
    super::number("SDRMM_CAPTURE_HISTORY_SECONDS", 1)
}

pub(super) fn check(engine: &Engine, elapsed: Duration) {
    for set in &engine.snapshot().device_sets {
        let history = set.pre_roll.as_ref().expect("armed history");
        assert!(
            history.error.is_none(),
            "history {} failed after {elapsed:?}: {:?}",
            set.device.id(),
            history.error
        );
    }
}

pub(super) fn start(engine: &Engine, sets: &[(u32, Vec<u32>)]) {
    for (ds, _) in sets {
        engine
            .arm_pre_roll(*ds, NODE.to_owned(), 0, pre_roll_s())
            .expect("arm history");
    }
    let deadline = Instant::now() + Duration::from_secs(10 + u64::from(pre_roll_s()));
    loop {
        let snapshot = engine.snapshot();
        let ready = snapshot.device_sets.iter().all(|set| {
            let history = set.pre_roll.as_ref().expect("armed history");
            assert!(
                history.error.is_none(),
                "history failed: {:?}",
                history.error
            );
            history.held_samples == history.capacity_samples
        });
        if ready {
            break;
        }
        assert!(Instant::now() < deadline, "history never filled");
        thread::sleep(Duration::from_millis(10));
    }
    for (ds, _) in sets {
        engine
            .start_pre_roll_capture(*ds, NODE)
            .expect("capture history");
    }
}

pub(super) fn finish(
    engine: &Engine,
    sets: &[(u32, Vec<u32>)],
    directory: &Path,
    seconds: u64,
    allow_drops: bool,
) {
    for (ds, _) in sets {
        let status = engine
            .stop_pre_roll_capture(*ds, NODE)
            .expect("finish history capture");
        let capture = status.capture.expect("history recording");
        eprintln!(
            "history ds={ds} samples={} bytes={} overruns={} error={:?}",
            capture.samples, capture.bytes, capture.overruns, status.error
        );
        assert!(status.error.is_none(), "history failed: {:?}", status.error);
        assert!(capture.error.is_none());
        let reader = SigmfReader::open(&directory.join(&capture.file)).expect("history file");
        assert_eq!(reader.total_samples(), capture.samples);
        assert_eq!(
            capture.bytes,
            capture.samples * sdrmm_recorder::BYTES_PER_SAMPLE
        );
        if !allow_drops {
            assert_eq!(capture.overruns, 0);
            assert!(
                capture.samples
                    >= (seconds + u64::from(pre_roll_s())).saturating_sub(1) * status.sample_rate,
                "history recording is too short"
            );
        }
        engine.drop_pre_roll(*ds, NODE).expect("disarm history");
    }
}
