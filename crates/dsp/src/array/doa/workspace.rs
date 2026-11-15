use num_complex::Complex;

use super::DoaError;
use super::layout::Layout;
use super::space::{FB_NEEDS_SYMMETRY, NEEDS_LINE_OR_CIRCLE, Space, unsupported};
use crate::array::covariance::{
    CovarianceError, PhaseMode, forward_backward, smooth, smooth_diagonal,
};
use crate::array::manifold::{MAX_ELEMENTS, Permutation};
use crate::math::complex::narrow;
use crate::math::linalg::{CMat, LinalgError};

pub(super) struct Workspace {
    pub work: CMat,
    staging: CMat,
    whiten: [f32; MAX_ELEMENTS],
    diagonal: [f32; MAX_ELEMENTS],
}

impl Workspace {
    pub fn new(elements: usize) -> Result<Self, LinalgError> {
        Ok(Self {
            work: CMat::identity(elements)?,
            staging: CMat::identity(elements)?,
            whiten: [1.0; MAX_ELEMENTS],
            diagonal: [1.0; MAX_ELEMENTS],
        })
    }

    pub fn prepare(
        &mut self,
        space: Space,
        fb: bool,
        layout: &Layout,
        phase: Option<&PhaseMode>,
        r: &CMat,
    ) -> Result<usize, DoaError> {
        match space {
            Space::Element => {
                self.work.clone_from(r);
                if fb {
                    let pairs = layout
                        .antipodes
                        .as_ref()
                        .ok_or(unsupported(FB_NEEDS_SYMMETRY))?;
                    forward_backward(&mut self.work, pairs)?;
                }
            }
            Space::Line { subarrays } => {
                let line = layout.line.ok_or(unsupported(NEEDS_LINE_OR_CIRCLE))?;
                smooth(r, &line.order, subarrays, fb, &mut self.work)?;
            }
            Space::Modes => {
                let phase = phase.ok_or(unsupported(NEEDS_LINE_OR_CIRCLE))?;
                phase.transform(r, &mut self.work)?;
                if fb {
                    let reversal = Permutation::reversal(self.work.order());
                    forward_backward(&mut self.work, &reversal)?;
                }
            }
            Space::Vandermonde { subarrays } => {
                let phase = phase.ok_or(unsupported(NEEDS_LINE_OR_CIRCLE))?;
                self.prepare_vandermonde(phase, subarrays, fb, r)?;
            }
        }
        self.whiten_unless(space);
        Ok(self.work.order())
    }

    fn prepare_vandermonde(
        &mut self,
        phase: &PhaseMode,
        subarrays: usize,
        fb: bool,
        r: &CMat,
    ) -> Result<(), DoaError> {
        phase.transform_vandermonde(r, &mut self.staging)?;
        let full = self.staging.order();
        smooth(
            &self.staging,
            &Permutation::identity(full),
            subarrays,
            fb,
            &mut self.work,
        )?;
        let len = smooth_diagonal(phase.noise_diagonal(), subarrays, fb, &mut self.diagonal);
        if len != self.work.order() {
            return Err(CovarianceError::TooFewForSmoothing.into());
        }
        for (weight, &noise) in self.whiten.iter_mut().zip(&self.diagonal[..len]) {
            *weight = if noise > 0.0 {
                noise.sqrt().recip()
            } else {
                0.0
            };
        }
        for row in 0..len {
            for col in 0..len {
                let scale = self.whiten[row] * self.whiten[col];
                self.work.set(row, col, self.work.get(row, col) * scale);
            }
        }
        Ok(())
    }

    fn whiten_unless(&mut self, space: Space) {
        if !matches!(space, Space::Vandermonde { .. }) {
            self.whiten.fill(1.0);
        }
    }

    pub fn order(&self) -> usize {
        self.work.order()
    }

    pub fn weights<'a>(&'a self, space: Space, phase: Option<&'a PhaseMode>) -> Option<&'a [f32]> {
        let m = self.order();
        match space {
            Space::Element | Space::Line { .. } => None,
            Space::Modes => phase.map(PhaseMode::mode_magnitudes),
            Space::Vandermonde { .. } => Some(&self.whiten[..m]),
        }
    }

    pub fn whitening(&self) -> &[f32] {
        &self.whiten[..self.order()]
    }

    pub fn steer(
        &self,
        space: Space,
        layout: &Layout,
        phase: Option<&PhaseMode>,
        element: &[Complex<f32>],
        azimuth_deg: f64,
        out: &mut [Complex<f32>],
    ) -> usize {
        let m = self.order();
        let out = &mut out[..m];
        match (space, layout.line) {
            (Space::Element, _) => out.copy_from_slice(&element[..m]),
            (Space::Line { .. }, Some(line)) => {
                for (i, slot) in out.iter_mut().enumerate() {
                    *slot = element[line.order.get(i)];
                }
            }
            (Space::Line { .. }, None) => out.fill(Complex::new(0.0, 0.0)),
            (Space::Modes | Space::Vandermonde { .. }, _) => {
                let weights = self.weights(space, phase).unwrap_or(&[]);
                vandermonde(azimuth_deg, weights, out);
            }
        }
        m
    }
}

fn vandermonde(azimuth_deg: f64, weights: &[f32], out: &mut [Complex<f32>]) {
    let turn = Complex::from_polar(1.0f64, azimuth_deg.to_radians());
    let mut power = Complex::new(1.0f64, 0.0);
    for (slot, index) in out.iter_mut().zip(0..) {
        let weight = f64::from(weights.get(index).copied().unwrap_or(0.0));
        *slot = narrow(power * weight);
        power *= turn;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array::doa::testing::uca_layout as uca;

    #[test]
    fn working_steering_is_vandermonde_on_a_circle() {
        let (layout, phase) = uca(8, 2.0);
        let mut workspace = Workspace::new(8).unwrap();
        let r = CMat::identity(8).unwrap();
        let space = Space::Vandermonde { subarrays: 1 };
        let m = workspace
            .prepare(space, false, &layout, Some(&phase), &r)
            .unwrap();
        assert_eq!(m, 7);
        for i in 0..m {
            assert!((workspace.work.get(i, i).re - 1.0).abs() < 1e-4);
        }
        let mut out = [Complex::new(0.0f32, 0.0); 16];
        let written = workspace.steer(space, &layout, Some(&phase), &[], 30.0, &mut out);
        assert_eq!(written, 7);
        let ratio = out[1] / out[0] * workspace.whitening()[0] / workspace.whitening()[1];
        assert!((ratio.arg() - 30f32.to_radians()).abs() < 1e-5);
    }
}
