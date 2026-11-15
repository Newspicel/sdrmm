use super::RHO_TABLE;
use crate::radar::threshold::{CfarStatistic, MAX_ALPHA, ThresholdError, alpha};

/// CFAR factors designed ahead for every look count and range correlation in `RHO_TABLE`.
pub struct AlphaTable {
    looks: usize,
    alpha: Vec<f32>,
    edge: Vec<f32>,
}

impl AlphaTable {
    pub fn new(
        stat: CfarStatistic,
        cells: usize,
        edge_cells: usize,
        pfa: f64,
        max_looks: u32,
    ) -> Result<Self, ThresholdError> {
        let looks = max_looks.max(1);
        let mut table = Self {
            looks: looks as usize,
            alpha: Vec::with_capacity(looks as usize * RHO_TABLE.len()),
            edge: Vec::with_capacity(looks as usize * RHO_TABLE.len()),
        };
        for look in 1..=looks {
            for rho in RHO_TABLE {
                table.alpha.push(design(stat, cells, pfa, look, rho)?);
                table
                    .edge
                    .push(design(CfarStatistic::Ca, edge_cells, pfa, look, rho)?);
            }
        }
        Ok(table)
    }

    #[must_use]
    pub fn pick(&self, looks: u32, correlation: f64) -> (f32, f32) {
        let look = (looks.max(1) as usize).min(self.looks) - 1;
        let row = look * RHO_TABLE.len();
        let column = RHO_TABLE
            .iter()
            .position(|&rho| rho >= correlation)
            .unwrap_or(RHO_TABLE.len() - 1);
        (self.alpha[row + column], self.edge[row + column])
    }
}

fn design(
    stat: CfarStatistic,
    cells: usize,
    pfa: f64,
    looks: u32,
    correlation: f64,
) -> Result<f32, ThresholdError> {
    match alpha(stat, cells, pfa, looks, correlation) {
        Ok(value) => Ok(value as f32),
        Err(ThresholdError::Unreachable(_)) if correlation > 1.0 => Ok(MAX_ALPHA as f32),
        Err(error) => Err(error),
    }
}
