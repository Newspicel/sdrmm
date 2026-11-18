use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::ComplexOnePole;

const BAND_CUTOFF_HZ: f64 = 1_600.0;
const GUARD_OFFSET_HZ: f64 = 3_400.0;
const GUARD_CUTOFF_HZ: f64 = 500.0;
const STAGES: usize = 2;
const WINDOW_S: f64 = 0.25;
const SLEEP_S: f64 = 2.0;
const SETTLE_SAMPLES: usize = 64;
/// Band to guard power per unit of bandwidth below which a window counts as quiet. Noise alone
/// spreads from 1.0 to 1.3 over a window, RDS that still decodes stays above 1.2.
const PRESENT_RATIO: f64 = 1.18;
/// Quiet windows in a row that put a listening front end to sleep.
const QUIET_WINDOWS: u8 = 2;

/// Watches the RDS band so a station without RDS costs almost nothing. The band just above the
/// subcarrier carries no programme and serves as the noise reference. While the band holds only
/// noise the front end sleeps and wakes for one window now and then; any decoded block, or one
/// window with energy above the noise, keeps it awake.
pub(super) struct Presence {
    band: ComplexOnePole,
    guard: ComplexOnePole,
    turn: Complex<f32>,
    rotor: Complex<f32>,
    band_power: f64,
    guard_power: f64,
    seen: usize,
    window: usize,
    sleep: usize,
    quiet: u8,
    state: State,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Listening,
    Probing,
    Sleeping { slept: usize },
}

impl Presence {
    pub(super) fn new(mpx_rate: f64, baseband_rate: f64) -> Self {
        Self {
            band: ComplexOnePole::new(baseband_rate, BAND_CUTOFF_HZ, STAGES),
            guard: ComplexOnePole::new(baseband_rate, GUARD_CUTOFF_HZ, STAGES),
            turn: Complex::from_polar(1.0, (-TAU * GUARD_OFFSET_HZ / baseband_rate) as f32),
            rotor: Complex::new(1.0, 0.0),
            band_power: 0.0,
            guard_power: 0.0,
            seen: 0,
            window: (WINDOW_S * baseband_rate) as usize,
            sleep: (SLEEP_S * mpx_rate) as usize,
            quiet: 0,
            state: State::Listening,
        }
    }

    /// Whether the next `len` composite samples are to be decoded, and whether they wake the
    /// front end, whose filters then hold stale history. `needed` wakes it at once.
    pub(super) fn listen(&mut self, len: usize, needed: bool) -> (bool, bool) {
        let State::Sleeping { slept } = &mut self.state else {
            return (true, false);
        };
        *slept += len;
        if *slept < self.sleep && !needed {
            return (false, false);
        }
        self.state = State::Probing;
        self.restart();
        (true, true)
    }

    /// Takes the decimated subcarrier of a decoded block and whether a valid block arrived.
    pub(super) fn observe(&mut self, baseband: &[Complex<f32>], decoded: bool) {
        if decoded {
            self.state = State::Listening;
            self.quiet = 0;
            self.restart();
            return;
        }
        self.accumulate(baseband);
        if self.seen < self.window + SETTLE_SAMPLES {
            return;
        }
        if self.ratio() >= PRESENT_RATIO {
            self.state = State::Listening;
            self.quiet = 0;
        } else {
            self.quiet = self.quiet.saturating_add(1);
            if self.state == State::Probing || self.quiet >= QUIET_WINDOWS {
                self.state = State::Sleeping { slept: 0 };
                self.quiet = 0;
            }
        }
        self.restart();
    }

    #[cfg(test)]
    pub(super) fn sleeping(&self) -> bool {
        matches!(self.state, State::Sleeping { .. })
    }

    fn accumulate(&mut self, baseband: &[Complex<f32>]) {
        for &sample in baseband {
            let band = self.band.process(sample);
            let guard = self.guard.process(sample * self.rotor);
            self.rotor *= self.turn;
            self.seen += 1;
            if self.seen > SETTLE_SAMPLES {
                self.band_power += f64::from(band.norm_sqr());
                self.guard_power += f64::from(guard.norm_sqr());
            }
        }
        self.rotor /= self.rotor.norm();
    }

    /// Band to guard power per unit of bandwidth over the current window.
    fn ratio(&self) -> f64 {
        let guard = self.guard_power * BAND_CUTOFF_HZ / GUARD_CUTOFF_HZ;
        self.band_power / guard.max(f64::MIN_POSITIVE)
    }

    fn restart(&mut self) {
        self.band.reset();
        self.guard.reset();
        self.band_power = 0.0;
        self.guard_power = 0.0;
        self.seen = 0;
    }
}
