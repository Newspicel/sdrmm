use crate::runtime::SpectrumSnapshot;

/// Tells which spectrum frames were taken wholly at the tuning they carry.
///
/// A frame is stamped with the centre the radio was set to when it was transformed, but a
/// retune leaves up to `in_flight` samples from the old tuning in the radio's buffers. Frame
/// timestamps count samples, so a frame is clean once all of its samples come after those.
#[derive(Debug)]
pub(super) struct Settle {
    in_flight: u64,
    center_hz: Option<f64>,
    clean_from: u64,
    last: Option<u64>,
    advance: u64,
    moving: bool,
}

impl Settle {
    pub(super) const fn new(in_flight: u64) -> Self {
        Self {
            in_flight,
            center_hz: None,
            clean_from: 0,
            last: None,
            advance: 0,
            moving: false,
        }
    }

    pub(super) const fn set_in_flight(&mut self, in_flight: u64) {
        self.in_flight = in_flight;
    }

    /// The radio was asked to move: frames still stamped where it was no longer count.
    pub(super) const fn moved(&mut self) {
        self.moving = true;
    }

    pub(super) fn clean(&mut self, frame: &SpectrumSnapshot) -> bool {
        let at = frame.timestamp;
        let same = self.center_hz == Some(frame.center_hz);
        match self.last {
            Some(last) if at < last => self.center_hz = None,
            Some(last) if same => self.advance = at - last,
            _ => {}
        }
        self.last = Some(at);
        if self.center_hz != Some(frame.center_hz) {
            self.center_hz = Some(frame.center_hz);
            self.moving = false;
            self.clean_from = at.saturating_sub(self.advance) + self.in_flight;
        } else if self.moving {
            return false;
        }
        at >= self.clean_from + self.advance + frame.db.len() as u64
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    const SIZE: usize = 4_096;
    const ADVANCE: u64 = 24_000;

    fn frame(center_hz: f64, timestamp: u64) -> SpectrumSnapshot {
        SpectrumSnapshot {
            seq: 0,
            timestamp,
            center_hz,
            span_hz: 2_400_000.0,
            db: Arc::from(vec![-100.0f32; SIZE]),
        }
    }

    fn first_clean(settle: &mut Settle, center_hz: f64, from: u64) -> u64 {
        (0..1_000)
            .map(|i| from + i * ADVANCE)
            .find(|&at| settle.clean(&frame(center_hz, at)))
            .expect("a clean frame")
    }

    #[test]
    fn frames_wait_out_what_the_radio_still_holds_from_its_old_tuning() {
        let in_flight = 393_216;
        let mut settle = Settle::new(in_flight);
        let settled = first_clean(&mut settle, 100e6, 0);
        settle.moved();
        assert!(
            !settle.clean(&frame(100e6, settled + ADVANCE)),
            "the old centre"
        );
        let moved_at = settled + 2 * ADVANCE;
        let clean = first_clean(&mut settle, 102e6, moved_at);
        assert!(
            clean >= moved_at - ADVANCE + in_flight + SIZE as u64,
            "{clean}"
        );
        assert!(
            clean < moved_at + in_flight + 2 * ADVANCE,
            "waited too long: {clean}"
        );
    }

    #[test]
    fn a_radio_with_nothing_in_flight_is_read_on_the_next_frame() {
        let mut settle = Settle::new(0);
        let settled = first_clean(&mut settle, 100e6, 0);
        settle.moved();
        let moved_at = settled + ADVANCE;
        assert!(!settle.clean(&frame(102e6, moved_at)));
        assert!(settle.clean(&frame(102e6, moved_at + ADVANCE)));
    }

    #[test]
    fn a_decoder_moving_inside_the_window_costs_no_wait() {
        let mut settle = Settle::new(393_216);
        let settled = first_clean(&mut settle, 100e6, 0);
        assert!(settle.clean(&frame(100e6, settled + ADVANCE)));
    }

    #[test]
    fn a_restarted_stream_is_waited_out_like_a_retune() {
        let mut settle = Settle::new(100_000);
        let settled = first_clean(&mut settle, 100e6, 0);
        assert!(settle.clean(&frame(100e6, settled + ADVANCE)));
        assert!(
            !settle.clean(&frame(100e6, 10)),
            "samples counted from zero again"
        );
        assert!(!settle.clean(&frame(100e6, 10 + ADVANCE)));
    }
}
