use crate::channel::{ChannelHealth, ChannelStatus, SettingChoice};
use crate::fusion::{DfEstimate, DfFusionState, NavTarget, NavTargetKind};
use crate::present::{Field, FieldValue, Unit, count, fields, number, percent, position, text_of};
use crate::processor::beamformer::BeamformerReading;
use crate::processor::correlator::CorrelatorReading;
use crate::processor::df::{DfPeak, DfReading};
use crate::processor::polarimeter::{Hand, PolarimeterReading};
use crate::processor::spatial::{SpatialPeak, SpatialReading};
use crate::processor::stitch::StitchReading;
use crate::radar::{RadarUpdate, ReferenceMode, TrackState};

fn degrees(value: Option<f32>) -> Option<FieldValue> {
    number(value.map(|deg| deg.rem_euclid(360.0)), Unit::Degrees, 0)
}

fn db(value: Option<f32>, digits: u8) -> Option<FieldValue> {
    number(value, Unit::Db, digits)
}

fn snr_health(snr_db: Option<f32>) -> ChannelHealth {
    snr_db.map_or_else(ChannelHealth::default, |db| {
        ChannelHealth::default().snr(db)
    })
}

/// The readouts of a peak past the first, keyed by its rank.
fn ranked<T>(peaks: &[T], text: impl Fn(&T) -> String) -> Vec<Field> {
    peaks
        .iter()
        .enumerate()
        .skip(1)
        .filter_map(|(index, peak)| {
            let rank = index + 1;
            text_of(text(peak))
                .map(|value| Field::new(format!("peak_{rank}"), format!("#{rank}"), value))
        })
        .collect()
}

fn df_peak_text(peak: &DfPeak) -> String {
    let deg = peak.true_deg.unwrap_or(peak.relative_deg).rem_euclid(360.0);
    format!(
        "{deg:.0}° {:.0}% ±{:.0}°",
        peak.confidence.clamp(0.0, 1.0) * 100.0,
        peak.sigma_deg
    )
}

impl DfReading {
    #[must_use]
    pub fn status(&self) -> ChannelStatus {
        let first = self.peaks.first();
        let mut readouts = fields(&[
            (
                "bearing",
                "Bearing",
                degrees(first.and_then(|peak| peak.true_deg)),
            ),
            (
                "relative",
                "Rel",
                degrees(first.map(|peak| peak.relative_deg)),
            ),
            (
                "sigma",
                "±",
                number(first.map(|peak| peak.sigma_deg), Unit::Degrees, 1),
            ),
            ("fit", "Fit", percent(self.fit)),
            ("snr", "SNR", db(Some(self.snr_db), 0)),
            ("sources", "Src", count(Some(self.sources))),
        ]);
        readouts.extend(ranked(&self.peaks, df_peak_text));
        ChannelStatus {
            health: snr_health(Some(self.snr_db)),
            readouts,
            choices: Vec::new(),
        }
    }
}

fn signed_db(value: Option<f32>) -> Option<FieldValue> {
    text_of(format!("{:+.1} dB", value?))
}

fn out_text(center_hz: f64, rate: f64) -> Option<FieldValue> {
    (rate > 0.0)
        .then(|| format!("{:.3} MHz {:.0} kS/s", center_hz / 1e6, rate / 1e3))
        .and_then(text_of)
}

