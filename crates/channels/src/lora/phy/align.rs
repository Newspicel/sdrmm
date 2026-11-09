use super::{
    chirp::{Slope, wrap},
    filter::Window,
    payload::Lock,
    receiver::{Settings, Tools, threshold},
    ring::Ring,
    soft::Statistics,
};

const SYNC_SPACING: f64 = 8.0;
const REFINEMENTS: usize = 3;
const PAYLOAD_DELAY_SYMBOLS: f64 = 2.25;
const LOOKAHEAD_CHIPS: f64 = 2.0;
const PLAUSIBLE_MARGIN: f32 = 2.5;
const MAX_STEP_BINS: f64 = 3.0;
/// A refinement that moves the grid by less than this, in chips and in bins, ends the search.
const SETTLED: f64 = 0.05;

pub(crate) struct Candidate {
    pub up_bins: f64,
    pub down_bins: f64,
    pub window_start: u64,
}

pub(crate) enum Outcome {
    Waiting,
    Rejected,
    Locked(Box<Lock>),
}

#[derive(Clone, Copy, Debug)]
struct Grid {
    sfd: f64,
    cfo_bins: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct Reading {
    bins: f64,
    ratio: f32,
    spread: f32,
    energy: f32,
    noise: f32,
}

fn extract(tools: &mut Tools, ring: &Ring, settings: &Settings, start_chip: f64, cfo_bins: f64) {
    let window = Window {
        start_chip,
        cfo_bins,
        inverted: settings.inverted,
    };
    tools.extractor.extract(ring, window, &mut tools.samples);
}

fn dechirped(tools: &mut Tools, slope: Slope) -> Reading {
    tools.dechirper.transform(&tools.samples, slope);
    let dechirper = &tools.dechirper;
    let peak = dechirper.peak();
    let energies = dechirper.energies();
    let n = energies.len();
    let around = energies[(peak.bin + n - 1) % n] + peak.energy + energies[(peak.bin + 1) % n];
    Reading {
        bins: peak.bin as f64 + dechirper.fraction(peak.bin),
        ratio: peak.ratio(),
        spread: if peak.mean > 0.0 {
            around / peak.mean
        } else {
            0.0
        },
        energy: peak.energy,
        noise: dechirper.noise(peak.bin),
    }
}

struct Sfd {
    preamble: [Reading; 2],
    sync: [Reading; 2],
    down: [Reading; 2],
}

fn at_offset(
    tools: &mut Tools,
    ring: &Ring,
    settings: &Settings,
    grid: Grid,
    offset: f64,
    slope: Slope,
) -> Reading {
    let n = (1u64 << settings.spreading_factor) as f64;
    extract(tools, ring, settings, grid.sfd + offset * n, grid.cfo_bins);
    dechirped(tools, slope)
}

const AROUND: usize = 5;

/// Both slopes read on the five symbols around the coarse SFD, from two before it to two after.
struct Boundary {
    up: [Reading; AROUND],
    down: [Reading; AROUND],
}

impl Boundary {
    fn read(tools: &mut Tools, ring: &Ring, settings: &Settings, grid: Grid) -> Self {
        let n = (1u64 << settings.spreading_factor) as f64;
        let mut up = [Reading::default(); AROUND];
        let mut down = [Reading::default(); AROUND];
        for (k, (up, down)) in up.iter_mut().zip(&mut down).enumerate() {
            let offset = k as f64 - (AROUND / 2) as f64;
            extract(tools, ring, settings, grid.sfd + offset * n, grid.cfo_bins);
            *up = dechirped(tools, Slope::Up);
            *down = dechirped(tools, Slope::Down);
        }
        Self { up, down }
    }

    /// How well an SFD at symbol `at` explains the readings: the last sync symbol rises, both
    /// downchirps fall, and each of the three reads weak the other way. A neighbouring guess
    /// swaps a strong reading for a weak one on both sides of the sum.
    fn evidence(&self, at: usize) -> f32 {
        let rising = self.up[at - 1].spread - self.down[at - 1].spread;
        let falling = |k: usize| self.down[k].spread - self.up[k].spread;
        rising + falling(at) + falling(at + 1)
    }
}

impl Sfd {
    fn score(&self) -> f32 {
        self.readings().map(|r| r.ratio).sum()
    }

    /// Moves the grid so preamble and downchirps land on bin zero. A reading whose peak sits
    /// far from zero is a noise peak at low SNR, not an offset, and is left out.
    fn refine(&self, grid: Grid, n: f64) -> Grid {
        let mean = |readings: &[Reading; 2]| {
            let (sum, count) = readings
                .iter()
                .map(|r| wrap(r.bins, n))
                .filter(|bins| bins.abs() <= MAX_STEP_BINS)
                .fold((0.0, 0.0), |(sum, count), bins| (sum + bins, count + 1.0));
            if count > 0.0 { sum / count } else { 0.0 }
        };
        let up = mean(&self.preamble);
        let down = mean(&self.down);
        Grid {
            sfd: grid.sfd - (up - down) / 2.0,
            cfo_bins: grid.cfo_bins + (up + down) / 2.0,
        }
    }

