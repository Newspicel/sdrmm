mod assess;
mod bearing;
mod config;
mod esprit;
mod estimate;
mod fit;
mod layout;
mod likelihood;
mod order;
mod peaks;
mod probe;
mod quality;
mod report;
mod root_music;
mod search;
mod setup;
mod space;
mod spectrum;
mod squelch;
mod subspace;
#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;
mod workspace;

use num_complex::Complex;

use crate::array::covariance::{CovarianceError, PhaseMode};
use crate::array::manifold::{AliasReport, Manifold, ManifoldError, SteeringGrid};
use crate::math::linalg::{
    CMat, Cholesky, Eigen, HermitianEigen, LinalgError, MAX_ORDER, MAX_POLY_DEGREE, Roots,
};

pub use bearing::{apart_deg, mirror_deg, on_side};
pub use config::{DoaConfig, Estimator, OrderRule, SourceCount, SquelchConfig, UlaSide};
pub use esprit::{EspritSolvers, MAX_ESPRIT_SOURCES, esprit};
pub use fit::joint_fit;
pub use layout::line_azimuths;
pub use likelihood::{quantize_likelihood, temper_likelihood};
pub use order::{SourceCounter, dominance_count, mdl_count};
pub use peaks::{Candidate, GridShape, MAX_CANDIDATES, local_maxima, refine};
pub use quality::{
    MAX_SIGMA_DEG, confidence, crb_sigma_rad, mismatch_share, mismatch_sigma_deg, model_sigma_rad,
    rate_spread, total_sigma_deg,
};
pub use report::{DoaReport, Peak};
pub use root_music::root_music;
pub use space::{
    ARRAY_CHANGED, ELEVATION_NEEDS_2D, ELEVATION_NEEDS_GRID, FB_NEEDS_SYMMETRY,
    LOADING_OUT_OF_RANGE, NEEDS_LINE_OR_CIRCLE, PEAK_RANGE_OUT_OF_RANGE, PEAKS_OUT_OF_RANGE,
    SIDE_NEEDS_LINE, SQUELCH_OUT_OF_RANGE, TABLE_NEEDS_GRID, TOO_FEW_ELEMENTS, TOO_MANY_SOURCES,
};
pub use spectrum::{Pseudospectrum, bartlett, capon, music};
pub use squelch::Squelch;

use layout::Layout;
use space::Space;
use workspace::Workspace;

pub const MAX_PEAKS: usize = 4;
pub const LIKELIHOOD_POINTS: usize = 360;
pub const LIKELIHOOD_FLOOR: f32 = 0.02;
pub const CONFIDENCE_WINDOW_DEG: f64 = 5.0;
pub const SIGMA_FLOOR_DEG: f64 = 2.0;
pub const DEFAULT_CAL_SIGMA_DEG: f32 = 3.0;
pub const ANTENNA_PHASE_SIGMA_DEG: f64 = 5.0;
pub const MEASURED_PHASE_SIGMA_DEG: f64 = 2.0;
pub const NOISE_ABOVE_DB: f32 = 6.0;
pub const DOMINANT_WITHIN_DB: f32 = 12.0;
pub const STABLE_REPORTS: u8 = 3;

const RING_STEP_DEG: f64 = 1.0;
const MIRROR_APART_DEG: f64 = 1.0;
const ROOT_RADIUS_SLACK: f64 = 0.1;
const LIKELIHOOD_WINDOW_DEG: f64 = 10.0;
const LIKELIHOOD_COARSE_DEG: f64 = 0.5;
const CURVATURE_STEP_DEG: f64 = 0.25;
const EIGEN_FLOOR: f32 = 1e-12;
const DETECTION_SIGMAS: f64 = 3.0;
const MIN_SHARE: f64 = 1e-12;
const MIN_SNR_DB: f32 = -60.0;

type C32 = Complex<f32>;
type C64 = Complex<f64>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reconfigure {
    InPlace,
    NeedsNew,
}

#[derive(Clone, Copy, Debug, PartialEq, thiserror::Error)]
pub enum DoaError {
    #[error(transparent)]
    Linalg(#[from] LinalgError),
    #[error(transparent)]
    Manifold(#[from] ManifoldError),
    #[error(transparent)]
    Covariance(#[from] CovarianceError),
    #[error("{0}")]
    Unsupported(&'static str),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Found {
    azimuth_deg: f64,
    elevation_deg: f64,
    fit_scale: f32,
    power: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Stats {
    order: usize,
    sources: usize,
    noise: f32,
    snapshots: f64,
}

struct Solvers {
    eigen: HermitianEigen,
    decomposition: Eigen,
    loaded: CMat,
    projector: CMat,
    working_chol: Cholesky,
    element_chol: Cholesky,
    fit_chol: Cholesky,
    roots: Roots,
    esprit: EspritSolvers,
    coeffs: [C64; MAX_POLY_DEGREE + 1],
    found: [C64; MAX_POLY_DEGREE],
}

impl Solvers {
    fn new(elements: usize) -> Result<Self, LinalgError> {
        Ok(Self {
            eigen: HermitianEigen::new(elements)?,
            decomposition: Eigen::new(),
            loaded: CMat::identity(elements)?,
            projector: CMat::identity(elements)?,
            working_chol: Cholesky::new(MAX_ORDER)?,
            element_chol: Cholesky::new(MAX_ORDER)?,
            fit_chol: Cholesky::new(MAX_ORDER)?,
            roots: Roots::new(MAX_POLY_DEGREE)?,
            esprit: EspritSolvers::new()?,
            coeffs: [C64::new(0.0, 0.0); MAX_POLY_DEGREE + 1],
            found: [C64::new(0.0, 0.0); MAX_POLY_DEGREE],
        })
    }
}

pub struct Doa {
    config: DoaConfig,
    freq_hz: f64,
    manifold: Manifold,
    layout: Layout,
    phase: Option<PhaseMode>,
    phase_fault: Option<CovarianceError>,
    plan: Result<Space, DoaError>,
    grid: SteeringGrid,
    ring: SteeringGrid,
    alias: AliasReport,
    workspace: Workspace,
    solvers: Solvers,
    counter: SourceCounter,
    squelch: Squelch,
    spectrum: Vec<f32>,
}
