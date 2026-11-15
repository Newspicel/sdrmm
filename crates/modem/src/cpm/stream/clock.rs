use num_complex::Complex;
use sdrmm_dsp::farrow;

const FARROW_TAPS: usize = 4;

#[derive(Clone, Debug)]
pub(super) struct SymbolClock {
    recent: [Complex<f32>; FARROW_TAPS],
    newest: i64,
    next: f64,
    sps: f64,
    gain: f64,
}

impl SymbolClock {
    pub(super) fn new(sps: f64, gain: f64) -> Self {
        Self {
            recent: [Complex::new(0.0, 0.0); FARROW_TAPS],
            newest: -1,
            next: sps,
            sps,
            gain,
        }
    }

    pub(super) fn push(&mut self, y: Complex<f32>) -> Option<Complex<f32>> {
        self.recent.rotate_left(1);
        self.recent[FARROW_TAPS - 1] = y;
        self.newest += 1;
        let base = self.next.floor();
        if (base as i64) + 2 > self.newest {
            return None;
        }
        Some(farrow(&self.recent, (self.next - base) as f32))
    }

    pub(super) fn steer(&mut self, epoch: Option<f64>) {
        let error = epoch.map_or(0.0, |target| {
            let gap = (target - self.next).rem_euclid(self.sps);
            if gap > self.sps / 2.0 {
                gap - self.sps
            } else {
                gap
            }
        });
        self.next += self.sps + self.gain * error;
    }

    pub(super) fn reset(&mut self) {
        *self = Self::new(self.sps, self.gain);
    }
}
