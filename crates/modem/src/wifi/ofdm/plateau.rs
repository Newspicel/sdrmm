use num_complex::Complex;

use super::tables::SHORT;

pub(super) const WINDOW: usize = 48;
const PLATEAU: f32 = 0.7;
pub(super) const PLATEAU_SAMPLES: usize = 24;
pub(super) const PROBE: usize = WINDOW / 2;
const _: () = assert!(PROBE <= PLATEAU_SAMPLES && 2 * PROBE == WINDOW);
const MIN_POWER: f32 = 1e-10;
const LANES: usize = 8;
const SIGNS: [f32; LANES] = [1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0];

/// The short training detector: lag-16 autocorrelation against power over a sliding window.
#[derive(Clone, Copy, Default)]
pub(super) struct Plateau {
    correlation: Complex<f32>,
    energy: f32,
    pub(super) run: usize,
    pub(super) holding: bool,
    pub(super) primed: bool,
}

impl Plateau {
    pub(super) fn slide(&mut self, h: &[Complex<f32>], at: usize) {
        let span = at
            .checked_sub(1)
            .and_then(|from| h[from..].first_chunk::<{ WINDOW + SHORT + 1 }>());
        match span {
            Some(span) if self.primed => {
                let (leaving, entering) = (span[0], span[WINDOW]);
                let (leaving_lag, entering_lag) = (span[SHORT], span[WINDOW + SHORT]);
                self.correlation += entering * entering_lag.conj() - leaving * leaving_lag.conj();
                self.energy += entering_lag.norm_sqr() - leaving_lag.norm_sqr();
            }
            _ => {
                self.correlation = (0..WINDOW)
                    .map(|k| h[at + k] * h[at + k + SHORT].conj())
                    .sum();
                self.energy = (0..WINDOW).map(|k| h[at + k + SHORT].norm_sqr()).sum();
                self.primed = true;
            }
        }
    }

    pub(super) fn step(&mut self) -> bool {
        if !on_plateau(self.correlation, self.energy) {
            self.run = 0;
            self.holding = false;
            return false;
        }
        if self.holding {
            return false;
        }
        self.run += 1;
        self.run == PLATEAU_SAMPLES
    }
}

fn on_plateau(correlation: Complex<f32>, energy: f32) -> bool {
    energy > MIN_POWER * WINDOW as f32
        && correlation.norm_sqr() > PLATEAU * PLATEAU * energy * energy
}

/// Plateau probes on a fixed grid of `PROBE` samples. A window spans two grid blocks, so each
/// block is summed once and serves two probes.
#[derive(Clone, Copy, Default)]
pub(super) struct Probe {
    block: Option<u64>,
    correlation: Complex<f32>,
    energy: f32,
}

impl Probe {
    /// Whether the window at `at`, the grid point `absolute`, sits on the plateau.
    pub(super) fn on(&mut self, h: &[Complex<f32>], at: usize, absolute: u64) -> bool {
        let block = absolute / PROBE as u64;
        let (correlation, energy) = match self.block {
            Some(cached) if cached == block => (self.correlation, self.energy),
            _ => block_sums(h, at),
        };
        let (next_correlation, next_energy) = block_sums(h, at + PROBE);
        *self = Self {
            block: Some(block + 1),
            correlation: next_correlation,
            energy: next_energy,
        };
        on_plateau(correlation + next_correlation, energy + next_energy)
    }
}

/// The lag-`SHORT` correlation and energy of the `PROBE` samples from `at`. The samples are
/// read as interleaved floats, so every sum is a plain lane-wise multiply-add.
fn block_sums(h: &[Complex<f32>], at: usize) -> (Complex<f32>, f32) {
    let (Some(early), Some(late)) = (
        h.get(at..).and_then(|h| h.first_chunk::<PROBE>()),
        h.get(at + SHORT..).and_then(|h| h.first_chunk::<PROBE>()),
    ) else {
        return (Complex::default(), 0.0);
    };
    let (early, late) = (floats(early), floats(late));
    let (mut re, mut im, mut energy) = ([0.0f32; LANES], [0.0f32; LANES], [0.0f32; LANES]);
    for (x, y) in early
        .as_chunks::<LANES>()
        .0
        .iter()
        .zip(late.as_chunks::<LANES>().0)
    {
        for lane in 0..LANES {
            re[lane] += x[lane] * y[lane];
            im[lane] += x[lane ^ 1] * y[lane] * SIGNS[lane];
            energy[lane] += y[lane] * y[lane];
        }
    }
    let sum = |lanes: [f32; LANES]| lanes.iter().sum::<f32>();
    (Complex::new(sum(re), sum(im)), sum(energy))
}

fn floats(z: &[Complex<f32>; PROBE]) -> [f32; 2 * PROBE] {
    let mut out = [0.0; 2 * PROBE];
    for (pair, z) in out.as_chunks_mut::<2>().0.iter_mut().zip(z) {
        *pair = [z.re, z.im];
    }
    out
}
