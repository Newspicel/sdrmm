//! The search JT65 and JT9 share. Both send one minute of slow MFSK whose lowest tone is a sync
//! tone, keyed on in a fixed pattern of symbols and never used for data, so a transmission shows
//! where that tone's power follows the pattern.

use std::{f64::consts::TAU, sync::Arc};

use num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};
use sdrmm_dsp::math::{fft::Transform, stats::quantile};

pub(crate) const SAMPLE_RATE: f64 = 12_000.0;
pub(crate) const SLOT_SAMPLES: usize = 720_000;
pub(crate) const MAX_TONES: usize = 66;
const SLOT_S: f64 = SLOT_SAMPLES as f64 / SAMPLE_RATE;
const FRAMES_PER_SYMBOL: usize = 4;
const EARLY_FRAMES: isize = 8;
const NOISE_QUANTILE: f32 = 0.4;
const COARSE_TIME_STEPS: i32 = 4;
const COARSE_FREQUENCY_STEPS: i32 = 8;
const DRIFT_STEPS: i32 = 4;
const DRIFT_STEP_HZ: f64 = 0.5;
const FINE_STEPS: i32 = 4;

/// What sets one mode's sync search apart.
pub(crate) struct Layout {
    pub(crate) symbol_samples: f64,
    /// 1 where the sync tone is sent, 0 where a data tone is.
    pub(crate) sync: &'static [u8],
    /// Tone slots from the sync tone up, the sync tone included.
    pub(crate) tones: usize,
    pub(crate) frame_fft: usize,
    /// Bins of the slot's spectrum kept around a candidate: the baseband rate times the slot length.
    pub(crate) baseband_bins: usize,
    /// The least sync-tone contrast, in noise units, worth a closer look.
    pub(crate) min_score: f32,
}

impl Layout {
    pub(crate) fn tone_hz(&self) -> f64 {
        SAMPLE_RATE / self.symbol_samples
    }

    pub(crate) fn symbols(&self) -> usize {
        self.sync.len()
    }

    fn sync_symbols(&self) -> usize {
        self.sync.iter().map(|&flag| usize::from(flag)).sum()
    }

    fn baseband_rate(&self) -> f64 {
        self.baseband_bins as f64 / SLOT_S
    }

    fn baseband_symbol(&self) -> f64 {
        self.symbol_samples * self.baseband_rate() / SAMPLE_RATE
    }

    fn centre_hz(&self) -> f64 {
        (self.tones - 1) as f64 / 2.0 * self.tone_hz()
    }
}

/// A message read from a slot: its sync tone frequency, where it starts in the slot, its SNR in
/// 2500 Hz and how many data symbols its strongest tone got wrong.
#[derive(Clone, Debug)]
pub(crate) struct Heard {
    pub(crate) text: String,
    pub(crate) frequency_hz: f32,
    pub(crate) start_s: f32,
    pub(crate) snr_db: f32,
    pub(crate) hard_errors: u32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Candidate {
    pub(crate) frequency_hz: f64,
    pub(crate) start_s: f64,
    pub(crate) score: f32,
}

/// A transmission pinned down: its sync tone frequency at mid-transmission, where it starts in
/// the slot, and the share of sync-symbol power its sync tone holds.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Lock {
    pub(crate) frequency_hz: f64,
    pub(crate) start_s: f64,
    pub(crate) sync: f32,
    track: Track,
}

#[derive(Clone, Copy, Debug)]
struct Track {
    start: f64,
    offset_hz: f64,
    drift_hz: f64,
}

/// Power per tone slot, per symbol.
pub(crate) type Spectra = Vec<[f32; MAX_TONES]>;

pub(crate) struct SyncTone {
    layout: &'static Layout,
    forward: Arc<dyn RealToComplex<f32>>,
    audio: Vec<f32>,
    input: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    frame: Arc<dyn RealToComplex<f32>>,
    frame_input: Vec<f32>,
    frame_output: Vec<Complex<f32>>,
    frame_scratch: Vec<Complex<f32>>,
    inverse: Transform,
    baseband: Vec<Complex<f32>>,
    power: Vec<f32>,
    low_bin: usize,
    bins: usize,
    frames: usize,
    noise: Vec<f32>,
}

impl SyncTone {
    pub(crate) fn new(layout: &'static Layout) -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let forward = planner.plan_fft_forward(SLOT_SAMPLES);
        let frame = planner.plan_fft_forward(layout.frame_fft);
        Self {
            audio: Vec::with_capacity(SLOT_SAMPLES),
            input: forward.make_input_vec(),
            spectrum: forward.make_output_vec(),
            scratch: forward.make_scratch_vec(),
            forward,
            frame_input: frame.make_input_vec(),
            frame_output: frame.make_output_vec(),
            frame_scratch: frame.make_scratch_vec(),
            frame,
            inverse: Transform::inverse(layout.baseband_bins),
            baseband: vec![Complex::default(); layout.baseband_bins],
            power: Vec::new(),
            low_bin: 0,
            bins: 0,
            frames: 0,
            noise: Vec::new(),
            layout,
        }
    }

