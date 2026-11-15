use num_complex::Complex;

use super::{super::SPS, View};

const SEARCH: isize = 2;
/// The lag for the fine frequency estimate is this share of the known symbols.
const LAG_SHARE: usize = 4;
const MAX_LAG_SYMBOLS: usize = 16;

pub(crate) struct Acquired {
    pub coherence: f32,
    pub instant: f64,
    pub omega: f32,
    pub carrier: Complex<f32>,
    pub last: Complex<f32>,
}

impl Acquired {
    pub(crate) fn view<'a>(&self, y: &'a [Complex<f32>]) -> View<'a> {
        View {
            y,
            origin: self.instant,
            omega: self.omega,
        }
    }
}

pub(crate) fn acquire(
    y: &[Complex<f32>],
    anchor: usize,
    known: &[Complex<f32>],
    segment: usize,
) -> Option<Acquired> {
    let view = View::new(y);
    let rough = lag_one(&view, anchor as f64, known)?;
    let score =
        |shift: isize| segmented(&view, anchor as f64 + shift as f64, known, segment, rough);
    let (best, best_score) = (-SEARCH..=SEARCH)
        .filter_map(|shift| Some((shift, score(shift)?)))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    let before = score(best - 1).unwrap_or(best_score);
    let after = score(best + 1).unwrap_or(best_score);
    let curvature = before - 2.0 * best_score + after;
    let fraction = if curvature < 0.0 {
        (0.5 * (before - after) / curvature).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    let anchor = anchor as f64 + best as f64 + f64::from(fraction);
    let count = known.len();
    let products = |k: usize| -> Option<Complex<f32>> {
        Some(view.raw(instant(anchor, count, k))? * known[k].conj())
    };
    let coarse = lag_one(&view, anchor, known)?;
    let lag = (count / LAG_SHARE).clamp(1, MAX_LAG_SYMBOLS);
    let mut lagged = Complex::new(0.0f32, 0.0);
    let turn = Complex::from_polar(1.0, -coarse * lag as f32);
    for k in lag..count {
        lagged += products(k)? * products(k - lag)?.conj() * turn;
    }
    let per_symbol = coarse + lagged.arg() / lag as f32;
    let mut carrier = Complex::new(0.0f32, 0.0);
    let mut magnitude = 0.0f32;
    for k in 0..count {
        let product = products(k)?;
        magnitude += product.norm();
        carrier += product * Complex::from_polar(1.0, per_symbol * (count - 1 - k) as f32);
    }
    Some(Acquired {
        coherence: carrier.norm() / magnitude.max(f32::MIN_POSITIVE),
        instant: anchor,
        omega: per_symbol / SPS as f32,
        carrier: carrier / count as f32,
        last: known[count - 1],
    })
}

fn instant(anchor: f64, count: usize, k: usize) -> f64 {
    anchor - (SPS * (count - 1 - k)) as f64
}

fn lag_one(view: &View<'_>, anchor: f64, known: &[Complex<f32>]) -> Option<f32> {
    let count = known.len();
    let mut sum = Complex::new(0.0f32, 0.0);
    let mut previous = view.raw(instant(anchor, count, 0))? * known[0].conj();
    for (k, &symbol) in known.iter().enumerate().skip(1) {
        let current = view.raw(instant(anchor, count, k))? * symbol.conj();
        sum += current * previous.conj();
        previous = current;
    }
    Some(sum.arg())
}

fn segmented(
    view: &View<'_>,
    anchor: f64,
    known: &[Complex<f32>],
    segment: usize,
    per_symbol: f32,
) -> Option<f32> {
    let mut total = 0.0;
    for (index, chunk) in known.chunks(segment).enumerate() {
        let mut sum = Complex::new(0.0f32, 0.0);
        for (offset, &symbol) in chunk.iter().enumerate() {
            let k = index * segment + offset;
            let turn = Complex::from_polar(1.0, -per_symbol * offset as f32);
            sum += view.raw(instant(anchor, known.len(), k))? * symbol.conj() * turn;
        }
        total += sum.norm();
    }
    Some(total)
}