    fn sync_word(&self, n: f64) -> u8 {
        let nibble = |r: &Reading| {
            ((wrap(r.bins, n).rem_euclid(n) / SYNC_SPACING).round() as i64).rem_euclid(16) as u8
        };
        (nibble(&self.sync[0]) << 4) | nibble(&self.sync[1])
    }

    fn statistics(&self) -> Statistics {
        let noise = self.down.iter().map(|r| r.noise).sum::<f32>() / 2.0;
        let energy = self.down.iter().map(|r| r.energy).sum::<f32>() / 2.0;
        Statistics {
            amplitude: (energy - noise).max(noise).sqrt(),
            noise,
        }
    }

    fn readings(&self) -> impl Iterator<Item = &Reading> {
        self.preamble.iter().chain(&self.sync).chain(&self.down)
    }

    /// Every reading stands clear of the noise on its own.
    fn confident(&self, chips: usize) -> bool {
        let limit = threshold(chips);
        self.readings().all(|r| r.ratio >= limit)
    }

    /// Together the readings stand clear of what noise alone scores, though some may not; such a
    /// lock is kept only if its CRC holds.
    fn plausible(&self, chips: usize) -> bool {
        let ln = (chips as f32).ln();
        let floor = ln + ln.ln() + PLAUSIBLE_MARGIN;
        self.score() >= floor * self.readings().count() as f32
    }
}

fn coarse(candidate: &Candidate, n: f64) -> Grid {
    let cfo_bins = wrap(candidate.up_bins + candidate.down_bins, n) / 2.0;
    let late = (candidate.up_bins - cfo_bins).rem_euclid(n);
    Grid {
        sfd: candidate.window_start as f64 - late,
        cfo_bins,
    }
}

pub(crate) fn lock(
    candidate: &Candidate,
    settings: &Settings,
    tools: &mut Tools,
    ring: &Ring,
) -> Outcome {
    let chips = 1usize << settings.spreading_factor;
    let n = chips as f64;
    let boundary = coarse(candidate, n);
    let latest = Window {
        start_chip: boundary.sfd + 2.0 * n + LOOKAHEAD_CHIPS,
        cfo_bins: boundary.cfo_bins,
        inverted: settings.inverted,
    };
    if latest.last_wide_index(chips) >= ring.end() {
        return Outcome::Waiting;
    }
    let around = Boundary::read(tools, ring, settings, boundary);
    let Some(at) =
        (1..AROUND - 1).max_by(|&a, &b| around.evidence(a).total_cmp(&around.evidence(b)))
    else {
        return Outcome::Rejected;
    };
    let mut grid = Grid {
        sfd: boundary.sfd + (at as f64 - (AROUND / 2) as f64) * n,
        ..boundary
    };
    let mut sfd = Sfd {
        preamble: [
            at_offset(tools, ring, settings, grid, -4.0, Slope::Up),
            at_offset(tools, ring, settings, grid, -3.0, Slope::Up),
        ],
        sync: [
            at_offset(tools, ring, settings, grid, -2.0, Slope::Up),
            around.up[at - 1],
        ],
        down: [around.down[at], around.down[at + 1]],
    };
    for _ in 0..REFINEMENTS {
        let next = sfd.refine(grid, n);
        let settled = (next.sfd - grid.sfd).abs() < SETTLED
            && (next.cfo_bins - grid.cfo_bins).abs() < SETTLED;
        grid = next;
        if settled {
            break;
        }
        sfd.preamble = [
            at_offset(tools, ring, settings, grid, -4.0, Slope::Up),
            at_offset(tools, ring, settings, grid, -3.0, Slope::Up),
        ];
        sfd.down = [
            at_offset(tools, ring, settings, grid, 0.0, Slope::Down),
            at_offset(tools, ring, settings, grid, 1.0, Slope::Down),
        ];
    }
    sfd.sync = [
        at_offset(tools, ring, settings, grid, -2.0, Slope::Up),
        at_offset(tools, ring, settings, grid, -1.0, Slope::Up),
    ];
    if !sfd.plausible(chips) {
        return Outcome::Rejected;
    }
    let statistics = sfd.statistics();
    let snr = (statistics.amplitude * statistics.amplitude / statistics.noise) / chips as f32;
    Outcome::Locked(Box::new(Lock::new(
        settings,
        grid.sfd + PAYLOAD_DELAY_SYMBOLS * n,
        grid.cfo_bins,
        sfd.sync_word(n),
        statistics,
        10.0 * snr.max(f32::MIN_POSITIVE).log10(),
        sfd.confident(chips),
    )))
}
