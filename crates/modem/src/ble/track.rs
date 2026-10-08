use num_complex::Complex;

use super::SPS;

const SEARCH: isize = 2;
const CARRIER_MEMORY: f32 = 0.85;
const SYMBOL_TIMING_GAIN: f64 = 0.02;
const BLOCK_TIMING_GAIN: f64 = 0.08;
const PILOT_MEMORY: f32 = 0.8;
const MAX_TIMING_STEP: f64 = 0.5;
const SMOOTHING_BLOCKS: usize = 8;
const HALF_SAMPLE: f64 = 0.5;
const BLOCK: f64 = (SPS * 4) as f64;
const QUARTER: Complex<f32> = Complex::new(0.0, 1.0);

pub(crate) fn pseudo_symbols(symbols: impl Iterator<Item = bool>, out: &mut Vec<Complex<f32>>) {
    let mut current = Complex::new(1.0, 0.0);
    for symbol in symbols {
        current *= if symbol { QUARTER } else { -QUARTER };
        out.push(current);
    }
}

#[derive(Clone, Copy)]
pub(crate) struct View<'a> {
    y: &'a [Complex<f32>],
    origin: f64,
    omega: f32,
}

impl<'a> View<'a> {
    pub(crate) fn new(y: &'a [Complex<f32>]) -> Self {
        Self {
            y,
            origin: 0.0,
            omega: 0.0,
        }
    }

    fn raw(&self, t: f64) -> Option<Complex<f32>> {
        if t < 0.0 {
            return None;
        }
        let index = t.floor();
        let weight = (t - index) as f32;
        let index = index as usize;
        Some(self.y.get(index)? * (1.0 - weight) + self.y.get(index + 1)? * weight)
    }

    pub(crate) fn at(&self, t: f64) -> Option<Complex<f32>> {
        let turn = -self.omega * (t - self.origin) as f32;
        Some(self.raw(t)? * Complex::from_polar(1.0, turn))
    }
}

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
    let lag = (count / 4).clamp(1, 16);
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

pub(crate) struct Coherent<'a> {
    view: View<'a>,
    position: f64,
    carrier: Complex<f32>,
    previous: Complex<f32>,
}

impl<'a> Coherent<'a> {
    pub(crate) fn new(
        view: View<'a>,
        position: f64,
        carrier: Complex<f32>,
        previous: Complex<f32>,
    ) -> Self {
        Self {
            view,
            position,
            carrier,
            previous,
        }
    }

    pub(crate) fn position(&self) -> f64 {
        self.position
    }

    pub(crate) fn symbol(&mut self) -> Option<f32> {
        self.position += SPS as f64;
        let y = self.view.at(self.position)?;
        let scale = self.carrier.norm().max(f32::MIN_POSITIVE);
        let expected = self.carrier * self.previous;
        let soft = (y * expected.conj()).im / scale;
        let current = self.previous * if soft > 0.0 { QUARTER } else { -QUARTER };
        let reference = self.carrier * current;
        let slope = self.view.at(self.position + HALF_SAMPLE)?
            - self.view.at(self.position - HALF_SAMPLE)?;
        let error = f64::from((slope * reference.conj()).re / (scale * scale));
        self.position += SYMBOL_TIMING_GAIN * error.clamp(-MAX_TIMING_STEP, MAX_TIMING_STEP);
        self.carrier = self.carrier * CARRIER_MEMORY + y * current.conj() * (1.0 - CARRIER_MEMORY);
        self.previous = current;
        Some(soft)
    }
}

pub(crate) struct Blocks<'a> {
    view: View<'a>,
    position: f64,
    reference: Complex<f32>,
}

impl<'a> Blocks<'a> {
    pub(crate) fn new(view: View<'a>, position: f64, reference: Complex<f32>) -> Self {
        Self {
            view,
            position,
            reference,
        }
    }

    pub(crate) fn position(&self) -> f64 {
        self.position
    }

    pub(crate) fn reference(&self) -> Complex<f32> {
        self.reference
    }

    pub(crate) fn block(&mut self) -> Option<(Complex<f32>, Complex<f32>)> {
        let t = self.position;
        let step = SPS as f64;
        let [y0, y1, y2, y3] = [0.0, 1.0, 2.0, 3.0].map(|k| self.view.at(t + k * step));
        let (y0, y1, y2, y3) = (y0?, y1?, y2?, y3?);
        let pilot = y0 - y2;
        let data = y1 + y3;
        let slope = |at: f64| -> Option<Complex<f32>> {
            Some(self.view.at(at + HALF_SAMPLE)? - self.view.at(at - HALF_SAMPLE)?)
        };
        let derivative = slope(t)? - slope(t + 2.0 * step)?;
        let scale = self.reference.norm_sqr().max(f32::MIN_POSITIVE);
        let error = f64::from((derivative * self.reference.conj()).re / scale);
        self.reference = self.reference * PILOT_MEMORY + pilot * (1.0 - PILOT_MEMORY);
        self.position += BLOCK + BLOCK_TIMING_GAIN * error.clamp(-MAX_TIMING_STEP, MAX_TIMING_STEP);
        Some((pilot, data))
    }
}

pub(crate) fn smoothed_soft(pilots: &[Complex<f32>], data: &[Complex<f32>], out: &mut Vec<f32>) {
    for (index, &value) in data.iter().enumerate() {
        let low = index.saturating_sub(SMOOTHING_BLOCKS);
        let high = (index + SMOOTHING_BLOCKS + 1).min(pilots.len());
        let window = &pilots[low..high];
        let drift: Complex<f32> = window.windows(2).map(|pair| pair[1] * pair[0].conj()).sum();
        let turn = if drift.norm() > 0.0 { drift.arg() } else { 0.0 };
        let reference: Complex<f32> = window
            .iter()
            .enumerate()
            .map(|(offset, &pilot)| {
                let distance = (low + offset) as f32 - index as f32;
                pilot * Complex::from_polar(1.0, -turn * distance)
            })
            .sum();
        let scale = reference.norm().max(f32::MIN_POSITIVE);
        out.push((value * reference.conj()).im / scale);
    }
}

pub(crate) fn carrier_from_pilot(reference: Complex<f32>, last: Complex<f32>) -> Complex<f32> {
    reference * last.conj() * 0.5
}
