use num_complex::Complex;
use sdrmm_dsp::math::fft::Transform;

/// How far off the carrier itself may sit: L-band aircraft Doppler near 1.25 kHz plus LNB drift.
pub(super) const PULL_IN_HZ: f64 = 3_000.0;
const LINE_BALANCE: f32 = 4.0;
const PAIRS: usize = 4;
const LONES: usize = 2;
const CANDIDATE_SHARE: f32 = 0.4;
const PEAK_OVER_MEAN: f32 = 1.0;
const SEPARATION_BINS: i64 = 4;
const LINE_HALF_BINS: i64 = 1;
const TRIAL_SECONDS: f64 = 0.6;
const MIN_SHIFT_HZ: f64 = 1.0;
const AVERAGE_SECONDS: f64 = 1.0;
const SEARCH_SECONDS: f64 = 0.6;

/// Both lines of the pair must stand up. Unscrambled data puts a strong line at the carrier
/// itself, which alone would win half the line spacing off; summing keeps noise sensitivity.
fn pair_score(left: f32, right: f32) -> f32 {
    if LINE_BALANCE * left.min(right) < left.max(right) {
        return 0.0;
    }
    left + right
}

fn blocks_in(seconds: f64, size: usize, fs: f64) -> u32 {
    (seconds * fs / size as f64).ceil().max(1.0) as u32
}

/// The demod's own verdict after a block: whether it holds lock, and its constellation error.
#[derive(Clone, Copy)]
pub(super) struct Lock {
    pub(super) locked: bool,
    pub(super) error: f32,
}

impl Lock {
    fn held_error(self) -> f32 {
        if self.locked {
            self.error
        } else {
            f32::INFINITY
        }
    }
}

#[derive(Clone, Copy)]
struct Candidate {
    bin: i64,
    score: f32,
    error: f32,
}

const EMPTY: Candidate = Candidate {
    bin: 0,
    score: 0.0,
    error: f32::INFINITY,
};

/// The best few scores in order, one per peak.
struct Ranked<const N: usize> {
    entries: [Candidate; N],
    count: usize,
}

impl<const N: usize> Ranked<N> {
    const fn new() -> Self {
        Self {
            entries: [EMPTY; N],
            count: 0,
        }
    }

    fn entries(&self) -> &[Candidate] {
        &self.entries[..self.count]
    }

    fn best(&self) -> f32 {
        self.entries()
            .first()
            .map_or(0.0, |candidate| candidate.score)
    }

    fn near(&self, bin: i64) -> Option<usize> {
        self.entries()
            .iter()
            .position(|candidate| (candidate.bin - bin).abs() <= SEPARATION_BINS)
    }

    fn offer(&mut self, bin: i64, score: f32) {
        if let Some(index) = self.near(bin) {
            if self.entries[index].score >= score {
                return;
            }
            self.entries.copy_within(index + 1..self.count, index);
            self.count -= 1;
        }
        let slot = self
            .entries()
            .iter()
            .position(|candidate| candidate.score < score)
            .unwrap_or(self.count);
        if slot >= N {
            return;
        }
        let last = self.count.min(N - 1);
        self.entries.copy_within(slot..last, slot + 1);
        self.entries[slot] = Candidate {
            bin,
            score,
            error: f32::INFINITY,
        };
        self.count = (self.count + 1).min(N);
    }

    /// The entry that held lock with the least error.
    fn held(&self) -> Option<&Candidate> {
        self.entries()
            .iter()
            .filter(|candidate| candidate.error.is_finite())
            .min_by(|a, b| a.error.total_cmp(&b.error))
    }

    /// Drops what falls below a share of the best or below `minimum`.
    fn gate(&mut self, minimum: f32) {
        let floor = (CANDIDATE_SHARE * self.best()).max(minimum);
        self.count = self
            .entries()
            .iter()
            .take_while(|candidate| candidate.score >= floor)
            .count();
    }
}

enum Phase {
    Search,
    Trial(usize),
    Settled,
}

/// Finds the carrier from the line pair of the squared signal. Periodic data puts a comb of
/// such pairs one pattern spacing apart and can leave the carrier only its own lone line, so
/// the best few of each get a trial in turn and the demod's own lock picks among them.
pub(super) struct CoarseAcquisition {
    size: usize,
    resolution: f64,
    tone_bins: i64,
    range_bins: i64,
    search_blocks: u32,
    trial_blocks: u32,
    clean_error: f32,
    smoothing: f32,
    buffer: Vec<Complex<f32>>,
    squared: Vec<Complex<f32>>,
    fft: Transform,
    spectrum: Vec<f32>,
    blocks: u32,
    phase: Phase,
    pairs: Ranked<PAIRS>,
    lones: Ranked<LONES>,
}

impl CoarseAcquisition {
    /// A trial that locks below `clean_error` ends the search at once.
    pub(super) fn new(size: usize, fs: f64, line_offset_hz: f64, clean_error: f32) -> Self {
        let resolution = fs / size as f64;
        Self {
            size,
            resolution,
            tone_bins: (line_offset_hz / resolution).round() as i64,
            range_bins: (2.0 * PULL_IN_HZ / resolution) as i64,
            search_blocks: blocks_in(SEARCH_SECONDS, size, fs),
            trial_blocks: blocks_in(TRIAL_SECONDS, size, fs),
            clean_error,
            smoothing: (size as f64 / fs / AVERAGE_SECONDS).min(0.5) as f32,
            buffer: Vec::with_capacity(size),
            squared: vec![Complex::new(0.0, 0.0); size],
            fft: Transform::forward(size),
            spectrum: vec![0.0; size],
            blocks: 0,
            phase: Phase::Search,
            pairs: Ranked::new(),
            lones: Ranked::new(),
        }
    }

