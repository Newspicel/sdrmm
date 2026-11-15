use crate::math::stats::{finite_quantile, median};

/// The median of exponentially distributed power sits `ln 2` below the mean, and a detection
/// threshold quoted in dB over the noise is understood against the mean.
const MEDIAN_TO_MEAN_DB: f32 = 1.591_745_2;

const SPAN_FLOOR_HALF_BINS: usize = 48;
const SPAN_FLOOR_STRIDE_BINS: usize = 8;
const SPAN_FLOOR_PERCENTILE: f32 = 0.2;

/// A noise floor measured from each bin's own neighbourhood rather than from one figure for the
/// whole span.
///
/// A global percentile is only the noise floor when the span is mostly noise. One strong signal
/// drags a wide skirt of its own keying energy across the span, and every bin of that skirt then
/// stands far above the global figure even though nothing is transmitting there.
pub struct NoiseFloor {
    half: usize,
    stride: usize,
    cells: Vec<f32>,
    knots: Vec<f32>,
}

pub struct SpanFloor {
    local: NoiseFloor,
    curve: Vec<f32>,
    scratch: Vec<f32>,
}

impl NoiseFloor {
    #[must_use]
    pub fn new(half: usize, stride: usize) -> Self {
        let half = half.max(1);
        Self {
            half,
            stride: stride.max(1),
            cells: Vec::with_capacity(2 * half + 1),
            knots: Vec::new(),
        }
    }

    /// Writes the estimated mean noise power in dB for every bin of `power_db`.
    pub fn estimate(&mut self, power_db: &[f32], out: &mut Vec<f32>) {
        out.clear();
        if power_db.is_empty() {
            return;
        }
        out.resize(power_db.len(), f32::NEG_INFINITY);
        self.place_knots(power_db);
        let spans = self.knots.len() - 1;
        if spans == 0 {
            out.fill(self.knots[0]);
            return;
        }
        let last = power_db.len() - 1;
        for span in 0..spans {
            let start = span * self.stride;
            let end = if span + 1 == spans {
                last
            } else {
                (span + 1) * self.stride
            };
            let (from, to) = (self.knots[span], self.knots[span + 1]);
            let width = (end - start) as f32;
            for (offset, slot) in out[start..=end].iter_mut().enumerate() {
                *slot = from + (to - from) * offset as f32 / width;
            }
        }
    }

    fn place_knots(&mut self, power_db: &[f32]) {
        let last = power_db.len() - 1;
        self.knots.clear();
        let mut centre = 0;
        while centre < last {
            let value = self.mean_at(power_db, centre);
            self.knots.push(value);
            centre += self.stride;
        }
        let value = self.mean_at(power_db, last);
        self.knots.push(value);
    }

    fn mean_at(&mut self, power_db: &[f32], centre: usize) -> f32 {
        let low = centre.saturating_sub(self.half);
        let high = (centre + self.half + 1).min(power_db.len());
        self.cells.clear();
        self.cells.extend(
            power_db[low..high]
                .iter()
                .copied()
                .filter(|v| v.is_finite()),
        );
        median(&mut self.cells).map_or(f32::NEG_INFINITY, |middle| middle + MEDIAN_TO_MEAN_DB)
    }
}

impl Default for SpanFloor {
    fn default() -> Self {
        Self {
            local: NoiseFloor::new(SPAN_FLOOR_HALF_BINS, SPAN_FLOOR_STRIDE_BINS),
            curve: Vec::new(),
            scratch: Vec::new(),
        }
    }
}

impl SpanFloor {
    pub fn read(&mut self, db: &[f32]) -> Option<f32> {
        self.local.estimate(db, &mut self.curve);
        finite_quantile(&self.curve, &mut self.scratch, SPAN_FLOOR_PERCENTILE)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::{math::db::Decibels, math::rng::Rng};

    pub(in crate::measure::spectrum) fn exponential_power_db(
        len: usize,
        mean_db: f32,
        seed: u64,
    ) -> Vec<f32> {
        let mut rng = Rng::mixed(seed);
        (0..len)
            .map(|_| mean_db + (-rng.open_unit().ln()).power_to_db())
            .collect()
    }

    #[test]
    fn the_floor_reads_mean_noise_beside_busy_channels() {
        let mut db = exponential_power_db(4_096, -90.0, 0xF00D);
        for cell in db.iter_mut().step_by(10) {
            *cell = -30.0;
        }
        let floor = SpanFloor::default().read(&db).unwrap();
        assert!((floor - -90.0).abs() < 1.5, "floor read {floor}");
        assert_eq!(SpanFloor::default().read(&[f32::NEG_INFINITY; 4]), None);
    }

    #[test]
    fn the_floor_looks_past_a_skirt_over_most_of_the_span() {
        let mut db = exponential_power_db(4_096, -100.0, 0x5C1A);
        for cell in &mut db[800..3_300] {
            *cell += 8.0;
        }
        let floor = SpanFloor::default().read(&db).unwrap();
        assert!((floor - -100.0).abs() < 1.5, "floor read {floor}");
    }

    #[test]
    fn the_local_floor_reads_the_mean_power_of_noise() {
        for mean_db in [-90.0f32, -20.0, 12.0] {
            let power = exponential_power_db(4_096, mean_db, 0x5EED);
            let mut floor = Vec::new();
            NoiseFloor::new(48, 8).estimate(&power, &mut floor);
            assert_eq!(floor.len(), power.len());
            let worst = floor
                .iter()
                .map(|value| (value - mean_db).abs())
                .fold(0.0f32, f32::max);
            assert!(worst < 2.0, "mean {mean_db} dB read off by {worst} dB");
        }
    }

    #[test]
    fn a_carrier_does_not_lift_the_floor_it_stands_on() {
        let mut power = exponential_power_db(4_096, -80.0, 0x1234);
        let mut plain = Vec::new();
        NoiseFloor::new(48, 8).estimate(&power, &mut plain);
        for cell in &mut power[2_046..2_050] {
            *cell = 0.0;
        }
        let mut lifted = Vec::new();
        NoiseFloor::new(48, 8).estimate(&power, &mut lifted);
        let worst = plain
            .iter()
            .zip(&lifted)
            .map(|(before, after)| (after - before).abs())
            .fold(0.0f32, f32::max);
        assert!(
            worst < 1.0,
            "an 80 dB carrier moved the floor by {worst} dB"
        );
    }

    #[test]
    fn the_floor_follows_a_raised_shoulder_instead_of_averaging_it_away() {
        let mut power = exponential_power_db(4_096, -80.0, 0xABCD);
        for cell in &mut power[2_048..3_072] {
            *cell += 30.0;
        }
        let mut floor = Vec::new();
        NoiseFloor::new(48, 8).estimate(&power, &mut floor);
        assert!((floor[2_600] - -50.0).abs() < 2.0, "{}", floor[2_600]);
        assert!((floor[1_000] - -80.0).abs() < 2.0, "{}", floor[1_000]);
    }
}
