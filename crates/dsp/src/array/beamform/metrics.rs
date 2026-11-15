use super::{BeamError, Constraints, MAX_CONSTRAINTS, WeightSet, ZERO};
use crate::math::db::Decibels;
use crate::math::linalg::{CMat, MAX_ORDER};

const SNR_FLOOR: f32 = 1e-6;
const NULL_FLOOR: f32 = 1e-6;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BeamMetrics {
    pub output_power: f32,
    pub snr_db: Option<f32>,
    pub gain_db: Option<f32>,
    pub cancelled_db: Option<f32>,
    pub null_depth_db: [f32; MAX_CONSTRAINTS],
    pub nulls: usize,
}

pub fn beam_metrics(
    r: &CMat,
    weights: &WeightSet,
    noise: Option<&[f32]>,
    primary: Option<usize>,
    nulls: &Constraints,
) -> Result<BeamMetrics, BeamError> {
    let n = weights.len();
    if r.order() != n {
        return Err(BeamError::Lanes(r.order()));
    }
    if nulls.count() > 0 && nulls.order() != n {
        return Err(BeamError::Lanes(nulls.order()));
    }
    let mut textbook = [ZERO; MAX_ORDER];
    weights.textbook(&mut textbook[..n]);
    let output = r.quad(&textbook[..n]).max(0.0);
    let mut metrics = BeamMetrics {
        output_power: output,
        ..BeamMetrics::default()
    };
    if let Some(noise) = noise {
        if noise.len() != n {
            return Err(BeamError::Lanes(noise.len()));
        }
        (metrics.snr_db, metrics.gain_db) = snr_and_gain(r, weights, noise, output);
    }
    if let Some(primary) = primary {
        if primary >= n {
            return Err(BeamError::Lanes(primary + 1));
        }
        let lane = r.get(primary, primary).re;
        metrics.cancelled_db = (output > 0.0 && lane > 0.0).then(|| (lane / output).power_to_db());
    }
    fill_null_depths(weights, nulls, &mut metrics);
    Ok(metrics)
}

fn snr_and_gain(
    r: &CMat,
    weights: &WeightSet,
    noise: &[f32],
    output: f32,
) -> (Option<f32>, Option<f32>) {
    let floor: f32 = weights
        .as_slice()
        .iter()
        .zip(noise)
        .map(|(w, sigma)| w.norm_sqr() * sigma)
        .sum();
    if !(floor.is_finite() && floor > 0.0) {
        return (None, None);
    }
    let snr = ((output - floor) / floor).max(SNR_FLOOR).power_to_db();
    let best = noise
        .iter()
        .enumerate()
        .filter(|(_, sigma)| sigma.is_finite() && **sigma > 0.0)
        .map(|(lane, sigma)| ((r.get(lane, lane).re - sigma) / sigma).max(SNR_FLOOR))
        .fold(None, |best: Option<f32>, value| {
            Some(best.map_or(value, |best| best.max(value)))
        });
    (Some(snr), best.map(|best| snr - best.power_to_db()))
}

fn fill_null_depths(weights: &WeightSet, nulls: &Constraints, metrics: &mut BeamMetrics) {
    let count = nulls.count();
    if count < 2 {
        return;
    }
    let main = weights.response(nulls.vector(0)).norm();
    metrics.nulls = count - 1;
    for (slot, index) in metrics.null_depth_db.iter_mut().zip(1..count) {
        let depth = weights.response(nulls.vector(index)).norm();
        let ratio = if main > 0.0 { depth / main } else { 1.0 };
        *slot = ratio.max(NULL_FLOOR).amplitude_to_db();
    }
}

#[cfg(test)]
mod tests {
    use num_complex::Complex;

    use super::*;
    use crate::array::beamform::testing::{
        TONE, covariance, kraken, noise_scene, settle, steering,
    };
    use crate::array::beamform::{BlockSolver, LaneNoise};
    use crate::scene::SceneSignal;

    #[test]
    fn gain_readout_is_ten_log_n_for_equal_lanes() {
        let manifold = kraken();
        let mut scene = noise_scene(TONE, 10.0, &[0.0; 5]);
        let mut noise = LaneNoise::new(5).unwrap();
        settle(&mut scene, &mut noise);
        let lanes = scene.render(40_000).unwrap();
        let r = covariance(&lanes);
        let mut solver = BlockSolver::new(5).unwrap();
        let mut weights = WeightSet::zeros(5);
        solver
            .das(&steering(&manifold, 137.0), &mut weights)
            .unwrap();
        let metrics =
            beam_metrics(&r, &weights, noise.noise(), None, &Constraints::new(5)).unwrap();
        let gain = metrics.gain_db.unwrap();
        assert!((gain - 7.0).abs() < 0.5, "gain {gain} dB");
        assert!((metrics.snr_db.unwrap() - 17.0).abs() < 0.5);
        assert!(metrics.cancelled_db.is_none());
        assert_eq!(metrics.nulls, 0);
    }

    #[test]
    fn band_full_hides_snr_readouts() {
        let mut scene = noise_scene(SceneSignal::Broadband, 20.0, &[0.0; 5]);
        let mut noise = LaneNoise::new(5).unwrap();
        let lanes = settle(&mut scene, &mut noise);
        assert!(noise.band_full());
        assert_eq!(noise.noise(), None);
        let r = covariance(&lanes);
        let weights = WeightSet::unit(5, 0);
        let metrics =
            beam_metrics(&r, &weights, noise.noise(), None, &Constraints::new(5)).unwrap();
        assert_eq!(metrics.snr_db, None);
        assert_eq!(metrics.gain_db, None);
        assert!(metrics.output_power > 50.0);
        noise.reset();
        assert_eq!(noise.noise(), None);
        assert!(!noise.band_full());
    }

    #[test]
    fn null_depths_and_cancellation_are_read_from_the_weights() {
        let manifold = kraken();
        let mut constraints = Constraints::new(5);
        constraints
            .push(&steering(&manifold, 10.0), Complex::new(1.0, 0.0))
            .unwrap();
        constraints
            .push(&steering(&manifold, 140.0), Complex::new(0.0, 0.0))
            .unwrap();
        let mut solver = BlockSolver::new(5).unwrap();
        let mut weights = WeightSet::zeros(5);
        solver.lcmv(None, &constraints, 0.0, &mut weights).unwrap();
        let r = CMat::identity(5).unwrap();
        let metrics = beam_metrics(&r, &weights, None, Some(0), &constraints).unwrap();
        assert_eq!(metrics.nulls, 1);
        assert!(metrics.null_depth_db[0] < -40.0);
        let expected = (1.0 / weights.norm_sqr()).power_to_db();
        assert!((metrics.cancelled_db.unwrap() - expected).abs() < 1e-3);
        assert_eq!(
            beam_metrics(&r, &WeightSet::zeros(3), None, None, &constraints),
            Err(BeamError::Lanes(5))
        );
        assert_eq!(
            beam_metrics(&r, &weights, None, Some(5), &constraints),
            Err(BeamError::Lanes(6))
        );
    }
}
