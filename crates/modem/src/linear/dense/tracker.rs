use std::f64::consts::PI;

use num_complex::Complex;

use super::line_timing::loop_gains;
use crate::constellation::Constellation;

/// Symbols of blind acquisition: power for the gain, the fourth power for the phase.
const ACQUIRE_SYMBOLS: u64 = 12_000;

/// Noise bandwidths of the fourth-power loop in cycles per symbol, each held until the symbol
/// count beside it: wide enough first to pull in what a short frequency estimate missed, narrow
/// at the end so the phase it hands over is quiet.
const ACQUIRE_GEARS: [(f64, u64); 3] = [(1e-3, 3_000), (3e-4, 6_000), (1e-4, ACQUIRE_SYMBOLS)];

/// Noise bandwidth of the decision-directed phase loop once trusted points steer it.
const TRACK_BW: f64 = 0.002;

/// Share of the gain error taken per trusted symbol.
const GAIN_STEP: f64 = 1e-3;

/// Share of the error taken into the equaliser per trusted symbol.
const EQUALISER_STEP: f64 = 1e-2;

/// Trust radius to start from, in point spacings, and its floor as a share of it. Phase and gain
/// errors move a point by its radius times the error, so in spacings the start tolerates the
/// same blind-start error at every order.
const START_SPACINGS: f64 = 16.0;
const FLOOR_SHARE: f64 = 2.0 / 3.0;

/// An axis error past this share of the point spacing counts against trust. Right decisions at
/// 55 dB MER cross it under one time in a hundred, guesses one time in three.
const EDGE: f64 = 0.35;

/// Edge-crossing rates under which the radius grows and over which it shrinks.
const GROW_BELOW: f64 = 0.04;
const SHRINK_ABOVE: f64 = 0.10;

/// Relative radius change per symbol, spread over the trusted symbols, so a dense grid with few
/// points near the centre grows as fast as a coarse one.
const GROW: f64 = 4e-5;

/// Largest relative radius change one trusted symbol may make.
const MAX_GROW_STEP: f64 = 0.01;

/// Widest loop bandwidth per trusted update, in cycles per update. Trusted points arrive
/// sparsely while the radius is small, and a loop sampled that sparsely must stay narrow
/// against its own update rate to stay damped.
const MAX_UPDATE_BW: f64 = 0.02;

/// Largest share of the gain error taken per trusted update.
const MAX_GAIN_STEP: f64 = 0.05;

const MISFIT_SYMBOLS: f64 = 512.0;
const SHARE_SYMBOLS: f64 = 1024.0;

/// Symbols at the floor with guesses for decisions before the tracker starts over.
const LOST_SYMBOLS: u64 = 30_000;

