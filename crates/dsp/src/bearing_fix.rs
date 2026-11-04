/// One line of position on a local flat plane: the point sought lies along `bearing_deg`,
/// clockwise from north, as seen from (`east_m`, `north_m`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BearingLine {
    pub east_m: f64,
    pub north_m: f64,
    pub bearing_deg: f64,
    pub sigma_deg: f64,
}

/// Where the lines cross, with a one-sigma error ellipse.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineFix {
    pub east_m: f64,
    pub north_m: f64,
    pub major_m: f64,
    pub minor_m: f64,
    /// The major axis, clockwise from north, in `0..180`.
    pub major_bearing_deg: f64,
    /// Root mean square distance from the fix to the lines.
    pub spread_m: f64,
}

const REWEIGHTS: usize = 3;
const MIN_RANGE_M: f64 = 1.0;
const MIN_SIGMA_DEG: f64 = 0.01;
/// Below this the normal matrix is too close to singular: the lines run parallel.
const MIN_CONDITION: f64 = 1e-9;

#[derive(Clone, Copy)]
struct Normal {
    east: f64,
    north: f64,
    offset: f64,
}

impl BearingLine {
    fn normal(&self) -> Normal {
        let (sin, cos) = self.bearing_deg.to_radians().sin_cos();
        Normal {
            east: cos,
            north: -sin,
            offset: cos * self.east_m - sin * self.north_m,
        }
    }

    fn range_m(&self, east_m: f64, north_m: f64) -> f64 {
        let (sin, cos) = self.bearing_deg.to_radians().sin_cos();
        sin * (east_m - self.east_m) + cos * (north_m - self.north_m)
    }

    fn sigma_rad(&self) -> f64 {
        self.sigma_deg.abs().max(MIN_SIGMA_DEG).to_radians()
    }

    fn miss_m(&self, east_m: f64, north_m: f64) -> f64 {
        let normal = self.normal();
        normal.east * east_m + normal.north * north_m - normal.offset
    }
}

#[derive(Clone, Copy, Default)]
struct Normals {
    ee: f64,
    nn: f64,
    en: f64,
    e: f64,
    n: f64,
}

impl Normals {
    fn add(&mut self, normal: Normal, weight: f64) {
        self.ee += weight * normal.east * normal.east;
        self.nn += weight * normal.north * normal.north;
        self.en += weight * normal.east * normal.north;
        self.e += weight * normal.east * normal.offset;
        self.n += weight * normal.north * normal.offset;
    }

    fn determinant(&self) -> Option<f64> {
        let determinant = self.ee * self.nn - self.en * self.en;
        let trace = self.ee + self.nn;
        (determinant.is_finite() && determinant > MIN_CONDITION * trace * trace)
            .then_some(determinant)
    }
}

struct Solved {
    east_m: f64,
    north_m: f64,
    cee: f64,
    cnn: f64,
    cen: f64,
}

fn solve(lines: &[BearingLine], weight: impl Fn(&BearingLine) -> f64) -> Option<Solved> {
    let mut normals = Normals::default();
    for line in lines {
        normals.add(line.normal(), weight(line));
    }
    let determinant = normals.determinant()?;
    Some(Solved {
        east_m: (normals.e * normals.nn - normals.n * normals.en) / determinant,
        north_m: (normals.n * normals.ee - normals.e * normals.en) / determinant,
        cee: normals.nn / determinant,
        cnn: normals.ee / determinant,
        cen: -normals.en / determinant,
    })
}

fn across_weight(line: &BearingLine, east_m: f64, north_m: f64) -> f64 {
    let range = line.range_m(east_m, north_m).abs().max(MIN_RANGE_M);
    let across = range * line.sigma_rad();
    1.0 / (across * across)
}

/// Weighted least squares crossing of two or more lines of position.
///
/// Each line weighs by how far its angular error spreads at the fix, so a near station counts
/// more than a far one; the fix is refined a few times since that distance depends on it. With
/// more lines than unknowns, a spread wider than the stated errors widens the ellipse to match.
/// `None` when fewer than two lines remain, they run parallel, or the crossing lies behind a
/// line's origin by more than its own uncertainty.
#[must_use]
pub fn cross(lines: &[BearingLine]) -> Option<LineFix> {
    if lines.len() < 2 {
        return None;
    }
    let mut solved = solve(lines, |line| line.sigma_rad().powi(-2))?;
    for _ in 0..REWEIGHTS {
        let (east_m, north_m) = (solved.east_m, solved.north_m);
        solved = solve(lines, |line| across_weight(line, east_m, north_m))?;
    }
    let fix = shaped(lines, &solved);
    let ahead = lines
        .iter()
        .all(|line| line.range_m(fix.east_m, fix.north_m) >= -fix.major_m);
    ahead.then_some(fix)
}

