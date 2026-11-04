//! What a decoder's signal looks like on the air, so Signal ID can name it.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{EventKind, Modulation, ProtocolMatch};
use crate::channel::ChannelParams;

const MIN_SCORE: f32 = 0.35;
const SKIRT: f64 = 1.5;
const MIN_SKIRT_FRACTION: f64 = 0.05;
const UNMEASURED: f32 = 0.75;
const UNKNOWN_DIAL: f32 = 0.8;
const GENERIC_WITH_DIAL: f32 = 0.95;
const OUT_OF_ALLOCATION: f32 = 0.4;

const WEIGHT_BANDWIDTH: f32 = 1.0;
const WEIGHT_SYMBOL_RATE: f32 = 2.0;
const WEIGHT_DEVIATION: f32 = 1.5;
const WEIGHT_BURST: f32 = 1.5;
const WEIGHT_SYMBOL_LENGTH: f32 = 2.0;

const ALLOCATION_SKIRT_FRACTION: f64 = 0.05;
const ALLOCATION_MIN_SKIRT_HZ: f64 = 1_000.0;

/// Every modulation a heard signal can be classified as.
pub const ANY_MODULATION: &[Modulation] = &[
    Modulation::Carrier,
    Modulation::Ook,
    Modulation::Am,
    Modulation::Ssb,
    Modulation::Fm,
    Modulation::Fsk2,
    Modulation::Fsk4,
    Modulation::Fsk8,
    Modulation::Psk2,
    Modulation::Psk4,
    Modulation::Ofdm,
    Modulation::NoiseLike,
    Modulation::Unknown,
];

/// A closed range of a measured quantity, in the unit its field names.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct HzRange {
    pub low: f64,
    pub high: f64,
}

impl HzRange {
    #[must_use]
    pub const fn new(low: f64, high: f64) -> Self {
        Self { low, high }
    }

    /// `center`, give or take `fraction` of it.
    #[must_use]
    pub const fn about(center: f64, fraction: f64) -> Self {
        Self::new(center * (1.0 - fraction), center * (1.0 + fraction))
    }

    #[must_use]
    pub const fn khz(low: f64, high: f64) -> Self {
        Self::new(low * 1e3, high * 1e3)
    }

    #[must_use]
    pub const fn mhz(low: f64, high: f64) -> Self {
        Self::new(low * 1e6, high * 1e6)
    }

    /// A channel `half` kHz either side of `center` kHz.
    #[must_use]
    pub const fn spot_khz(center: f64, half: f64) -> Self {
        Self::new((center - half) * 1e3, (center + half) * 1e3)
    }

    #[must_use]
    pub fn contains(self, value: f64) -> bool {
        (self.low..=self.high).contains(&value)
    }

    /// 1 inside the range, falling to 0 over a skirt beyond either edge.
    #[must_use]
    pub fn score(self, value: f64) -> f32 {
        let width = (self.high - self.low)
            .max(self.high * MIN_SKIRT_FRACTION)
            .max(1.0);
        let edge = if value < self.low {
            self.low
        } else {
            self.high
        };
        self.score_within(value, (width * SKIRT).min(edge.max(1.0)))
    }

    /// Like [`Self::score`] for a dial frequency, whose skirt is a small part of the band.
    #[must_use]
    pub fn allocation_score(self, value: f64) -> f32 {
        let skirt =
            ((self.high - self.low) * ALLOCATION_SKIRT_FRACTION).max(ALLOCATION_MIN_SKIRT_HZ);
        self.score_within(value, skirt)
    }

    fn score_within(self, value: f64, skirt: f64) -> f32 {
        if self.contains(value) {
            return 1.0;
        }
        let distance = if value < self.low {
            self.low - value
        } else {
            value - self.high
        };
        (1.0 - distance / skirt.max(1.0)).clamp(0.0, 1.0) as f32
    }
}

