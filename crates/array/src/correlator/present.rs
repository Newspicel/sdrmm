use sdrmm_wire::channel::ChannelStatus;
use sdrmm_wire::present::{Unit, count, fields, number};
use sdrmm_wire::processor::view::{Layer, ProcessorView, SurfaceView};

use super::{Baseline, CorrelatorReading};
use crate::Report;

const BASELINE_TITLE: &str = "Coherence, phase, delay, SNR";

fn baseline_layer(baseline: &Baseline) -> Layer {
    Layer {
        label: format!("{}-{}", baseline.a + 1, baseline.b + 1),
        text: format!(
            "{:.2} ∠{:.0}° {:.1} ns {:.0} dB",
            baseline.coherence, baseline.phase_deg, baseline.delay_ns, baseline.snr_db
        ),
        title: BASELINE_TITLE.to_owned(),
        phase_deg: Some(baseline.phase_deg),
    }
}

impl Report for CorrelatorReading {
    fn status(&self) -> ChannelStatus {
        let best = self
            .baselines
            .iter()
            .map(|baseline| baseline.coherence)
            .reduce(f32::max);
        ChannelStatus::readouts(fields(&[
            (
                "integrated",
                "Int",
                number(Some(self.integrated_s), Unit::Seconds, 1),
            ),
            (
                "baselines",
                "Baselines",
                count(Some(self.baselines.len() as f64)),
            ),
            ("coherence", "Best coh", number(best, Unit::None, 2)),
        ]))
    }

    fn view(&self) -> ProcessorView {
        ProcessorView {
            surface: Some(SurfaceView {
                layers: self.baselines.iter().map(baseline_layer).collect(),
                ..SurfaceView::default()
            }),
            ..ProcessorView::default()
        }
    }
}
