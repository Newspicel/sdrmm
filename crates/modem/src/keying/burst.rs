use std::f64::consts::PI;

const RAMP_S: f64 = 0.002;

/// The envelope that keys a carrier up and down over a raised cosine, so a burst starts and
/// ends without a click across the band.
pub struct Burst {
    ramp_len: usize,
    state: State,
}

enum State {
    Idle,
    Rising(usize),
    Running,
    Falling(usize),
}

impl Burst {
    #[must_use]
    pub fn new(rate: f64) -> Self {
        Self {
            ramp_len: (RAMP_S * rate) as usize,
            state: State::Idle,
        }
    }

    #[must_use]
    pub const fn ramp_len(&self) -> usize {
        self.ramp_len
    }

    /// The gain of the next sample while there is `more` to send, and `None` once the carrier
    /// is down.
    pub fn next(&mut self, more: bool) -> Option<f32> {
        match self.state {
            State::Idle if !more => None,
            State::Idle => {
                self.state = State::Rising(1);
                Some(self.ramp(0))
            }
            State::Running if !more => {
                self.state = State::Falling(1);
                Some(self.ramp(self.ramp_len - 1))
            }
            State::Running => Some(1.0),
            State::Rising(k) => {
                self.state = if k + 1 < self.ramp_len {
                    State::Rising(k + 1)
                } else {
                    State::Running
                };
                Some(self.ramp(k))
            }
            State::Falling(k) => {
                self.state = if k + 1 < self.ramp_len {
                    State::Falling(k + 1)
                } else {
                    State::Idle
                };
                Some(self.ramp(self.ramp_len - 1 - k))
            }
        }
    }

    fn ramp(&self, k: usize) -> f32 {
        (0.5 * (1.0 - (PI * (k + 1) as f64 / self.ramp_len as f64).cos())) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 48_000.0;

    #[test]
    fn an_empty_payload_never_keys_the_carrier() {
        let mut burst = Burst::new(RATE);
        assert_eq!(burst.next(false), None);
        assert_eq!(burst.next(false), None);
    }

    #[test]
    fn the_envelope_rises_holds_and_falls_to_silence() {
        let mut burst = Burst::new(RATE);
        let ramp = burst.ramp_len();
        let rise: Vec<f32> = (0..ramp).map(|_| burst.next(true).unwrap()).collect();
        assert!(rise[0] < 0.01, "first sample {}", rise[0]);
        assert!(rise.windows(2).all(|p| p[1] > p[0]), "rise not monotonic");
        assert!(
            (rise[ramp - 1] - 1.0).abs() < 1e-6,
            "peak {}",
            rise[ramp - 1]
        );
        assert_eq!(burst.next(true), Some(1.0));

        let fall: Vec<f32> = std::iter::from_fn(|| burst.next(false)).collect();
        assert_eq!(fall.len(), ramp);
        assert!(fall.windows(2).all(|p| p[1] < p[0]), "fall not monotonic");
        assert!(fall[ramp - 1] < 1e-3, "last sample {}", fall[ramp - 1]);
        assert_eq!(burst.next(false), None);
    }
}
