use std::f32::consts::FRAC_1_SQRT_2;
use std::f64::consts::TAU;

use num_complex::Complex;

use super::batch::{BatchKernel, BatchShape, DopplerTaper, WeightsAt};
use super::wiener::{GroupPlan, GroupSums, SolveStats, WeightTable, WienerSolver};
use crate::math::rng::Rng;

type C32 = Complex<f32>;

pub(crate) type Path = (usize, C32);

pub(crate) fn unit_noise(rng: &mut Rng) -> C32 {
    rng.normal_pair(FRAC_1_SQRT_2)
}

pub(crate) fn fm_reference(len: usize, seed: u64) -> Vec<C32> {
    let mut rng = Rng::mixed(seed);
    let mut phase = 0.0f64;
    let mut drive = 0.0f64;
    (0..len)
        .map(|_| {
            drive = 0.995 * drive + 0.1 * (rng.uniform() - 0.5);
            phase += drive;
            C32::new(phase.cos() as f32, phase.sin() as f32)
        })
        .collect()
}

pub(crate) fn direct_paths() -> Vec<Path> {
    vec![
        (0, C32::new(3.0, 1.0)),
        (1, C32::new(-0.8, 0.5)),
        (4, C32::new(0.3, -0.6)),
        (9, C32::new(0.1, 0.2)),
    ]
}

pub(crate) fn channel(
    reference: &[C32],
    paths: &[Path],
    drift_hz: f64,
    fs: f64,
    thermal: f32,
    seed: u64,
) -> Vec<C32> {
    let mut rng = Rng::mixed(seed);
    (0..reference.len())
        .map(|n| {
            let direct: C32 = paths
                .iter()
                .filter(|(delay, _)| n >= *delay)
                .map(|(delay, gain)| reference[n - delay] * gain)
                .sum();
            let turn = C32::from_polar(1.0, (TAU * drift_hz * n as f64 / fs) as f32);
            direct * turn + unit_noise(&mut rng) * thermal
        })
        .collect()
}

pub(crate) struct Spectra {
    pub(crate) models: Vec<C32>,
    pub(crate) products: Vec<C32>,
    pub(crate) energy: Vec<f64>,
}

pub(crate) fn spectra_of(kernel: &mut BatchKernel, lanes: &[Vec<C32>]) -> Spectra {
    let shape = kernel.shape();
    let (m, nb, k) = (shape.fft_len, shape.batches, shape.lanes);
    let mut spectrum = vec![C32::default(); m];
    let mut out = Spectra {
        models: vec![C32::default(); nb * m],
        products: vec![C32::default(); nb * k * m],
        energy: vec![0.0; nb * k],
    };
    for b in 0..nb {
        kernel
            .reference(
                &lanes[0],
                b,
                &mut spectrum,
                &mut out.models[b * m..(b + 1) * m],
            )
            .unwrap();
        for lane in 0..k {
            let at = (b * k + lane) * m;
            kernel
                .surveillance(
                    &lanes[lane + 1],
                    b,
                    &spectrum,
                    &mut out.products[at..at + m],
                )
                .unwrap();
            out.energy[b * k + lane] = shape.energy(&lanes[lane + 1], b);
        }
    }
    out
}

pub(crate) struct Caf {
    pub(crate) gates: Vec<C32>,
    pub(crate) cube: Vec<C32>,
    pub(crate) stats: Option<SolveStats>,
    pub(crate) table: Option<WeightTable>,
}

impl Caf {
    pub(crate) fn gate(&self, shape: &BatchShape, lane: usize, batch: usize, tau: usize) -> C32 {
        self.gates[(lane * shape.batches + batch) * shape.gates + tau]
    }

    pub(crate) fn cell(&self, shape: &BatchShape, lane: usize, tau: usize, row: usize) -> C32 {
        self.cube[(lane * shape.gates + tau) * shape.batches + row]
    }

