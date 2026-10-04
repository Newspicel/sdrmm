use num_complex::Complex;

use super::kernel::{BLOCK_FLOATS, Plan};

mod avx2;
mod avx512;
mod sse2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Isa {
    Sse2,
    Avx2,
    Avx512,
}

fn avx2_available() -> bool {
    is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma")
}

fn avx512_at_full_clock() -> bool {
    is_x86_feature_detected!("avx512f") && is_x86_feature_detected!("avx512vbmi2")
}

impl Isa {
    pub(crate) fn detect() -> Self {
        if avx512_at_full_clock() {
            Self::Avx512
        } else if avx2_available() {
            Self::Avx2
        } else {
            Self::Sse2
        }
    }

    #[cfg(test)]
    pub(crate) fn available() -> Vec<Self> {
        [
            (Self::Sse2, true),
            (Self::Avx2, avx2_available()),
            (Self::Avx512, avx512_at_full_clock()),
        ]
        .into_iter()
        .filter_map(|(isa, available)| available.then_some(isa))
        .collect()
    }

    pub(crate) fn real_block<const FOLD: bool>(
        self,
        floats: &[f32],
        plan: &Plan<f32>,
    ) -> [f32; BLOCK_FLOATS] {
        match self {
            Self::Sse2 => unsafe { sse2::real_block::<FOLD>(floats, plan) },
            Self::Avx2 => unsafe { avx2::real_block::<FOLD>(floats, plan) },
            Self::Avx512 => unsafe { avx512::real_block::<FOLD>(floats, plan) },
        }
    }

    pub(crate) fn complex_real_block<const FOLD: bool>(
        self,
        floats: &[f32],
        plan: &Plan<f32>,
    ) -> [f32; BLOCK_FLOATS] {
        self.real_block::<FOLD>(floats, plan)
    }

    pub(crate) fn complex_block<const FOLD: bool>(
        self,
        floats: &[f32],
        plan: &Plan<Complex<f32>>,
    ) -> [f32; BLOCK_FLOATS] {
        match self {
            Self::Sse2 => unsafe { sse2::complex_block::<FOLD>(floats, plan) },
            Self::Avx2 => unsafe { avx2::complex_block::<FOLD>(floats, plan) },
            Self::Avx512 => unsafe { avx512::complex_block::<FOLD>(floats, plan) },
        }
    }

    pub(crate) unsafe fn plane_dot<const N: usize>(
        self,
        re: [*const f32; N],
        im: [*const f32; N],
        taps: &[f32],
    ) -> [Complex<f32>; N] {
        match self {
            Self::Sse2 => unsafe { sse2::plane_dot(re, im, taps) },
            Self::Avx2 => unsafe { avx2::plane_dot(re, im, taps) },
            Self::Avx512 => unsafe { avx512::plane_dot(re, im, taps) },
        }
    }

    pub(crate) fn plane_interpolated(
        self,
        re: &[f32],
        im: &[f32],
        lower: &[f32],
        slope: &[f32],
        mu: f32,
    ) -> Complex<f32> {
        match self {
            Self::Sse2 => unsafe { sse2::plane_interpolated(re, im, lower, slope, mu) },
            Self::Avx2 => unsafe { avx2::plane_interpolated(re, im, lower, slope, mu) },
            Self::Avx512 => unsafe { avx512::plane_interpolated(re, im, lower, slope, mu) },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Isa;

    #[test]
    fn every_isa_is_tested_where_the_runner_promises_it() {
        if std::env::var_os("SDRMM_REQUIRE_AVX512").is_some() {
            assert_eq!(Isa::available(), [Isa::Sse2, Isa::Avx2, Isa::Avx512]);
        }
    }
}