    /// Takes one raw sample; true once a block is full and [`Self::decide`] is due.
    pub(super) fn push(&mut self, sample: Complex<f32>) -> bool {
        self.buffer.push(sample);
        self.buffer.len() >= self.size
    }

    /// The carrier to tune to next, when it differs from `tuned_hz`.
    pub(super) fn decide(&mut self, tuned_hz: f64, lock: Lock) -> Option<f64> {
        let target = self.step(tuned_hz, lock);
        self.buffer.clear();
        target.filter(|hz| (hz - tuned_hz).abs() > MIN_SHIFT_HZ)
    }

    fn step(&mut self, tuned_hz: f64, lock: Lock) -> Option<f64> {
        let settled = matches!(self.phase, Phase::Settled);
        if settled && lock.locked {
            self.blocks = 0;
            return None;
        }
        self.accumulate();
        if let Phase::Trial(index) = self.phase {
            return self.trial(index, lock);
        }
        self.blocks += 1;
        if self.blocks < self.search_blocks {
            return None;
        }
        self.blocks = 0;
        self.rank();
        if settled && self.offers(tuned_hz) {
            return None;
        }
        self.begin_trial(0)
    }

    fn accumulate(&mut self) {
        for (squared, value) in self.squared.iter_mut().zip(&self.buffer) {
            *squared = value * value;
        }
        self.fft.process(&mut self.squared);
        for (bin, value) in self.spectrum.iter_mut().zip(&self.squared) {
            *bin += self.smoothing * (value.norm_sqr() - *bin);
        }
    }

    fn candidates(&self) -> impl Iterator<Item = &Candidate> {
        self.pairs.entries().iter().chain(self.lones.entries())
    }

    fn candidate_mut(&mut self, index: usize) -> Option<&mut Candidate> {
        let paired = self.pairs.count;
        if index < paired {
            return self.pairs.entries.get_mut(index);
        }
        self.lones.entries[..self.lones.count].get_mut(index - paired)
    }

    /// A lost lock whose carrier still ranks needs no new round of trials.
    fn offers(&self, tuned_hz: f64) -> bool {
        let tolerance = SEPARATION_BINS as f64 * self.resolution / 2.0;
        self.candidates()
            .any(|candidate| (self.hz(candidate.bin) - tuned_hz).abs() <= tolerance)
    }

    fn begin_trial(&mut self, index: usize) -> Option<f64> {
        let bin = self.candidate_mut(index)?.bin;
        self.phase = Phase::Trial(index);
        Some(self.hz(bin))
    }

    /// Lines that are not pairs mean periodic data, whose comb can hold a false lock, so each
    /// pair is tried unless one locks clean and the least error wins. Otherwise the first lock
    /// is the carrier.
    fn line_rich(&self) -> bool {
        self.lones.count > 0
    }

    fn holds(&self, lock: Lock) -> bool {
        lock.held_error() < self.clean_error || (lock.locked && !self.line_rich())
    }

    fn trial(&mut self, index: usize, lock: Lock) -> Option<f64> {
        self.blocks += 1;
        if self.holds(lock) {
            self.phase = Phase::Settled;
            self.blocks = 0;
            return None;
        }
        if self.blocks < self.trial_blocks {
            return None;
        }
        self.blocks = 0;
        if let Some(candidate) = self.candidate_mut(index) {
            candidate.error = lock.held_error();
        }
        let pairs_held = index + 1 == self.pairs.count && self.pairs.held().is_some();
        if !pairs_held && let Some(next) = self.begin_trial(index + 1) {
            return Some(next);
        }
        let held = self.pairs.held().or_else(|| self.lones.held());
        let (phase, bin) = match held {
            Some(candidate) => (Phase::Settled, candidate.bin),
            None => (Phase::Search, self.candidates().next()?.bin),
        };
        self.phase = phase;
        Some(self.hz(bin))
    }

    fn hz(&self, bin: i64) -> f64 {
        bin as f64 * self.resolution / 2.0
    }

    fn line(&self, center: i64) -> f32 {
        let size = self.size as i64;
        (center - LINE_HALF_BINS..=center + LINE_HALF_BINS)
            .map(|bin| self.spectrum[bin.rem_euclid(size) as usize])
            .sum()
    }

    /// What a line must beat to count: the mean, so window leakage never ranks.
    fn floor(&self) -> f32 {
        let mean = self.spectrum.iter().sum::<f32>() / self.size as f32;
        PEAK_OVER_MEAN * mean * (2 * LINE_HALF_BINS + 1) as f32
    }

    /// Ranks line pairs, then lone lines as carriers in their own right. A lone line on a ranked
    /// pair or its partners says nothing new, and one weaker than the best pair is noise. Lone
    /// lines get a trial only when no pair holds lock.
    fn rank(&mut self) {
        self.pairs.count = 0;
        self.lones.count = 0;
        let floor = self.floor();
        for k in -self.range_bins..=self.range_bins {
            let score = pair_score(self.line(k - self.tone_bins), self.line(k + self.tone_bins));
            if score > 2.0 * floor {
                self.pairs.offer(k, score);
            }
        }
        self.pairs.gate(0.0);
        for k in -self.range_bins..=self.range_bins {
            let partner = [k - self.tone_bins, k, k + self.tone_bins]
                .iter()
                .any(|&center| self.pairs.near(center).is_some());
            let line = self.line(k);
            if !partner && line > floor {
                self.lones.offer(k, line);
            }
        }
        self.lones.gate(self.pairs.best());
    }
}
