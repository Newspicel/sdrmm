use sdrmm_wire::channel::ChannelStatus;
use sdrmm_wire::present::{fields, text_of};
use sdrmm_wire::processor::view::{Mark, MarkShape, ProcessorView, SurfaceView};

use super::{SpatialPeak, SpatialReading};
use crate::Report;
use crate::present::{drops, ranked};

const SPATIAL_PEAK_ROWS: usize = 3;

fn spatial_peak_text(peak: &SpatialPeak) -> String {
    let deg = peak.true_deg.unwrap_or(peak.bearing_deg).rem_euclid(360.0);
    format!("{:.3} MHz {deg:.0}°", peak.freq_hz / 1e6)
}

impl Report for SpatialReading {
    fn status(&self) -> ChannelStatus {
        let mut peaks = self.peaks.clone();
        peaks.sort_by(|a, b| b.db.total_cmp(&a.db));
        peaks.truncate(SPATIAL_PEAK_ROWS);
        let mut readouts = fields(&[(
            "peak",
            "Peak",
            peaks.first().map(spatial_peak_text).and_then(text_of),
        )]);
        readouts.extend(ranked(&peaks, spatial_peak_text));
        ChannelStatus::readouts(readouts)
    }

    fn view(&self) -> ProcessorView {
        ProcessorView {
            flags: drops(self.dropped_frames, "Frames lost")
                .into_iter()
                .collect(),
            surface: Some(SurfaceView {
                marks: self
                    .peaks
                    .iter()
                    .map(|peak| Mark {
                        x: peak.freq_hz,
                        y: f64::from(peak.bearing_deg),
                        true_y: peak.true_deg.map(f64::from),
                        shape: MarkShape::Cross,
                        title: Some(format!("{:.0} dB", peak.db)),
                        ..Mark::default()
                    })
                    .collect(),
                heading_deg: self.azimuth_deg,
                ..SurfaceView::default()
            }),
            ..ProcessorView::default()
        }
    }
}
