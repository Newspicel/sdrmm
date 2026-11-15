const CROSSING_NUDGE: f64 = 0.125;

#[derive(Clone, Debug)]
pub struct BitSync {
    sample_rate: f64,
    increment: f64,
    phase: f64,
    positive: bool,
    primed: bool,
    since_symbol: usize,
}

impl BitSync {
    #[must_use]
    pub fn new(sample_rate: f64, baud: f64) -> Self {
        assert!(sample_rate > 0.0, "sample rate must be positive");
        let mut sync = Self {
            sample_rate,
            increment: 0.0,
            phase: 0.0,
            positive: false,
            primed: false,
            since_symbol: 0,
        };
        sync.set_baud(baud);
        sync.reset();
        sync
    }

    pub fn set_baud(&mut self, baud: f64) {
        assert!(
            baud.is_finite() && baud > 0.0 && baud * 2.0 <= self.sample_rate,
            "baud must be positive and at most half the sample rate"
        );
        self.increment = baud / self.sample_rate;
    }

    pub fn push(&mut self, sample: f32) -> Option<bool> {
        self.push_soft(sample).map(|v| v >= 0.0)
    }

    pub fn push_soft(&mut self, sample: f32) -> Option<f32> {
        let positive = sample >= 0.0;
        if self.primed && positive != self.positive {
            self.phase += CROSSING_NUDGE * (0.5 - self.phase);
        }
        self.positive = positive;
        self.primed = true;

        self.phase += self.increment;
        self.since_symbol += 1;
        if self.phase < 1.0 {
            return None;
        }
        self.phase -= 1.0;
        self.since_symbol = 0;
        Some(sample)
    }

    #[must_use]
    pub fn samples_since_symbol(&self) -> usize {
        self.since_symbol
    }

    pub fn reset(&mut self) {
        self.phase = 0.5;
        self.positive = false;
        self.primed = false;
        self.since_symbol = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::XorShift32;

    const RATE: f64 = 48_000.0;
    const BAUD: f64 = 1_200.0;
    const SAMPLES_PER_BIT: usize = 40;

    fn nrz(bits: &[bool]) -> Vec<f32> {
        bits.iter()
            .flat_map(|&b| std::iter::repeat_n(if b { 1.0 } else { -1.0 }, SAMPLES_PER_BIT))
            .collect()
    }

    fn bit_pattern(count: usize, seed: u32) -> Vec<bool> {
        let mut rng = XorShift32(seed);
        (0..count).map(|_| rng.next_f32() >= 0.0).collect()
    }

    fn recover(samples: &[f32]) -> Vec<bool> {
        let mut sync = BitSync::new(RATE, BAUD);
        samples.iter().filter_map(|&s| sync.push(s)).collect()
    }

    fn bits_align(tx: &[bool], rx: &[bool], settle: usize) -> bool {
        (0..3).any(|d| {
            let end = rx.len().saturating_sub(1);
            settle < end && (settle..end).all(|j| j + d < tx.len() && rx[j] == tx[j + d])
        })
    }

    #[test]
    fn soft_slices_agree_with_hard_ones_instant_for_instant() {
        let samples = nrz(&bit_pattern(200, 0x0bad_c0de));
        let mut hard = BitSync::new(RATE, BAUD);
        let mut soft = BitSync::new(RATE, BAUD);
        for &s in &samples {
            assert_eq!(hard.push(s), soft.push_soft(s).map(|v| v >= 0.0));
        }
    }

    #[test]
    fn recovers_a_bit_pattern_locked_to_the_waveform() {
        let bits = bit_pattern(500, 0x5eed_1234);
        let rx = recover(&nrz(&bits));
        assert_eq!(rx.len(), 500, "one slice per bit expected");
        assert_eq!(rx, bits);
    }

    #[test]
    fn locks_from_a_half_bit_phase_offset_and_stays_locked() {
        let bits = bit_pattern(500, 0xfeed_beef);
        let waveform = nrz(&bits);
        let rx = recover(&waveform[SAMPLES_PER_BIT / 2..]);
        assert!(
            (rx.len() as i64 - 500).abs() <= 2,
            "recovered {} bits from 500",
            rx.len()
        );
        assert!(bits_align(&bits, &rx, 64), "never locked to the bit clock");
    }

    #[test]
    fn free_runs_on_a_crossing_free_input() {
        let mut sync = BitSync::new(RATE, BAUD);
        let (mut count, mut since) = (0, 0);
        for _ in 0..400 {
            since += 1;
            if sync.push(1.0).is_some() {
                count += 1;
                since = 0;
            }
            assert_eq!(sync.samples_since_symbol(), since);
        }
        assert_eq!(count, 10, "constant input must keep the clock running");
    }

    #[test]
    fn set_baud_keeps_the_current_phase() {
        let mut sync = BitSync::new(RATE, BAUD);
        while sync.push(1.0).is_none() {}
        sync.set_baud(2.0 * BAUD);
        let mut gap = 0;
        while sync.push(1.0).is_none() {
            gap += 1;
        }
        assert_eq!(gap + 1, SAMPLES_PER_BIT / 2);
    }
}