fn shaped(lines: &[BearingLine], solved: &Solved) -> LineFix {
    let (east_m, north_m) = (solved.east_m, solved.north_m);
    let squared: f64 = lines
        .iter()
        .map(|line| line.miss_m(east_m, north_m).powi(2))
        .sum();
    let chi2: f64 = lines
        .iter()
        .map(|line| line.miss_m(east_m, north_m).powi(2) * across_weight(line, east_m, north_m))
        .sum();
    let freedom = lines.len().saturating_sub(2);
    let scale = if freedom > 0 {
        (chi2 / freedom as f64).max(1.0)
    } else {
        1.0
    };
    let (cee, cnn, cen) = (solved.cee * scale, solved.cnn * scale, solved.cen * scale);
    let half_trace = (cee + cnn) / 2.0;
    let root = ((cee - cnn) / 2.0).hypot(cen);
    let axis_from_east = (2.0 * cen).atan2(cee - cnn).to_degrees() / 2.0;
    LineFix {
        east_m,
        north_m,
        major_m: (half_trace + root).max(0.0).sqrt(),
        minor_m: (half_trace - root).max(0.0).sqrt(),
        major_bearing_deg: (90.0 - axis_from_east).rem_euclid(180.0),
        spread_m: (squared / lines.len() as f64).sqrt(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOISE_DEG: [f64; 12] = [
        0.9, -1.4, 0.3, 1.8, -0.6, -1.1, 0.5, 1.2, -1.9, 0.2, -0.4, 1.0,
    ];

    fn toward(from: (f64, f64), target: (f64, f64), sigma_deg: f64, noise_deg: f64) -> BearingLine {
        let bearing = (target.0 - from.0).atan2(target.1 - from.1).to_degrees();
        BearingLine {
            east_m: from.0,
            north_m: from.1,
            bearing_deg: bearing + noise_deg,
            sigma_deg,
        }
    }

    fn miss(fix: &LineFix, target: (f64, f64)) -> f64 {
        (fix.east_m - target.0).hypot(fix.north_m - target.1)
    }

    #[test]
    fn two_clean_lines_cross_exactly() {
        let target = (12_000.0, 30_000.0);
        let fix = cross(&[
            toward((-40_000.0, 0.0), target, 2.0, 0.0),
            toward((35_000.0, 5_000.0), target, 2.0, 0.0),
        ])
        .expect("a crossing");
        assert!(miss(&fix, target) < 1e-6, "{fix:?}");
        assert!(fix.spread_m < 1e-6, "{fix:?}");
        assert!(fix.major_m >= fix.minor_m && fix.minor_m > 0.0, "{fix:?}");
    }

    #[test]
    fn two_noisy_lines_land_within_their_ellipse() {
        let target = (0.0, 0.0);
        let stations = [(-30_000.0, 30_000.0), (40_000.0, 20_000.0)];
        for noise in NOISE_DEG.chunks(2) {
            let lines: Vec<BearingLine> = stations
                .iter()
                .zip(noise)
                .map(|(&from, &noise)| toward(from, target, 2.0, noise))
                .collect();
            let fix = cross(&lines).expect("a crossing");
            let off = miss(&fix, target);
            assert!(off < 3.0 * fix.major_m, "{off} m off: {fix:?}");
            assert!(off < 2_500.0, "{off} m off: {fix:?}");
        }
    }

    #[test]
    fn a_third_station_tightens_a_noisy_fix() {
        let target = (5_000.0, -8_000.0);
        let stations = [
            (-30_000.0, 30_000.0),
            (40_000.0, 20_000.0),
            (0.0, -60_000.0),
        ];
        let lines = |count: usize| -> Vec<BearingLine> {
            stations
                .iter()
                .take(count)
                .zip(NOISE_DEG.iter().skip(4))
                .map(|(&from, &noise)| toward(from, target, 2.0, noise))
                .collect()
        };
        let two = cross(&lines(2)).expect("two stations");
        let three = cross(&lines(3)).expect("three stations");
        assert!(miss(&three, target) < 2_500.0, "{three:?}");
        assert!(
            three.major_m * three.minor_m < two.major_m * two.minor_m,
            "{two:?} {three:?}"
        );
        assert!(three.spread_m > 0.0);
    }

    #[test]
    fn the_ellipse_lies_along_the_lines_of_a_narrow_crossing() {
        let target = (0.0, 50_000.0);
        let fix = cross(&[
            toward((-5_000.0, 0.0), target, 2.0, 0.0),
            toward((5_000.0, 0.0), target, 2.0, 0.0),
        ])
        .expect("a crossing");
        let off_north = fix.major_bearing_deg.min(180.0 - fix.major_bearing_deg);
        assert!(off_north < 1.0, "{fix:?}");
        assert!(fix.major_m > 5.0 * fix.minor_m, "{fix:?}");
    }

    #[test]
    fn parallel_lines_and_a_single_line_give_no_fix() {
        let one = toward((0.0, 0.0), (0.0, 10_000.0), 2.0, 0.0);
        assert_eq!(cross(&[one]), None);
        let beside = BearingLine {
            east_m: 1_000.0,
            ..one
        };
        assert_eq!(cross(&[one, beside]), None);
        assert_eq!(cross(&[one, one]), None);
    }

    #[test]
    fn lines_that_only_meet_behind_their_origins_give_no_fix() {
        let target = (0.0, -20_000.0);
        let away = |from: (f64, f64)| BearingLine {
            bearing_deg: toward(from, target, 2.0, 0.0).bearing_deg + 180.0,
            ..toward(from, target, 2.0, 0.0)
        };
        assert_eq!(
            cross(&[away((-10_000.0, 0.0)), away((10_000.0, 0.0))]),
            None
        );
    }
}
