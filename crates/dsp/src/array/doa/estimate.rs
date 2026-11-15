use super::probe::Probe;
use super::setup::usable;
use super::space::Space;
use super::{
    Doa, DoaError, DoaReport, EIGEN_FLOOR, Estimator, Found, MAX_PEAKS, Pseudospectrum,
    SourceCount, Stats,
};
use crate::array::covariance::load_diagonal;
use crate::array::manifold::Manifold;
use crate::math::db::Decibels;
use crate::math::linalg::{CMat, HermitianEigen, MAX_ORDER};

impl Doa {
    pub fn estimate(
        &mut self,
        manifold: &Manifold,
        r: &CMat,
        snapshots: f64,
        cal_sigma_deg: f32,
        out: &mut DoaReport,
    ) -> Result<(), DoaError> {
        let space = self.plan?;
        self.check_input(manifold, r)?;
        self.workspace.prepare(
            space,
            self.config.forward_backward,
            &self.layout,
            usable(self.phase.as_ref(), self.phase_fault),
            r,
        )?;
        let delta = self.decompose()?;
        *out = DoaReport {
            snapshots,
            ..DoaReport::default()
        };
        let stats = self.statistics(delta, snapshots, out);
        out.loading_used = self.fill_spectrum(manifold, space, stats)?;
        let mut found = [Found::default(); MAX_PEAKS];
        let count = if out.squelch_open {
            self.solvers
                .element_chol
                .factor_loaded(r, self.config.loading)?;
            self.find(manifold, space, stats, &mut found)?
        } else {
            0
        };
        self.assess(
            manifold,
            r,
            stats,
            f64::from(cal_sigma_deg),
            &found[..count],
            out,
        )
    }

    pub(super) fn decompose(&mut self) -> Result<f32, DoaError> {
        let solvers = &mut self.solvers;
        let work = &self.workspace.work;
        let m = work.order();
        solvers.loaded.clone_from(work);
        let delta = load_diagonal(&mut solvers.loaded, self.config.loading);
        if solvers.eigen.order() != m {
            solvers.eigen = HermitianEigen::new(m)?;
        }
        solvers
            .eigen
            .solve(&solvers.loaded, &mut solvers.decomposition)?;
        Ok(delta)
    }

    pub(super) fn statistics(&mut self, delta: f32, snapshots: f64, out: &mut DoaReport) -> Stats {
        let m = self.workspace.order();
        let floor = EIGEN_FLOOR * self.workspace.work.trace_re();
        let values = self.solvers.decomposition.values();
        let mut desc = [0.0f32; MAX_ORDER];
        for (k, slot) in desc.iter_mut().enumerate().take(m) {
            *slot = (values[m - 1 - k] - delta).max(floor);
        }
        let desc = &desc[..m];
        let (adopted, raw) = match self.config.sources {
            SourceCount::Fixed(d) => (usize::from(d), usize::from(d)),
            SourceCount::Auto => self.counter.update(self.config.rule, desc, snapshots),
        };
        let sources = adopted.min(m - 1);
        let noise = desc[sources..].iter().sum::<f32>() / (m - sources) as f32;
        let rest = desc[1..].iter().sum::<f32>() / (m - 1) as f32;
        out.eigenvalues[..m].copy_from_slice(desc);
        out.order = m;
        out.sources = sources;
        out.sources_raw = raw.min(m - 1);
        out.noise_power = noise;
        out.eig_ratio_db = (desc[0] / rest).power_to_db();
        out.lambda12_db = (desc[0] / desc[1]).power_to_db();
        out.squelch_open = self.squelch.update(self.config.squelch, out.eig_ratio_db);
        Stats {
            order: m,
            sources,
            noise,
            snapshots,
        }
    }

    pub(super) fn fill_spectrum(
        &mut self,
        manifold: &Manifold,
        space: Space,
        stats: Stats,
    ) -> Result<f32, DoaError> {
        let mut loading_used = self.config.loading;
        if self.config.estimator == Estimator::Capon {
            let mean = self.workspace.work.trace_re() / stats.order as f32;
            let absolute = self
                .solvers
                .working_chol
                .factor_loaded(&self.workspace.work, self.config.loading)?;
            loading_used = if mean > 0.0 { absolute / mean } else { 0.0 };
        }
        let mut spectrum = std::mem::take(&mut self.spectrum);
        let probe = self.probe(manifold, space, stats);
        for (point, value) in spectrum.iter_mut().enumerate().take(self.grid.points()) {
            *value = probe.at_point(&self.grid, point);
        }
        self.spectrum = spectrum;
        Ok(loading_used)
    }

    pub(super) fn probe<'a>(
        &'a self,
        manifold: &'a Manifold,
        space: Space,
        stats: Stats,
    ) -> Probe<'a> {
        let pseudo = match self.config.estimator {
            Estimator::Bartlett => Pseudospectrum::Bartlett(&self.workspace.work),
            Estimator::Capon => Pseudospectrum::Capon(&self.solvers.working_chol),
            Estimator::Music | Estimator::RootMusic | Estimator::Esprit => Pseudospectrum::Music {
                eigen: &self.solvers.decomposition,
                signals: stats.sources.max(1),
            },
        };
        Probe {
            pseudo,
            space,
            workspace: &self.workspace,
            layout: &self.layout,
            phase: usable(self.phase.as_ref(), self.phase_fault),
            manifold,
            freq_hz: self.freq_hz,
        }
    }

    pub(super) fn wanted(&self, stats: Stats) -> usize {
        stats
            .sources
            .max(1)
            .min(usize::from(self.config.max_peaks))
            .min(MAX_PEAKS)
    }

    pub(super) fn find(
        &mut self,
        manifold: &Manifold,
        space: Space,
        stats: Stats,
        out: &mut [Found; MAX_PEAKS],
    ) -> Result<usize, DoaError> {
        match self.config.estimator {
            Estimator::Bartlett | Estimator::Capon | Estimator::Music => {
                Ok(self.grid_peaks(manifold, space, stats, out))
            }
            Estimator::RootMusic | Estimator::Esprit => {
                self.subspace_peaks(manifold, space, stats, out)
            }
        }
    }
}