/// What Signal ID measured of one signal.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SignalMeasure {
    pub bandwidth_hz: f64,
    pub symbol_rate_hz: Option<f64>,
    /// Zero when the signal shows no frequency shift to measure.
    pub deviation_hz: f64,
    pub ofdm_symbol_us: Option<f64>,
    pub burst_ms: Option<f64>,
    /// Where the signal sits on the dial, when the dial is known.
    pub frequency_hz: Option<f64>,
}

fn unit_prior() -> f32 {
    1.0
}

/// One way a decoder's signal shows on the air.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SignalSignature {
    pub name: String,
    pub modulations: Vec<Modulation>,
    pub bandwidth_hz: HzRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_rate_hz: Option<HzRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deviation_hz: Option<HzRange>,
    /// Bursts the signal must show.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub burst_ms: Option<HzRange>,
    /// Bursts that count for the signal when heard, and against nothing when not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub burst_hint_ms: Option<HzRange>,
    /// The useful OFDM symbol length.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_us: Option<HzRange>,
    /// Where the service is allocated; empty for anywhere.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frequencies: Vec<HzRange>,
    #[serde(default = "unit_prior")]
    pub prior: f32,
    pub why: String,
}

impl SignalSignature {
    #[must_use]
    pub fn new(name: &str, modulations: &[Modulation], bandwidth_hz: HzRange, why: &str) -> Self {
        Self {
            name: name.to_owned(),
            modulations: modulations.to_vec(),
            bandwidth_hz,
            symbol_rate_hz: None,
            deviation_hz: None,
            burst_ms: None,
            burst_hint_ms: None,
            symbol_us: None,
            frequencies: Vec::new(),
            prior: unit_prior(),
            why: why.to_owned(),
        }
    }

    #[must_use]
    pub fn symbol_rate(mut self, hz: HzRange) -> Self {
        self.symbol_rate_hz = Some(hz);
        self
    }

    #[must_use]
    pub fn deviation(mut self, hz: HzRange) -> Self {
        self.deviation_hz = Some(hz);
        self
    }

    #[must_use]
    pub fn bursts(mut self, ms: HzRange) -> Self {
        self.burst_ms = Some(ms);
        self
    }

    #[must_use]
    pub fn burst_hint(mut self, ms: HzRange) -> Self {
        self.burst_hint_ms = Some(ms);
        self
    }

    #[must_use]
    pub fn symbol_length(mut self, us: HzRange) -> Self {
        self.symbol_us = Some(us);
        self
    }

    #[must_use]
    pub fn on(mut self, frequencies: &[HzRange]) -> Self {
        self.frequencies = frequencies.to_vec();
        self
    }

    #[must_use]
    pub fn prior(mut self, prior: f32) -> Self {
        self.prior = prior;
        self
    }

    #[must_use]
    pub fn in_allocation(&self, frequency_hz: f64) -> bool {
        self.frequencies
            .iter()
            .any(|range| range.contains(frequency_hz))
    }

    /// How loosely the symbol rate is pinned, relative to the rate: a tighter signature wins a
    /// tie.
    #[must_use]
    pub fn symbol_rate_tolerance(&self) -> f64 {
        self.symbol_rate_hz.map_or(f64::INFINITY, |rate| {
            (rate.high - rate.low) / rate.high.max(1.0)
        })
    }

    /// How well `measure` fits, unnamed by any decoder, or `None` below the floor worth
    /// offering.
    #[must_use]
    pub fn matched(&self, measure: &SignalMeasure) -> Option<ProtocolMatch> {
        let (total, weight) = self.waveform_score(measure)?;
        let allocation = measure
            .frequency_hz
            .and_then(|hz| self.allocation_score(hz));
        let placed = match (measure.frequency_hz, self.frequencies.is_empty()) {
            (Some(_), false) => {
                OUT_OF_ALLOCATION + (1.0 - OUT_OF_ALLOCATION) * allocation.unwrap_or(0.0)
            }
            (Some(_), true) => GENERIC_WITH_DIAL,
            (None, false) => UNKNOWN_DIAL,
            (None, true) => 1.0,
        };
        let score = total / weight * placed * self.prior;
        (score >= MIN_SCORE).then(|| ProtocolMatch {
            name: self.name.clone(),
            type_id: None,
            score,
            confirmed: false,
            why: if allocation == Some(1.0) {
                format!("{}, inside its allocation", self.why)
            } else {
                self.why.clone()
            },
        })
    }

