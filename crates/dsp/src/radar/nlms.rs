mod block;
mod delay_line;
mod surveillance;

use super::RadarDspError;
pub use super::ratio::HEALTH_WINDOW_S;
pub use block::BlockNlms;
pub use surveillance::SurveillanceNlms;

pub const MAX_CANCELLER_TAPS: usize = 4096;

const REGRESSOR_FLOOR: f32 = 1e-6;

fn check_canceller(taps: usize, lead: usize, step: f32) -> Result<usize, RadarDspError> {
    let order = taps + lead;
    if order == 0 || order > MAX_CANCELLER_TAPS || !(step > 0.0 && step <= 1.0) {
        return Err(RadarDspError::Setting);
    }
    Ok(order)
}

#[cfg(test)]
mod tests {
    use num_complex::Complex;

    use super::*;
    use crate::math::db::Decibels;
    use crate::radar::fixtures::{channel, direct_paths, fm_reference};

    type C32 = Complex<f32>;

    const FS: f64 = 266_666.67;

    fn direct_path_recording() -> (Vec<C32>, Vec<C32>) {
        let reference = fm_reference((1.25 * FS) as usize, 3);
        let surveillance = channel(&reference, &direct_paths(), 0.0, FS, 3e-3, 5);
        (reference, surveillance)
    }

    #[test]
    fn nlms_converges_on_a_static_direct_path() {
        let (reference, surveillance) = direct_path_recording();
        let mut nlms = SurveillanceNlms::new(14, 2, 0.05, FS).unwrap();
        let mut out = vec![C32::default(); reference.len()];
        assert_eq!(
            nlms.process(&reference, &surveillance, &mut out),
            reference.len()
        );
        let supp = nlms.suppression_db();
        assert!(supp >= 30.0, "{supp}");
        assert_eq!(nlms.latency(), 2);
        assert_eq!(nlms.resets(), 0);
    }

    #[test]
    fn nlms_output_is_the_surveillance_delayed_by_the_lead() {
        let reference = vec![C32::default(); 64];
        let surveillance: Vec<C32> = (0..64).map(|n| C32::new(n as f32, 0.0)).collect();
        let mut nlms = SurveillanceNlms::new(4, 3, 0.05, FS).unwrap();
        let mut out = vec![C32::default(); 64];
        nlms.process(&reference, &surveillance, &mut out);
        for n in 3..64 {
            assert_eq!(out[n], surveillance[n - 3]);
        }
    }

    #[test]
    fn block_nlms_reaches_nlms_within_3_db() {
        let (reference, surveillance) = direct_path_recording();
        let mut nlms = SurveillanceNlms::new(14, 2, 0.05, FS).unwrap();
        let mut block = BlockNlms::new(14, 2, 0.05, FS).unwrap();
        let mut out = vec![C32::default(); reference.len()];
        nlms.process(&reference, &surveillance, &mut out);
        for (start, end) in [(0, 1000), (1000, 1001), (1001, reference.len())] {
            block.process(
                &reference[start..end],
                &surveillance[start..end],
                &mut out[start..end],
            );
        }
        let (single, batched) = (nlms.suppression_db(), block.suppression_db());
        assert!(batched >= single - 3.0, "{batched} vs {single}");
        assert_eq!(block.latency(), 2 + 16);
        let tail = &out[out.len() - 4096..];
        let residual: f32 = tail.iter().map(C32::norm_sqr).sum::<f32>() / 4096.0;
        let input: f32 =
            surveillance.iter().map(C32::norm_sqr).sum::<f32>() / surveillance.len() as f32;
        assert!((input / residual).power_to_db() > 25.0);
    }

    #[test]
    fn invalid_cancellers_are_refused() {
        assert!(SurveillanceNlms::new(0, 0, 0.05, FS).is_err());
        assert!(SurveillanceNlms::new(4, 0, 0.0, FS).is_err());
        assert!(BlockNlms::new(4, 0, f32::NAN, FS).is_err());
        assert!(BlockNlms::new(4, 0, 0.1, 0.0).is_err());
    }

    #[test]
    fn a_non_finite_input_resets_and_counts() {
        let (reference, mut surveillance) = direct_path_recording();
        surveillance[5000] = C32::new(f32::NAN, 0.0);
        let mut nlms = SurveillanceNlms::new(14, 2, 0.05, FS).unwrap();
        let mut block = BlockNlms::new(14, 2, 0.05, FS).unwrap();
        let mut out = vec![C32::default(); reference.len()];
        nlms.process(&reference, &surveillance, &mut out);
        assert!(nlms.resets() >= 1);
        assert!(out.iter().all(|value| value.is_finite()));
        block.process(&reference, &surveillance, &mut out);
        assert!(block.resets() >= 1);
        assert!(out[20_000..].iter().all(|value| value.is_finite()));
        assert!(block.suppression_db() > 25.0);
    }
}
