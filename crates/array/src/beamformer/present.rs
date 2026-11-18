use sdrmm_wire::channel::{ChannelStatus, SettingChoice};
use sdrmm_wire::present::{FieldValue, fields, text_of};
use sdrmm_wire::processor::view::{PatternView, ProcessorView};

use super::BeamformerReading;
use crate::Report;
use crate::present::{angle_gap, db, degrees, flags, snr_health};

const NULL_CLOSE_DEG: f32 = 10.0;

fn signed_db(value: Option<f32>) -> Option<FieldValue> {
    text_of(format!("{:+.1} dB", value?))
}

fn out_text(center_hz: f64, rate: f64) -> Option<FieldValue> {
    (rate > 0.0)
        .then(|| format!("{:.3} MHz {:.0} kS/s", center_hz / 1e6, rate / 1e3))
        .and_then(text_of)
}

fn null_close(reading: &BeamformerReading) -> bool {
    reading.steer_deg.is_some_and(|steer| {
        reading
            .nulls_deg
            .iter()
            .any(|deg| angle_gap(*deg, steer) < NULL_CLOSE_DEG)
    })
}

impl Report for BeamformerReading {
    fn status(&self) -> ChannelStatus {
        let readouts = fields(&[
            ("mode", "Mode", text_of(self.mode.choice_label())),
            ("snr", "SNR", db(self.snr_db, 0)),
            ("gain", "Gain", signed_db(self.sinr_gain_db)),
            ("steer", "Steer", degrees(self.steer_deg)),
            ("cut", "Cut", db(self.cancelled_db, 0)),
            ("level", "Level", db(Some(self.output_db), 1)),
            ("out", "Out", out_text(self.out_center_hz, self.out_rate)),
        ]);
        ChannelStatus {
            health: snr_health(self.snr_db),
            readouts,
            choices: Vec::new(),
        }
    }

    fn view(&self) -> ProcessorView {
        let reset = format!("Weights restarted {} times", self.resets);
        ProcessorView {
            flags: flags(&[
                (self.no_steer, "No steer", "No bearing to steer at"),
                (self.steer_stale, "Stale", "Last bearing is old"),
                (self.singular, "Singular", "Lanes too alike to solve"),
                (self.resets > 0 || self.diverged, "Reset", &reset),
                (self.band_full, "Band full", "Band wider than the lanes"),
                (
                    null_close(self),
                    "Null close",
                    "A null sits next to the steer",
                ),
            ]),
            pattern: Some(PatternView {
                pattern: self.pattern.clone(),
                steer_deg: self.steer_deg,
                nulls_deg: self.nulls_deg.clone(),
                weights: self.weights.clone(),
            }),
            ..ProcessorView::default()
        }
    }
}
