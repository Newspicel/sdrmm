#[derive(Default)]
pub struct Clock {
    now: f64,
    origin: Option<(i64, f64)>,
    delay_ticks: f64,
}

impl Clock {
    pub fn with_delay(delay: std::time::Duration) -> Self {
        Self {
            delay_ticks: delay.as_secs_f64() * 90_000.0,
            ..Self::default()
        }
    }

    pub fn reset(&mut self) {
        self.now = 0.0;
        self.origin = None;
    }

    pub fn advance(&mut self, samples: usize, rate: f64) {
        self.now += samples as f64 * 90000.0 / rate;
    }

    pub fn due(&mut self, pts: i64) -> bool {
        self.due_with_lead(pts, 0.0)
    }

    pub fn due_with_lead(&mut self, pts: i64, lead_ms: f64) -> bool {
        let (first, started) = *self
            .origin
            .get_or_insert((pts, self.now + self.delay_ticks));
        let delta = (pts - first + (1i64 << 32)).rem_euclid(1i64 << 33) - (1i64 << 32);
        if (started + delta as f64 - self.now).abs() > 900_000.0 {
            self.origin = Some((pts, self.now + self.delay_ticks));
            return self.delay_ticks == 0.0;
        }
        self.now + lead_ms * 90.0 + 1.0 >= started + delta as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_and_reordered_video_share_a_wrapping_presentation_clock() {
        let mut clock = Clock::default();
        let first = (1i64 << 33) - 1800;
        assert!(clock.due(first));
        assert!(!clock.due(1800));
        clock.advance(960, 48000.0);
        assert!(clock.due(0));
        assert!(!clock.due(1800));
        clock.advance(960, 48000.0);
        assert!(clock.due(1800));
        assert!(clock.due(900));
    }

    #[test]
    fn buffered_clock_staggers_frames_received_in_the_same_burst() {
        let mut clock = Clock::with_delay(std::time::Duration::from_millis(360));
        assert!(!clock.due(90_000));
        assert!(!clock.due(91_800));
        clock.advance(17_280, 48_000.0);
        assert!(clock.due(90_000));
        assert!(!clock.due(91_800));
        clock.advance(960, 48_000.0);
        assert!(clock.due(91_800));

        clock.reset();
        assert!(!clock.due(200_000));
    }

    #[test]
    fn a_large_timestamp_discontinuity_reanchors_playback() {
        let mut clock = Clock::default();
        assert!(clock.due(90000));
        assert!(clock.due(90_000_000));
        assert!(!clock.due(90_003_600));
    }
}
