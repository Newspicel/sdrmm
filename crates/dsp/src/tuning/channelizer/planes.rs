use std::f64::consts::TAU;

/// Wanted bins are computed in groups of this many lanes.
pub(super) const LANE_GROUP: usize = 4;

/// Real and imaginary parts in separate planes, so the kernels load each with one vector.
#[derive(Clone, Debug)]
pub(super) struct Planes {
    pub(super) re: Vec<f32>,
    pub(super) im: Vec<f32>,
}

impl Planes {
    pub(super) fn zeroed(len: usize) -> Self {
        Self {
            re: vec![0.0; len],
            im: vec![0.0; len],
        }
    }
}

/// For each output phase, the rotation of every branch slot into every wanted bin.
pub(super) fn twiddle_table(bins: usize, wanted: &[i32]) -> Vec<Planes> {
    let padded = wanted.len().next_multiple_of(LANE_GROUP);
    (0..bins)
        .map(|phase| {
            let mut table = Planes::zeroed(bins * padded);
            for slot in 0..bins {
                let step = (phase + slot + 1) % bins;
                for (lane, &bin) in wanted.iter().enumerate() {
                    let turns = f64::from(bin) * step as f64 / bins as f64;
                    let (sin, cos) = (-TAU * turns).sin_cos();
                    table.re[slot * padded + lane] = cos as f32;
                    table.im[slot * padded + lane] = sin as f32;
                }
            }
            table
        })
        .collect()
}
