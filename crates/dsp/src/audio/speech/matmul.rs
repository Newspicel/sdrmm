use crate::math::{fastmath::madd, neural::f16_to_f32};

const PANEL_COLS: usize = 64;
const PANEL_DEPTH: usize = 256;
const TILE_ROWS: usize = 4;
const LANES: usize = 16;

/// A row-major `depth x cols` matrix of half floats inside the model bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct HalfMatrix {
    pub at: usize,
    pub depth: usize,
    pub cols: usize,
}

impl HalfMatrix {
    pub fn row(&self, bytes: &[u8], index: usize, out: &mut [f32]) {
        let start = self.at + 2 * index * self.cols;
        for (value, pair) in out
            .iter_mut()
            .zip(bytes[start..start + 2 * self.cols].as_chunks::<2>().0)
        {
            *value = f16_to_f32(u16::from_le_bytes(*pair));
        }
    }
}

/// `out = input x matrix` for `rows` input rows, the columns shared out over `threads`.
pub(super) fn matmul(
    bytes: &[u8],
    matrix: &HalfMatrix,
    input: &[f32],
    rows: usize,
    out: &mut [f32],
    threads: usize,
) {
    let cols = matrix.cols;
    let panels = cols.div_ceil(PANEL_COLS);
    let threads = threads.clamp(1, panels.max(1));
    let chunk = panels.div_ceil(threads) * PANEL_COLS;
    if threads == 1 {
        let local = columns(bytes, matrix, input, rows, 0..cols);
        out[..rows * cols].copy_from_slice(&local);
        return;
    }
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..cols)
            .step_by(chunk)
            .map(|lo| {
                let span = lo..(lo + chunk).min(cols);
                scope.spawn(move || (span.clone(), columns(bytes, matrix, input, rows, span)))
            })
            .collect();
        for worker in workers {
            match worker.join() {
                Ok((span, local)) => place(out, cols, &span, &local),
                Err(panic) => std::panic::resume_unwind(panic),
            }
        }
    });
}

fn place(out: &mut [f32], cols: usize, span: &std::ops::Range<usize>, local: &[f32]) {
    let width = span.len();
    for (row, values) in local.chunks_exact(width).enumerate() {
        out[row * cols + span.start..row * cols + span.end].copy_from_slice(values);
    }
}

fn columns(
    bytes: &[u8],
    matrix: &HalfMatrix,
    input: &[f32],
    rows: usize,
    span: std::ops::Range<usize>,
) -> Vec<f32> {
    let width = span.len();
    let mut local = vec![0.0; rows * width];
    let mut panel = vec![0.0; PANEL_DEPTH * PANEL_COLS];
    for c0 in span.clone().step_by(PANEL_COLS) {
        let pc = (span.end - c0).min(PANEL_COLS);
        for k0 in (0..matrix.depth).step_by(PANEL_DEPTH) {
            let pk = (matrix.depth - k0).min(PANEL_DEPTH);
            pack(bytes, matrix, (k0, pk), (c0, pc), &mut panel);
            let block = Block {
                input,
                depth: matrix.depth,
                k0,
                pk,
                col: c0 - span.start,
                pc,
                width,
            };
            let mut r0 = 0;
            while r0 + TILE_ROWS <= rows {
                block.tile::<TILE_ROWS>(&panel, r0, &mut local);
                r0 += TILE_ROWS;
            }
            for row in r0..rows {
                block.tile::<1>(&panel, row, &mut local);
            }
        }
    }
    local
}

fn pack(
    bytes: &[u8],
    matrix: &HalfMatrix,
    (k0, pk): (usize, usize),
    (c0, pc): (usize, usize),
    panel: &mut [f32],
) {
    for k in 0..pk {
        let start = matrix.at + 2 * ((k0 + k) * matrix.cols + c0);
        let line = &mut panel[k * PANEL_COLS..(k + 1) * PANEL_COLS];
        for (value, pair) in line
            .iter_mut()
            .zip(bytes[start..start + 2 * pc].as_chunks::<2>().0)
        {
            *value = f16_to_f32(u16::from_le_bytes(*pair));
        }
        line[pc..].fill(0.0);
    }
}

struct Block<'a> {
    input: &'a [f32],
    depth: usize,
    k0: usize,
    pk: usize,
    col: usize,
    pc: usize,
    width: usize,
}

impl Block<'_> {
    #[inline(always)]
    fn tile<const R: usize>(&self, panel: &[f32], r0: usize, local: &mut [f32]) {
        let lines: [&[f32]; R] = std::array::from_fn(|i| {
            let at = (r0 + i) * self.depth + self.k0;
            &self.input[at..at + self.pk]
        });
        for l0 in (0..self.pc).step_by(LANES) {
            let used = (self.pc - l0).min(LANES);
            let mut acc = [[0.0f32; LANES]; R];
            for (i, sums) in acc.iter_mut().enumerate() {
                let at = (r0 + i) * self.width + self.col + l0;
                sums[..used].copy_from_slice(&local[at..at + used]);
            }
            for k in 0..self.pk {
                let Some(b) = panel[k * PANEL_COLS + l0..].first_chunk::<LANES>() else {
                    return;
                };
                for i in 0..R {
                    let a = lines[i][k];
                    for lane in 0..LANES {
                        acc[i][lane] = madd(a, b[lane], acc[i][lane]);
                    }
                }
            }
            for (i, sums) in acc.iter().enumerate() {
                let at = (r0 + i) * self.width + self.col + l0;
                local[at..at + used].copy_from_slice(&sums[..used]);
            }
        }
    }
}
