use super::{
    chirp::{Slope, wrap},
    drift::{Clock, DriftFit, Prior},
    filter::Window,
    payload::{Lock, Start},
    receiver::{Settings, Tools, threshold},
    ring::{Front, Ring},
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
/// Preamble upchirps an eight symbol preamble puts before the sync word.
const PREAMBLE_SYMBOLS: f64 = 8.0;
/// The preamble upchirps the drift fit reads off the narrow stream, all eight of them.
const LOOSE_SYMBOLS: [f64; 8] = [-10.0, -9.0, -8.0, -7.0, -6.0, -5.0, -4.0, -3.0];
/// The preamble upchirps before the refinement's own, read on the grid instead when the narrow
/// stream cannot be trusted.
const FAR_SYMBOLS: [f64; 6] = [-10.0, -9.0, -8.0, -7.0, -6.0, -5.0];
/// The frequency offset, as a share of the band, past which the half-band edge cuts into the
/// narrow stream's chirps and bends their readings from one symbol to the next.
const LOOSE_CFO: f64 = 0.1;
/// The sender clock error the drift fit still finds its readings at, in ppm.
const REACH_PPM: f64 = 150.0;
/// The sender clock error expected before any preamble is read, in ppm.
const PRIOR_PPM: f64 = 50.0;
/// How far the clock may stray from what the frequency offset implies when the carrier is
/// known, in ppm. A real sender's frequency also settles during the preamble, which reads like
/// a clock drift on the upchirps alone, so the crystal's own word weighs more.
const COUPLED_PPM: f64 = 5.0;
const OUTLIER_BINS: f64 = 1.0;
/// The frequency offset, as a share of the band, from which the grid half a symbol and half
/// the band away is tried as well. A sender off by more than a quarter of the band reads like
/// one off by less, here by at least this much.
const TWIN_FROM: f64 = 0.15;
/// The first preamble upchirp the refinement reads, and the first the drift fit relies on when
/// it finds no drift.
const NEAR_SYMBOL: f64 = -4.0;
/// Where the refined grid is on time: halfway between the preamble and downchirp readings.
const REFINED_AT: f64 = -1.5;

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
    /// The grid the preamble and downchirp readings were taken on, which the last refinement
    /// may have moved on from.
    read_on: Grid,
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
        let limit = reach(n, PREAMBLE_SYMBOLS);
        let mean = |readings: &[Reading; 2]| {
            let (sum, count) = readings
                .iter()
                .map(|r| wrap(r.bins, n))
                .filter(|bins| bins.abs() <= limit)
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

/// How far from bin zero a chirp may read and still count: a few bins of coarse error, and as
/// far again as a sender clock off by [`REACH_PPM`] carries it over `symbols`.
fn reach(n: f64, symbols: f64) -> f64 {
    MAX_STEP_BINS + REACH_PPM * 1e-6 * n * symbols
}

/// The preamble upchirp at symbol `k` read off the narrow stream from the whole chip nearest
/// the grid, without the fractional delay and mixing of a grid reading, at a fraction of its
/// cost. The half-band edge may bend such a reading by a constant, so its offset is left free.
fn loose_reading(
    tools: &mut Tools,
    narrow: &Ring,
    settings: &Settings,
    grid: Grid,
    k: f64,
) -> (f64, f32) {
    let n = (1u64 << settings.spreading_factor) as f64;
    let at = grid.sfd + k * n;
    let start = at.round();
    narrow.copy(start as i64, &mut tools.samples);
    if settings.inverted {
        for sample in &mut tools.samples {
            *sample = sample.conj();
        }
    }
    tools.dechirper.transform(&tools.samples, Slope::Up);
    let peak = tools.dechirper.peak();
    let bins = peak.bin as f64 + tools.dechirper.fraction(peak.bin);
    (wrap(bins - (start - at) - grid.cfo_bins, n), peak.ratio())
}

/// What the sender's clock is expected to do: drift with the frequency offset when the carrier
/// is known, since one crystal sets both.
fn prior(settings: &Settings, grid: Grid, n: f64) -> Prior {
    let sign = if settings.inverted { -1.0 } else { 1.0 };
    if settings.carrier_hz > 0.0 {
        Prior {
            drift: -sign * grid.cfo_bins * settings.bandwidth_hz / settings.carrier_hz,
            spread: COUPLED_PPM * 1e-6 * n,
        }
    } else {
        Prior {
            drift: 0.0,
            spread: PRIOR_PPM * 1e-6 * n,
        }
    }
}

/// The early preamble upchirps: off the narrow stream while the offset leaves its chirps whole,
/// else on the grid.
fn read_far(tools: &mut Tools, front: &Front, settings: &Settings, grid: Grid, fit: &mut DriftFit) {
    let n = (1u64 << settings.spreading_factor) as f64;
    let floor = n.ln() as f32;
    let counts = |k: f64, bins: f64, level: f32| {
        level >= floor && bins.abs() <= reach(n, (k - REFINED_AT).abs())
    };
    if grid.cfo_bins.abs() > LOOSE_CFO * n {
        for k in FAR_SYMBOLS {
            let reading = at_offset(tools, front.wide(), settings, grid, k, Slope::Up);
            let bins = wrap(reading.bins, n);
            if counts(k, bins, reading.ratio) {
                fit.up(k, bins);
            }
        }
    } else {
        for k in LOOSE_SYMBOLS {
            let (bins, level) = loose_reading(tools, front.narrow(), settings, grid, k);
            if counts(k, bins, level) {
                fit.loose(k, bins);
            }
        }
    }
}

/// The whole preamble, the sync word and both downchirps, fitted for how far the sender's chip
/// clock carries each symbol from the last. At SF12 a 50 ppm clock moves every symbol by a fifth
/// of a chip, which the four readings of the refinement mistake for a frequency offset. The
/// readings the refinement already took are moved onto the final grid.
fn clock(tools: &mut Tools, front: &Front, settings: &Settings, grid: Grid, sfd: &Sfd) -> Clock {
    let n = (1u64 << settings.spreading_factor) as f64;
    let floor = n.ln() as f32;
    let late = grid.sfd - sfd.read_on.sfd;
    let turned = grid.cfo_bins - sfd.read_on.cfo_bins;
    let mut fit = DriftFit::default();
    read_far(tools, front, settings, grid, &mut fit);
    let mut add = |k: f64, slope: Slope, reading: &Reading, period: f64, moved: f64| {
        let bins = wrap(reading.bins, period) + moved;
        if reading.ratio < floor || bins.abs() > reach(n, (k - REFINED_AT).abs()) {
            return;
        }
        match slope {
            Slope::Up => fit.up(k, bins),
            Slope::Down => fit.down(k, bins),
        }
    };
    for (k, reading) in [NEAR_SYMBOL, NEAR_SYMBOL + 1.0]
        .into_iter()
        .zip(&sfd.preamble)
    {
        add(k, Slope::Up, reading, n, late - turned);
    }
    for (k, reading) in [-2.0, -1.0].into_iter().zip(&sfd.sync) {
        add(k, Slope::Up, reading, SYNC_SPACING, 0.0);
    }
    for (k, reading) in [0.0, 1.0].into_iter().zip(&sfd.down) {
        add(k, Slope::Down, reading, n, -late - turned);
    }
    fit.solve(prior(settings, grid, n), OUTLIER_BINS, NEAR_SYMBOL)
        .unwrap_or_default()
}

fn coarse(candidate: &Candidate, n: f64, cfo_bins: f64) -> Grid {
    let late = (candidate.up_bins - cfo_bins).rem_euclid(n);
    Grid {
        sfd: candidate.window_start as f64 - late,
        cfo_bins,
    }
}

/// The grid the preamble and downchirp bins call for and, when its offset is large, the twin
/// they allow as well: an upchirp and a downchirp read the same bins after a shift of half a
/// symbol and half the band, so an offset past a quarter of the band reads like one inside it.
fn grids(candidate: &Candidate, n: f64) -> (Grid, Option<Grid>) {
    let cfo_bins = wrap(candidate.up_bins + candidate.down_bins, n) / 2.0;
    let twin = cfo_bins - (n / 2.0).copysign(cfo_bins);
    let primary = coarse(candidate, n, cfo_bins);
    let twin = (cfo_bins.abs() >= TWIN_FROM * n).then(|| coarse(candidate, n, twin));
    (primary, twin)
}

/// A coarse grid with the readings around it and the symbol among them that looks most like
/// the SFD.
struct Placed {
    boundary: Grid,
    around: Boundary,
    at: usize,
    evidence: f32,
}

impl Placed {
    fn read(tools: &mut Tools, ring: &Ring, settings: &Settings, boundary: Grid) -> Self {
        let around = Boundary::read(tools, ring, settings, boundary);
        let (at, evidence) = (1..AROUND - 1).map(|at| (at, around.evidence(at))).fold(
            (AROUND / 2, f32::NEG_INFINITY),
            |best, next| {
                if next.1 > best.1 { next } else { best }
            },
        );
        Self {
            boundary,
            around,
            at,
            evidence,
        }
    }
}

/// The grid refined from the placed SFD and every SFD reading taken on it.
fn settle(tools: &mut Tools, ring: &Ring, settings: &Settings, placed: Placed) -> (Grid, Sfd) {
    let n = (1u64 << settings.spreading_factor) as f64;
    let Placed {
        boundary,
        around,
        at,
        ..
    } = placed;
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
        read_on: grid,
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
        sfd.read_on = grid;
    }
    sfd.sync = [
        at_offset(tools, ring, settings, grid, -2.0, Slope::Up),
        at_offset(tools, ring, settings, grid, -1.0, Slope::Up),
    ];
    (grid, sfd)
}

/// Places, refines and fits the SFD a candidate points at. Marked cold: it runs once a frame,
/// and laid out apart it leaves the search loop that runs every window as tight as before.
#[cold]
pub(crate) fn lock(
    candidate: &Candidate,
    settings: &Settings,
    tools: &mut Tools,
    front: &Front,
) -> Outcome {
    let ring = front.wide();
    let chips = 1usize << settings.spreading_factor;
    let n = chips as f64;
    let (primary, twin) = grids(candidate, n);
    let unread = std::iter::once(primary).chain(twin).any(|grid| {
        let latest = Window {
            start_chip: grid.sfd + 2.0 * n + LOOKAHEAD_CHIPS,
            cfo_bins: grid.cfo_bins,
            inverted: settings.inverted,
        };
        latest.last_wide_index(chips) >= ring.end()
    });
    if unread {
        return Outcome::Waiting;
    }
    let first = Placed::read(tools, ring, settings, primary);
    let second = twin
        .map(|twin| Placed::read(tools, ring, settings, twin))
        .filter(|second| second.evidence > first.evidence);
    let first = settle(tools, ring, settings, first);
    let (grid, sfd) = match second.map(|second| settle(tools, ring, settings, second)) {
        Some(second) if second.1.score() > first.1.score() => second,
        _ => first,
    };
    if !sfd.plausible(chips) {
        return Outcome::Rejected;
    }
    let statistics = sfd.statistics();
    let snr = (statistics.amplitude * statistics.amplitude / statistics.noise) / chips as f32;
    let clock = clock(tools, front, settings, grid, &sfd);
    let start = Start {
        origin: grid.sfd + clock.late_chips + PAYLOAD_DELAY_SYMBOLS * (n + clock.drift),
        cfo_bins: grid.cfo_bins + clock.cfo_bins,
        drift: clock.drift,
    };
    Outcome::Locked(Box::new(Lock::new(
        settings,
        start,
        sfd.sync_word(n),
        statistics,
        10.0 * snr.max(f32::MIN_POSITIVE).log10(),
        sfd.confident(chips),
    )))
}
