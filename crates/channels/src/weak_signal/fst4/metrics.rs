use std::f32::consts::TAU;

use num_complex::Complex;

use super::code::N;
use super::{SYMBOLS, SYNC_A, SYNC_B, SYNC_STARTS};

const GRAY: [usize; 4] = [0, 1, 3, 2];
const STREAM: usize = 2 * SYMBOLS;
const GROUP: usize = 8;
const MIN_SYNC_SYMBOLS: usize = 16;
const MIN_SYNC_BITS: usize = 46;
const SCALE: f32 = 2.83;
const DATA_BLOCKS: [usize; 4] = [16, 92, 168, 244];
const DATA_BLOCK_BITS: usize = 60;

/// Soft bits from one frame, judged over one, two, four and eight symbols, and each symbol's
/// tone powers.
pub(super) struct Metrics {
    pub(super) llrs: [[f32; N]; 4],
    pub(super) powers: Vec<[f32; 4]>,
}

/// The tone references, one symbol long, each starting at phase zero.
pub(super) fn references(samples: usize) -> [Vec<Complex<f32>>; 4] {
    std::array::from_fn(|tone| {
        let step = (tone as f32 - 1.5) * TAU / samples as f32;
        (0..samples)
            .map(|index| Complex::from_polar(1.0, step * index as f32))
            .collect()
    })
}

/// Measures a frame of `SYMBOLS` symbols of `references[0].len()` samples each. `None` when its
/// sync symbols say it is not a frame.
pub(super) fn measure(
    frame: &[Complex<f32>],
    references: &[Vec<Complex<f32>>; 4],
) -> Option<Metrics> {
    let samples = references[0].len();
    if frame.len() < SYMBOLS * samples {
        return None;
    }
    let tones: Vec<[Complex<f32>; 4]> = frame
        .chunks_exact(samples)
        .take(SYMBOLS)
        .map(|symbol| {
            std::array::from_fn(|tone| {
                symbol
                    .iter()
                    .zip(&references[tone])
                    .map(|(sample, reference)| sample * reference.conj())
                    .sum()
            })
        })
        .collect();
    let powers: Vec<[f32; 4]> = tones.iter().map(|row| row.map(|c| c.norm_sqr())).collect();
    if sync_symbols(&powers) < MIN_SYNC_SYMBOLS {
        return None;
    }
    let mut metrics = [[0.0f32; STREAM]; 4];
    for (group, chunk) in tones.as_chunks::<GROUP>().0.iter().enumerate() {
        group_metrics(chunk, group * 2 * GROUP, &mut metrics);
    }
    if sync_bits(&metrics[0]) < MIN_SYNC_BITS {
        return None;
    }
    for column in &mut metrics {
        normalize(column);
    }
    let llrs = metrics.map(|column| {
        let mut llr = [0.0f32; N];
        for (block, &start) in DATA_BLOCKS.iter().enumerate() {
            llr[block * DATA_BLOCK_BITS..(block + 1) * DATA_BLOCK_BITS]
                .copy_from_slice(&column[start..start + DATA_BLOCK_BITS]);
        }
        llr
    });
    Some(Metrics { llrs, powers })
}

fn sync_words() -> impl Iterator<Item = (usize, &'static [u8; 8])> {
    SYNC_STARTS
        .iter()
        .enumerate()
        .map(|(index, &start)| (start, if index % 2 == 0 { &SYNC_A } else { &SYNC_B }))
}

fn sync_symbols(powers: &[[f32; 4]]) -> usize {
    sync_words()
        .flat_map(|(start, word)| {
            word.iter().enumerate().filter(move |&(offset, &tone)| {
                let row = &powers[start + offset];
                (0..4).max_by(|&a, &b| row[a].total_cmp(&row[b])) == Some(usize::from(tone))
            })
        })
        .count()
}

