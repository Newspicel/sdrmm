use num_complex::Complex;

use super::{GroupPlan, rotation};
use crate::math::complex::narrow;
use crate::radar::RadarDspError;
use crate::radar::batch::{BatchShape, WeightsAt};

type C32 = Complex<f32>;
type C64 = Complex<f64>;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MixTerm {
    pub group: usize,
    pub tap: usize,
    pub factor: C32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Blend {
    first: usize,
    second: usize,
    share: f32,
}

pub struct WeightTable {
    pub(super) lanes: usize,
    pub(super) taps: usize,
    pub(super) order: usize,
    pub(super) fft_len: usize,
    doppler_taps: usize,
    pub(super) active: bool,
    pub(super) spectra: Vec<C32>,
    weights: Vec<C64>,
    pub(super) blends: Vec<Blend>,
    phases: Vec<C32>,
}

impl WeightTable {
    pub fn new(shape: &BatchShape, plan: &GroupPlan) -> Result<Self, RadarDspError> {
        if !shape.eca() || !plan.valid_for(shape) {
            return Err(RadarDspError::Shape);
        }
        let (groups, lanes, taps, m) = (plan.groups(), shape.lanes, plan.taps(), shape.fft_len);
        let mut blends = Vec::with_capacity(shape.batches);
        let mut phases = Vec::with_capacity(shape.batches * 2 * taps);
        for batch in 0..shape.batches {
            let (first, second, share) = plan.blend(batch);
            blends.push(Blend {
                first,
                second,
                share,
            });
            for group in [first, second] {
                for tap in 0..taps {
                    let nu = tap as f64 - plan.doppler_taps as f64;
                    phases.push(narrow(rotation(nu * plan.cycles(group, batch))));
                }
            }
        }
        Ok(Self {
            lanes,
            taps,
            order: shape.order(),
            fft_len: m,
            doppler_taps: plan.doppler_taps,
            active: false,
            spectra: vec![C32::default(); groups * lanes * taps * m],
            weights: vec![C64::default(); groups * lanes * taps * shape.order()],
            blends,
            phases,
        })
    }

    #[must_use]
    pub fn at(&self, lane: usize, batch: usize) -> WeightsAt<'_> {
        WeightsAt::of(self, lane, batch)
    }

    #[must_use]
    pub fn weights(&self, group: usize, lane: usize) -> &[C64] {
        let len = self.taps * self.order;
        let at = (group * self.lanes + lane) * len;
        self.weights.get(at..at + len).unwrap_or(&[])
    }

    #[must_use]
    pub const fn doppler_taps(&self) -> usize {
        self.doppler_taps
    }

    #[must_use]
    pub fn spectra(&self) -> &[C32] {
        &self.spectra
    }

    pub fn terms(&self, batch: usize, out: &mut [MixTerm]) -> Result<(), RadarDspError> {
        let terms = out.get_mut(..2 * self.taps).ok_or(RadarDspError::Shape)?;
        let mix = self.mix(batch).ok_or(RadarDspError::Shape)?;
        for (slot, term) in terms.iter_mut().zip(mix) {
            *slot = term;
        }
        Ok(())
    }

    pub(crate) fn combine(
        &self,
        lane: usize,
        batch: usize,
        kernel: &mut [C32],
    ) -> Result<bool, RadarDspError> {
        let Some(mix) = self.mix(batch) else {
            return Err(RadarDspError::Shape);
        };
        if lane >= self.lanes || kernel.len() < self.fft_len {
            return Err(RadarDspError::Shape);
        }
        if !self.active {
            return Ok(false);
        }
        let kernel = &mut kernel[..self.fft_len];
        kernel.fill(C32::default());
        for term in mix.filter(|term| term.factor != C32::default()) {
            let spectrum = self.spectrum(term.group, lane, term.tap);
            for (out, w) in kernel.iter_mut().zip(spectrum) {
                *out += term.factor * w;
            }
        }
        Ok(true)
    }

    fn mix(&self, batch: usize) -> Option<impl Iterator<Item = MixTerm> + '_> {
        let blend = self.blends.get(batch)?;
        let taps = self.taps;
        let phases = self.phases.get(batch * 2 * taps..(batch + 1) * 2 * taps)?;
        let shares = [
            (blend.first, 1.0 - blend.share),
            (blend.second, blend.share),
        ];
        Some(
            shares
                .into_iter()
                .enumerate()
                .flat_map(move |(slot, (group, share))| {
                    (0..taps).map(move |tap| MixTerm {
                        group,
                        tap,
                        factor: if share == 0.0 {
                            C32::default()
                        } else {
                            phases[slot * taps + tap] * share
                        },
                    })
                }),
        )
    }

    fn spectrum(&self, group: usize, lane: usize, tap: usize) -> &[C32] {
        let at = ((group * self.lanes + lane) * self.taps + tap) * self.fft_len;
        &self.spectra[at..at + self.fft_len]
    }

    pub(super) fn spectrum_mut(&mut self, group: usize, lane: usize, tap: usize) -> &mut [C32] {
        let at = ((group * self.lanes + lane) * self.taps + tap) * self.fft_len;
        &mut self.spectra[at..at + self.fft_len]
    }

    pub(super) fn weights_mut(&mut self, group: usize, lane: usize) -> &mut [C64] {
        let len = self.taps * self.order;
        let at = (group * self.lanes + lane) * len;
        &mut self.weights[at..at + len]
    }
}
