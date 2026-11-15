mod angle;
mod bessel;
mod erf;
mod exp_integral;
pub mod gamma;
mod optimize;
mod sinc;

pub use angle::{circular_mean_deg, norm_deg, wrap_deg, wrap_rad};
pub use bessel::{SpecialError, bessel_i0, bessel_j};
pub use erf::erf;
pub use exp_integral::exponential_integral;
pub use optimize::{brent_max, parabolic_peak};
pub use sinc::sinc;
