use std::f64::consts::TAU;

use num_complex::Complex;

use super::mode::Robustness;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    pub start: usize,
    pub shift: i32,
    pub metric: f32,
    pub runner_up: f32,
}

pub struct Framing {
    symbols: usize,
    depth: usize,
    width: usize,
    low: i32,
    useful: usize,
    reach: i32,
    pilots: Vec<Vec<(usize, Complex<f32>)>>,
    history: Vec<Complex<f32>>,
    stored: usize,
    products: Vec<Complex<f32>>,
    magnitudes: Vec<f32>,
    sums: Vec<Complex<f32>>,
    norms: Vec<f32>,
    rows: usize,
    frame: Vec<(Complex<f32>, f32)>,
}

fn unit(phase: u16) -> Complex<f32> {
    Complex::from_polar(1.0, (TAU * f64::from(phase) / 1024.0) as f32)
}

fn pilots(mode: Robustness, symbol: usize) -> Vec<(usize, Complex<f32>)> {
    let (low, high) = mode.span();
    let (_, depth, _) = mode.gain_grid();
    let earlier = (symbol + mode.symbols() - depth as usize) % mode.symbols();
    (low..=high)
        .filter(|&k| !mode.unused(k) && mode.is_gain_ref(symbol, k))
        .map(|k| {
            let now = unit(mode.pilot_phase(symbol, k));
            let then = unit(mode.pilot_phase(earlier, k));
            ((k - low) as usize, now.conj() * then)
        })
        .collect()
}

impl Framing {
    #[must_use]
    pub fn new(mode: Robustness, reach: i32) -> Self {
        let (low, high) = mode.span();
        let width = (high - low + 1) as usize;
        let symbols = mode.symbols();
        let depth = mode.gain_grid().1 as usize;
        let shifts = (2 * reach + 1) as usize;
        Self {
            symbols,
            depth,
            width,
            low,
            useful: mode.useful(),
            reach,
            pilots: (0..symbols).map(|symbol| pilots(mode, symbol)).collect(),
            history: vec![Complex::default(); depth * width],
            stored: 0,
            products: vec![Complex::default(); width],
            magnitudes: vec![0.0; width],
            sums: vec![Complex::default(); symbols * symbols * shifts],
            norms: vec![0.0; symbols * symbols * shifts],
            rows: 0,
            frame: vec![(Complex::default(), 0.0); symbols],
        }
    }

    fn shifts(&self) -> usize {
        (2 * self.reach + 1) as usize
    }

    fn compare(&mut self, bins: &[Complex<f32>]) -> bool {
        if self.stored < self.depth {
            return false;
        }
        let slot = self.stored % self.depth;
        let earlier = &self.history[slot * self.width..(slot + 1) * self.width];
        for ((product, magnitude), (&now, &then)) in self
            .products
            .iter_mut()
            .zip(&mut self.magnitudes)
            .zip(bins.iter().zip(earlier))
        {
            *product = now * then.conj();
            *magnitude = product.norm();
        }
        true
    }

    fn correlate(&self, symbol: usize, shift: i32) -> (Complex<f32>, f32) {
        let width = self.width as i32;
        self.pilots[symbol]
            .iter()
            .filter_map(|&(index, reference)| {
                let at = index as i32 + shift;
                (0..width).contains(&at).then(|| {
                    let at = at as usize;
                    (self.products[at] * reference, self.magnitudes[at])
                })
            })
            .fold((Complex::default(), 0.0), |(sum, norm), (value, size)| {
                (sum + value, norm + size)
            })
    }

    pub fn measure_all(&mut self, bins: &[Complex<f32>]) -> Option<usize> {
        if !self.compare(bins) {
            return None;
        }
        let row = self.rows % self.symbols;
        let shifts = self.shifts();
        for symbol in 0..self.symbols {
            for (index, shift) in (-self.reach..=self.reach).enumerate() {
                let (sum, norm) = self.correlate(symbol, shift);
                let at = (row * self.symbols + symbol) * shifts + index;
                self.sums[at] = sum;
                self.norms[at] = norm;
            }
        }
        self.rows += 1;
        Some(row)
    }

    fn metric(&self, start: usize, index: usize) -> f32 {
        let shifts = self.shifts();
        let (sum, norm) = (0..self.symbols)
            .map(|row| {
                let symbol = (row + self.symbols - start) % self.symbols;
                (row * self.symbols + symbol) * shifts + index
            })
            .fold((Complex::<f32>::default(), 0.0f32), |(sum, norm), at| {
                (sum + self.sums[at], norm + self.norms[at])
            });
        sum.norm() / norm.max(f32::EPSILON)
    }

