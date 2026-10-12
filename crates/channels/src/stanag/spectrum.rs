use num_complex::Complex;
use sdrmm_dsp::{fft::Transform, hann};

use super::{NOMINAL_CARRIER_HZ, PASSBAND_HIGH_HZ, PASSBAND_LOW_HZ, RATE_HZ};

const FFT_LEN: usize = 384;
pub(super) const BIN_HZ: f64 = RATE_HZ / FFT_LEN as f64;
const LOW_BIN: isize = ((PASSBAND_LOW_HZ - NOMINAL_CARRIER_HZ) / BIN_HZ) as isize;
const HIGH_BIN: isize = ((PASSBAND_HIGH_HZ - NOMINAL_CARRIER_HZ) / BIN_HZ) as isize;
const OCCUPIED_RATIO: f32 = 4.0;
const QUIET_PERCENTILE: f64 = 0.1;
const BUSY_PERCENTILE: f64 = 0.9;
const EDGE_FRACTION: f64 = 0.02;
const ALE_TONES: usize = 8;
const ALE_FIRST_HZ: f64 = 750.0;
const ALE_SPACING_HZ: f64 = 250.0;
const ALE_SEARCH_BINS: isize = 6;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Occupancy {
    pub occupied: bool,
    pub center_hz: f64,
    pub bandwidth_hz: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Comb {
    pub contrast_db: f32,
    pub evenness_db: f32,
    pub offset_hz: f64,
}

pub(super) struct Spectrum {
    fft: Transform,
    taper: Vec<f32>,
    frame: Vec<Complex<f32>>,
    power: Vec<f32>,
    band: Vec<f32>,
    sorted: Vec<f32>,
}

fn bin_index(offset: isize) -> usize {
    offset.rem_euclid(FFT_LEN as isize) as usize
}

fn db(ratio: f64) -> f32 {
    (10.0 * ratio.max(f64::MIN_POSITIVE).log10()) as f32
}

impl Spectrum {
    pub fn new() -> Self {
        let width = (HIGH_BIN - LOW_BIN + 1) as usize;
        Self {
            fft: Transform::forward(FFT_LEN),
            taper: hann(FFT_LEN),
            frame: vec![Complex::default(); FFT_LEN],
            power: vec![0.0; FFT_LEN],
            band: vec![0.0; width],
            sorted: vec![0.0; width],
        }
    }

    pub fn measure(&mut self, x: &[Complex<f32>]) -> Occupancy {
        self.power.fill(0.0);
        for start in (0..x.len().saturating_sub(FFT_LEN - 1)).step_by(FFT_LEN / 2) {
            for ((slot, &sample), &weight) in
                self.frame.iter_mut().zip(&x[start..]).zip(&self.taper)
            {
                *slot = sample * weight;
            }
            self.fft.process(&mut self.frame);
            for (power, bin) in self.power.iter_mut().zip(&self.frame) {
                *power += bin.norm_sqr();
            }
        }
        for (slot, offset) in self.band.iter_mut().zip(LOW_BIN..=HIGH_BIN) {
            *slot = self.power[bin_index(offset)];
        }
        self.occupancy()
    }

    fn percentile(&self, fraction: f64) -> f32 {
        self.sorted[((self.sorted.len() - 1) as f64 * fraction) as usize]
    }

    fn occupancy(&mut self) -> Occupancy {
        self.sorted.copy_from_slice(&self.band);
        self.sorted.sort_unstable_by(f32::total_cmp);
        let quiet = self.percentile(QUIET_PERCENTILE);
        let busy = self.percentile(BUSY_PERCENTILE);
        let occupied = busy > OCCUPIED_RATIO * quiet && busy > f32::MIN_POSITIVE;
        let total: f64 = self
            .band
            .iter()
            .map(|&p| f64::from((p - quiet).max(0.0)))
            .sum();
        if total <= f64::MIN_POSITIVE {
            return Occupancy::default();
        }
        let mut running = 0.0;
        let mut low = None;
        let mut high = 0.0;
        for (&power, offset) in self.band.iter().zip(LOW_BIN..) {
            running += f64::from((power - quiet).max(0.0)) / total;
            let hz = offset as f64 * BIN_HZ;
            if low.is_none() && running >= EDGE_FRACTION {
                low = Some(hz);
            }
            if running <= 1.0 - EDGE_FRACTION {
                high = hz;
            }
        }
        let low = low.unwrap_or(high);
        Occupancy {
            occupied,
            center_hz: (low + high) / 2.0,
            bandwidth_hz: (high - low).max(BIN_HZ),
        }
    }

    pub fn ale_comb(&self) -> Comb {
        let tone = |k: usize| {
            ((ALE_FIRST_HZ + ALE_SPACING_HZ * k as f64 - NOMINAL_CARRIER_HZ) / BIN_HZ) as isize
        };
        let gap = (ALE_SPACING_HZ / 2.0 / BIN_HZ) as isize;
        let mut best = Comb::default();
        for shift in -ALE_SEARCH_BINS..=ALE_SEARCH_BINS {
            let on = |k: usize| f64::from(self.power[bin_index(tone(k) + shift)]);
            let mut loud: f64 = 0.0;
            let mut faint = f64::MAX;
            let mut tones = 0.0;
            for k in 0..ALE_TONES {
                loud = loud.max(on(k));
                faint = faint.min(on(k));
                tones += on(k);
            }
            let between: f64 = (0..=ALE_TONES)
                .map(|k| f64::from(self.power[bin_index(tone(k) - gap + shift)]))
                .sum();
            let contrast_db = db((tones / ALE_TONES as f64) / (between / (ALE_TONES + 1) as f64));
            if contrast_db > best.contrast_db {
                best = Comb {
                    contrast_db,
                    evenness_db: db(faint / loud),
                    offset_hz: shift as f64 * BIN_HZ,
                };
            }
        }
        best
    }
}