    /// Takes one slot of audio and finds where in `low_hz..high_hz` a sync tone could be.
    pub(crate) fn load(&mut self, samples: &[f32], low_hz: f32, high_hz: f32) {
        self.audio.clear();
        self.audio
            .extend_from_slice(&samples[..samples.len().min(SLOT_SAMPLES)]);
        self.transform();
        let audio = std::mem::take(&mut self.audio);
        self.spectrogram(&audio, low_hz, high_hz);
        self.audio = audio;
    }

    fn transform(&mut self) {
        self.input.fill(0.0);
        self.input[..self.audio.len()].copy_from_slice(&self.audio);
        if self
            .forward
            .process_with_scratch(&mut self.input, &mut self.spectrum, &mut self.scratch)
            .is_err()
        {
            self.spectrum.fill(Complex::default());
        }
    }

    /// Takes a decoded transmission out of the slot, one symbol at a time at the amplitude and
    /// phase each was heard with, so what it left behind is not mistaken for other stations.
    pub(crate) fn subtract(&mut self, lock: &Lock, tones: &[u8]) {
        let layout = self.layout;
        let first = lock.start_s * SAMPLE_RATE;
        for (symbol, &tone) in tones.iter().enumerate() {
            let begin = (first + symbol as f64 * layout.symbol_samples).round();
            let end = (first + (symbol + 1) as f64 * layout.symbol_samples).round();
            let progress = (symbol as f64 + 0.5) / layout.symbols() as f64 - 0.5;
            let frequency = lock.frequency_hz
                + f64::from(tone) * layout.tone_hz()
                + lock.track.drift_hz * progress;
            self.cancel(begin, end, frequency);
        }
        self.transform();
    }

    fn cancel(&mut self, begin: f64, end: f64, frequency_hz: f64) {
        let length = self.audio.len() as f64;
        let (low, high) = (begin.clamp(0.0, length), end.clamp(0.0, length));
        if high <= low {
            return;
        }
        let span = &mut self.audio[low as usize..high as usize];
        let step = Complex::from_polar(1.0, -TAU * frequency_hz / SAMPLE_RATE);
        let start = Complex::from_polar(1.0, -TAU * frequency_hz * (low - begin) / SAMPLE_RATE);
        let mut phasor = start;
        let mut sum = Complex::<f64>::default();
        for &sample in span.iter() {
            sum += phasor * f64::from(sample);
            phasor *= step;
        }
        let amplitude = sum * (2.0 / (end - begin));
        phasor = start;
        for sample in span.iter_mut() {
            *sample -= (amplitude * phasor.conj()).re as f32;
            phasor *= step;
        }
    }

    fn frame_hz(&self) -> f64 {
        SAMPLE_RATE / self.layout.frame_fft as f64
    }

    fn frame_start(&self, frame: usize) -> usize {
        (frame as f64 * self.layout.symbol_samples / FRAMES_PER_SYMBOL as f64).round() as usize
    }

    fn frames_in(&self, length: usize, window: usize) -> usize {
        let Some(room) = length.checked_sub(window) else {
            return 0;
        };
        let step = self.layout.symbol_samples / FRAMES_PER_SYMBOL as f64;
        let mut frames = (room as f64 / step).floor() as usize + 1;
        while frames > 0 && self.frame_start(frames - 1) + window > length {
            frames -= 1;
        }
        frames
    }

    fn spectrogram(&mut self, samples: &[f32], low_hz: f32, high_hz: f32) {
        let window = self.layout.symbol_samples.round() as usize;
        let last = self.layout.frame_fft / 2;
        self.low_bin = ((f64::from(low_hz) / self.frame_hz()).floor() as usize).min(last);
        let high_bin = ((f64::from(high_hz) / self.frame_hz()).ceil() as usize).min(last);
        self.bins = high_bin + 1 - self.low_bin;
        self.frames = self.frames_in(samples.len(), window);
        self.power.clear();
        self.power.resize(self.bins * self.frames, 0.0);
        for frame in 0..self.frames {
            let start = self.frame_start(frame);
            self.frame_input.fill(0.0);
            self.frame_input[..window].copy_from_slice(&samples[start..start + window]);
            if self
                .frame
                .process_with_scratch(
                    &mut self.frame_input,
                    &mut self.frame_output,
                    &mut self.frame_scratch,
                )
                .is_err()
            {
                continue;
            }
            for bin in 0..self.bins {
                self.power[bin * self.frames + frame] =
                    self.frame_output[self.low_bin + bin].norm_sqr();
            }
        }
        self.normalise();
    }

