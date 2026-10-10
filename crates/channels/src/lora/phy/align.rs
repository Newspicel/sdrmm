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
    energy: f32,
    noise: f32,
}

fn read(
    tools: &mut Tools,
    ring: &Ring,
    settings: &Settings,
    start_chip: f64,
    cfo_bins: f64,
    slope: Slope,
) -> Reading {
    let window = Window {
        start_chip,
        cfo_bins,
        inverted: settings.inverted,
    };
    tools.extractor.extract(ring, window, &mut tools.samples);
    tools.dechirper.transform(&tools.samples, slope);
    let peak = tools.dechirper.peak();
    Reading {
        bins: peak.bin as f64 + tools.dechirper.fraction(peak.bin),
        ratio: peak.ratio(),
        energy: peak.energy,
        noise: tools.dechirper.median_noise(),
    }
}

struct Sfd {
    preamble: [Reading; 2],
    sync: [Reading; 2],
    down: [Reading; 2],
}

fn readings(tools: &mut Tools, ring: &Ring, settings: &Settings, grid: Grid) -> Sfd {
    let n = (1u64 << settings.spreading_factor) as f64;
    let mut at = |offset: f64, slope| {
        read(
            tools,
            ring,
            settings,
            grid.sfd + offset * n,
            grid.cfo_bins,
            slope,
        )
    };
    Sfd {
        preamble: [at(-4.0, Slope::Up), at(-3.0, Slope::Up)],
        sync: [at(-2.0, Slope::Up), at(-1.0, Slope::Up)],
        down: [at(0.0, Slope::Down), at(1.0, Slope::Down)],
    }
}

impl Sfd {
    fn score(&self) -> f32 {
        self.preamble
            .iter()
            .chain(&self.sync)
            .chain(&self.down)
            .map(|r| r.ratio)
            .sum()
    }

    fn refine(&self, grid: Grid, n: f64) -> Grid {
        let mean =
            |readings: &[Reading; 2]| readings.iter().map(|r| wrap(r.bins, n)).sum::<f64>() / 2.0;
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

    fn valid(&self, chips: usize) -> bool {
        let limit = threshold(chips);
        self.preamble
            .iter()
            .chain(&self.sync)
            .chain(&self.down)
            .all(|r| r.ratio >= limit)
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
    let Some((mut grid, mut sfd)) = [-1.0, 0.0, 1.0]
        .into_iter()
        .map(|shift| {
            let grid = Grid {
                sfd: boundary.sfd + shift * n,
                ..boundary
            };
            (grid, readings(tools, ring, settings, grid))
        })
        .max_by(|a, b| a.1.score().total_cmp(&b.1.score()))
    else {
        return Outcome::Rejected;
    };
    for _ in 0..REFINEMENTS {
        grid = sfd.refine(grid, n);
        sfd = readings(tools, ring, settings, grid);
    }
    if !sfd.valid(chips) {
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
    )))
}
