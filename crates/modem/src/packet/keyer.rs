use std::f64::consts::TAU;

use sdrmm_dsp::Scrambler;

use super::{AFSK_MARK_HZ, AFSK_SPACE_HZ, PacketMode};

/// The line code of a packet bit stream: NRZI, then the G3RUH scrambler for 9600 baud.
pub struct Line {
    level: bool,
    scrambler: Option<Scrambler>,
}

impl Line {
    #[must_use]
    pub fn new(mode: PacketMode) -> Self {
        Self {
            level: false,
            scrambler: match mode {
                PacketMode::Afsk1200 => None,
                PacketMode::G3ruh9600 => Some(Scrambler::g3ruh()),
            },
        }
    }

    pub fn push(&mut self, bit: bool) -> bool {
        if !bit {
            self.level = !self.level;
        }
        match self.scrambler.as_mut() {
            Some(scrambler) => scrambler.push(self.level),
            None => self.level,
        }
    }
}

/// The FM modulating audio of a line level: a 1200 or 2200 Hz tone for AFSK, the level itself
/// for G3RUH.
pub enum Keyer {
    Afsk { tone_phase: f64 },
    G3ruh,
}

impl Keyer {
    #[must_use]
    pub const fn new(mode: PacketMode) -> Self {
        match mode {
            PacketMode::Afsk1200 => Self::Afsk { tone_phase: 0.0 },
            PacketMode::G3ruh9600 => Self::G3ruh,
        }
    }

    pub fn next(&mut self, level: bool, rate: f64) -> f32 {
        match self {
            Self::Afsk { tone_phase } => {
                let freq = if level { AFSK_MARK_HZ } else { AFSK_SPACE_HZ };
                *tone_phase += TAU * freq / rate;
                if *tone_phase > TAU {
                    *tone_phase -= TAU;
                }
                tone_phase.sin() as f32
            }
            Self::G3ruh => {
                if level {
                    1.0
                } else {
                    -1.0
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nrzi_holds_on_one_and_toggles_on_zero() {
        let mut line = Line::new(PacketMode::Afsk1200);
        let levels: Vec<bool> = [true, false, false, true].map(|bit| line.push(bit)).into();
        assert_eq!(levels, vec![false, true, false, false]);
    }

    #[test]
    fn g3ruh_keys_the_level_and_afsk_a_unit_tone() {
        let mut keyer = Keyer::new(PacketMode::G3ruh9600);
        assert_eq!(
            (keyer.next(true, 48e3), keyer.next(false, 48e3)),
            (1.0, -1.0)
        );
        let mut keyer = Keyer::new(PacketMode::Afsk1200);
        let peak = (0..480)
            .map(|_| keyer.next(true, 48e3).abs())
            .fold(0.0f32, f32::max);
        assert!((peak - 1.0).abs() < 1e-3);
    }
}
