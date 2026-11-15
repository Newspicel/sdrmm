use super::super::{mapping::Mapping, params::CpmParams};

/// Residual ISI, as a share of the decision half-spacing, below which a tap is dropped.
const RESIDUAL_ISI: f64 = 0.005;

/// Largest symbol-combination count averaged exactly; past it the mean is drawn at random.
const EXACT_COMBINATIONS: u64 = 1 << 20;

const RANDOM_DRAWS: usize = 1 << 16;

const DRAW_SEED: u32 = 0x9e37_79b9;

#[derive(Clone, Debug, PartialEq)]
pub struct SymbolResponse {
    taps: Vec<f32>,
    lead: usize,
    mean_abs: f32,
}

impl SymbolResponse {
    #[must_use]
    pub fn of(params: &CpmParams, receive_filter: &[f32]) -> Self {
        assert!(!receive_filter.is_empty(), "receive filter must have taps");
        let cascade = convolve(params.freq_pulse(), receive_filter);
        let sps = params.sps();
        let peak = cascade
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.abs().total_cmp(&b.abs()))
            .map_or(0, |(i, _)| i);

        let first = -((peak as f64 / sps).floor() as i64);
        let last = ((cascade.len() - 1 - peak) as f64 / sps).floor() as i64;
        let weights: Vec<f64> = (first..=last)
            .map(|i| sps * interpolate(&cascade, peak as f64 + i as f64 * sps))
            .collect();
        let cursor = (-first) as usize;

        let mapping = params.mapping();
        let floor =
            RESIDUAL_ISI * f64::from(mapping.min_spacing() / 2.0) / f64::from(mapping.max_level());
        let (lo, hi) = keep_window(&weights, cursor, floor);
        let taps: Vec<f32> = weights[lo..=hi].iter().map(|&w| w as f32).collect();
        let mean_abs = model_mean_abs(&taps, params.mapping());
        assert!(mean_abs > 0.0, "the pulse cascade has no energy");
        Self {
            taps,
            lead: cursor - lo,
            mean_abs,
        }
    }

    #[must_use]
    pub fn taps(&self) -> &[f32] {
        &self.taps
    }

    #[must_use]
    pub fn lead(&self) -> usize {
        self.lead
    }

    #[must_use]
    pub fn is_isi_free(&self) -> bool {
        self.taps.len() == 1
    }

    pub(super) fn mean_abs(&self) -> f32 {
        self.mean_abs
    }
}

fn convolve(a: &[f32], b: &[f32]) -> Vec<f64> {
    let mut out = vec![0.0f64; a.len() + b.len() - 1];
    for (i, &x) in a.iter().enumerate() {
        for (j, &h) in b.iter().enumerate() {
            out[i + j] += f64::from(x) * f64::from(h);
        }
    }
    out
}

fn interpolate(samples: &[f64], at: f64) -> f64 {
    if at < 0.0 || at >= (samples.len() - 1) as f64 {
        return samples.get(at.round() as usize).copied().unwrap_or(0.0);
    }
    let i = at.floor() as usize;
    let mu = at - i as f64;
    samples[i] * (1.0 - mu) + samples[i + 1] * mu
}

fn keep_window(weights: &[f64], cursor: usize, floor: f64) -> (usize, usize) {
    let mut lo = cursor;
    while lo > 0 && weights[lo - 1].abs() >= floor {
        lo -= 1;
    }
    let mut hi = cursor;
    while hi + 1 < weights.len() && weights[hi + 1].abs() >= floor {
        hi += 1;
    }
    (lo, hi)
}

fn model_mean_abs(taps: &[f32], mapping: &Mapping) -> f32 {
    let m = mapping.m();
    let levels = mapping.levels();
    match (m as u64).checked_pow(taps.len() as u32) {
        Some(count) if count <= EXACT_COMBINATIONS => {
            let mut total = 0.0f64;
            for combination in 0..count {
                let mut rest = combination as usize;
                let mut y = 0.0f64;
                for &tap in taps {
                    y += f64::from(tap) * f64::from(levels[rest % m]);
                    rest /= m;
                }
                total += y.abs();
            }
            (total / count as f64) as f32
        }
        _ => {
            let mut state = DRAW_SEED;
            let mut total = 0.0f64;
            for _ in 0..RANDOM_DRAWS {
                let mut y = 0.0f64;
                for &tap in taps {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    y += f64::from(tap) * f64::from(levels[state as usize % m]);
                }
                total += y.abs();
            }
            (total / RANDOM_DRAWS as f64) as f32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pulse::{self, Norm};

    const SPS: f64 = 10.0;

    fn gmsk(bt: f64, span: usize) -> CpmParams {
        CpmParams::from_h(
            Mapping::natural(2),
            0.5,
            pulse::gaussian_freq(SPS, bt, span, Norm::Area),
            SPS,
        )
    }

    fn gmsk_rx(bt: f64, span: usize) -> Vec<f32> {
        pulse::gaussian_freq(SPS, bt, span, Norm::Area)
    }

    #[test]
    fn a_full_response_pulse_through_its_matched_filter_has_no_isi() {
        let params = CpmParams::from_h(Mapping::natural(2), 0.5, pulse::rect(SPS, Norm::Area), SPS);
        let response = SymbolResponse::of(&params, &pulse::rect(SPS, Norm::Area));
        assert!(response.is_isi_free(), "taps {:?}", response.taps());
    }

    #[test]
    fn a_gaussian_cascade_spreads_a_symbol_and_conserves_its_level() {
        for (bt, span) in [(0.5, 3), (0.3, 4)] {
            let response = SymbolResponse::of(&gmsk(bt, span), &gmsk_rx(bt, span));
            assert!(
                response.taps().len() >= 3,
                "BT {bt}: only {} taps",
                response.taps().len()
            );
            let sum: f32 = response.taps().iter().sum();
            assert!((sum - 1.0).abs() < 2e-2, "BT {bt}: Σtaps = {sum}");
            assert_eq!(response.lead(), (response.taps().len() - 1) / 2);
        }
        let wide = SymbolResponse::of(&gmsk(0.3, 4), &gmsk_rx(0.3, 4));
        let narrow = SymbolResponse::of(&gmsk(0.5, 3), &gmsk_rx(0.5, 3));
        assert!(wide.taps().len() >= narrow.taps().len());
    }
}
