pub mod cck;
pub mod chip;
pub mod css;
pub mod dsss;
pub mod fhss;
pub mod pn;

use sdrmm_dsp::Decibels;

pub use cck::{CckDemod, CckMod, CckMode, CckParams, Codebook};
pub use chip::{ChipShaper, ChipTiming, find_burst, integrate, integrate_chips, sample_at};
pub use css::{CssDemod, CssMod, CssParams, MAX_SPREADING_FACTOR, MIN_SPREADING_FACTOR};
pub use dsss::{Acquisition, DsssDemod, DsssMod, DsssParams, MAX_CHIPS};
pub use fhss::{FhssDemod, FhssMod, HopSequence, HopSequencer};
pub use pn::{MAX_LFSR_DEGREE, PnError, PnSequence};

/// How much a preamble of `preamble` symbols costs a payload of `payload`, in dB.
fn framing_overhead_db(preamble: usize, payload: usize) -> f64 {
    ((preamble + payload) as f64 / payload as f64).power_to_db()
}
