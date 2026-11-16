use num_complex::Complex;

use crate::{
    math::fft::FftPair,
    math::stats::{median, quantile},
};

const MIN_RECORD: usize = 16;
const MAX_LAG_FRACTION: usize = 4;
const MIN_STRENGTH: f32 = 0.05;
const MAX_STRENGTH: f32 = 0.5;
const PROMINENCE: f32 = 4.0;
const GUARD_WINDOW_FRACTION: usize = 8;
const MAX_GUARD_FRACTION: f64 = 0.8;
const MIN_GUARD_FRACTION: usize = 32;
const GUARD_SHOULDER_FRACTION: usize = 16;
const MIN_GUARD_STRENGTH: f32 = 0.1;
const PULSED: f32 = 0.5;
const QUIET_QUANTILE: f32 = 0.2;
const LOUD_QUANTILE: f32 = 0.9;
/// Symbol periods the sliding correlation must span to show the guard's rhythm.
const GUARD_PERIODS: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CyclicPrefix {
    pub useful: usize,
    pub guard: Option<usize>,
    pub strength: f32,
}

pub struct CyclicPrefixSearch {
    fft: FftPair,
    buf: Vec<Complex<f32>>,
    magnitude: Vec<f32>,
    scratch: Vec<f32>,
    len: usize,
}

