use num_complex::Complex;

use super::ENERGY_FLOOR;
use crate::constellation::Constellation;

const LEVEL_TOLERANCE: f32 = 1e-3;

/// What the equaliser steers towards before its decisions can be trusted: a constant modulus
/// for ring tables, or per-axis Sato levels for tables built on a grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum BlindTarget {
    Modulus(f32),
    Axes { re: f32, im: f32 },
}

impl BlindTarget {
    pub(super) fn for_table(table: &Constellation) -> Self {
        let points = table.points();
        let moment = |part: fn(&Complex<f32>) -> f32, low: i32, high: i32| {
            let below: f32 = points.iter().map(|p| part(p).abs().powi(low)).sum();
            let above: f32 = points.iter().map(|p| part(p).abs().powi(high)).sum();
            if below > ENERGY_FLOOR {
                above / below
            } else {
                0.0
            }
        };
        if is_separable(points) {
            Self::Axes {
                re: moment(|p| p.re, 1, 2),
                im: moment(|p| p.im, 1, 2),
            }
        } else {
            Self::Modulus(moment(|p| p.norm(), 2, 4))
        }
    }

    pub(super) fn error(self, z: Complex<f32>, decision: Complex<f32>) -> Complex<f32> {
        match self {
            Self::Modulus(r2) => z * (r2 - z.norm_sqr()),
            Self::Axes { re, im } => {
                let directed = decision - z;
                let sato = Complex::new(re * z.re.signum() - z.re, im * z.im.signum() - z.im);
                Complex::new(
                    agreeing(directed.re, sato.re),
                    agreeing(directed.im, sato.im),
                )
            }
        }
    }
}

/// The decision-directed error where the blind one agrees on its sign, else nothing.
fn agreeing(directed: f32, blind: f32) -> f32 {
    if directed * blind > 0.0 {
        directed
    } else {
        0.0
    }
}

/// Whether the table is the product of its in-phase and quadrature levels.
fn is_separable(points: &[Complex<f32>]) -> bool {
    let levels = |part: fn(&Complex<f32>) -> f32| {
        let mut found: Vec<f32> = Vec::new();
        for p in points {
            let v = part(p);
            if found.iter().all(|&f| (f - v).abs() > LEVEL_TOLERANCE) {
                found.push(v);
            }
        }
        found.len()
    };
    levels(|p| p.re) * levels(|p| p.im) == points.len()
}
