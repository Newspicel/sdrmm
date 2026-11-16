mod density;
mod model;

use model::Model;

pub const MIN_ALPHA: f64 = 1e-3;
pub const MAX_ALPHA: f64 = 1e6;

const BISECTION_STEPS: usize = 60;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CfarStatistic {
    Ca,
    Os { rank: f32 },
    Go,
}

#[derive(Clone, Copy, Debug, PartialEq, thiserror::Error)]
pub enum ThresholdError {
    #[error("CFAR has too few training cells: {0}")]
    Cells(usize),
    #[error("CFAR needs at least one look")]
    Looks,
    #[error("range correlation must be at least 1, got {0}")]
    Correlation(f64),
    #[error("OS rank must be above 0 and at most 1, got {0}")]
    Rank(f32),
    #[error("false alarm rate must be between 0 and 1, got {0}")]
    Pfa(f64),
    #[error("no threshold from {MIN_ALPHA} to {MAX_ALPHA} gives a false alarm rate of {0}")]
    Unreachable(f64),
}

#[must_use]
pub fn os_order(rank: f32, cells: usize) -> usize {
    ((f64::from(rank) * cells as f64).round() as usize).clamp(1, cells.max(1))
}

#[must_use]
pub fn pfa(stat: CfarStatistic, cells: usize, alpha: f64, looks: u32, correlation: f64) -> f64 {
    if !(alpha >= 0.0 && alpha.is_finite()) {
        return f64::NAN;
    }
    Model::new(stat, cells, looks, correlation).map_or(f64::NAN, |model| model.pfa(alpha))
}

pub fn alpha(
    stat: CfarStatistic,
    cells: usize,
    pfa: f64,
    looks: u32,
    correlation: f64,
) -> Result<f64, ThresholdError> {
    if !(pfa > 0.0 && pfa < 1.0) {
        return Err(ThresholdError::Pfa(pfa));
    }
    let model = Model::new(stat, cells, looks, correlation)?;
    if model.pfa(MIN_ALPHA) < pfa || model.pfa(MAX_ALPHA) > pfa {
        return Err(ThresholdError::Unreachable(pfa));
    }
    let (mut low, mut high) = (MIN_ALPHA.ln(), MAX_ALPHA.ln());
    for _ in 0..BISECTION_STEPS {
        let middle = f64::midpoint(low, high);
        if model.pfa(middle.exp()) > pfa {
            low = middle;
        } else {
            high = middle;
        }
    }
    Ok(f64::midpoint(low, high).exp())
}

#[cfg(test)]
mod tests;