/// Gain, carrier phase and equaliser for a dense square grid, steered only by the points it can
/// already decide right.
///
/// After a blind start the decision-directed loops listen only to points inside a trust radius.
/// Near the centre a phase or gain error moves a point least, so those decisions come right
/// first; the radius grows while the decided points stay well inside their cells and shrinks when
/// they stray. Steps are scaled by how many points are trusted, so the loops keep their bandwidth
/// while the radius is small. Without this, outer points decided wrong pull a 65536-point grid
/// into a slow false lock.
pub(super) struct TrustTracker {
    table: Constellation,
    spacing: f64,
    full_radius: f64,
    start_radius: f64,
    fourth: Complex<f64>,
    fourth_size: f64,
    acquire: [(f64, f64); 3],
    count: u64,
    power: f64,
    amp: f64,
    theta: f64,
    omega: f64,
    radius: f64,
    misfit: f64,
    inner_power: f64,
    share: f64,
    lost: u64,
    restarts: u64,
    taps: Vec<Complex<f64>>,
    history: Vec<Complex<f64>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DenseState {
    pub acquiring: bool,
    pub trust_radius: f64,
    pub full_radius: f64,
    pub trusted_share: f64,
    pub edge_rate: f64,
    pub freq_cycles_per_symbol: f64,
    /// Times lock was lost and blind acquisition started over.
    pub restarts: u64,
}

impl TrustTracker {
    pub(super) fn new(table: Constellation, equaliser_taps: usize) -> Self {
        let spacing = table.min_distance();
        let full_radius = table
            .points()
            .iter()
            .map(|p| f64::from(p.norm()))
            .fold(0.0, f64::max);
        let fourth = table
            .points()
            .iter()
            .map(|&p| {
                let p = Complex::new(f64::from(p.re), f64::from(p.im));
                (p * p) * (p * p)
            })
            .sum::<Complex<f64>>()
            / table.len() as f64;
        let start_radius = START_SPACINGS * spacing;
        let mut taps = vec![Complex::new(0.0, 0.0); equaliser_taps];
        if let Some(centre) = taps.get_mut(equaliser_taps / 2) {
            *centre = Complex::new(1.0, 0.0);
        }
        Self {
            table,
            spacing,
            full_radius,
            start_radius,
            fourth: fourth / fourth.norm().max(f64::MIN_POSITIVE),
            fourth_size: fourth.norm().max(f64::MIN_POSITIVE),
            acquire: ACQUIRE_GEARS.map(|(bw, _)| loop_gains(bw)),
            count: 0,
            power: 0.0,
            amp: 1.0,
            theta: 0.0,
            omega: 0.0,
            radius: start_radius,
            misfit: GROW_BELOW,
            inner_power: start_radius * start_radius / 2.0,
            share: 0.0,
            lost: 0,
            restarts: 0,
            history: vec![Complex::new(0.0, 0.0); equaliser_taps],
            taps,
        }
    }

    /// Starts the carrier at `cycles_per_symbol`, as a block estimate found it.
    pub(super) fn seed_frequency(&mut self, cycles_per_symbol: f64) {
        self.omega = std::f64::consts::TAU * cycles_per_symbol;
    }

    pub(super) fn push(&mut self, y: Complex<f32>) -> Complex<f32> {
        self.count += 1;
        let y = Complex::new(f64::from(y.re), f64::from(y.im));
        if self.acquiring() {
            self.power += (y.norm_sqr() - self.power) / self.count as f64;
            self.amp = self.power.sqrt().recip();
        }
        let equalised = self.equalise(y);
        let z = equalised * Complex::from_polar(self.amp, -self.theta);
        if self.acquiring() {
            self.steer_blind(z);
        } else {
            self.steer_trusted(z);
        }
        self.theta = wrap(self.theta);
        Complex::new(z.re as f32, z.im as f32)
    }

    pub(super) fn state(&self) -> DenseState {
        DenseState {
            acquiring: self.acquiring(),
            trust_radius: self.radius,
            full_radius: self.full_radius,
            trusted_share: self.share,
            edge_rate: self.misfit,
            freq_cycles_per_symbol: self.omega / std::f64::consts::TAU,
            restarts: self.restarts,
        }
    }

    /// Starts over from blind acquisition, keeping the carrier frequency it had found.
    pub(super) fn restart(&mut self) {
        self.count = 0;
        self.power = 0.0;
        self.radius = self.start_radius;
        self.misfit = GROW_BELOW;
        self.inner_power = self.start_radius * self.start_radius / 2.0;
        self.share = 0.0;
        self.lost = 0;
        let centre = self.taps.len() / 2;
        for (k, tap) in self.taps.iter_mut().enumerate() {
            *tap = Complex::new(if k == centre { 1.0 } else { 0.0 }, 0.0);
        }
        self.history.fill(Complex::new(0.0, 0.0));
    }

    pub(super) fn reset(&mut self) {
        self.restart();
        self.theta = 0.0;
        self.omega = 0.0;
        self.amp = 1.0;
    }

    fn acquiring(&self) -> bool {
        self.count <= ACQUIRE_SYMBOLS
    }

    fn equalise(&mut self, y: Complex<f64>) -> Complex<f64> {
        if self.taps.is_empty() {
            return y;
        }
        self.history.rotate_right(1);
        self.history[0] = y;
        self.taps
            .iter()
            .zip(&self.history)
            .map(|(w, s)| w * s)
            .sum()
    }

