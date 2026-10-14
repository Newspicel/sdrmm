use num_complex::Complex;

use crate::ident::detect::{Band, Detector as SignalDetector, Faint, Search};

const FAINT_MIN_BANDWIDTH_HZ: f64 = 20_000.0;
const FAINT_THRESHOLD_DB: f32 = 3.0;

pub(super) struct Detector {
    detector: SignalDetector,
    rate: f64,
}

pub(super) struct Bands {
    pub(super) clear: Vec<Band>,
    pub(super) faint: Vec<Band>,
}

impl Detector {
    pub(super) fn new(rate: f64) -> Self {
        let size = ((rate / 250.0) as usize)
            .next_power_of_two()
            .clamp(4096, 262_144);
        Self {
            detector: SignalDetector::with_size(size, 33),
            rate,
        }
    }

    pub(super) fn measure(&mut self, iq: &[Complex<f32>]) -> Bands {
        let clear = self
            .detector
            .measure(
                iq,
                self.rate,
                &Search {
                    half_span_hz: self.rate / 2.0,
                    threshold_db: 12.0,
                    gap_hz: 3000.0,
                    dominated: false,
                    artifact_hz: None,
                },
            )
            .bands;
        let faint = self.detector.faint(
            self.rate,
            &Faint {
                half_span_hz: self.rate / 2.0,
                min_bandwidth_hz: FAINT_MIN_BANDWIDTH_HZ.min(self.rate / 8.0),
                threshold_db: FAINT_THRESHOLD_DB,
            },
        );
        Bands { clear, faint }
    }
}