impl CyclicPrefixSearch {
    #[must_use]
    pub fn new(len: usize) -> Self {
        let len = len.next_power_of_two().max(2);
        Self {
            fft: FftPair::new(2 * len),
            buf: vec![Complex::default(); 2 * len],
            magnitude: vec![0.0; len],
            scratch: Vec::with_capacity(len),
            len,
        }
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn search(
        &mut self,
        iq: &[Complex<f32>],
        min_lag: usize,
        max_lag: usize,
    ) -> Option<CyclicPrefix> {
        let n = iq.len().min(self.len);
        let max_lag = max_lag.min(n / MAX_LAG_FRACTION);
        if n < MIN_RECORD || min_lag < 1 || min_lag + 2 >= max_lag {
            return None;
        }
        let total = self.autocorrelate(iq, n);
        if total <= f32::MIN_POSITIVE {
            return None;
        }
        for lag in min_lag..=max_lag {
            self.magnitude[lag] = self.buf[lag].norm() / total * (n as f32 / (n - lag) as f32);
        }
        let useful = (min_lag + 1..max_lag)
            .max_by(|&a, &b| self.magnitude[a].total_cmp(&self.magnitude[b]))?;
        let strength = self.magnitude[useful];
        if !(MIN_STRENGTH..=MAX_STRENGTH).contains(&strength)
            || strength < self.magnitude[useful - 1]
            || strength < self.magnitude[useful + 1]
        {
            return None;
        }
        let baseline = self.median(min_lag, max_lag);
        if strength < baseline * PROMINENCE {
            return None;
        }
        let guard = self.guard(iq, n, useful)?;
        Some(CyclicPrefix {
            useful,
            guard,
            strength,
        })
    }

    fn autocorrelate(&mut self, iq: &[Complex<f32>], n: usize) -> f32 {
        self.buf[..n].copy_from_slice(&iq[..n]);
        self.buf[n..].fill(Complex::default());
        self.power_spectrum_to_correlation();
        self.buf[0].re
    }

    fn power_spectrum_to_correlation(&mut self) {
        self.fft.forward(&mut self.buf);
        for bin in &mut self.buf {
            *bin = Complex::new(bin.norm_sqr(), 0.0);
        }
        self.fft.inverse_scaled(&mut self.buf);
    }

    fn median(&mut self, lo: usize, hi: usize) -> f32 {
        self.scratch.clear();
        self.scratch.extend_from_slice(&self.magnitude[lo..=hi]);
        median(&mut self.scratch).unwrap_or_default()
    }

    #[expect(
        clippy::option_option,
        reason = "outer None rejects the prefix, inner None means no guard"
    )]
    fn guard(&mut self, iq: &[Complex<f32>], n: usize, useful: usize) -> Option<Option<usize>> {
        let window = (useful / GUARD_WINDOW_FRACTION).max(1);
        let span = n.checked_sub(useful + window)?;
        let longest = (useful as f64 * (1.0 + MAX_GUARD_FRACTION)) as usize;
        if span < GUARD_PERIODS * longest {
            return Some(None);
        }
        self.sliding_correlation(iq, useful, window, span);
        if !self.pulsed(span) {
            return None;
        }
        let mean = self.scratch.iter().sum::<f32>() / span as f32;
        for (slot, &value) in self.buf.iter_mut().zip(&self.scratch) {
            *slot = Complex::new(value - mean, 0.0);
        }
        self.buf[span..].fill(Complex::default());
        self.power_spectrum_to_correlation();
        let energy = self.buf[0].re;
        if energy <= f32::MIN_POSITIVE {
            return Some(None);
        }
        let shortest = useful + (useful / MIN_GUARD_FRACTION).max(1);
        let symbol = (shortest..=longest.min(span / 2))
            .max_by(|&a, &b| self.buf[a].re.total_cmp(&self.buf[b].re))?;
        let shoulder = (useful / GUARD_SHOULDER_FRACTION).max(1);
        let peak = self.buf[symbol].re;
        let prominent = peak > self.buf[symbol - shoulder].re
            && symbol + shoulder < 2 * self.len
            && peak > self.buf[symbol + shoulder].re;
        Some((prominent && peak / energy >= MIN_GUARD_STRENGTH).then_some(symbol - useful))
    }

    fn sliding_correlation(
        &mut self,
        iq: &[Complex<f32>],
        useful: usize,
        window: usize,
        span: usize,
    ) {
        self.scratch.clear();
        let mut running = Complex::<f32>::default();
        for k in 0..window {
            running += iq[k] * iq[k + useful].conj();
        }
        self.scratch.push(running.norm());
        for m in 1..span {
            running += iq[m + window - 1] * iq[m + window - 1 + useful].conj();
            running -= iq[m - 1] * iq[m - 1 + useful].conj();
            self.scratch.push(running.norm());
        }
    }

    fn pulsed(&mut self, span: usize) -> bool {
        self.magnitude[..span].copy_from_slice(&self.scratch[..span]);
        let quiet = quantile(&mut self.magnitude[..span], QUIET_QUANTILE).unwrap_or_default();
        let loud = quantile(&mut self.magnitude[..span], LOUD_QUANTILE).unwrap_or_default();
        quiet < loud * PULSED
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;
    use crate::{math::rng::Rng, testutil::complex_noise};

    fn ofdm(
        useful: usize,
        guard: usize,
        carriers: usize,
        symbols: usize,
        seed: u64,
    ) -> Vec<Complex<f32>> {
        let mut fft = FftPair::new(useful);
        let mut rng = Rng::mixed(seed);
        let mut out = Vec::with_capacity(symbols * (useful + guard));
        let mut symbol = vec![Complex::default(); useful];
        for _ in 0..symbols {
            symbol.fill(Complex::default());
            for k in 1..=carriers / 2 {
                for index in [k, useful - k] {
                    let phase = (rng.bits() % 4) as f32 * TAU / 4.0 + TAU / 8.0;
                    symbol[index] = Complex::from_polar(1.0, phase);
                }
            }
            fft.inverse_scaled(&mut symbol);
            out.extend_from_slice(&symbol[useful - guard..]);
            out.extend_from_slice(&symbol);
        }
        out
    }

    #[test]
    fn a_cyclic_prefix_gives_away_the_useful_symbol_and_the_guard() {
        let mut iq = ofdm(256, 32, 96, 120, 0x1234);
        for (s, n) in iq.iter_mut().zip(complex_noise(1 << 16, 7e-5, 0x77)) {
            *s += n;
        }
        let mut search = CyclicPrefixSearch::new(1 << 15);
        let found = search
            .search(&iq, 16, 2_048)
            .expect("an OFDM signal correlates with itself one useful symbol later");
        assert_eq!(found.useful, 256);
        assert_eq!(found.guard, Some(32));
        assert!(found.strength > 0.08, "strength {}", found.strength);
    }

    #[test]
    fn a_long_symbol_is_found_as_readily_as_a_short_one() {
        let iq = ofdm(1_024, 128, 400, 40, 0x51);
        let mut search = CyclicPrefixSearch::new(1 << 16);
        let found = search.search(&iq, 16, 8_192).expect("found");
        assert_eq!(found.useful, 1_024);
        let guard = found.guard.expect("the symbol period stands out");
        assert!((guard as i64 - 128).abs() <= 3, "guard {guard}");
    }

    #[test]
    fn a_guard_most_of_a_useful_symbol_long_is_found() {
        let iq = ofdm(256, 200, 96, 60, 0x5d);
        let mut search = CyclicPrefixSearch::new(1 << 15);
        let found = search.search(&iq, 16, 2_048).expect("found");
        assert_eq!(found.useful, 256);
        let guard = found.guard.expect("the symbol period stands out");
        assert!((guard as i64 - 200).abs() <= 3, "guard {guard}");
    }

    #[test]
    fn noise_has_no_cyclic_prefix() {
        let iq = complex_noise(1 << 15, 1.0, 0x9a);
        let mut search = CyclicPrefixSearch::new(1 << 15);
        assert_eq!(search.search(&iq, 16, 2_048), None);
    }

    #[test]
    fn a_bare_carrier_correlates_everywhere_and_so_nowhere_in_particular() {
        let iq: Vec<Complex<f32>> = (0..1 << 14)
            .map(|k| Complex::from_polar(1.0, TAU * 0.01 * k as f32))
            .collect();
        let mut search = CyclicPrefixSearch::new(1 << 14);
        assert_eq!(search.search(&iq, 16, 1_024), None);
    }

    #[test]
    fn a_signal_that_merely_repeats_itself_is_not_ofdm() {
        let burst = complex_noise(300, 1.0, 0x1234);
        let mut iq = Vec::new();
        for _ in 0..60 {
            iq.extend_from_slice(&burst);
        }
        let mut search = CyclicPrefixSearch::new(1 << 15);
        assert_eq!(search.search(&iq, 16, 2_048), None);
    }

    #[test]
    fn an_alternating_preamble_is_not_ofdm() {
        let mut iq: Vec<Complex<f32>> = Vec::new();
        let mut phase = 0.0f32;
        for k in 0..20_000 {
            let up = (k / 40) % 2 == 0;
            phase += if up { 0.3 } else { -0.3 };
            iq.push(Complex::from_polar(1.0, phase));
        }
        iq.extend(complex_noise(12_000, 1.0, 0x51));
        let mut search = CyclicPrefixSearch::new(1 << 15);
        assert_eq!(search.search(&iq, 16, 2_048), None);
    }

    #[test]
    fn too_short_a_record_declines_rather_than_guesses() {
        let iq = ofdm(256, 32, 96, 1, 0x3);
        let mut search = CyclicPrefixSearch::new(1 << 12);
        assert_eq!(search.search(&iq[..64], 16, 2_048), None);
    }
}
