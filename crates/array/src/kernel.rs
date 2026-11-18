//! Kernels a host may run on an accelerator, named for the work they do. A processor asks for
//! one through its runner host; the host never learns which processor asked.

use std::sync::Arc;

use num_complex::Complex;
use sdrmm_dsp::radar::batch::{BatchShape, MAX_SURVEILLANCE};
use sdrmm_dsp::radar::wiener::GroupPlan;
use sdrmm_wire::radar::ReferenceHealth;

type C32 = Complex<f32>;

/// What a cross-ambiguity kernel is built for: the batch shape, the Doppler window and the
/// clutter groups it cancels.
#[derive(Clone, Copy, Debug)]
pub struct CafPlan<'a> {
    pub shape: BatchShape,
    pub window: &'a [f32],
    pub groups: Option<&'a GroupPlan>,
    pub loading: f32,
}

/// A cross-ambiguity kernel an accelerator runs.
pub trait CafKernel: Send {
    fn run(&mut self, job: &CpiJob, out: &mut CubeOut) -> Result<(), String>;
}

pub struct CpiJob {
    pub generation: u64,
    pub seq: u64,
    pub start_index: u64,
    pub start_unix_ns: u64,
    pub phase_ready: bool,
    pub reference: ReferenceHealth,
    pub front_suppression_db: [f32; MAX_SURVEILLANCE],
    pub canceller_resets: u64,
    pub lanes: usize,
    pub window: usize,
    pub samples: Vec<C32>,
}

impl CpiJob {
    #[must_use]
    pub fn new(lanes: usize, window: usize) -> Self {
        Self {
            generation: 0,
            seq: 0,
            start_index: 0,
            start_unix_ns: 0,
            phase_ready: false,
            reference: ReferenceHealth::default(),
            front_suppression_db: [0.0; MAX_SURVEILLANCE],
            canceller_resets: 0,
            lanes,
            window,
            samples: vec![C32::default(); lanes * window],
        }
    }

    #[must_use]
    pub fn lane(&self, index: usize) -> &[C32] {
        let start = index * self.window;
        self.samples.get(start..start + self.window).unwrap_or(&[])
    }

    pub fn lane_mut(&mut self, index: usize) -> &mut [C32] {
        let start = index * self.window;
        self.samples
            .get_mut(start..start + self.window)
            .unwrap_or(&mut [])
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CafError {
    #[error("CAF helper stopped")]
    Crew,
    #[error("CPI does not match the plan")]
    Shape,
}

pub struct CubeOut {
    pub cube: Vec<C32>,
    pub suppression_db: [f32; MAX_SURVEILLANCE],
    pub unsuppressed_groups: u32,
}

impl CubeOut {
    #[must_use]
    pub fn new(shape: &BatchShape) -> Self {
        Self {
            cube: vec![C32::default(); shape.lanes * shape.gates * shape.batches],
            suppression_db: [0.0; MAX_SURVEILLANCE],
            unsuppressed_groups: 0,
        }
    }

    #[must_use]
    pub fn cell(&self, shape: &BatchShape, lane: usize, gate: usize, row: usize) -> C32 {
        self.cube
            .get((lane * shape.gates + gate) * shape.batches + row)
            .copied()
            .unwrap_or_default()
    }
}

pub trait CafBackend: Send {
    fn run(&mut self, job: &CpiJob, out: &mut CubeOut) -> Result<(), CafError>;
    fn run_shared(&mut self, job: &Arc<CpiJob>, out: &mut CubeOut) -> Result<(), CafError> {
        self.run(job, out)
    }
    fn gpu(&self) -> bool;
    fn threads(&self) -> u32;
}
