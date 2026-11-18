//! DAB transmission frames carrying random bits, for tests that need a DAB-shaped signal but
//! not its content.

use num_complex::Complex;
use sdrmm_modem::ofdm::dab::{FrameModulator, TransmissionMode};

use crate::ber::rng::Rng;

const SEED: u64 = 0x0DAB;

/// `frames` Mode I frames of random differential QPSK.
#[must_use]
pub fn random_frames(frames: usize) -> Vec<Complex<f32>> {
    let mut modulator = FrameModulator::new(TransmissionMode::I);
    let mode = modulator.mode();
    let mut rng = Rng::new(SEED);
    let mut iq = Vec::with_capacity(frames * mode.frame());
    for _ in 0..frames {
        let symbols: Vec<Vec<bool>> = (1..mode.symbols)
            .map(|_| {
                (0..mode.symbol_bits())
                    .map(|_| rng.next_u64() & 1 == 1)
                    .collect()
            })
            .collect();
        modulator.frame(&symbols, &mut iq);
    }
    iq
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdrmm_modem::ofdm::dab::Mode;

    #[test]
    fn frames_have_the_mode_i_length() {
        assert_eq!(
            random_frames(2).len(),
            2 * Mode::new(TransmissionMode::I).frame()
        );
    }
}
