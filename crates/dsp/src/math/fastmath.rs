mod atan;
#[cfg(target_arch = "x86_64")]
mod avx;
mod exp;
mod log;
mod phase_diff;

pub use atan::{fast_arg, fast_atan2};
pub use exp::{fast_db_to_power, fast_exp2};
pub use log::{fast_log2, fast_log10, fast_power_db};
pub use phase_diff::phase_diff_into;

/// Whether the target fuses a multiply and an add in one instruction.
pub(crate) const NATIVE_FMA: bool = cfg!(any(target_arch = "aarch64", target_feature = "fma"));
const MANTISSA_BITS: u32 = 23;

/// `a * b + c`, fused only where the target has the instruction. Without it `mul_add` becomes a
/// software routine that costs tens of nanoseconds per call on x86.
#[must_use]
#[inline(always)]
pub fn madd(a: f32, b: f32, c: f32) -> f32 {
    if NATIVE_FMA {
        a.mul_add(b, c)
    } else {
        a * b + c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn madd_matches_multiply_then_add_within_two_roundings() {
        for (a, b, c) in [(1.5f32, -2.25, 0.75), (1e-3, 3e4, -29.9), (0.1, 0.1, -0.01)] {
            let product = f64::from(a) * f64::from(b);
            let exact = product + f64::from(c);
            let got = f64::from(madd(a, b, c));
            let bound = (product.abs() + exact.abs()) * f64::from(f32::EPSILON);
            assert!(
                (got - exact).abs() <= bound,
                "{a} * {b} + {c}: {got} vs {exact}"
            );
        }
    }
}
