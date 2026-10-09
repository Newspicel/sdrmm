use sdrmm_dsp::{airtime::Burst, fastmath::fast_power_db};
use sdrmm_wire::{AirtimeSpan, IsmKind, IsmKindLoad, airtime::SURVEY_CENTRES};

use crate::airtime::{bin_offset_hz, fft_len, frame_s};

const MHZ: f64 = 1e6;
const SLOTS: usize = 128;

#[derive(Clone, Copy)]
struct Kind {
    bursts: u32,
    covered: u64,
    until: u64,
    frames: u64,
    peak: f32,
    steady: bool,
    centres: [u32; SLOTS],
}

impl Kind {
    const EMPTY: Self = Self {
        bursts: 0,
        covered: 0,
        until: 0,
        frames: 0,
        peak: f32::NEG_INFINITY,
        steady: false,
        centres: [0; SLOTS],
    };

    fn active(&self) -> bool {
        self.bursts > 0 || self.steady
    }
}

fn cover(covered: &mut u64, until: &mut u64, first: u64, frames: u64) {
    let end = first + frames;
    let start = first.max(*until);
    if end > start {
        *covered += end - start;
        *until = end;
    }
}

pub(crate) struct Tally {
    frequency_hz: f64,
    len: usize,
    base_mhz: f64,
    kinds: [Kind; IsmKind::ALL.len()],
    covered: u64,
    until: u64,
}

impl Tally {
    pub(crate) fn new(frequency_hz: f64, span: AirtimeSpan) -> Self {
        Self {
            frequency_hz,
            len: fft_len(span),
            base_mhz: ((frequency_hz - span.sample_rate_hz() / 2.0) / MHZ).floor(),
            kinds: [Kind::EMPTY; IsmKind::ALL.len()],
            covered: 0,
            until: 0,
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
        cover(
            &mut tally.covered,
            &mut tally.until,
            burst.first,
            burst.frames,
        );
        cover(
            &mut self.covered,
            &mut self.until,
            burst.first,
            burst.frames,
        );
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
            share(self.covered, measured)
        }
    }

    pub(crate) fn report(&mut self, measured: u64) -> Vec<IsmKindLoad> {
        let loads = IsmKind::ALL
            .iter()
            .zip(&self.kinds)
            .filter(|(_, tally)| tally.active())
            .map(|(&kind, tally)| IsmKindLoad {
                kind,
                bursts: tally.bursts,
                airtime: if tally.steady {
                    1.0
                } else {
                    share(tally.covered, measured)
                },
                mean_us: (tally.frames as f64 * frame_s() * 1e6 / f64::from(tally.bursts.max(1)))
                    as f32,
                peak_dbfs: tally.peak,
                centres_mhz: self.centres(&tally.centres),
            })
            .collect();
        for tally in &mut self.kinds {
            let until = tally.until;
            *tally = Kind {
                until,
                ..Kind::EMPTY
            };
        }
        self.covered = 0;
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