    fn normalise(&mut self) {
        let mut sorted = vec![0.0f32; self.frames];
        self.noise.clear();
        for row in self.power.chunks_exact_mut(self.frames.max(1)) {
            sorted.copy_from_slice(row);
            let floor = quantile(&mut sorted, NOISE_QUANTILE)
                .filter(|&level| level > 0.0)
                .unwrap_or(1.0);
            self.noise.push(floor);
            for value in row.iter_mut() {
                *value /= floor;
            }
        }
    }

    /// The likeliest sync tones in `low_hz..high_hz`, strongest first.
    pub(crate) fn candidates(&self, low_hz: f32, high_hz: f32, limit: usize) -> Vec<Candidate> {
        let reach = (self.layout.symbols() - 1) * FRAMES_PER_SYMBOL + 1;
        if self.frames < reach {
            return Vec::new();
        }
        let lags = -EARLY_FRAMES..=(self.frames - reach) as isize;
        let best: Vec<(f32, isize)> = (0..self.bins)
            .map(|bin| {
                let row = &self.power[bin * self.frames..(bin + 1) * self.frames];
                lags.clone()
                    .map(|lag| (self.pattern_score(row, lag), lag))
                    .fold((f32::MIN, 0), |a, b| if b.0 > a.0 { b } else { a })
            })
            .collect();
        let mut found: Vec<Candidate> = (1..best.len().saturating_sub(1))
            .filter(|&bin| {
                let frequency = (self.low_bin + bin) as f64 * self.frame_hz();
                best[bin].0 >= self.layout.min_score
                    && best[bin].0 > best[bin - 1].0
                    && best[bin].0 >= best[bin + 1].0
                    && (f64::from(low_hz)..=f64::from(high_hz)).contains(&frequency)
            })
            .map(|bin| Candidate {
                frequency_hz: (self.low_bin + bin) as f64 * self.frame_hz(),
                start_s: best[bin].1 as f64 * self.layout.symbol_samples
                    / FRAMES_PER_SYMBOL as f64
                    / SAMPLE_RATE,
                score: best[bin].0,
            })
            .collect();
        found.sort_by(|a, b| b.score.total_cmp(&a.score));
        found.truncate(limit);
        found
    }

    /// Mean normalised power in sync symbols less that in data symbols, for a transmission whose
    /// first symbol starts at frame `lag`; frames before the slot count as silence.
    fn pattern_score(&self, row: &[f32], lag: isize) -> f32 {
        let layout = self.layout;
        let (mut on, mut off) = (0.0f32, 0.0f32);
        for (symbol, &flag) in layout.sync.iter().enumerate() {
            let frame = lag + (symbol * FRAMES_PER_SYMBOL) as isize;
            let value = usize::try_from(frame)
                .ok()
                .and_then(|frame| row.get(frame))
                .copied()
                .unwrap_or(0.0);
            if flag == 1 {
                on += value;
            } else {
                off += value;
            }
        }
        let sync = layout.sync_symbols() as f32;
        on / sync - off / (layout.symbols() as f32 - sync)
    }

    /// Moves a candidate's sync tone to baseband and fits its start, frequency and drift.
    pub(crate) fn lock(&mut self, candidate: &Candidate) -> Lock {
        let layout = self.layout;
        let centre = candidate.frequency_hz + layout.centre_hz();
        self.downconvert(centre);
        let symbol = layout.baseband_symbol();
        let rate = layout.baseband_rate();
        let mut track = Track {
            start: candidate.start_s * rate,
            offset_hz: 0.0,
            drift_hz: 0.0,
        };
        let reach_hz = self.frame_hz();
        track = self.best(track, COARSE_TIME_STEPS, symbol / 8.0, 0, 0.0);
        track = self.best(
            track,
            0,
            0.0,
            COARSE_FREQUENCY_STEPS,
            reach_hz / f64::from(COARSE_FREQUENCY_STEPS),
        );
        track = self.best_drift(track);
        track = self.best(track, COARSE_TIME_STEPS, symbol / 16.0, 0, 0.0);
        track = self.best(
            track,
            FINE_STEPS,
            symbol / 32.0,
            FINE_STEPS,
            layout.tone_hz() / 40.0,
        );
        Lock {
            frequency_hz: candidate.frequency_hz + track.offset_hz,
            start_s: track.start / rate,
            sync: self.contrast(track),
            track,
        }
    }

    fn best(
        &self,
        around: Track,
        time_steps: i32,
        time_step: f64,
        frequency_steps: i32,
        frequency_step: f64,
    ) -> Track {
        let mut best = (around, self.contrast(around));
        for time in -time_steps..=time_steps {
            for frequency in -frequency_steps..=frequency_steps {
                let track = Track {
                    start: around.start + f64::from(time) * time_step,
                    offset_hz: around.offset_hz + f64::from(frequency) * frequency_step,
                    ..around
                };
                let score = self.contrast(track);
                if score > best.1 {
                    best = (track, score);
                }
            }
        }
        best.0
    }

