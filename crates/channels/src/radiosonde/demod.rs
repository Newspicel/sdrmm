use num_complex::Complex;
use sdrmm_dsp::{FmDemod, MovingAverage, demod::sync::CrossingClock};

pub(crate) const RATE: f64 = 48_000.0;
pub(super) const CHUNK: usize = 4_096;

const NOMINAL_DEVIATION_HZ: f64 = 2_400.0;
const DC_TRACK_SAMPLES: f32 = 2_400.0;
const BOXCAR_MAX: usize = 32;
const TIMING_GAIN: f64 = 0.12;

pub(super) struct FrontEnd {
    fm: FmDemod,
    dc: f32,
    audio: Vec<f32>,
}

impl FrontEnd {
    pub(super) fn new() -> Self {
        Self {
            fm: FmDemod::new(RATE, NOMINAL_DEVIATION_HZ),
            dc: 0.0,
            audio: Vec::with_capacity(CHUNK),
        }
    }

    pub(super) fn demodulate(&mut self, iq: &[Complex<f32>]) -> &[f32] {
        self.fm.process(iq, &mut self.audio);
        for sample in &mut self.audio {
            self.dc += (*sample - self.dc) / DC_TRACK_SAMPLES;
            *sample -= self.dc;
        }
        &self.audio
    }
}

pub(super) struct SymbolClock {
    boxcar: MovingAverage,
    clock: CrossingClock,
}

impl SymbolClock {
    pub(super) fn new(baud: f64, smoothing: f64) -> Self {
        let samples = (RATE / baud * smoothing).round() as usize;
        Self {
            boxcar: MovingAverage::new(samples.clamp(1, BOXCAR_MAX)),
            clock: CrossingClock::new(RATE, baud, TIMING_GAIN),
        }
    }

    pub(super) fn push(&mut self, x: f32) -> Option<f32> {
        let smoothed = self.boxcar.push(x);
        self.clock.push(smoothed)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Polarity {
    Normal,
    Inverted,
}

impl Polarity {
    pub(super) fn apply(self, bit: bool) -> bool {
        match self {
            Self::Normal => bit,
            Self::Inverted => !bit,
        }
    }
}

pub(super) struct SyncMatcher {
    pattern: u64,
    mask: u64,
    tolerance: u32,
    register: u64,
}

impl SyncMatcher {
    pub(super) fn from_bits(bits: &[u8], tolerance: u32) -> Self {
        let pattern = bits
            .iter()
            .fold(0u64, |acc, &bit| (acc << 1) | u64::from(bit == b'1'));
        let mask = if bits.len() >= 64 {
            u64::MAX
        } else {
            (1u64 << bits.len()) - 1
        };
        Self {
            pattern,
            mask,
            tolerance,
            register: 0,
        }
    }

    pub(super) fn from_bytes_lsb_first(bytes: &[u8], tolerance: u32) -> Self {
        let mut bits = [0u8; 64];
        let mut len = 0;
        for &byte in bytes.iter().take(8) {
            for shift in 0..8 {
                bits[len] = if (byte >> shift) & 1 == 1 { b'1' } else { b'0' };
                len += 1;
            }
        }
        Self::from_bits(&bits[..len], tolerance)
    }

    pub(super) fn push(&mut self, bit: bool) -> Option<Polarity> {
        self.register = (self.register << 1) | u64::from(bit);
        let distance = ((self.register ^ self.pattern) & self.mask).count_ones();
        let width = self.mask.count_ones();
        if distance <= self.tolerance {
            Some(Polarity::Normal)
        } else if width - distance <= self.tolerance {
            Some(Polarity::Inverted)
        } else {
            None
        }
    }

    pub(super) fn last_bits(&self, count: u32) -> u64 {
        self.register & ((1u64 << count) - 1)
    }

    pub(super) fn clear(&mut self) {
        self.register = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_recovers_symbols_from_a_square_wave_with_rate_error() {
        let baud = 4_800.0;
        let mut clock = SymbolClock::new(baud, 0.5);
        let sps = RATE / (baud * 1.002);
        let bits: Vec<bool> = (0..2_000).map(|k| (k * 7 + k / 3) % 5 < 2).collect();
        let total = (bits.len() as f64 * sps) as usize;
        let mut decided = Vec::new();
        for n in 0..total {
            let index = ((n as f64 / sps) as usize).min(bits.len() - 1);
            let x = if bits[index] { 1.0 } else { -1.0 };
            if let Some(symbol) = clock.push(x) {
                decided.push(symbol > 0.0);
            }
        }
        let tail = &decided[decided.len() - 1_000..];
        let found = (0..bits.len() - 1_000).any(|offset| {
            bits[offset..offset + 1_000]
                .iter()
                .zip(tail)
                .all(|(a, b)| a == b)
        });
        assert!(found, "decided symbols do not match the transmitted bits");
    }

    #[test]
    fn sync_matcher_flags_inverted_patterns() {
        let mut matcher = SyncMatcher::from_bits(b"1100101", 0);
        let mut seen = None;
        for &bit in b"0011010" {
            seen = matcher.push(bit == b'1');
        }
        assert_eq!(seen, Some(Polarity::Inverted));
    }
}
