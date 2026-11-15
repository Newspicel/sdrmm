use crate::math::stats::finite_quantile;

const FLOOR_PERCENTILE: f32 = 0.25;
const FLOOR_MARGIN_DB: f32 = 10.0;
const DEFAULT_DB_RANGE: f32 = 70.0;
const PEAK_MARGIN_DB: f32 = 15.0;
const EMPTY_FLOOR_DB: f32 = -100.0;
const EMPTY_WINDOW: (f32, f32) = (EMPTY_FLOOR_DB, EMPTY_FLOOR_DB + DEFAULT_DB_RANGE);

const WINDOW_SNAP_DB: f32 = 12.0;
const FLOOR_GLIDE: f32 = 0.1;
const CEILING_RELEASE: f32 = 0.03;

#[derive(Default)]
pub struct DbWindowSmoother {
    current: Option<(f32, f32)>,
}

/// The dB range to draw: the floor a margin under the noise, the ceiling high enough for the
/// loudest bin.
#[must_use]
pub fn adaptive_db_window(db: &[f32], scratch: &mut Vec<f32>) -> (f32, f32) {
    let Some(floor) = finite_quantile(db, scratch, FLOOR_PERCENTILE) else {
        return EMPTY_WINDOW;
    };
    let peak = db
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(f32::NEG_INFINITY, f32::max);
    let min = floor - FLOOR_MARGIN_DB;
    (min, (min + DEFAULT_DB_RANGE).max(peak + PEAK_MARGIN_DB))
}

impl DbWindowSmoother {
    pub fn follow(&mut self, target: (f32, f32)) -> (f32, f32) {
        let next = match self.current {
            Some((min, max)) if (target.0 - min).abs() <= WINDOW_SNAP_DB => {
                let min = min + (target.0 - min) * FLOOR_GLIDE;
                let max = if target.1 > max {
                    target.1
                } else {
                    max + (target.1 - max) * CEILING_RELEASE
                };
                (min, max.max(min + DEFAULT_DB_RANGE))
            }
            _ => target,
        };
        self.current = Some(next);
        next
    }
}

pub fn quantize_db(db: &[f32], db_min: f32, db_max: f32, out: &mut [u8]) {
    assert_eq!(db.len(), out.len(), "quantize length mismatch");
    let span = (db_max - db_min).max(f32::MIN_POSITIVE);
    let top = f32::from(u8::MAX);
    for (&v, slot) in db.iter().zip(out.iter_mut()) {
        let t = ((v - db_min) / span).clamp(0.0, 1.0);
        *slot = (t * top + 0.5) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position(db: f32, window: (f32, f32)) -> f32 {
        (db - window.0) / (window.1 - window.0)
    }

    #[test]
    fn bare_noise_stays_at_the_bottom_of_the_scale() {
        let mut scratch = Vec::new();
        let noise: Vec<f32> = (0..512)
            .map(|i| -60.0 + ((i * 37) % 23) as f32 * 0.5 - 5.0)
            .collect();
        let window = adaptive_db_window(&noise, &mut scratch);

        let hottest = noise.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            position(hottest, window) < 0.4,
            "loudest noise bin reached {:.2} of the scale, window {window:?}",
            position(hottest, window)
        );
    }

    #[test]
    fn a_carrier_moves_the_ceiling_and_not_the_floor() {
        let mut scratch = Vec::new();
        let mut bins = vec![-95.0f32; 512];
        let quiet = adaptive_db_window(&bins, &mut scratch);
        bins[100] = -12.0;
        let loud = adaptive_db_window(&bins, &mut scratch);

        assert!((quiet.0 - loud.0).abs() < f32::EPSILON, "floor moved");
        assert!(
            loud.1 > quiet.1,
            "ceiling did not make room for the carrier"
        );
        let at = position(-12.0, loud);
        assert!(
            (0.75..1.0).contains(&at),
            "carrier at {at:.2} of the scale, window {loud:?}"
        );
    }

    #[test]
    fn floor_ignores_an_occupied_quarter_of_the_band() {
        let mut scratch = Vec::new();
        let mut bins = vec![-95.0f32; 400];
        bins.extend(std::iter::repeat_n(-40.0f32, 100));
        let (min, _) = adaptive_db_window(&bins, &mut scratch);
        assert!(
            (-95.0 - FLOOR_MARGIN_DB - min).abs() < 1.0,
            "floor read as {min}, expected the noise and not the occupancy"
        );
    }

    #[test]
    fn non_finite_bins_do_not_reach_the_window() {
        let mut scratch = Vec::new();
        assert_eq!(
            adaptive_db_window(&[f32::NEG_INFINITY; 8], &mut scratch),
            EMPTY_WINDOW
        );
        assert_eq!(adaptive_db_window(&[], &mut scratch), EMPTY_WINDOW);

        let mixed = [f32::NEG_INFINITY, -70.0, -70.0, f32::NAN, -70.0];
        let (min, max) = adaptive_db_window(&mixed, &mut scratch);
        assert!(min.is_finite() && max.is_finite(), "{min} {max}");
        assert!((min - (-80.0)).abs() < f32::EPSILON, "floor read as {min}");
    }

    #[test]
    fn window_is_never_empty() {
        let mut scratch = Vec::new();
        for bins in [vec![0.0f32; 4], vec![-200.0f32; 4], vec![-60.0f32; 1]] {
            let (min, max) = adaptive_db_window(&bins, &mut scratch);
            assert!(max > min, "degenerate window {min}..{max}");
        }
    }

    #[test]
    fn the_window_ceiling_rises_at_once_and_falls_slowly() {
        let mut smoother = DbWindowSmoother::default();
        assert_eq!(smoother.follow((-100.0, -30.0)), (-100.0, -30.0));
        assert_eq!(smoother.follow((-100.0, -10.0)).1, -10.0);
        let fallen = smoother.follow((-100.0, -30.0)).1;
        assert!(fallen > -12.0 && fallen < -10.0, "ceiling at {fallen}");
    }

    #[test]
    fn the_window_floor_glides_and_snaps_on_a_jump() {
        let mut smoother = DbWindowSmoother::default();
        smoother.follow((-100.0, -30.0));
        let glided = smoother.follow((-98.0, -28.0)).0;
        assert!(glided > -100.0 && glided < -99.0, "floor at {glided}");
        assert_eq!(smoother.follow((-60.0, 10.0)), (-60.0, 10.0));
    }

    #[test]
    fn quantize_maps_range_to_bytes() {
        let db = [-120.0, -70.0, -20.0];
        let mut out = [0u8; 3];
        quantize_db(&db, -120.0, -20.0, &mut out);
        assert_eq!(out[0], 0);
        assert_eq!(out[2], 255);
        assert!((out[1] as i32 - 128).abs() <= 1);
    }
}
