mod iq;
mod wave;

use num_complex::Complex;

pub use iq::{add_noise, resample, scale, shift, silence};
pub use wave::{fm_modulate, fsk, ook, tone_audio};

pub struct Signal {
    pub id: &'static str,
    pub label: &'static str,
    pub rate_hz: f64,
    pub render: fn() -> Vec<Complex<f32>>,
}

pub struct Catalog {
    pub signals: &'static [Signal],
    pub default_id: &'static str,
    pub center_hz: f64,
}

pub static EMPTY_CATALOG: Catalog = Catalog {
    signals: &[],
    default_id: "",
    center_hz: 0.0,
};

impl Catalog {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.signals.is_empty()
    }

    #[must_use]
    pub fn find(&self, id: &str) -> Option<&'static Signal> {
        self.signals.iter().find(|signal| signal.id == id)
    }

    #[must_use]
    pub fn default_signal(&self) -> Option<&'static Signal> {
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
