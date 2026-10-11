use num_complex::Complex;
use sdrmm_dsp::{BitSync, Decimator, Nco, RealDecimator, design_lowpass, fft::Transform, hann};

pub(crate) const RATE: f64 = 24_000.0;
pub(crate) const BIT_RATE: f64 = 400.0;
pub(crate) const CHIP_RATE: f64 = 2.0 * BIT_RATE;
pub(crate) const PHASE_DEVIATION: f64 = 1.1;
pub(crate) const PREAMBLE_S: f64 = 0.16;
const DECIMATION: usize = 3;
const WORK_RATE: f64 = RATE / DECIMATION as f64;
const SAMPLES_PER_CHIP: usize = (WORK_RATE / CHIP_RATE) as usize;
const BASEBAND_TAPS: usize = 63;
const BASEBAND_CUTOFF_HZ: f64 = 2_000.0;
const REFERENCE_LEN: usize = 160;
const FFT_LEN: usize = 2_048;
const HOP: usize = 512;
const DETECT_RATIO: f32 = 25.0;
const FLOOR_WEIGHT: f32 = 0.1;
const POWER_FLOOR: f32 = 1e-20;

pub(crate) struct Search {
    fft: Transform,
    window: Vec<f32>,
    recent: Vec<Complex<f32>>,
    at: usize,
    since_hop: usize,
    spectrum: Vec<Complex<f32>>,
    power: Vec<f32>,
    floor: Vec<f32>,
    primed: bool,
    half_bins: usize,
}

impl Search {
    pub(crate) fn new(bandwidth_hz: f64) -> Self {
        let mut search = Self {
            fft: Transform::forward(FFT_LEN),
            window: hann(FFT_LEN),
            recent: vec![Complex::new(0.0, 0.0); FFT_LEN],
            at: 0,
            since_hop: 0,
            spectrum: vec![Complex::new(0.0, 0.0); FFT_LEN],
            power: vec![0.0; FFT_LEN],
            floor: vec![0.0; FFT_LEN],
            primed: false,
            half_bins: 0,
        };
        search.set_bandwidth(bandwidth_hz);
        search
    }

    pub(crate) fn set_bandwidth(&mut self, bandwidth_hz: f64) {
        let bin_hz = RATE / FFT_LEN as f64;
        self.half_bins = ((bandwidth_hz / 2.0 / bin_hz) as usize).min(FFT_LEN / 2 - 2);
    }

    pub(crate) fn reset(&mut self) {
        self.recent.fill(Complex::new(0.0, 0.0));
        self.at = 0;
        self.since_hop = 0;
        self.primed = false;
    }

    pub(crate) fn feed(&mut self, iq: &[Complex<f32>]) {
        for &sample in iq {
            self.store(sample);
        }
        self.since_hop = 0;
    }

    fn store(&mut self, sample: Complex<f32>) {
        self.recent[self.at] = sample;
        self.at = (self.at + 1) % FFT_LEN;
    }

    pub(crate) fn push(&mut self, sample: Complex<f32>) -> Option<f64> {
        self.store(sample);
        self.since_hop += 1;
        if self.since_hop < HOP {
            return None;
        }
        self.since_hop = 0;
        self.scan()
    }

    fn scan(&mut self) -> Option<f64> {
        for (k, slot) in self.spectrum.iter_mut().enumerate() {
            *slot = self.recent[(self.at + k) % FFT_LEN] * self.window[k];
        }
        self.fft.process(&mut self.spectrum);
        for (power, bin) in self.power.iter_mut().zip(&self.spectrum) {
            *power = bin.norm_sqr();
        }
        let found = self.primed.then(|| self.strongest_new_bin()).flatten();
        match found {
            Some(bin) => Some(self.frequency(bin)),
            None => {
                self.learn();
                None
            }
        }
    }