    fn waveform_score(&self, measure: &SignalMeasure) -> Option<(f32, f32)> {
        let mut total = self.bandwidth_hz.score(measure.bandwidth_hz) * WEIGHT_BANDWIDTH;
        let mut weight = WEIGHT_BANDWIDTH;
        let mut add = |part: f32, part_weight: f32| {
            total += part_weight * part;
            weight += part_weight;
        };
        if let Some(expected) = self.symbol_rate_hz {
            let part = measure
                .symbol_rate_hz
                .map_or(UNMEASURED, |measured| expected.score(measured));
            add(part, WEIGHT_SYMBOL_RATE);
        }
        if let Some(expected) = self.deviation_hz {
            let part = if measure.deviation_hz > 0.0 {
                expected.score(measure.deviation_hz)
            } else {
                UNMEASURED
            };
            add(part, WEIGHT_DEVIATION);
        }
        if let Some(expected) = self.symbol_us {
            let part = measure
                .ofdm_symbol_us
                .map_or(UNMEASURED, |measured| expected.score(measured));
            add(part, WEIGHT_SYMBOL_LENGTH);
        }
        if let Some(expected) = self.burst_ms {
            add(expected.score(measure.burst_ms?), WEIGHT_BURST);
        }
        if let (Some(expected), Some(measured)) = (self.burst_hint_ms, measure.burst_ms) {
            add(expected.score(measured), WEIGHT_BURST);
        }
        Some((total, weight))
    }

    fn allocation_score(&self, frequency_hz: f64) -> Option<f32> {
        if self.frequencies.is_empty() {
            return None;
        }
        Some(
            self.frequencies
                .iter()
                .map(|allocation| allocation.allocation_score(frequency_hz))
                .fold(0.0, f32::max),
        )
    }
}

/// How Signal ID proves a signal is a decoder's: it runs the decoder on the signal and waits for
/// an event of a `proof` kind whose payload does not fail its own integrity check.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SignalTrial {
    pub proof: Vec<EventKind>,
    /// Settings tried side by side; empty tries the decoder's defaults.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<ChannelParams>,
}

impl SignalTrial {
    #[must_use]
    pub fn proven_by(kind: EventKind) -> Self {
        Self {
            proof: vec![kind],
            settings: Vec::new(),
        }
    }

    #[must_use]
    pub fn trying(mut self, settings: Vec<ChannelParams>) -> Self {
        self.settings = settings;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pager() -> SignalSignature {
        SignalSignature::new(
            "Pager",
            &[Modulation::Fsk2],
            HzRange::new(6e3, 20e3),
            "keying",
        )
        .symbol_rate(HzRange::about(1_200.0, 0.05))
        .on(&[HzRange::mhz(148.0, 175.0)])
    }

    #[test]
    fn a_signal_inside_its_allocation_scores_full_and_says_so() {
        let found = pager().matched(&SignalMeasure {
            bandwidth_hz: 12_500.0,
            symbol_rate_hz: Some(1_200.0),
            frequency_hz: Some(150e6),
            ..SignalMeasure::default()
        });
        let found = found.as_ref();
        assert_eq!(found.map(|m| m.score), Some(1.0));
        assert!(found.is_some_and(|m| m.why.ends_with("inside its allocation")));
        assert!(found.is_some_and(|m| m.type_id.is_none()));
    }

    #[test]
    fn a_required_burst_that_was_not_heard_is_no_match() {
        let bursty = pager().bursts(HzRange::new(25.0, 35.0));
        assert!(bursty.matched(&SignalMeasure::default()).is_none());
    }

    #[test]
    fn a_signature_survives_the_wire() {
        let signature = pager().deviation(HzRange::new(3e3, 6e3)).prior(0.9);
        let json = serde_json::to_value(&signature).expect("signature serializes");
        let back: SignalSignature = serde_json::from_value(json).expect("signature parses");
        assert_eq!(back, signature);
    }
}
