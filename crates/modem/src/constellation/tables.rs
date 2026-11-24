mod apsk;
mod gray;
mod labelling;

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use num_complex::Complex;
use sdrmm_dsp::math::complex::narrow;

pub use apsk::{ApskRing, apsk, apsk16_dvbs2, apsk32_dvbs2};
pub use gray::{gray, gray_inverse};
pub use labelling::{gray_penalty, label_by_descent};

use super::{Constellation, ConstellationError};

pub const PI_2_ROTATION: f64 = FRAC_PI_2;

pub const PI_4_ROTATION: f64 = FRAC_PI_4;

const MAX_ORDER: u32 = 1024;

/// Square QAM slices on its lattice in constant time, so it goes far past the tables that scan.
const MAX_SQUARE_QAM_ORDER: u32 = 1 << 20;

/// Side of the square grid and of each removed corner block, in points, for the cross QAMs.
const CROSS_32: (i32, i32) = (6, 1);
const CROSS_128: (i32, i32) = (12, 2);

pub fn pam(m: u32) -> Result<Constellation, ConstellationError> {
    check_order("PAM", m, true)?;
    let n = m as usize;
    let points = (0..n)
        .map(|i| Complex::new((2 * i) as f32 - (m - 1) as f32, 0.0))
        .collect();
    Constellation::from_points(points, (0..m).map(gray).collect())
}

pub fn ask(m: u32) -> Result<Constellation, ConstellationError> {
    check_order("ASK", m, true)?;
    let points = (0..m).map(|i| Complex::new(i as f32, 0.0)).collect();
    Constellation::from_points(points, (0..m).map(gray).collect())
}

#[must_use]
pub fn bpsk() -> Constellation {
    match pam(2) {
        Ok(table) => table,
        Err(_) => unreachable!("pam(2) is a valid table by construction"),
    }
}

pub fn ook() -> Result<Constellation, ConstellationError> {
    ask(2)
}

pub fn psk(m: u32) -> Result<Constellation, ConstellationError> {
    psk_rotated(m, 0.0)
}

pub fn psk_rotated(m: u32, phase_rad: f64) -> Result<Constellation, ConstellationError> {
    check_order("PSK", m, true)?;
    let points = (0..m).map(|i| polar(1.0, phase_rad, i, m)).collect();
    Constellation::from_points(points, (0..m).map(gray).collect())
}

pub fn qam_square(m: u32) -> Result<Constellation, ConstellationError> {
    let square = m >= 4 && m.ilog2().is_multiple_of(2);
    check_order_up_to("square QAM", m, square, MAX_SQUARE_QAM_ORDER)?;
    let side = 1u32 << (m.ilog2() / 2);
    let (points, labels) = gray_grid(m, |i| (2 * i) as f32 - (side - 1) as f32);
    Constellation::from_points(points, labels)
}

pub fn qam_cross(m: u32) -> Result<Constellation, ConstellationError> {
    check_order("cross QAM", m, m == 32 || m == 128)?;
    let (side, corner) = if m == 32 { CROSS_32 } else { CROSS_128 };
    let coord = |i: i32| (2 * i - (side - 1)) as f32;
    let mut points = Vec::with_capacity(m as usize);
    for qi in 0..side {
        for ii in 0..side {
            let in_corner =
                (ii < corner || ii >= side - corner) && (qi < corner || qi >= side - corner);
            if !in_corner {
                points.push(Complex::new(coord(ii), coord(qi)));
            }
        }
    }
    debug_assert_eq!(points.len(), m as usize);
    let labels = label_by_descent(&points);
    Constellation::from_points(points, labels)
}

pub fn qam_star(radii: &[f64], points_per_ring: u32) -> Result<Constellation, ConstellationError> {
    let rings = u32::try_from(radii.len()).unwrap_or(u32::MAX);
    let m = rings.saturating_mul(points_per_ring);
    let ordered = radii.first().is_some_and(|&r| r > 0.0)
        && radii.windows(2).all(|w| w[1] > w[0])
        && radii.iter().all(|r| r.is_finite());
    check_order(
        "star QAM",
        m,
        rings.is_power_of_two() && points_per_ring.is_power_of_two() && ordered,
    )?;
    let phase_bits = points_per_ring.ilog2();
    let mut points = Vec::with_capacity(m as usize);
    let mut labels = Vec::with_capacity(m as usize);
    for (ring, &radius) in radii.iter().enumerate() {
        for k in 0..points_per_ring {
            points.push(polar(radius, 0.0, k, points_per_ring));
            labels.push(gray(k) | (gray(ring as u32) << phase_bits));
        }
    }
    Constellation::from_points(points, labels)
}

pub fn qam_hierarchical(m: u32, alpha: f64) -> Result<Constellation, ConstellationError> {
    check_order(
        "hierarchical QAM",
        m,
        (m == 16 || m == 64) && alpha.is_finite() && alpha >= 1.0,
    )?;
    let quadrant = (1u32 << (m.ilog2() / 2)) / 2;
    let (points, labels) = gray_grid(m, |i| {
        let (sign, step) = if i < quadrant {
            (-1.0, f64::from(quadrant - 1 - i))
        } else {
            (1.0, f64::from(i - quadrant))
        };
        (sign * (alpha + 2.0 * step)) as f32
    });
    Constellation::from_points(points, labels)
}

#[must_use]
pub fn offset_rotation(m: u32) -> f64 {
    PI / f64::from(m.max(2))
}

fn check_order(family: &'static str, m: u32, ok: bool) -> Result<(), ConstellationError> {
    check_order_up_to(family, m, ok, MAX_ORDER)
}

fn check_order_up_to(
    family: &'static str,
    m: u32,
    ok: bool,
    max: u32,
) -> Result<(), ConstellationError> {
    if ok && m >= 2 && m.is_power_of_two() && m <= max {
        return Ok(());
    }
    Err(ConstellationError::UnsupportedOrder { family, m })
}

/// The `k`-th of `count` points spaced evenly round a ring of `radius`, starting at `phase_rad`.
fn polar(radius: f64, phase_rad: f64, k: u32, count: u32) -> Complex<f32> {
    let theta = phase_rad + TAU * f64::from(k) / f64::from(count);
    narrow(Complex::from_polar(radius, theta))
}

/// A square grid of `m` points whose rails sit at `coord(i)`, each axis Gray coded on its own.
fn gray_grid(m: u32, coord: impl Fn(u32) -> f32) -> (Vec<Complex<f32>>, Vec<u32>) {
    let half = m.ilog2() / 2;
    let side = 1u32 << half;
    let mut points = Vec::with_capacity(m as usize);
    let mut labels = Vec::with_capacity(m as usize);
    for qi in 0..side {
        for ii in 0..side {
            points.push(Complex::new(coord(ii), coord(qi)));
            labels.push(gray(ii) | (gray(qi) << half));
        }
    }
    (points, labels)
}

#[cfg(test)]
mod tests;