    #[must_use]
    pub fn best(&self) -> Option<Candidate> {
        if self.rows < self.symbols {
            return None;
        }
        let mut best = Candidate {
            start: 0,
            shift: 0,
            metric: 0.0,
            runner_up: 0.0,
        };
        for start in 0..self.symbols {
            for (index, shift) in (-self.reach..=self.reach).enumerate() {
                let metric = self.metric(start, index);
                if metric > best.metric {
                    best = Candidate {
                        start,
                        shift,
                        metric,
                        runner_up: best.metric,
                    };
                } else if metric > best.runner_up {
                    best.runner_up = metric;
                }
            }
        }
        Some(best)
    }

    pub fn measure(&mut self, bins: &[Complex<f32>], symbol: usize) {
        self.frame[symbol] = if self.compare(bins) {
            self.correlate(symbol, 0)
        } else {
            (Complex::default(), 0.0)
        };
    }

    #[must_use]
    pub fn frame_metric(&self) -> f32 {
        let (sum, norm) = self.frame.iter().fold(
            (Complex::<f32>::default(), 0.0f32),
            |(sum, norm), &(value, size)| (sum + value, norm + size),
        );
        sum.norm() / norm.max(f32::EPSILON)
    }

    pub fn store(&mut self, bins: &[Complex<f32>]) {
        let slot = self.stored % self.depth;
        self.history[slot * self.width..(slot + 1) * self.width].copy_from_slice(bins);
        self.stored += 1;
    }

    pub fn shift(&mut self, shift: i32) {
        for slot in self.history.chunks_mut(self.width) {
            if shift > 0 {
                slot.rotate_left(shift as usize);
            } else {
                slot.rotate_right(shift.unsigned_abs() as usize);
            }
        }
    }

    pub fn realign(&mut self, adjust: i64) {
        let step = TAU * adjust as f64 / self.useful as f64;
        for slot in self.history.chunks_mut(self.width) {
            for (index, bin) in slot.iter_mut().enumerate() {
                let k = f64::from(self.low + index as i32);
                *bin *= Complex::from_polar(1.0, (step * k) as f32);
            }
        }
    }

    pub fn restart(&mut self) {
        self.rows = 0;
        self.frame.fill((Complex::default(), 0.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drm::cells::{Cell, Layout};

    fn symbol_bins(layout: &Layout, symbol: usize, shift: i32, time: usize) -> Vec<Complex<f32>> {
        let mode = layout.mode;
        let (low, high) = mode.span();
        let mut bins = vec![Complex::default(); (high - low + 1) as usize];
        let frame = (symbol / mode.symbols()) % mode.frames();
        for (offset, cell) in layout
            .symbol(frame, symbol % mode.symbols())
            .iter()
            .enumerate()
        {
            let k = layout.low + offset as i32;
            let seed = (k.unsigned_abs() as usize * 7919 + time * 104_729) % 4;
            let value = match *cell {
                Cell::Pilot { value, .. } => value,
                Cell::Unused => continue,
                _ => Complex::from_polar(1.0, (TAU * (seed as f64 + 0.5) / 4.0) as f32),
            };
            let channel = Complex::from_polar(0.8, (0.4 * f64::from(k) + 0.1 * time as f64) as f32);
            let at = k - low + shift;
            if (0..bins.len() as i32).contains(&at) {
                bins[at as usize] = value * channel;
            }
        }
        bins
    }

    #[test]
    fn gain_references_find_the_frame_start_and_carrier_shift() {
        for mode in Robustness::ALL {
            let layout = Layout::new(mode, mode.widest()).expect("widest occupancy");
            let (symbols, depth) = (mode.symbols(), mode.gain_grid().1 as usize);
            let (offset, shift) = (5, 2);
            let mut framing = Framing::new(mode, 4);
            let mut found = None;
            for time in 0..2 * symbols + depth {
                let bins = symbol_bins(&layout, offset + time, shift, time);
                let row = framing.measure_all(&bins);
                framing.store(&bins);
                if (offset + time) % symbols == 0 {
                    found = row.zip(framing.best());
                }
            }
            let (row, best) = found.expect("a candidate");
            assert_eq!(best.start, row, "{mode:?}");
            assert_eq!(best.shift, shift, "{mode:?}");
            assert!(best.metric > 0.9, "{mode:?} {best:?}");
            assert!(best.runner_up < 0.6, "{mode:?} {best:?}");
        }
    }
}
