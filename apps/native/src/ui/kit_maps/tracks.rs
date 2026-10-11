use crate::ui::map::{Geo, geo::EARTH_RADIUS_KM};

pub const TRAIL_CAPACITY: usize = 512;
pub const TRAIL_TRIM: usize = TRAIL_CAPACITY / 4;
pub const TRAIL_TOLERANCE_M: f64 = 25.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Trail {
    pub points: Vec<Geo>,
    last_at: i64,
    dir_x: f64,
    dir_y: f64,
    reach: f64,
}

impl Default for Trail {
    fn default() -> Self {
        Self {
            points: Vec::new(),
            last_at: i64::MIN,
            dir_x: 0.0,
            dir_y: 0.0,
            reach: 0.0,
        }
    }
}

#[must_use]
pub fn offset_m(from: Geo, to: Geo) -> (f64, f64) {
    let earth_m = EARTH_RADIUS_KM * 1_000.0;
    let d_lon = (to.lon - from.lon + 180.0).rem_euclid(360.0) - 180.0;
    let x = d_lon.to_radians() * from.lat.to_radians().cos() * earth_m;
    let y = (to.lat - from.lat).to_radians() * earth_m;
    (x, y)
}

impl Trail {
    pub fn extend(&mut self, point: Geo, at: i64) -> bool {
        if at < self.last_at {
            return false;
        }
        self.last_at = at;
        let Some(&last) = self.points.last() else {
            self.points.push(point);
            return true;
        };
        if self.points.len() >= 2 {
            let anchor = self.points[self.points.len() - 2];
            if self.extends_segment(anchor, point) {
                if let Some(end) = self.points.last_mut() {
                    *end = point;
                }
                return true;
            }
        }
        let (dx, dy) = offset_m(last, point);
        let step = dx.hypot(dy);
        if step < TRAIL_TOLERANCE_M {
            return false;
        }
        self.points.push(point);
        self.dir_x = dx / step;
        self.dir_y = dy / step;
        self.reach = step;
        if self.points.len() > TRAIL_CAPACITY {
            self.points.drain(..TRAIL_TRIM);
        }
        true
    }

    fn extends_segment(&mut self, anchor: Geo, point: Geo) -> bool {
        let (dx, dy) = offset_m(anchor, point);
        let along = dx * self.dir_x + dy * self.dir_y;
        let across = (dx * self.dir_y - dy * self.dir_x).abs();
        if across > TRAIL_TOLERANCE_M || along < self.reach {
            return false;
        }
        self.reach = along;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NORTH_M: f64 = 1.0 / 111_195.0;

    fn north(metres: f64) -> Geo {
        Geo::new(50.0 + metres * NORTH_M, 8.0)
    }

    #[test]
    fn a_straight_run_keeps_only_its_ends() {
        let mut trail = Trail::default();
        for (step, metres) in [0.0, 100.0, 200.0, 300.0].into_iter().enumerate() {
            assert!(trail.extend(north(metres), step as i64));
        }
        assert_eq!(trail.points.len(), 2);
        assert!((trail.points[1].lat - north(300.0).lat).abs() < 1e-9);
    }

    #[test]
    fn a_turn_starts_a_new_segment_and_jitter_is_ignored() {
        let mut trail = Trail::default();
        trail.extend(north(0.0), 0);
        trail.extend(north(100.0), 1);
        assert!(!trail.extend(north(95.0), 2));
        let east = Geo::new(north(100.0).lat, 8.01);
        assert!(trail.extend(east, 3));
        assert_eq!(trail.points.len(), 3);
    }

    #[test]
    fn late_fixes_are_dropped() {
        let mut trail = Trail::default();
        trail.extend(north(0.0), 10);
        assert!(!trail.extend(north(500.0), 5));
        assert_eq!(trail.points.len(), 1);
    }

    #[test]
    fn a_long_zigzag_is_capped() {
        let mut trail = Trail::default();
        for step in 0..(TRAIL_CAPACITY + 1) {
            let lon = if step % 2 == 0 { 8.0 } else { 8.01 };
            trail.extend(Geo::new(50.0 + step as f64 * 0.01, lon), step as i64);
        }
        assert_eq!(trail.points.len(), TRAIL_CAPACITY + 1 - TRAIL_TRIM);
    }

    #[test]
    fn offsets_wrap_the_antimeridian() {
        let (dx, _) = offset_m(Geo::new(0.0, 179.9), Geo::new(0.0, -179.9));
        assert!(dx > 0.0 && dx < 30_000.0);
    }
}
