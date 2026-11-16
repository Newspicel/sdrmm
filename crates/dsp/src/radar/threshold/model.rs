use super::density::{
    closed_pfa, greatest_density, ln_unit_mean_gamma_pdf, order_density, survival,
};
use super::{BISECTION_STEPS, CfarStatistic, ThresholdError, os_order};
use crate::math::special::gamma::{gamma_p, ln_gamma};

const Z_MIN: f64 = 1e-18;
pub(super) const Z_MAX: f64 = 40.0;
const INTERVALS: usize = 4096;
const SPREAD_WIDTHS: f64 = 40.0;

/// Probability of false alarm of one CFAR statistic as a function of its factor: closed form
/// for cell averaging, a Simpson integral over the noise estimate's density otherwise.
pub(super) enum Model {
    Closed {
        looks: u32,
        shape: f64,
        cells: f64,
    },
    Integral {
        looks: u32,
        nodes: Vec<f64>,
        weights: Vec<f64>,
    },
}

impl Model {
    pub(super) fn new(
        stat: CfarStatistic,
        cells: usize,
        looks: u32,
        correlation: f64,
    ) -> Result<Self, ThresholdError> {
        if cells == 0 {
            return Err(ThresholdError::Cells(cells));
        }
        if looks == 0 {
            return Err(ThresholdError::Looks);
        }
        if !(correlation >= 1.0 && correlation.is_finite()) {
            return Err(ThresholdError::Correlation(correlation));
        }
        let effective = (cells as f64 / correlation).max(1.0);
        let per_cell = f64::from(looks);
        match stat {
            CfarStatistic::Ca => Ok(Self::Closed {
                looks,
                shape: effective * per_cell,
                cells: effective,
            }),
            CfarStatistic::Os { rank } => {
                if !(rank > 0.0 && rank <= 1.0) {
                    return Err(ThresholdError::Rank(rank));
                }
                let order = (os_order(rank, cells) as f64 / correlation).max(1.0);
                let ln_choose = ln_gamma(effective + 1.0)
                    - ln_gamma(order + 1.0)
                    - ln_gamma(effective - order + 1.0);
                Ok(Self::integral(
                    looks,
                    order_span(effective, order, per_cell),
                    |z| order_density(z, effective, order, per_cell, ln_choose),
                ))
            }
            CfarStatistic::Go => {
                if cells < 2 {
                    return Err(ThresholdError::Cells(cells));
                }
                let half = (0.5 * effective).max(1.0) * per_cell;
                Ok(Self::integral(looks, span(1.0, half.sqrt().recip()), |z| {
                    greatest_density(z, half)
                }))
            }
        }
    }

    fn integral(looks: u32, span: (f64, f64), density: impl Fn(f64) -> f64) -> Self {
        let low = span.0.max(Z_MIN).ln();
        let step = (span.1.ln() - low) / INTERVALS as f64;
        let (nodes, weights) = (0..=INTERVALS)
            .map(|index| {
                let z = (low + index as f64 * step).exp();
                (z, simpson_coefficient(index) * step / 3.0 * z * density(z))
            })
            .unzip();
        Self::Integral {
            looks,
            nodes,
            weights,
        }
    }

    pub(super) fn pfa(&self, alpha: f64) -> f64 {
        match self {
            Self::Closed {
                looks,
                shape,
                cells,
            } => closed_pfa(*looks, *shape, alpha / cells),
            Self::Integral {
                looks,
                nodes,
                weights,
            } => nodes
                .iter()
                .zip(weights)
                .map(|(z, weight)| weight * survival(*looks, alpha * z))
                .sum(),
        }
    }
}

fn span(center: f64, spread: f64) -> (f64, f64) {
    if !(spread > 0.0 && spread.is_finite() && center.is_finite()) {
        return (0.0, Z_MAX);
    }
    (
        (center - SPREAD_WIDTHS * spread).max(0.0),
        (center + SPREAD_WIDTHS * spread).min(Z_MAX),
    )
}

fn order_span(cells: f64, order: f64, looks: f64) -> (f64, f64) {
    let fraction = order / (cells + 1.0);
    let center = quantile(looks, fraction);
    let density = ln_unit_mean_gamma_pdf(looks, center).exp();
    span(
        center,
        (fraction * (1.0 - fraction) / (cells + 2.0)).sqrt() / density,
    )
}

fn quantile(looks: f64, fraction: f64) -> f64 {
    let (mut low, mut high) = (0.0, Z_MAX);
    for _ in 0..BISECTION_STEPS {
        let middle = f64::midpoint(low, high);
        if gamma_p(looks, looks * middle) < fraction {
            low = middle;
        } else {
            high = middle;
        }
    }
    f64::midpoint(low, high)
}

fn simpson_coefficient(index: usize) -> f64 {
    if index == 0 || index == INTERVALS {
        1.0
    } else if index % 2 == 1 {
        4.0
    } else {
        2.0
    }
}