    pub(crate) fn peak(&self, shape: &BatchShape, lane: usize, from_gate: usize) -> (usize, usize) {
        let mut best = (0, 0, 0.0f32);
        for tau in from_gate..shape.gates {
            for row in 0..shape.batches {
                let power = self.cell(shape, lane, tau, row).norm_sqr();
                if power > best.2 {
                    best = (tau, row, power);
                }
            }
        }
        (best.0, best.1)
    }
}

pub(crate) fn run_caf(
    shape: BatchShape,
    plan: Option<&GroupPlan>,
    lanes: &[Vec<C32>],
    taper: DopplerTaper,
) -> Caf {
    let mut kernel = BatchKernel::new(shape);
    let spectra = spectra_of(&mut kernel, lanes);
    let solved = plan.map(|plan| solve(shape, plan, &spectra));
    let gates = residual_gates(
        &mut kernel,
        &spectra,
        solved.as_ref().map(|(_, table)| table),
    );
    let cube = doppler_cube(&mut kernel, &gates, taper);
    let (stats, table) = solved.unzip();
    Caf {
        gates,
        cube,
        stats,
        table,
    }
}

pub(crate) fn correlate(
    shape: &BatchShape,
    reference: &[C32],
    surveillance: &[C32],
    batch: usize,
    lag: i64,
) -> C32 {
    let pre = shape.pre() as i64;
    (0..shape.batch_len)
        .map(|n| {
            let at = pre + (batch * shape.batch_len + n) as i64;
            surveillance[(at + lag) as usize] * reference[at as usize].conj()
        })
        .sum()
}

fn solve(shape: BatchShape, plan: &GroupPlan, spectra: &Spectra) -> (SolveStats, WeightTable) {
    let (m, nb, k) = (shape.fft_len, shape.batches, shape.lanes);
    let mut solver = WienerSolver::new(shape, plan, 1e-4).unwrap();
    let mut sums = GroupSums::new(&shape, plan);
    for b in 0..nb {
        solver
            .accumulate(
                &mut sums,
                b,
                &spectra.models[b * m..(b + 1) * m],
                &spectra.products[b * k * m..(b + 1) * k * m],
                &spectra.energy[b * k..(b + 1) * k],
            )
            .unwrap();
    }
    let mut table = WeightTable::new(&shape, plan).unwrap();
    let stats = solver.solve(&sums, &mut table).unwrap();
    (stats, table)
}

fn residual_gates(
    kernel: &mut BatchKernel,
    spectra: &Spectra,
    table: Option<&WeightTable>,
) -> Vec<C32> {
    let shape = kernel.shape();
    let (m, nb, k) = (shape.fft_len, shape.batches, shape.lanes);
    let mut gates = vec![C32::default(); k * nb * shape.gates];
    for lane in 0..k {
        for b in 0..nb {
            let weights = table.map_or(WeightsAt::none(), |table| table.at(lane, b));
            let at = (lane * nb + b) * shape.gates;
            kernel
                .residual(
                    &spectra.products[(b * k + lane) * m..(b * k + lane + 1) * m],
                    &spectra.models[b * m..(b + 1) * m],
                    weights,
                    &mut gates[at..at + shape.gates],
                )
                .unwrap();
        }
    }
    gates
}

fn doppler_cube(kernel: &mut BatchKernel, gates: &[C32], taper: DopplerTaper) -> Vec<C32> {
    let shape = kernel.shape();
    let (nb, k) = (shape.batches, shape.lanes);
    let mut window = vec![0.0f32; nb];
    taper.fill(&mut window);
    let mut cube = vec![C32::default(); k * shape.gates * nb];
    for lane in 0..k {
        for tau in 0..shape.gates {
            let at = (lane * shape.gates + tau) * nb;
            let series = &mut cube[at..at + nb];
            for (b, value) in series.iter_mut().enumerate() {
                *value = gates[(lane * nb + b) * shape.gates + tau];
            }
            kernel.doppler(series, &window).unwrap();
        }
    }
    cube
}