    fn steer_blind(&mut self, z: Complex<f64>) {
        let z2 = z * z;
        let err = ((z2 * z2) * self.fourth.conj()).im / (4.0 * self.fourth_size);
        let err = err.clamp(-1.0, 1.0);
        let gear = ACQUIRE_GEARS
            .iter()
            .position(|&(_, until)| self.count <= until)
            .unwrap_or(ACQUIRE_GEARS.len() - 1);
        let (kp, ki) = self.acquire[gear];
        self.omega += ki * err;
        self.theta += self.omega + kp * err;
    }

    fn steer_trusted(&mut self, z: Complex<f64>) {
        self.theta += self.omega;
        let decided = self.table.nearest(Complex::new(z.re as f32, z.im as f32));
        let x = Complex::new(f64::from(decided.re), f64::from(decided.im));
        let trusted = x.norm() <= self.radius;
        self.share += (f64::from(u8::from(trusted)) - self.share) / SHARE_SYMBOLS;
        if trusted {
            self.learn(z, x);
        }
        self.supervise();
    }

    fn learn(&mut self, z: Complex<f64>, x: Complex<f64>) {
        let miss = (z - x) / self.spacing;
        let crossed =
            f64::from(u8::from(miss.re.abs() > EDGE)) + f64::from(u8::from(miss.im.abs() > EDGE));
        self.misfit += (0.5 * crossed - self.misfit) / MISFIT_SYMBOLS;
        self.inner_power += (x.norm_sqr() - self.inner_power) / MISFIT_SYMBOLS;
        self.adapt_equaliser(z, x);
        let norm = self.inner_power.max(f64::MIN_POSITIVE);
        let zx = z * x.conj();
        let phase_err = (zx.im / norm).clamp(-1.0, 1.0);
        let gain_err = ((zx.re - x.norm_sqr()) / norm).clamp(-1.0, 1.0);
        let gap = self.share.max(SHARE_SYMBOLS.recip()).recip();
        let bw = TRACK_BW.min(MAX_UPDATE_BW / gap);
        let (kp, ki) = loop_gains(bw * gap);
        self.theta += kp * phase_err;
        self.omega += ki * phase_err / gap;
        self.amp *= 1.0 - (GAIN_STEP * gap).min(MAX_GAIN_STEP) * gain_err;
        let grow = (GROW * gap).min(MAX_GROW_STEP);
        if self.misfit < GROW_BELOW {
            self.radius = (self.radius * (1.0 + grow)).min(self.full_radius * 1.01);
        } else if self.misfit > SHRINK_ABOVE {
            let floor = self.start_radius * FLOOR_SHARE;
            self.radius = (self.radius * (1.0 - 4.0 * grow)).max(floor);
        }
    }

    fn adapt_equaliser(&mut self, z: Complex<f64>, x: Complex<f64>) {
        if self.taps.len() < 2 {
            return;
        }
        let back = (x - z) * Complex::from_polar(self.amp.recip(), self.theta);
        let step = EQUALISER_STEP * self.amp * self.amp;
        let centre = self.taps.len() / 2;
        for (k, (tap, s)) in self.taps.iter_mut().zip(&self.history).enumerate() {
            if k != centre {
                *tap += back * s.conj() * step;
            }
        }
    }

    fn supervise(&mut self) {
        let at_floor = self.radius <= self.start_radius * FLOOR_SHARE * 1.001;
        if at_floor && self.misfit > SHRINK_ABOVE * 1.5 {
            self.lost += 1;
        } else {
            self.lost = 0;
        }
        if self.lost > LOST_SYMBOLS {
            self.restarts += 1;
            self.restart();
        }
    }
}

fn wrap(theta: f64) -> f64 {
    if theta > PI || theta <= -PI {
        theta - std::f64::consts::TAU * ((theta + PI) / std::f64::consts::TAU).floor()
    } else {
        theta
    }
}