    fn best_drift(&self, around: Track) -> Track {
        let mut best = (around, self.contrast(around));
        for step in -DRIFT_STEPS..=DRIFT_STEPS {
            let track = Track {
                drift_hz: f64::from(step) * DRIFT_STEP_HZ,
                ..around
            };
            let score = self.contrast(track);
            if score > best.1 {
                best = (track, score);
            }
        }
        best.0
    }

    fn downconvert(&mut self, centre_hz: f64) {
        let middle = (centre_hz * SLOT_S).round() as isize;
        let size = self.layout.baseband_bins as isize;
        self.baseband.fill(Complex::default());
        for offset in -size / 2..size / 2 {
            if let Some(&bin) = usize::try_from(middle + offset)
                .ok()
                .and_then(|index| self.spectrum.get(index))
            {
                self.baseband[offset.rem_euclid(size) as usize] = bin;
            }
        }
        self.inverse.process(&mut self.baseband);
    }

    /// The share of power the sync tone holds in sync symbols beyond what it holds in data
    /// symbols, which noise leaves near zero.
    fn contrast(&self, track: Track) -> f32 {
        let layout = self.layout;
        let (mut on, mut off) = (0.0f64, 0.0f64);
        for (symbol, &flag) in layout.sync.iter().enumerate() {
            let power = self.tone_power(track, symbol, 0);
            if flag == 1 {
                on += power;
            } else {
                off += power;
            }
        }
        let sync = layout.sync_symbols() as f64;
        let data = layout.symbols() as f64 - sync;
        let (on, off) = (on / sync, off / data);
        if on + off > 0.0 {
            ((on - off) / (on + off)) as f32
        } else {
            0.0
        }
    }

    fn tone_frequency(&self, track: Track, symbol: usize, tone: usize) -> f64 {
        let layout = self.layout;
        let progress = (symbol as f64 + 0.5) / layout.symbols() as f64 - 0.5;
        tone as f64 * layout.tone_hz() - layout.centre_hz()
            + track.offset_hz
            + track.drift_hz * progress
    }

    fn tone_power(&self, track: Track, symbol: usize, tone: usize) -> f64 {
        self.tone_amplitude(track, symbol, tone).norm_sqr()
    }

    fn tone_amplitude(&self, track: Track, symbol: usize, tone: usize) -> Complex<f64> {
        let layout = self.layout;
        let length = layout.baseband_symbol();
        let begin = (track.start + symbol as f64 * length).round() as isize;
        let samples = length.round() as isize;
        let step = -TAU * self.tone_frequency(track, symbol, tone) / layout.baseband_rate();
        let rotation = Complex::from_polar(1.0, step);
        let first = begin.max(0);
        let mut phasor = Complex::from_polar(1.0, step * (first - begin) as f64);
        let mut sum = Complex::<f64>::default();
        for &sample in self
            .baseband
            .get(
                first as usize
                    ..(begin + samples).clamp(first, self.baseband.len() as isize) as usize,
            )
            .unwrap_or_default()
        {
            sum += Complex::new(f64::from(sample.re), f64::from(sample.im)) * phasor;
            phasor *= rotation;
        }
        sum
    }

    /// Power in every tone slot of every symbol of the transmission [`SyncTone::lock`] last
    /// moved to baseband.
    pub(crate) fn spectra(&self, lock: &Lock) -> Spectra {
        let layout = self.layout;
        let track = lock.track;
        (0..layout.symbols())
            .map(|symbol| {
                let mut row = [0.0f32; MAX_TONES];
                for (tone, slot) in row.iter_mut().take(layout.tones).enumerate() {
                    *slot = self.tone_power(track, symbol, tone) as f32;
                }
                row
            })
            .collect()
    }
}

/// Continuous-phase FSK at 12 kHz: tone `n` of each symbol `n * tone_hz` above `audio_hz`.
#[cfg(any(test, feature = "synth"))]
pub(crate) fn fsk(tones: &[u8], symbol_samples: f64, tone_hz: f64, audio_hz: f32) -> Vec<f32> {
    let mut wave = Vec::new();
    let mut phase = 0.0f64;
    for (symbol, &tone) in tones.iter().enumerate() {
        let begin = (symbol as f64 * symbol_samples).round() as usize;
        let end = ((symbol + 1) as f64 * symbol_samples).round() as usize;
        let step = TAU * (f64::from(audio_hz) + f64::from(tone) * tone_hz) / SAMPLE_RATE;
        for _ in begin..end {
            wave.push(phase.sin() as f32);
            phase = (phase + step).rem_euclid(TAU);
        }
    }
    wave
}
