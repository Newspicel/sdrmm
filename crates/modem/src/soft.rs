use sdrmm_dsp::coding::fec::conv;

pub const LLR_SATURATION: f32 = 8.0;

const SOFTBIT_FULL_SCALE: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoftBit(pub f32);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Llr(pub f32);

impl SoftBit {
    #[must_use]
    pub fn bit(self) -> bool {
        is_one(self.0)
    }

    #[must_use]
    pub fn is_erasure(self) -> bool {
        is_erased(self.0)
    }

    #[must_use]
    pub fn to_fec(self) -> conv::Soft {
        to_fec(self.0, SOFTBIT_FULL_SCALE)
    }
}

impl Llr {
    #[must_use]
    pub fn bit(self) -> bool {
        is_one(self.0)
    }

    #[must_use]
    pub fn is_erasure(self) -> bool {
        is_erased(self.0)
    }

    #[must_use]
    pub fn to_fec(self) -> conv::Soft {
        to_fec(self.0, LLR_SATURATION)
    }
}

impl From<Llr> for SoftBit {
    fn from(llr: Llr) -> Self {
        Self(llr.0)
    }
}

#[must_use]
pub fn argmax(statistics: &[f32]) -> u8 {
    assert!(!statistics.is_empty(), "no statistics, no decision");
    let mut best = 0usize;
    for (k, &s) in statistics.iter().enumerate() {
        if s >= statistics[best] {
            best = k;
        }
    }
    best as u8
}

fn is_one(value: f32) -> bool {
    value > 0.0
}

fn is_erased(value: f32) -> bool {
    value == 0.0
}

/// Scales `value` so `full_scale` lands on the decoder's confident level, then saturates.
fn to_fec(value: f32, full_scale: f32) -> conv::Soft {
    let full = f32::from(conv::CONFIDENT);
    (value * full / full_scale).clamp(-full, full) as conv::Soft
}

#[cfg(test)]
mod tests {
    use sdrmm_dsp::coding::fec::conv::{CONFIDENT, ERASURE};

    use super::*;

    #[test]
    fn sign_convention_positive_is_one() {
        assert!(SoftBit(0.3).bit());
        assert!(!SoftBit(-0.3).bit());
        assert!(Llr(5.0).bit());
        assert!(!Llr(-5.0).bit());
    }

    #[test]
    fn zero_is_an_erasure_and_slices_to_zero() {
        assert!(SoftBit(0.0).is_erasure());
        assert!(Llr(0.0).is_erasure());
        assert!(!SoftBit(1e-9).is_erasure());
        assert_eq!(SoftBit(0.0).to_fec(), ERASURE);
        assert_eq!(Llr(0.0).to_fec(), ERASURE);
    }

    #[test]
    fn softbit_to_fec_scales_and_saturates() {
        assert_eq!(SoftBit(1.0).to_fec(), CONFIDENT);
        assert_eq!(SoftBit(-1.0).to_fec(), -CONFIDENT);
        assert_eq!(SoftBit(0.5).to_fec(), CONFIDENT / 2);
        assert_eq!(SoftBit(37.0).to_fec(), CONFIDENT);
        assert_eq!(SoftBit(-2.5).to_fec(), -CONFIDENT);
    }

    #[test]
    fn llr_to_fec_saturates_at_the_stated_point() {
        assert_eq!(Llr(LLR_SATURATION).to_fec(), CONFIDENT);
        assert_eq!(Llr(-LLR_SATURATION).to_fec(), -CONFIDENT);
        assert_eq!(Llr(100.0).to_fec(), CONFIDENT);
        assert_eq!(Llr(LLR_SATURATION / 2.0).to_fec(), CONFIDENT / 2);
        assert_eq!(Llr(0.125).to_fec(), 1);
        assert_eq!(Llr(0.06).to_fec(), 0);
    }

    #[test]
    fn llr_demotes_to_softbit_without_changing_the_number() {
        let soft: SoftBit = Llr(-2.75).into();
        assert_eq!(soft, SoftBit(-2.75));
    }
}