fn sync_bits(metric: &[f32; STREAM]) -> usize {
    sync_words()
        .map(|(start, word)| {
            word.iter()
                .enumerate()
                .map(|(offset, &tone)| {
                    let value = GRAY
                        .iter()
                        .position(|&gray| gray == usize::from(tone))
                        .unwrap_or(0);
                    let at = 2 * (start + offset);
                    usize::from((metric[at] >= 0.0) == (value >> 1 == 1))
                        + usize::from((metric[at + 1] >= 0.0) == (value & 1 == 1))
                })
                .sum::<usize>()
        })
        .sum()
}

/// Correlates every tone sequence of one, two, four and eight symbols. Each symbol's tones
/// turn by half a cycle, so neighbouring symbols join with a sign flip.
fn group_metrics(tones: &[[Complex<f32>; 4]], at: usize, metrics: &mut [[f32; STREAM]; 4]) {
    let single: Vec<[Complex<f32>; 4]> = tones
        .iter()
        .map(|row| std::array::from_fn(|value| row[GRAY[value]]))
        .collect();
    for (symbol, row) in single.iter().enumerate() {
        write_bits(
            &row.map(|c| c.norm_sqr()),
            2,
            at + 2 * symbol,
            &mut metrics[0],
        );
    }
    let pairs: Vec<Vec<Complex<f32>>> = single
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[first, second]| combine(first, second, -1.0))
        .collect();
    for (index, pair) in pairs.iter().enumerate() {
        write_bits(&squared(pair), 4, at + 4 * index, &mut metrics[1]);
    }
    let quads: Vec<Vec<Complex<f32>>> = pairs
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[first, second]| combine(first, second, 1.0))
        .collect();
    for (index, quad) in quads.iter().enumerate() {
        write_bits(&squared(quad), 8, at + 8 * index, &mut metrics[2]);
    }
    if let [first, second] = quads.as_slice() {
        octet_bits(first, second, at, &mut metrics[3]);
    }
}

fn combine(first: &[Complex<f32>], second: &[Complex<f32>], sign: f32) -> Vec<Complex<f32>> {
    first
        .iter()
        .flat_map(|&a| second.iter().map(move |&b| a + sign * b))
        .collect()
}

fn squared(values: &[Complex<f32>]) -> Vec<f32> {
    values.iter().map(Complex::norm_sqr).collect()
}

/// For each of `bits` bits of the sequence index, most significant first, the best amplitude
/// with the bit set less the best with it clear.
fn write_bits(power: &[f32], bits: usize, at: usize, metric: &mut [f32; STREAM]) {
    for bit in 0..bits {
        let shift = bits - 1 - bit;
        let (mut one, mut zero) = (0.0f32, 0.0f32);
        for (index, &value) in power.iter().enumerate() {
            if index >> shift & 1 == 1 {
                one = one.max(value);
            } else {
                zero = zero.max(value);
            }
        }
        metric[at + bit] = one.sqrt() - zero.sqrt();
    }
}

/// The eight-symbol metrics without listing all 65 536 sequences: a bit of the first half only
/// needs each first half's best partner, and the same for the second half.
fn octet_bits(
    first: &[Complex<f32>],
    second: &[Complex<f32>],
    at: usize,
    metric: &mut [f32; STREAM],
) {
    let mut best_first = vec![0.0f32; first.len()];
    let mut best_second = vec![0.0f32; second.len()];
    for (i, &a) in first.iter().enumerate() {
        for (j, &b) in second.iter().enumerate() {
            let power = (a + b).norm_sqr();
            best_first[i] = best_first[i].max(power);
            best_second[j] = best_second[j].max(power);
        }
    }
    write_bits(&best_first, 8, at, metric);
    write_bits(&best_second, 8, at + 8, metric);
}

fn normalize(column: &mut [f32; STREAM]) {
    let mean = column.iter().sum::<f32>() / STREAM as f32;
    let mean_square = column.iter().map(|value| value * value).sum::<f32>() / STREAM as f32;
    let variance = mean_square - mean * mean;
    let spread = if variance > 0.0 {
        variance
    } else {
        mean_square
    };
    if spread > 0.0 {
        let scale = SCALE / spread.sqrt();
        for value in column.iter_mut() {
            *value *= scale;
        }
    }
}
