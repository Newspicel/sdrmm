#[cfg(test)]
use super::kernel::{Accumulate, Plan};

#[cfg(test)]
pub(super) fn block<T, C, const FOLD: bool>(window: &[T], plan: &Plan<C>, out: &mut [T])
where
    T: Accumulate<C>,
    C: Copy,
{
    for (lane, sample) in out.iter_mut().enumerate() {
        *sample = plan.taps().fold(T::zero(), |sum, (front, back, value)| {
            let front = window[front / T::FLOATS + lane];
            let pair = if FOLD {
                front + window[back / T::FLOATS + lane]
            } else {
                front
            };
            sum.add_product(pair, value)
        });
    }
}

#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
mod fallback {
    use num_complex::Complex;

    use crate::fir::kernel::{Plan, interpolated_plane_tail, plane_tail};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) struct Isa;

    fn pair<const FOLD: bool>(floats: &[f32], front: usize, back: usize) -> f32 {
        if FOLD {
            floats[front] + floats[back]
        } else {
            floats[front]
        }
    }

    impl Isa {
        pub(crate) fn detect() -> Self {
            Self
        }

        #[cfg(test)]
        pub(crate) fn available() -> Vec<Self> {
            vec![Self]
        }

        pub(crate) fn real_blocks<const FOLD: bool>(
            self,
            floats: &[f32],
            plan: &Plan<f32>,
            out: &mut [f32],
        ) {
            for (lane, sample) in out.iter_mut().enumerate() {
                *sample = plan.taps().fold(0.0, |sum, (front, back, value)| {
                    sum + pair::<FOLD>(floats, front + lane, back + lane) * value
                });
            }
        }

        pub(crate) fn complex_real_blocks<const FOLD: bool>(
            self,
            floats: &[f32],
            plan: &Plan<f32>,
            out: &mut [f32],
        ) {
            self.real_blocks::<FOLD>(floats, plan, out);
        }

        pub(crate) fn complex_blocks<const FOLD: bool>(
            self,
            floats: &[f32],
            plan: &Plan<Complex<f32>>,
            out: &mut [f32],
        ) {
            for (lane, out) in out.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                let sum = plan
                    .taps()
                    .fold(Complex::new(0.0, 0.0), |sum, (front, back, value)| {
                        let (front, back) = (front + 2 * lane, back + 2 * lane);
                        let sample = Complex::new(
                            pair::<FOLD>(floats, front, back),
                            pair::<FOLD>(floats, front + 1, back + 1),
                        );
                        sum + sample * value
                    });
                *out = [sum.re, sum.im];
            }
        }

        pub(crate) unsafe fn plane_dot<const N: usize>(
            self,
            re: [*const f32; N],
            im: [*const f32; N],
            taps: &[f32],
        ) -> [Complex<f32>; N] {
            std::array::from_fn(|output| unsafe {
                plane_tail(Complex::new(0.0, 0.0), re[output], im[output], taps, 0)
            })
        }

        pub(crate) fn plane_interpolated(
            self,
            re: &[f32],
            im: &[f32],
            lower: &[f32],
            slope: &[f32],
            mu: f32,
        ) -> Complex<f32> {
            interpolated_plane_tail(Complex::new(0.0, 0.0), re, im, lower, slope, mu)
        }
    }
}

#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
pub(crate) use fallback::Isa;
