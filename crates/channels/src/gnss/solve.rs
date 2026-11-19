//! Satellite orbits from the broadcast ephemeris and the least-squares position and clock fix.

use sdrmm_dsp::math::geodesy::{Geodetic, ecef_vector_to_enu, geodetic};

use super::nav::Ephemeris;

pub(super) const SPEED_OF_LIGHT: f64 = 299_792_458.0;
pub(super) const MAX_SATELLITES: usize = 12;
const MU: f64 = 3.986_005e14;
const EARTH_RATE: f64 = 7.292_115_146_7e-5;
const RELATIVITY_F: f64 = -4.442_807_633e-10;
const HALF_WEEK_S: f64 = 302_400.0;
const NOMINAL_TRAVEL_S: f64 = 0.075;
const ELEVATION_MASK_DEG: f64 = 5.0;
const ITERATIONS: usize = 12;
const CONVERGED_M: f64 = 1e-3;
const OUTLIER_M: f64 = 50.0;
const ON_EARTH_M: f64 = 6.0e6;

type Vec3 = [f64; 3];

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn norm(a: Vec3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn since(t: f64, reference: f64) -> f64 {
    let dt = t - reference;
    if dt > HALF_WEEK_S {
        dt - 2.0 * HALF_WEEK_S
    } else if dt < -HALF_WEEK_S {
        dt + 2.0 * HALF_WEEK_S
    } else {
        dt
    }
}

impl Ephemeris {
    fn eccentric_anomaly(&self, t: f64) -> f64 {
        let a = self.sqrt_a * self.sqrt_a;
        let motion = (MU / (a * a * a)).sqrt() + self.delta_n;
        let mean = self.m0 + motion * since(t, self.toe);
        let mut eccentric = mean;
        for _ in 0..10 {
            eccentric = mean + self.e * eccentric.sin();
        }
        eccentric
    }

    /// Satellite clock offset in seconds at GPS time `t`, relativity and group delay included.
    pub(super) fn clock_offset_s(&self, t: f64) -> f64 {
        let dt = since(t, self.toc);
        let relativity = RELATIVITY_F * self.e * self.sqrt_a * self.eccentric_anomaly(t).sin();
        self.af0 + self.af1 * dt + self.af2 * dt * dt + relativity - self.tgd
    }

    /// Earth-fixed satellite position in metres at GPS time `t`, IS-GPS-200 table 20-IV.
    pub(super) fn position(&self, t: f64) -> Vec3 {
        let tk = since(t, self.toe);
        let a = self.sqrt_a * self.sqrt_a;
        let eccentric = self.eccentric_anomaly(t);
        let true_anomaly =
            ((1.0 - self.e * self.e).sqrt() * eccentric.sin()).atan2(eccentric.cos() - self.e);
        let latitude = true_anomaly + self.omega;
        let (sin2, cos2) = (2.0 * latitude).sin_cos();
        let u = latitude + self.cus * sin2 + self.cuc * cos2;
        let r = a * (1.0 - self.e * eccentric.cos()) + self.crs * sin2 + self.crc * cos2;
        let i = self.i0 + self.cis * sin2 + self.cic * cos2 + self.idot * tk;
        let node = self.omega0 + (self.omega_dot - EARTH_RATE) * tk - EARTH_RATE * self.toe;
        let (x, y) = (r * u.cos(), r * u.sin());
        let (sin_node, cos_node) = node.sin_cos();
        [
            x * cos_node - y * i.cos() * sin_node,
            x * sin_node + y * i.cos() * cos_node,
            y * i.sin(),
        ]
    }
}

/// One satellite's transmit time at the receiver's measurement instant.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Measurement {
    pub prn: u8,
    /// GPS seconds of week the signal left the satellite, by the satellite's clock.
    pub transmit_s: f64,
    pub ephemeris: Ephemeris,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Solution {
    pub at: Geodetic,
    pub clock_bias_m: f64,
    /// GPS seconds of week at the measurement instant, receiver clock removed.
    pub tow_s: f64,
    pub used: [u8; MAX_SATELLITES],
    pub used_count: usize,
    pub pdop: f64,
    pub hdop: f64,
    pub vdop: f64,
    pub residual_rms_m: f64,
}

impl Solution {
    pub fn satellites(&self) -> &[u8] {
        &self.used[..self.used_count]
    }
}

#[derive(Clone, Copy, Default)]
struct Satellite {
    prn: u8,
    position: Vec3,
    pseudorange_m: f64,
    active: bool,
}

#[derive(Clone, Copy, Default)]
struct Row {
    gradient: [f64; 4],
    residual: f64,
}

/// Position and receiver clock bias by Gauss-Newton least squares, with Earth rotation during
/// the signal's flight, the satellite clocks and a standard troposphere.
pub(super) fn solve(measurements: &[Measurement]) -> Option<Solution> {
    let mut satellites = [Satellite::default(); MAX_SATELLITES];
    let count = measurements.len().min(MAX_SATELLITES);
    let latest = measurements
        .iter()
        .map(|m| m.transmit_s)
        .fold(f64::MIN, f64::max);
    let receive_s = latest + NOMINAL_TRAVEL_S;
    for (slot, m) in satellites.iter_mut().zip(measurements) {
        let offset = m.ephemeris.clock_offset_s(m.transmit_s);
        *slot = Satellite {
            prn: m.prn,
            position: m.ephemeris.position(m.transmit_s - offset),
            pseudorange_m: SPEED_OF_LIGHT * (receive_s - m.transmit_s + offset),
            active: true,
        };
    }
    let satellites = &mut satellites[..count];
    loop {
        let (state, rows, normal) = iterate(satellites)?;
        let worst = worst_outlier(satellites, &rows);
        match worst {
            Some(index) if active(satellites) > 4 => satellites[index].active = false,
            Some(_) => return None,
            None => return Some(finish(satellites, &rows, &normal, state, receive_s)),
        }
    }
}

fn active(satellites: &[Satellite]) -> usize {
    satellites.iter().filter(|s| s.active).count()
}

fn worst_outlier(satellites: &[Satellite], rows: &[Row; MAX_SATELLITES]) -> Option<usize> {
    satellites
        .iter()
        .zip(rows)
        .enumerate()
        .filter(|(_, (s, row))| s.active && row.residual.abs() > OUTLIER_M)
        .max_by(|a, b| a.1.1.residual.abs().total_cmp(&b.1.1.residual.abs()))
        .map(|(index, _)| index)
}

type Normal = [[f64; 4]; 4];

fn iterate(satellites: &mut [Satellite]) -> Option<([f64; 4], [Row; MAX_SATELLITES], Normal)> {
    let mut state = [0.0; 4];
    for _ in 0..ITERATIONS {
        let rows = linearise(satellites, state);
        if active(satellites) < 4 {
            return None;
        }
        let normal = normal_matrix(satellites, &rows);
        let inverse = invert(normal)?;
        let mut step = [0.0; 4];
        for (row, s) in rows.iter().zip(satellites.iter()) {
            if !s.active {
                continue;
            }
            for (k, out) in step.iter_mut().enumerate() {
                *out += (0..4)
                    .map(|j| inverse[k][j] * row.gradient[j] * row.residual)
                    .sum::<f64>();
            }
        }
        for (value, delta) in state.iter_mut().zip(step) {
            *value += delta;
        }
        if norm([step[0], step[1], step[2]]) < CONVERGED_M {
            let rows = linearise(satellites, state);
            let normal = normal_matrix(satellites, &rows);
            return Some((state, rows, normal));
        }
    }
    None
}

fn linearise(satellites: &mut [Satellite], state: [f64; 4]) -> [Row; MAX_SATELLITES] {
    let receiver = [state[0], state[1], state[2]];
    let on_earth = norm(receiver) > ON_EARTH_M;
    let site = on_earth.then(|| geodetic(receiver));
    let mut rows = [Row::default(); MAX_SATELLITES];
    for (row, s) in rows.iter_mut().zip(satellites.iter_mut()) {
        let flight = norm(sub(s.position, receiver)) / SPEED_OF_LIGHT;
        let (sin, cos) = (EARTH_RATE * flight).sin_cos();
        let [x, y, z] = s.position;
        let rotated = [cos * x + sin * y, -sin * x + cos * y, z];
        let line = sub(rotated, receiver);
        let range = norm(line);
        let mut delay = 0.0;
        if let Some(site) = site {
            let elevation = elevation(site, line);
            if elevation < ELEVATION_MASK_DEG.to_radians() {
                s.active = false;
            }
            delay = troposphere_m(site, elevation);
        }
        *row = Row {
            gradient: [-line[0] / range, -line[1] / range, -line[2] / range, 1.0],
            residual: s.pseudorange_m - delay - range - state[3],
        };
    }
    rows
}

fn elevation(site: Geodetic, line: Vec3) -> f64 {
    let [east, north, up] = ecef_vector_to_enu(site, line);
    up.atan2(east.hypot(north))
}

/// Saastamoinen zenith delay of a standard atmosphere, mapped by the cosecant of elevation.
fn troposphere_m(site: Geodetic, elevation: f64) -> f64 {
    const HUMIDITY: f64 = 0.5;
    if elevation <= 0.0 || !(-100.0..=10_000.0).contains(&site.alt_m) {
        return 0.0;
    }
    let height = site.alt_m.max(0.0);
    let pressure = 1013.25 * (1.0 - 2.2557e-5 * height).powf(5.2568);
    let temperature = 15.0 - 6.5e-3 * height + 273.16;
    let vapour = 6.108 * HUMIDITY * ((17.15 * temperature - 4684.0) / (temperature - 38.45)).exp();
    let zenith = std::f64::consts::FRAC_PI_2 - elevation;
    let latitude = site.lat_deg.to_radians();
    let hydrostatic = 0.002_276_8 * pressure
        / (1.0 - 0.002_66 * (2.0 * latitude).cos() - 0.000_28 * height / 1e3)
        / zenith.cos();
    let wet = 0.002_277 * (1255.0 / temperature + 0.05) * vapour / zenith.cos();
    hydrostatic + wet
}

fn normal_matrix(satellites: &[Satellite], rows: &[Row; MAX_SATELLITES]) -> Normal {
    let mut normal = [[0.0; 4]; 4];
    for (row, _) in rows.iter().zip(satellites).filter(|(_, s)| s.active) {
        for (i, line) in normal.iter_mut().enumerate() {
            for (j, cell) in line.iter_mut().enumerate() {
                *cell += row.gradient[i] * row.gradient[j];
            }
        }
    }
    normal
}

/// Gauss-Jordan inverse with partial pivoting, nothing when the geometry is degenerate.
fn invert(mut m: Normal) -> Option<Normal> {
    let mut inverse = [[0.0; 4]; 4];
    for (i, row) in inverse.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    for col in 0..4 {
        let pivot = (col..4).max_by(|&a, &b| m[a][col].abs().total_cmp(&m[b][col].abs()))?;
        if m[pivot][col].abs() < 1e-12 {
            return None;
        }
        m.swap(col, pivot);
        inverse.swap(col, pivot);
        let scale = 1.0 / m[col][col];
        for j in 0..4 {
            m[col][j] *= scale;
            inverse[col][j] *= scale;
        }
        for row in 0..4 {
            if row == col {
                continue;
            }
            let factor = m[row][col];
            for j in 0..4 {
                m[row][j] -= factor * m[col][j];
                inverse[row][j] -= factor * inverse[col][j];
            }
        }
    }
    Some(inverse)
}

fn finish(
    satellites: &[Satellite],
    rows: &[Row; MAX_SATELLITES],
    normal: &Normal,
    state: [f64; 4],
    receive_s: f64,
) -> Solution {
    let ecef = [state[0], state[1], state[2]];
    let at = geodetic(ecef);
    let mut used = [0; MAX_SATELLITES];
    let mut used_count = 0;
    let mut squares = 0.0;
    for (s, row) in satellites.iter().zip(rows).filter(|(s, _)| s.active) {
        used[used_count] = s.prn;
        used_count += 1;
        squares += row.residual * row.residual;
    }
    let (pdop, hdop, vdop) =
        invert(*normal).map_or((f64::NAN, f64::NAN, f64::NAN), |q| dilution(at, &q));
    Solution {
        at,
        clock_bias_m: state[3],
        tow_s: receive_s - state[3] / SPEED_OF_LIGHT,
        used,
        used_count,
        pdop,
        hdop,
        vdop,
        residual_rms_m: (squares / used_count.max(1) as f64).sqrt(),
    }
}

/// Position, horizontal and vertical dilution of precision, the cofactor rotated to east,
/// north and up.
fn dilution(at: Geodetic, q: &Normal) -> (f64, f64, f64) {
    let axes = [
        ecef_vector_to_enu(at, [1.0, 0.0, 0.0]),
        ecef_vector_to_enu(at, [0.0, 1.0, 0.0]),
        ecef_vector_to_enu(at, [0.0, 0.0, 1.0]),
    ];
    let local = |a: usize, b: usize| -> f64 {
        let mut sum = 0.0;
        for i in 0..3 {
            for j in 0..3 {
                sum += axes[i][a] * q[i][j] * axes[j][b];
            }
        }
        sum
    };
    let pdop = (q[0][0] + q[1][1] + q[2][2]).sqrt();
    let hdop = (local(0, 0) + local(1, 1)).sqrt();
    let vdop = local(2, 2).sqrt();
    (pdop, hdop, vdop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdrmm_dsp::math::geodesy::ecef;

    fn orbit(plane: f64, phase: f64) -> Ephemeris {
        Ephemeris {
            sqrt_a: 5_153.7,
            e: 0.01,
            i0: 0.96,
            omega0: plane,
            m0: phase,
            omega: 0.5,
            toe: 345_600.0,
            toc: 345_600.0,
            af0: 1e-4,
            af1: 1e-12,
            omega_dot: -8e-9,
            ..Ephemeris::default()
        }
    }

    #[test]
    fn an_orbit_keeps_its_semi_major_axis_and_inclination() {
        let eph = orbit(1.0, 0.3);
        let position = eph.position(eph.toe + 1_000.0);
        let a = eph.sqrt_a * eph.sqrt_a;
        let r = norm(position);
        assert!(
            r > a * (1.0 - eph.e) - 1.0 && r < a * (1.0 + eph.e) + 1.0,
            "{r}"
        );
        let latitude = (position[2] / r).asin();
        assert!(latitude.abs() <= eph.i0 + 1e-9);
    }

    fn sky(truth: Vec3, tow: f64) -> Vec<Measurement> {
        let site = geodetic(truth);
        let mut out = Vec::new();
        let orbits = (0..24).map(|n| {
            let plane = f64::from(n % 6);
            (
                plane,
                f64::from(n / 6) * std::f64::consts::FRAC_PI_2 + plane * 0.5,
            )
        });
        for (index, (plane, phase)) in orbits.enumerate() {
            let eph = orbit(plane, phase);
            let mut transmit = tow - 0.07;
            for _ in 0..5 {
                let position = eph.position(transmit - eph.clock_offset_s(transmit));
                let flight = norm(sub(position, truth)) / SPEED_OF_LIGHT;
                let (sin, cos) = (EARTH_RATE * flight).sin_cos();
                let rotated = [
                    cos * position[0] + sin * position[1],
                    -sin * position[0] + cos * position[1],
                    position[2],
                ];
                let line = sub(rotated, truth);
                let delay = troposphere_m(site, elevation(site, line));
                transmit =
                    tow - (norm(line) + delay) / SPEED_OF_LIGHT + eph.clock_offset_s(transmit);
            }
            let line = sub(eph.position(transmit), truth);
            if elevation(site, line) > 10f64.to_radians() {
                out.push(Measurement {
                    prn: index as u8 + 1,
                    transmit_s: transmit,
                    ephemeris: eph,
                });
            }
        }
        out
    }

    #[test]
    fn a_synthetic_sky_solves_back_to_the_site_and_clock() {
        let site = Geodetic {
            lat_deg: 60.182,
            lon_deg: 24.828,
            alt_m: 47.0,
        };
        let truth = ecef(site);
        let measurements = sky(truth, 345_700.0);
        assert!(measurements.len() >= 5, "{}", measurements.len());
        let fix = solve(&measurements).expect("a fix");
        assert!(norm(sub(ecef(fix.at), truth)) < 0.05, "{:?}", fix.at);
        assert!(fix.pdop > 1.0 && fix.pdop < 10.0, "{}", fix.pdop);
        assert!(fix.hdop < fix.pdop && fix.vdop < fix.pdop);
        assert_eq!(fix.used_count, measurements.len());
        assert!((fix.tow_s - 345_700.0).abs() < 1e-9, "{}", fix.tow_s);
    }

    #[test]
    fn a_bad_pseudorange_is_dropped() {
        let site = Geodetic {
            lat_deg: -33.9,
            lon_deg: 151.2,
            alt_m: 20.0,
        };
        let truth = ecef(site);
        let mut measurements = sky(truth, 100_000.0);
        assert!(measurements.len() >= 6, "{}", measurements.len());
        measurements[2].transmit_s += 1e-6;
        let fix = solve(&measurements).expect("a fix");
        assert!(norm(sub(ecef(fix.at), truth)) < 0.05);
        assert!(!fix.satellites().contains(&measurements[2].prn));
    }

    #[test]
    fn three_satellites_make_no_fix() {
        let truth = ecef(Geodetic::default());
        let measurements = sky(truth, 200_000.0);
        assert!(solve(&measurements[..3]).is_none());
    }
}
