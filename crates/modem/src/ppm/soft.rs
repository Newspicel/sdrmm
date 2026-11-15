use super::MAX_SLOT_BITS;
use crate::{
    constellation::demap::energy_llrs,
    soft::{Llr, SoftBit},
};

pub fn llrs(statistics: &[f32], noise_var: f64, out: &mut [Llr]) {
    energy_llrs(statistics, noise_var, out);
}

pub fn soft_bits(statistics: &[f32], out: &mut [SoftBit]) {
    let peak = statistics.iter().copied().fold(0.0f32, f32::max);
    let scale = if peak > 0.0 { f64::from(peak) } else { 1.0 };
    let mut llrs = [Llr(0.0); MAX_SLOT_BITS];
    let llrs = &mut llrs[..out.len()];
    energy_llrs(statistics, scale, llrs);
    for (slot, &llr) in out.iter_mut().zip(llrs.iter()) {
        *slot = SoftBit(llr.0.clamp(-1.0, 1.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_bits_agree_with_the_hard_decision_and_abstain_on_silence() {
        let stats = [0.1f32, 4.0, 0.2, 0.05];
        let mut soft = [SoftBit(0.0); 2];
        soft_bits(&stats, &mut soft);
        assert!(soft[0].0 > 0.0 && soft[1].0 < 0.0, "{soft:?}");
        assert!(soft.iter().all(|s| s.0.abs() <= 1.0), "{soft:?}");

        let mut soft = [SoftBit(0.0); 2];
        soft_bits(&[0.0; 4], &mut soft);
        assert!(soft.iter().all(|s| s.is_erasure()), "{soft:?}");
    }
    #[test]
    fn llr_magnitude_grows_with_slot_separation() {
        let mut weak = [Llr(0.0); 1];
        let mut strong = [Llr(0.0); 1];
        llrs(&[1.0, 1.2], 1.0, &mut weak);
        llrs(&[1.0, 9.0], 1.0, &mut strong);
        assert!(strong[0].0 > weak[0].0 && weak[0].0 > 0.0);
    }
}
