mod demod;
mod grid;
mod modulator;
mod soft;
mod waveform;

pub use demod::{PpmDemod, SlotDetector, magnitudes};
pub use grid::SlotGrid;
pub use modulator::PpmMod;
pub use soft::{llrs, soft_bits};
pub use waveform::{OVERSAMPLE, SlotWaveform, slot_taps};

pub const MAX_SLOTS: usize = 256;

pub const MAX_SLOT_BITS: usize = MAX_SLOTS.trailing_zeros() as usize;

fn assert_slot_timing(samples_per_slot: f64, phase: f64) {
    assert!(
        samples_per_slot.is_finite() && samples_per_slot > 0.0,
        "samples per slot must be positive, got {samples_per_slot}"
    );
    assert!(
        phase.is_finite() && (0.0..1.0).contains(&phase),
        "phase is a sub-sample offset in [0, 1), got {phase}"
    );
}
