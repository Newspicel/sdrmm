use sdrmm_wire::PerReport;

use super::frame::Checked;

pub(crate) const MAX_GAP: u32 = 1_024;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Count {
    received: u64,
    errored: u64,
    sequenced: u64,
    bits: u64,
    bit_errors: u64,
    last: Option<u32>,
    restarts: u32,
}

impl Count {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn frame(&mut self, checked: Checked) {
        if checked.sequence.is_none() && checked.bit_errors * 4 > checked.bits {
            return;
        }
        self.bits += u64::from(checked.bits);
        self.bit_errors += u64::from(checked.bit_errors);
        match checked.sequence {
            Some(sequence) => self.received(sequence),
            None => self.errored += 1,
        }
    }

    fn received(&mut self, sequence: u32) {
        let step = match self.last {
            None => 1,
            Some(last) if sequence == last => return,
            Some(last) if sequence > last && sequence - last <= MAX_GAP => sequence - last,
            Some(_) => {
                self.restarts += 1;
                if sequence < MAX_GAP { sequence + 1 } else { 1 }
            }
        };
        self.sequenced += u64::from(step);
        self.received += 1;
        self.last = Some(sequence);
    }

    #[must_use]
    pub fn errored(&self) -> u64 {
        self.errored
    }

    #[must_use]
    pub fn frames(&self) -> u64 {
        self.received + self.errored
    }

    #[must_use]
    pub fn report(&self) -> PerReport {
        let expected = self.sequenced.max(self.frames());
        let missed = expected - self.received;
        PerReport {
            received: self.received,
            errored: self.errored,
            missed,
            expected,
            per: (missed as f64 / expected.max(1) as f64) as f32,
            bits: self.bits,
            bit_errors: self.bit_errors,
            ber: (self.bits > 0).then(|| (self.bit_errors as f64 / self.bits as f64) as f32),
            last_sequence: self.last,
            restarts: self.restarts,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good(sequence: u32) -> Checked {
        Checked {
            sequence: Some(sequence),
            bit_errors: 0,
            bits: 256,
        }
    }

    fn bad(bit_errors: u32) -> Checked {
        Checked {
            sequence: None,
            bit_errors,
            bits: 256,
        }
    }

    #[test]
    fn gaps_count_as_missed_packets() {
        let mut count = Count::default();
        for sequence in [10, 11, 12, 15, 16] {
            count.frame(good(sequence));
        }
        count.frame(bad(5));
        count.frame(bad(200));
        let report = count.report();
        assert_eq!(report.received, 5);
        assert_eq!(report.expected, 7);
        assert_eq!(report.missed, 2);
        assert_eq!(report.errored, 1);
        assert!((report.per - 2.0 / 7.0).abs() < 1e-6);
        assert_eq!(report.bits, 6 * 256);
        assert_eq!(report.bit_errors, 5);
        assert_eq!(report.last_sequence, Some(16));
    }

    #[test]
    fn a_restarted_transmitter_counts_from_zero() {
        let mut count = Count::default();
        for sequence in [98, 99, 2, 3] {
            count.frame(good(sequence));
        }
        let report = count.report();
        assert_eq!(report.restarts, 1);
        assert_eq!(report.missed, 2);
        assert_eq!(report.expected, 2 + 3 + 1);
    }

    #[test]
    fn a_wild_jump_restarts_without_inventing_losses() {
        let mut count = Count::default();
        count.frame(good(5));
        count.frame(good(5 + MAX_GAP + 1_000_000));
        let report = count.report();
        assert_eq!(report.missed, 0);
        assert_eq!(report.restarts, 1);
        assert_eq!(report.received, 2);
        assert_eq!(report.last_sequence, Some(5 + MAX_GAP + 1_000_000));
    }

    #[test]
    fn errored_frames_alone_are_all_missed() {
        let mut count = Count::default();
        count.frame(bad(3));
        count.frame(bad(4));
        let report = count.report();
        assert_eq!(report.expected, 2);
        assert_eq!(report.missed, 2);
        assert_eq!(report.per, 1.0);
    }

    #[test]
    fn a_repeated_frame_is_not_counted_twice() {
        let mut count = Count::default();
        count.frame(good(1));
        count.frame(good(1));
        assert_eq!(count.report().received, 1);
        assert_eq!(count.report().ber, Some(0.0));
    }
}