impl BeamformerReading {
    #[must_use]
    pub fn status(&self) -> ChannelStatus {
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
}

fn reference_text(update: &RadarUpdate) -> String {
    let reference = update.health.reference;
    match (reference.mode, reference.locked) {
        (ReferenceMode::Raw, _) => "Raw".to_owned(),
        (_, false) => "Lost".to_owned(),
        (ReferenceMode::Cma, true) => format!("CMA {:.0} dB", reference.quality_db),
        (ReferenceMode::DabRemod, true) => format!("DAB {:.0} dB", reference.quality_db),
    }
}

fn mean(values: &[f32]) -> Option<f32> {
    (!values.is_empty()).then(|| values.iter().sum::<f32>() / values.len() as f32)
}

impl RadarUpdate {
    #[must_use]
    pub fn status(&self) -> ChannelStatus {
        let health = &self.health;
        let targets = self
            .tracks
            .iter()
            .filter(|track| track.state == TrackState::Confirmed)
            .count();
        let lost = health.dropped_cpis + health.dropped_reports + health.lagged_updates;
        let readouts = fields(&[
            ("targets", "Targets", count(Some(targets as f64))),
            ("clutter", "Clutter", db(mean(&health.suppression_db), 0)),
            ("load", "Load", percent(health.load)),
            ("reference", "Ref", text_of(reference_text(self))),
            (
                "cpi",
                "CPI",
                number(Some(self.axes.cpi_ms / 1_000.0), Unit::Seconds, 2),
            ),
            (
                "drops",
                "Drops",
                count(Some(lost as f64).filter(|lost| *lost > 0.0)),
            ),
        ]);
        ChannelStatus {
            health: ChannelHealth::default().lock(health.reference.locked),
            readouts,
            choices: Vec::new(),
        }
    }
}

impl CorrelatorReading {
    #[must_use]
    pub fn status(&self) -> ChannelStatus {
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
}

const fn hand_text(hand: Hand) -> &'static str {
    match hand {
        Hand::Right => "RH",
        Hand::Left => "LH",
        Hand::Linear => "Lin",
    }
}

impl PolarimeterReading {
    #[must_use]
    pub fn status(&self) -> ChannelStatus {
        let readouts = fields(&[
            ("i", "I", db(Some(self.i_db), 1)),
            ("degree", "Pol", percent(self.degree.clamp(0.0, 1.0))),
            ("q", "Q", number(Some(self.q), Unit::None, 2)),
            (
                "tilt",
                "Tilt",
                number(Some(self.angle_deg), Unit::Degrees, 0),
            ),
            ("u", "U", number(Some(self.u), Unit::None, 2)),
            (
                "ellipticity",
                "Ellip",
                number(Some(self.ellipticity_deg), Unit::Degrees, 0),
            ),
            ("v", "V", number(Some(self.v), Unit::None, 2)),
            ("hand", "Hand", text_of(hand_text(self.hand))),
            ("snr", "SNR", db(self.snr_db, 0)),
        ]);
        ChannelStatus {
            health: snr_health(self.snr_db),
            readouts,
            choices: Vec::new(),
        }
    }
}

impl StitchReading {
    #[must_use]
    pub fn status(&self) -> ChannelStatus {
        ChannelStatus::readouts(fields(&[
            (
                "center",
                "Center",
                number(Some(self.center_hz), Unit::Hz, 0),
            ),
            ("span", "Span", number(Some(self.span_hz), Unit::Hz, 0)),
            ("lanes", "Lanes", count(Some(self.lanes.len() as f64))),
        ]))
    }
}

const SPATIAL_PEAK_ROWS: usize = 3;

fn spatial_peak_text(peak: &SpatialPeak) -> String {
    let deg = peak.true_deg.unwrap_or(peak.bearing_deg).rem_euclid(360.0);
    format!("{:.3} MHz {deg:.0}°", peak.freq_hz / 1e6)
}

impl SpatialReading {
    #[must_use]
    pub fn status(&self) -> ChannelStatus {
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
}

fn distance(metres: f64) -> String {
    if metres >= 1_000.0 {
        format!("{:.1} km", metres / 1_000.0)
    } else {
        format!("{metres:.0} m")
    }
}

fn spread(estimate: &DfEstimate) -> String {
    format!(
        "{} × {}",
        distance(estimate.ellipse_major_m),
        distance(estimate.ellipse_minor_m)
    )
}

fn guidance(nav: &NavTarget) -> String {
    let action = match nav.kind {
        NavTargetKind::Probe => "Drive across",
        NavTargetKind::Estimate => "Drive at it",
    };
    format!(
        "{action} · {:.0}° · {}",
        nav.bearing_deg.rem_euclid(360.0),
        distance(nav.distance_m)
    )
}

impl DfFusionState {
    /// Readouts for a triangulation face; `fade` is the half life Auto settled on.
    #[must_use]
    pub fn status(&self) -> ChannelStatus {
        let estimate = self.estimate.as_ref();
        ChannelStatus::readouts(fields(&[
            (
                "estimate",
                "Estimate",
                estimate.and_then(|fix| position(Some(fix.lat), Some(fix.lon))),
            ),
            ("spread", "Spread", estimate.map(spread).and_then(text_of)),
            (
                "guidance",
                "Guidance",
                self.nav.as_ref().map(guidance).and_then(text_of),
            ),
            ("bearings", "Bearings", count(Some(self.samples))),
            (
                "fade",
                "Half life",
                number(Some(self.half_life_s).filter(|s| *s > 0), Unit::Seconds, 0),
            ),
        ]))
    }
}

#[cfg(test)]
mod tests;
