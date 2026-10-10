use sdrmm_dsp::{airtime::Burst, fastmath::fast_power_db};
use sdrmm_wire::{AirtimeSpan, IsmKind, IsmKindLoad, airtime::SURVEY_CENTRES};

use crate::airtime::{bin_offset_hz, fft_len, frame_s};

const MHZ: f64 = 1e6;
const SLOTS: usize = 128;
const WINDOW_WORDS: usize = 512;
const WINDOW_FRAMES: u64 = WINDOW_WORDS as u64 * 64;

#[derive(Clone, Copy)]
struct Kind {
    bursts: u32,
    frames: u64,
    peak: f32,
    steady: bool,
    centres: [u32; SLOTS],
}

impl Kind {
    const EMPTY: Self = Self {
        bursts: 0,
        frames: 0,
        peak: f32::NEG_INFINITY,
        steady: false,
        centres: [0; SLOTS],
    };

    fn active(&self) -> bool {
        self.bursts > 0 || self.steady
    }
}

struct Coverage {
    marked: Vec<u64>,
    horizon: u64,
    covered: u64,
}

impl Coverage {
    fn new() -> Self {
        Self {
            marked: vec![0; WINDOW_WORDS],
            horizon: 0,
            covered: 0,
        }
    }

    fn slot(frame: u64) -> (usize, u64) {
        ((frame / 64) as usize % WINDOW_WORDS, 1 << (frame % 64))
    }

    fn mark(&mut self, first: u64, frames: u64) {
        let end = first + frames;
        if end > self.horizon {
            for frame in self.horizon.max(end.saturating_sub(WINDOW_FRAMES))..end {
                let (word, bit) = Self::slot(frame);
                self.marked[word] &= !bit;
            }
            self.horizon = end;
        }
        for frame in first.max(self.horizon.saturating_sub(WINDOW_FRAMES))..end {
            let (word, bit) = Self::slot(frame);
            if self.marked[word] & bit == 0 {
                self.marked[word] |= bit;
                self.covered += 1;
            }
        }
    }
}

pub(crate) struct Tally {
    frequency_hz: f64,
    len: usize,
    base_mhz: f64,
    kinds: [Kind; IsmKind::ALL.len()],
    kind_coverage: [Coverage; IsmKind::ALL.len()],
    coverage: Coverage,
}

impl Tally {
    pub(crate) fn new(frequency_hz: f64, span: AirtimeSpan) -> Self {
        Self {
            frequency_hz,
            len: fft_len(span),
            base_mhz: ((frequency_hz - span.sample_rate_hz() / 2.0) / MHZ).floor(),
            kinds: [Kind::EMPTY; IsmKind::ALL.len()],
            kind_coverage: std::array::from_fn(|_| Coverage::new()),
            coverage: Coverage::new(),
        }
    }

    fn slot(&self, centre_hz: f64) -> usize {
        ((centre_hz / MHZ).round() - self.base_mhz).clamp(0.0, (SLOTS - 1) as f64) as usize
    }

    pub(crate) fn add(&mut self, burst: &Burst, kind: IsmKind) {
        let centre = self.frequency_hz + bin_offset_hz(f64::from(burst.centre), self.len);
        let slot = self.slot(centre);
        let tally = &mut self.kinds[kind as usize];
        tally.bursts += 1;
        tally.frames += burst.frames;
        tally.peak = tally.peak.max(fast_power_db(burst.peak));
        tally.centres[slot] += 1;
        self.kind_coverage[kind as usize].mark(burst.first, burst.frames);
        self.coverage.mark(burst.first, burst.frames);
    }

    pub(crate) fn raised(&mut self, bins: impl Iterator<Item = (usize, f32)>) {
        for (bin, floor_db) in bins {
            let centre = self.frequency_hz + bin_offset_hz(bin as f64, self.len);
            let slot = self.slot(centre);
            let tally = &mut self.kinds[IsmKind::Continuous as usize];
            tally.steady = true;
            tally.peak = tally.peak.max(floor_db);
            tally.centres[slot] += 1;
        }
    }

    pub(crate) fn busy(&self, measured: u64) -> f32 {
        let steady = self.kinds[IsmKind::Continuous as usize].steady;
        if steady {
            1.0
        } else {
            share(self.coverage.covered, measured)
        }
    }

    pub(crate) fn report(&mut self, measured: u64) -> Vec<IsmKindLoad> {
        let loads = IsmKind::ALL
            .iter()
            .zip(&self.kinds)
            .zip(&self.kind_coverage)
            .filter(|((_, tally), _)| tally.active())
            .map(|((&kind, tally), coverage)| IsmKindLoad {
                kind,
                bursts: tally.bursts,
                airtime: if tally.steady {
                    1.0
                } else {
                    share(coverage.covered, measured)
                },
                mean_us: (tally.frames as f64 * frame_s() * 1e6 / f64::from(tally.bursts.max(1)))
                    as f32,
                peak_dbfs: tally.peak,
                centres_mhz: self.centres(&tally.centres),
            })
            .collect();
        self.kinds = [Kind::EMPTY; IsmKind::ALL.len()];
        for coverage in &mut self.kind_coverage {
            coverage.covered = 0;
        }
        self.coverage.covered = 0;
        loads
    }

    fn centres(&self, counts: &[u32; SLOTS]) -> Vec<f64> {
        let mut slots: Vec<usize> = (0..SLOTS).filter(|&slot| counts[slot] > 0).collect();
        slots.sort_by_key(|&slot| std::cmp::Reverse(counts[slot]));
        slots.truncate(SURVEY_CENTRES);
        slots
            .into_iter()
            .map(|slot| self.base_mhz + slot as f64)
            .collect()
    }
}

fn share(covered: u64, measured: u64) -> f32 {
    (covered as f64 / measured.max(1) as f64).min(1.0) as f32
}