    fn bins(&self) -> impl Iterator<Item = usize> + '_ {
        (0..=self.half_bins).chain(FFT_LEN - self.half_bins..FFT_LEN)
    }

    fn strongest_new_bin(&self) -> Option<usize> {
        let count = (2 * self.half_bins + 1) as f32;
        let mean = self.bins().map(|k| self.power[k]).sum::<f32>() / count;
        self.bins()
            .filter(|&k| {
                let reference = self.floor[k].max(mean).max(POWER_FLOOR);
                self.power[k] > DETECT_RATIO * reference
            })
            .max_by(|&a, &b| self.power[a].total_cmp(&self.power[b]))
    }

    fn learn(&mut self) {
        if !self.primed {
            self.floor.copy_from_slice(&self.power);
            self.primed = true;
            return;
        }
        for (floor, &power) in self.floor.iter_mut().zip(&self.power) {
            *floor += FLOOR_WEIGHT * (power - *floor);
        }
    }

    fn frequency(&self, bin: usize) -> f64 {
        let level = |k: usize| f64::from(self.power[k % FFT_LEN].max(POWER_FLOOR)).ln();
        let (left, centre, right) = (level(bin + FFT_LEN - 1), level(bin), level(bin + 1));
        let curvature = left - 2.0 * centre + right;
        let shift = if curvature < 0.0 {
            (0.5 * (left - right) / curvature).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        let signed = if bin > FFT_LEN / 2 {
            bin as f64 - FFT_LEN as f64
        } else {
            bin as f64
        };
        (signed + shift) * RATE / FFT_LEN as f64
    }
}

pub(crate) struct Demod {
    mixer: Nco,
    baseband: Decimator,
    mixed: Vec<Complex<f32>>,
    work: Vec<Complex<f32>>,
    reference: Vec<Complex<f32>>,
    at: usize,
    filled: usize,
    sum: Complex<f64>,
    quadrature: Vec<f32>,
    matched: RealDecimator,
    smoothed: Vec<f32>,
    sync: BitSync,
}

impl Demod {
    pub(crate) fn new() -> Self {
        Self {
            mixer: Nco::new(0.0, RATE as f32),
            baseband: Decimator::new(
                &design_lowpass(BASEBAND_TAPS, BASEBAND_CUTOFF_HZ / RATE),
                DECIMATION,
            ),
            mixed: Vec::new(),
            work: Vec::new(),
            reference: vec![Complex::new(0.0, 0.0); REFERENCE_LEN],
            at: 0,
            filled: 0,
            sum: Complex::new(0.0, 0.0),
            quadrature: Vec::new(),
            matched: RealDecimator::new(&[1.0 / SAMPLES_PER_CHIP as f32; SAMPLES_PER_CHIP], 1),
            smoothed: Vec::new(),
            sync: BitSync::new(WORK_RATE, CHIP_RATE),
        }
    }

    pub(crate) fn start(&mut self, carrier_hz: f64) {
        self.mixer.set_freq(-carrier_hz as f32, RATE as f32);
        self.mixer.reset();
        self.baseband.reset();
        self.reference.fill(Complex::new(0.0, 0.0));
        self.at = 0;
        self.filled = 0;
        self.sum = Complex::new(0.0, 0.0);
        self.matched.reset();
        self.sync.reset();
    }

    pub(crate) fn chips(&mut self, iq: &[Complex<f32>], out: &mut Vec<f32>) {
        self.mixed.clear();
        let mixer = &mut self.mixer;
        self.mixed
            .extend(iq.iter().map(|&sample| sample * mixer.next_sample()));
        self.baseband.process(&self.mixed, &mut self.work);
        self.quadrature.clear();
        for k in 0..self.work.len() {
            let sample = self.work[k];
            if let Some(value) = self.coherent(sample) {
                self.quadrature.push(value);
            }
        }
        self.matched.process(&self.quadrature, &mut self.smoothed);
        out.clear();
        out.extend(
            self.smoothed
                .iter()
                .filter_map(|&sample| self.sync.push_soft(sample)),
        );
    }

    fn coherent(&mut self, sample: Complex<f32>) -> Option<f32> {
        let leaving = self.reference[self.at];
        self.sum += to_c64(sample) - to_c64(leaving);
        self.reference[self.at] = sample;
        self.at = (self.at + 1) % REFERENCE_LEN;
        if self.filled < REFERENCE_LEN {
            self.filled += 1;
            return None;
        }
        let centre = to_c64(self.reference[(self.at + REFERENCE_LEN / 2) % REFERENCE_LEN]);
        let magnitude = self.sum.norm();
        if magnitude <= 0.0 {
            return Some(0.0);
        }
        Some((centre * self.sum.conj()).im as f32 / magnitude as f32)
    }
}

fn to_c64(sample: Complex<f32>) -> Complex<f64> {
    Complex::new(f64::from(sample.re), f64::from(sample.im))
}
