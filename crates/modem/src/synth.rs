mod basic;
mod c4fm;
mod iq;
pub mod morse;
mod speech;
pub mod ts;
mod wave;

use std::borrow::Cow;

use num_complex::Complex;

pub use basic::BASIC;
pub use c4fm::{bits, c4fm, c4fm_keyed, dibits, filler};
pub use iq::{add_noise, resample, scale, shift, shifted, silence, spaced};
pub use speech::speech_audio;
pub use wave::{fm_modulate, fsk, ook, tone_audio};

/// The rate most narrowband signals are synthesised at.
pub const NARROW_HZ: f64 = 240_000.0;
/// The rate audio-band signals are synthesised at.
pub const AUDIO_HZ: f64 = 48_000.0;
pub const WIDE_FM_HZ: f64 = 960_000.0;
pub const WIDEBAND_HZ: f64 = 2_000_000.0;

/// One pass of a synthetic signal, or why it could not be made.
pub type Rendered = Result<Vec<Complex<f32>>, SynthError>;

/// How a signal makes one pass of itself.
#[derive(Clone, Copy, Debug)]
pub enum Render {
    Always(fn() -> Vec<Complex<f32>>),
    /// A signal built from blocks that can refuse what they are asked for.
    Fallible(fn() -> Rendered),
}

/// A named signal a generator can play at `rate_hz`.
#[derive(Clone, Copy, Debug)]
pub struct Signal {
    pub id: &'static str,
    pub label: &'static str,
    pub rate_hz: f64,
    pub render: Render,
}

impl Signal {
    /// One pass of the signal at `rate_hz`.
    pub fn rendered(&self) -> Rendered {
        match self.render {
            Render::Always(render) => Ok(render()),
            Render::Fallible(render) => render(),
        }
    }
}

/// The signals a generator offers, and the one it starts on.
#[derive(Clone, Debug)]
pub struct Catalog {
    pub signals: Cow<'static, [Signal]>,
    pub default_id: &'static str,
}

pub static EMPTY_CATALOG: Catalog = Catalog {
    signals: Cow::Borrowed(&[]),
    default_id: "",
};

impl Catalog {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.signals.is_empty()
    }

    #[must_use]
    pub fn find(&self, id: &str) -> Option<&Signal> {
        self.signals.iter().find(|signal| signal.id == id)
    }

    #[must_use]
    pub fn default_signal(&self) -> Option<&Signal> {
        self.find(self.default_id).or_else(|| self.signals.first())
    }

    #[must_use]
    pub fn rates(&self) -> Vec<f64> {
        let mut rates: Vec<f64> = self.signals.iter().map(|signal| signal.rate_hz).collect();
        rates.sort_by(f64::total_cmp);
        rates.dedup();
        rates
    }
}

/// A capture a fixture generator writes: `iq` renders it at `rate`, `note` says what a decoder
/// must read from it.
pub struct Recipe {
    pub stem: &'static str,
    pub rate: f64,
    pub note: &'static str,
    pub iq: fn() -> Rendered,
}

/// The samples in `secs` seconds at `rate`.
#[must_use]
pub fn seconds(rate: f64, secs: f64) -> usize {
    (rate * secs) as usize
}

/// Why a synthetic signal could not be built from what it was asked for.
#[derive(Debug)]
pub struct SynthError(String);

impl std::fmt::Display for SynthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SynthError {}

/// Turns a missing or failed building block into a [`SynthError`] naming it.
pub trait Need<T> {
    fn need(self, what: &str) -> Result<T, SynthError>;
}

impl<T> Need<T> for Option<T> {
    fn need(self, what: &str) -> Result<T, SynthError> {
        self.ok_or_else(|| SynthError(format!("no {what}")))
    }
}

impl<T, E: std::fmt::Display> Need<T> for Result<T, E> {
    fn need(self, what: &str) -> Result<T, SynthError> {
        self.map_err(|error| SynthError(format!("{what}: {error}")))
    }
}
