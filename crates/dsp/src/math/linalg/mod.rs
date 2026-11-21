mod cholesky;
mod cholesky64;
mod cmat;
mod jacobi;
mod levinson;
mod qr;
mod roots;
mod schur;
#[cfg(test)]
pub(crate) mod testing;
mod vector;

pub use cholesky::Cholesky;
pub use cholesky64::{cholesky_c64, cholesky_solve_c64};
pub use cmat::CMat;
pub use jacobi::{Eigen, HermitianEigen};
pub use levinson::Toeplitz;
pub use qr::Qr;
pub use roots::Roots;
pub use schur::GeneralEigen;
pub use vector::{inner, norm_sqr};

pub const MAX_ORDER: usize = 16;
pub const MAX_SOLVE_ORDER: usize = 512;
pub const MAX_SCHUR_ORDER: usize = 32;
pub const MAX_POLY_DEGREE: usize = 2 * MAX_ORDER - 2;

#[derive(Clone, Copy, Debug, PartialEq, thiserror::Error)]
pub enum LinalgError {
    #[error("matrix order {0} is outside what this solver handles")]
    Order(usize),
    #[error("matrix is not positive definite at row {0}")]
    NotPositiveDefinite(usize),
    #[error("matrix is rank deficient at column {0}")]
    RankDeficient(usize),
    #[error("no factorization to solve with")]
    NotFactored,
    #[error("matrix holds a non-finite value")]
    NonFinite,
    #[error("no convergence after {0} iterations")]
    NotConverged(u32),
    #[error("polynomial has no roots to find")]
    Degenerate,
}
