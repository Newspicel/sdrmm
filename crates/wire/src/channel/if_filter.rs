use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub const MAX_IF_NOTCHES: usize = 8;
pub const MIN_IF_NOTCH_WIDTH_HZ: f64 = 20.0;
pub const MAX_IF_NOTCH_WIDTH_HZ: f64 = 50_000.0;
pub const DEFAULT_IF_NOTCH_WIDTH_HZ: f64 = 2_000.0;
pub const MAX_IF_EDGE_HZ: f64 = 5_000_000.0;
pub const MIN_IF_PASSBAND_HZ: f64 = 100.0;

/// Shapes the IF ahead of the demodulator, so a neighbour is cut before it can capture it.
///
/// Edges are offsets from the channel frequency and only narrow the decoder's own passband.
/// Notches sit at fixed radio frequencies: a retune moves the channel, never the notch, so a notch
/// stays on its interferer.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IfFilter {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub low_hz: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high_hz: Option<f64>,
    #[serde(default)]
    pub notches: Vec<IfNotch>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct IfNotch {
    pub frequency_hz: f64,
    #[serde(default = "default_if_notch_width_hz")]
    pub width_hz: f64,
}

const fn default_if_notch_width_hz() -> f64 {
    DEFAULT_IF_NOTCH_WIDTH_HZ
}

impl IfFilter {
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.enabled
            && (self.low_hz.is_some() || self.high_hz.is_some() || !self.notches.is_empty())
    }

    pub fn validate(&self) -> Result<(), String> {
        for (name, edge) in [("low", self.low_hz), ("high", self.high_hz)] {
            if edge.is_some_and(|hz| !(hz.is_finite() && hz.abs() <= MAX_IF_EDGE_HZ)) {
                return Err(format!(
                    "IF {name} edge must be within ±{MAX_IF_EDGE_HZ} Hz, got {edge:?}"
                ));
            }
        }
        if let (Some(low), Some(high)) = (self.low_hz, self.high_hz)
            && high - low < MIN_IF_PASSBAND_HZ
        {
            return Err(format!(
                "IF passband must be at least {MIN_IF_PASSBAND_HZ} Hz wide, got {low}..{high}"
            ));
        }
        if self.notches.len() > MAX_IF_NOTCHES {
            return Err(format!(
                "IF filter carries at most {MAX_IF_NOTCHES} notches, got {}",
                self.notches.len()
            ));
        }
        self.notches.iter().try_for_each(IfNotch::validate)
    }
}

impl IfNotch {
    fn validate(&self) -> Result<(), String> {
        if !(self.frequency_hz.is_finite() && self.frequency_hz > 0.0) {
            return Err(format!(
                "IF notch frequency must be positive, got {}",
                self.frequency_hz
            ));
        }
        if !(self.width_hz.is_finite()
            && (MIN_IF_NOTCH_WIDTH_HZ..=MAX_IF_NOTCH_WIDTH_HZ).contains(&self.width_hz))
        {
            return Err(format!(
                "IF notch width must be in {MIN_IF_NOTCH_WIDTH_HZ}..={MAX_IF_NOTCH_WIDTH_HZ}, got {}",
                self.width_hz
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notch(frequency_hz: f64, width_hz: f64) -> IfNotch {
        IfNotch {
            frequency_hz,
            width_hz,
        }
    }

    #[test]
    fn an_empty_or_disabled_filter_does_nothing() {
        assert!(!IfFilter::default().is_active());
        let parked = IfFilter {
            enabled: false,
            high_hz: Some(40_000.0),
            ..IfFilter::default()
        };
        assert!(!parked.is_active());
        assert!(
            IfFilter {
                enabled: true,
                ..parked
            }
            .is_active()
        );
    }

    #[test]
    fn edges_must_leave_a_passband() {
        let squeezed = IfFilter {
            low_hz: Some(10_000.0),
            high_hz: Some(10_050.0),
            ..IfFilter::default()
        };
        assert!(squeezed.validate().is_err());
        let one_sided = IfFilter {
            high_hz: Some(-20_000.0),
            ..IfFilter::default()
        };
        assert!(one_sided.validate().is_ok());
        let wild = IfFilter {
            low_hz: Some(f64::NAN),
            ..IfFilter::default()
        };
        assert!(wild.validate().is_err());
    }

    #[test]
    fn notches_are_bounded() {
        let mut filter = IfFilter {
            notches: vec![notch(100_000_000.0, DEFAULT_IF_NOTCH_WIDTH_HZ); MAX_IF_NOTCHES],
            ..IfFilter::default()
        };
        assert!(filter.validate().is_ok());
        filter.notches.push(notch(100_000_000.0, 1_000.0));
        assert!(filter.validate().is_err());
        for bad in [
            notch(0.0, 1_000.0),
            notch(1e8, 5.0),
            notch(1e8, f64::INFINITY),
        ] {
            let filter = IfFilter {
                notches: vec![bad],
                ..IfFilter::default()
            };
            assert!(filter.validate().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_notch_without_a_width_takes_the_default() {
        let parsed: IfNotch = serde_json::from_str(r#"{"frequency_hz": 99.9e6}"#).unwrap();
        assert_eq!(parsed.width_hz, DEFAULT_IF_NOTCH_WIDTH_HZ);
    }
}
