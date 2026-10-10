use num_complex::Complex;
use sdrmm_dsp::{FirC, FmDemod, design_lowpass};
use sdrmm_wire::SstvModulation;

pub(crate) const TONE_LOW_HZ: f64 = 1_000.0;
pub(crate) const TONE_HIGH_HZ: f64 = 2_600.0;
pub(crate) const FM_HALF_BAND_HZ: f64 = 7_500.0;
const TAPS: usize = 255;

pub(crate) fn occupied_band(modulation: SstvModulation) -> (f64, f64) {
    match modulation {
        SstvModulation::Usb => (TONE_LOW_HZ, TONE_HIGH_HZ),
        SstvModulation::Lsb => (-TONE_HIGH_HZ, -TONE_LOW_HZ),
        SstvModulation::Fm => (-FM_HALF_BAND_HZ, FM_HALF_BAND_HZ),
        SstvModulation::Am => (-TONE_HIGH_HZ, TONE_HIGH_HZ),
    }
}

pub(crate) fn band_filter(low_hz: f64, high_hz: f64, rate: f64) -> FirC {
    let half = (high_hz - low_hz) / 2.0 / rate;
    let center = (high_hz + low_hz) / 2.0 / rate;
    FirC::from_lowpass(&design_lowpass(TAPS, half), center)
}

enum Demod {
    Usb,
    Lsb,
    Fm(FmDemod),
    Am,
}

pub(crate) struct Carrier {
    demod: Demod,
    tone: FirC,
    audio: Vec<f32>,
    real: Vec<Complex<f32>>,
    out: Vec<Complex<f32>>,
}

impl Carrier {
    pub(crate) fn new(modulation: SstvModulation, rate: f64) -> Self {
        let demod = match modulation {
            SstvModulation::Usb => Demod::Usb,
            SstvModulation::Lsb => Demod::Lsb,
            SstvModulation::Fm => Demod::Fm(FmDemod::new(rate, FM_HALF_BAND_HZ)),
            SstvModulation::Am => Demod::Am,
        };
        Self {
            demod,
            tone: band_filter(TONE_LOW_HZ, TONE_HIGH_HZ, rate),
            audio: Vec::new(),
            real: Vec::new(),
            out: Vec::new(),
        }
    }

    pub(crate) fn process<'a>(&'a mut self, iq: &'a [Complex<f32>]) -> &'a [Complex<f32>] {
        match &mut self.demod {
            Demod::Usb => return iq,
            Demod::Lsb => {
                self.out.clear();
                self.out.extend(iq.iter().map(Complex::conj));
                return &self.out;
            }
            Demod::Fm(fm) => fm.process(iq, &mut self.audio),
            Demod::Am => {
                self.audio.clear();
                self.audio.extend(iq.iter().map(|s| s.norm()));
            }
        }
        self.real.clear();
        self.real
            .extend(self.audio.iter().map(|&a| Complex::new(a, 0.0)));
        self.tone.process(&self.real, &mut self.out);
        &self.out
    }
}
