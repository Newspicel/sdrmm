use super::mapping::Mapping;

/// Tolerance on a pulse's unit area.
const AREA_TOLERANCE: f64 = 1e-3;

const MIN_SPS: f64 = 2.0;

#[derive(Clone, Debug)]
pub struct CpmParams {
    mapping: Mapping,
    h: f64,
    freq_pulse: Vec<f32>,
    sps: f64,
}

impl CpmParams {
    #[must_use]
    pub fn from_h(mapping: Mapping, h: f64, freq_pulse: Vec<f32>, sps: f64) -> Self {
        assert!(
            h.is_finite() && h > 0.0,
            "modulation index must be positive"
        );
        assert!(
            sps.is_finite() && sps >= MIN_SPS,
            "need at least two samples per symbol"
        );
        assert!(
            h * f64::from(mapping.max_level()) < sps,
            "outer deviation exceeds Nyquist: h·max_level = {} at {sps} samples/symbol",
            h * f64::from(mapping.max_level())
        );
        assert!(!freq_pulse.is_empty(), "frequency pulse must have taps");
        assert!(
            freq_pulse.iter().all(|t| t.is_finite()),
            "frequency pulse taps must be finite"
        );
        assert_unit_area(&freq_pulse, "frequency pulse");
        Self {
            mapping,
            h,
            freq_pulse,
            sps,
        }
    }

    #[must_use]
    pub fn from_deviation(
        mapping: Mapping,
        deviation_hz: f64,
        baud: f64,
        freq_pulse: Vec<f32>,
        sps: f64,
    ) -> Self {
        assert!(
            deviation_hz.is_finite() && deviation_hz > 0.0,
            "deviation must be positive"
        );
        assert!(baud.is_finite() && baud > 0.0, "baud must be positive");
        let h = 2.0 * deviation_hz / (f64::from(mapping.max_level()) * baud);
        Self::from_h(mapping, h, freq_pulse, sps)
    }

    #[must_use]
    pub fn mapping(&self) -> &Mapping {
        &self.mapping
    }

    #[must_use]
    pub fn h(&self) -> f64 {
        self.h
    }

    #[must_use]
    pub fn freq_pulse(&self) -> &[f32] {
        &self.freq_pulse
    }

    #[must_use]
    pub fn sps(&self) -> f64 {
        self.sps
    }
}

/// Asserts `taps` sums to one, as `pulse::Norm::Area` builds it.
pub(super) fn assert_unit_area(taps: &[f32], name: &str) {
    let area: f64 = taps.iter().map(|&t| f64::from(t)).sum();
    assert!(
        (area - 1.0).abs() < AREA_TOLERANCE,
        "{name} must be unit-area (pulse::Norm::Area), got Σ = {area}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pulse::{self, Norm};

    fn dmr_mapping() -> Mapping {
        Mapping::new(vec![1.0, 3.0, -1.0, -3.0])
    }
    #[test]
    fn deviation_conversion_reproduces_the_documented_formula() {
        let p = CpmParams::from_deviation(
            dmr_mapping(),
            1_944.0,
            4_800.0,
            pulse::root_raised_cosine(10.0, 0.2, 8, Norm::Area),
            10.0,
        );
        assert!((p.h() - 0.27).abs() < 1e-12, "h = {}", p.h());
        let ais = CpmParams::from_deviation(
            Mapping::natural(2),
            2_400.0,
            9_600.0,
            pulse::gaussian_freq(5.0, 0.4, 3, Norm::Area),
            5.0,
        );
        assert!((ais.h() - 0.5).abs() < 1e-12);
    }

    #[test]
    #[should_panic(expected = "unit-area")]
    fn an_energy_normalised_pulse_is_rejected() {
        let _ = CpmParams::from_h(
            Mapping::natural(2),
            0.5,
            pulse::rect(8.0, Norm::Energy),
            8.0,
        );
    }

    #[test]
    #[should_panic(expected = "Nyquist")]
    fn a_deviation_past_nyquist_is_rejected() {
        let _ = CpmParams::from_h(Mapping::natural(4), 4.0, pulse::rect(8.0, Norm::Area), 8.0);
    }
}
