use std::ops::RangeInclusive;

use num_complex::Complex;
use sdrmm_dsp::FmDemod;

use super::{AFSK_BAUD, DEVIATION_HZ, G3RUH_BAUD, afsk::Afsk, g3ruh::G3ruh};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PacketMode {
    #[default]
    Afsk1200,
    G3ruh9600,
}

impl PacketMode {
    #[must_use]
    pub fn baud(self) -> f64 {
        match self {
            Self::Afsk1200 => AFSK_BAUD,
            Self::G3ruh9600 => G3RUH_BAUD,
        }
    }
}

enum Slicer {
    Afsk(Box<Afsk>),
    G3ruh(Box<G3ruh>),
}

/// FM IQ in, HDLC frames out with their FCS still attached and unchecked. AFSK runs several
/// lanes at once, so one transmission can come out more than once; [`super::RecentFrames`]
/// drops the repeats. A frame is lent to `heard` and gone after it returns.
pub struct PacketReceiver {
    mode: PacketMode,
    demod: FmDemod,
    slicer: Slicer,
    discriminated: Vec<f32>,
}

impl PacketReceiver {
    /// `bytes` bounds a frame's length, FCS included.
    #[must_use]
    pub fn new(mode: PacketMode, rate: f64, bytes: &RangeInclusive<usize>) -> Self {
        Self {
            mode,
            demod: FmDemod::new(rate, DEVIATION_HZ),
            slicer: match mode {
                PacketMode::Afsk1200 => Slicer::Afsk(Box::new(Afsk::new(rate, bytes))),
                PacketMode::G3ruh9600 => Slicer::G3ruh(Box::new(G3ruh::new(rate, bytes))),
            },
            discriminated: Vec::new(),
        }
    }

    #[must_use]
    pub fn mode(&self) -> PacketMode {
        self.mode
    }

    pub fn process(&mut self, iq: &[Complex<f32>], mut heard: impl FnMut(&[u8])) {
        self.slice(iq, None, &mut heard);
    }

    /// Like [`Self::process`], and appends the soft level of each bit the slicer decided to
    /// `levels`, mark positive: the flat-twist lane for AFSK, the line itself for G3RUH.
    pub fn process_watched(
        &mut self,
        iq: &[Complex<f32>],
        levels: &mut Vec<f32>,
        mut heard: impl FnMut(&[u8]),
    ) {
        self.slice(iq, Some(levels), &mut heard);
    }

    fn slice(
        &mut self,
        iq: &[Complex<f32>],
        levels: Option<&mut Vec<f32>>,
        heard: &mut impl FnMut(&[u8]),
    ) {
        self.demod.process(iq, &mut self.discriminated);
        match &mut self.slicer {
            Slicer::Afsk(afsk) => afsk.frames(&mut self.discriminated, levels, heard),
            Slicer::G3ruh(g3ruh) => g3ruh.frames(&self.discriminated, levels, heard),
        }
    }
}
