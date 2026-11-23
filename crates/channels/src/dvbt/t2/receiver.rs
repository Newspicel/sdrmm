use std::f32::consts::TAU;

use num_complex::Complex;
use sdrmm_decoder::SymbolTap;
use sdrmm_dsp::{FracResampler, math::fft::Transform, tuning::nco::Nco};
use sdrmm_modem::constellation::Constellation as Table;

use super::{
    Constellation, DecodeError,
    acquire::{Acquisition, Preamble},
    bicm,
    common::Multiplex,
    equalize::{Equalizer, Shape},
    mapping::Mapping,
    schedule::Report,
    signalling::{Pre, Signalling},
};
use sdrmm_modem::dvb::PACKET;

/// Share of the rate the clock trim keeps flat, past the widest T2 spectrum.
const TRIM_KEEP: f64 = 0.9;
/// A P2 prefix lag past this many samples is the radio's clock, not noise, and trims it at once.
const PREFIX_LAG_FLOOR: f64 = 0.3;
/// Half a P1 carrier in radians a sample: the furthest a P1 estimate may sit from the tracked
/// frequency and still mean the same carrier.
const P1_FREQUENCY_SLACK: f32 = TAU * 0.5 / 1024.0;
/// Samples of a frame's last symbol left for the next P1 search, so a window that tracked a
/// few samples late, as echoes pull it, cannot eat into the P1 it has to find.
const P1_LEAD: usize = 64;
/// A data symbol's prefix correlating below this was smeared by an echo or a fade. Its frame's
/// L1 already fixes where it starts, so it keeps that timing and leaves the pilots to judge it.
const SMEARED_PREFIX: f32 = 0.12;
/// Share of each symbol's measured clock error the trim takes on.
const TRIM_GAIN: f64 = 0.05;
/// The furthest a receiver clock is trusted to run from the transmitter's.
const MAX_CLOCK_ERROR: f64 = 300e-6;
struct Mode {
    fft: Transform,
    map: Mapping,
}

#[derive(Clone, Copy)]
enum State {
    Search,
    Skip(usize),
    P2(Preamble),
    Data {
        pre: Pre,
        symbol: usize,
        address: usize,
    },
}

pub struct Receiver {
    modes: Vec<Mode>,
    acquisition: Acquisition,
    equalizer: Equalizer,
    signalling: Signalling,
    scheduler: Multiplex,
    pending: Vec<Complex<f32>>,
    spectrum: Vec<Complex<f32>>,
    equalized: Vec<Complex<f32>>,
    gains: Vec<f32>,
    cells: Vec<Complex<f32>>,
    cell_gains: Vec<f32>,
    p2: Vec<Complex<f32>>,
    p2_gains: Vec<f32>,
    l1: Vec<Complex<f32>>,
    state: State,
    phase: f32,
    derotation: Nco,
    selected: Option<u8>,
    pub parameters: Option<Pre>,
    pub frequency: f32,
    pub errors: u32,
    pub last_error: Option<DecodeError>,
    pub frames: u32,
    locked: bool,
    fef: usize,
    consumed: u64,
    origin: u64,
    tables: Vec<Table>,
    cells_per_sample: f64,
    lag: f64,
    last_frame: Option<(u64, u64)>,
    clock: FracResampler,
    clock_error: f64,
    trimmed: Vec<Complex<f32>>,
}

impl Receiver {
    pub fn new(selected: Option<u8>) -> Result<Self, DecodeError> {
        let mut modes = Vec::with_capacity(6);
        for size in [1024, 2048, 4096, 8192, 16384, 32768] {
            modes.push(Mode {
                fft: Transform::forward(size),
                map: Mapping::new(size)?,
            });
        }
        Ok(Self {
            modes,
            acquisition: Acquisition::default(),
            equalizer: Equalizer::default(),
            signalling: Signalling::new()?,
            scheduler: Multiplex::new()?,
            pending: Vec::with_capacity(131072),
            spectrum: vec![Complex::default(); 32768],
            equalized: vec![Complex::default(); 27841],
            gains: vec![0.0; 27841],
            cells: vec![Complex::default(); 27841],
            cell_gains: vec![0.0; 27841],
            p2: vec![Complex::default(); 22432],
            p2_gains: vec![0.0; 22432],
            l1: vec![Complex::default(); 22432],
            state: State::Search,
            phase: 0.0,
            derotation: Nco::new(0.0, 1.0),
            selected,
            parameters: None,
            frequency: 0.0,
            errors: 0,
            last_error: None,
            frames: 0,
            locked: false,
            fef: 0,
            consumed: 0,
            origin: 0,
            tables: tables()?,
            cells_per_sample: 0.0,
            lag: 0.0,
            last_frame: None,
            clock: FracResampler::trimmable(TRIM_KEEP),
            clock_error: 0.0,
            trimmed: Vec::with_capacity(4200),
        })
    }

    pub fn reset(&mut self) {
        self.pending.clear();
        self.state = State::Search;
        self.parameters = None;
        self.phase = 0.0;
        self.frequency = 0.0;
        self.locked = false;
        self.lag = 0.0;
        self.last_frame = None;
        self.clock.reset();
        self.clock_error = 0.0;
        self.equalizer.reset();
        self.scheduler.reset();
        if self.clock.set_ratio(1.0).is_err() {
            self.fail(DecodeError::Parameters);
        }
    }

    pub fn select(&mut self, selected: Option<u8>) {
        if self.selected != selected {
            self.reset();
            self.selected = selected;
        }
    }

    pub const fn locked(&self) -> bool {
        self.locked
    }
    pub fn report(&self) -> Report {
        self.scheduler.report()
    }
    pub fn plp(&self) -> Option<super::signalling::Plp> {
        self.scheduler.selected()
    }
    /// Asks the selected PLP's next FEC block to keep its cells for the scope.
    pub fn request_tap(&mut self) {
        self.scheduler.request_tap();
    }

    /// Hands the kept cells to the scope once, at `rate` samples a second.
    pub fn tap(&mut self, out: &mut SymbolTap, rate: f64) {
        let Some((points, constellation)) = self.scheduler.tapped() else {
            return;
        };
        out.carriers(
            points,
            &self.tables[constellation.bits() / 2 - 1],
            self.cells_per_sample * rate,
            f64::from(self.frequency) * rate / std::f64::consts::TAU,
        );
        self.scheduler.release_tap();
    }

    /// How far the radio's sample clock runs from the transmitter's, as a share of its rate.
    pub const fn clock_error(&self) -> f64 {
        self.clock_error
    }

    pub fn snr(&self) -> f32 {
        -10.0 * self.equalizer.noise.max(1e-6).log10()
    }

    pub fn push(&mut self, iq: &[Complex<f32>], packets: &mut Vec<[u8; PACKET]>) {
        for block in iq.chunks(4096) {
            if block.iter().any(|p| !p.norm_sqr().is_finite()) {
                self.fail(DecodeError::NonFinite);
                self.pending.clear();
                continue;
            }
            self.clock.process(block, &mut self.trimmed);
            if self.pending.len() + self.trimmed.len() > self.pending.capacity() {
                self.fail(DecodeError::Capacity);
                self.pending.clear();
            }
            self.pending.extend_from_slice(&self.trimmed);
            loop {
                match self.step(packets) {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(error) => {
                        self.fail(error);
                        if !self.pending.is_empty() {
                            self.consume(1);
                        }
                    }
                }
            }
        }
    }

    fn fail(&mut self, error: DecodeError) {
        self.errors = self.errors.saturating_add(1);
        self.last_error = Some(error);
        self.locked = false;
        self.parameters = None;
        self.last_frame = None;
        self.state = State::Search;
        self.scheduler.reset();
    }

    fn step(&mut self, packets: &mut Vec<[u8; PACKET]>) -> Result<bool, DecodeError> {
        match self.state {
            State::Skip(remaining) => {
                let consumed = self.pending.len().min(remaining);
                self.consume(consumed);
                self.state = if consumed == remaining {
                    State::Search
                } else {
                    State::Skip(remaining - consumed)
                };
                Ok(consumed > 0)
            }
            State::Search => {
                if self.pending.len() < 8192 {
                    return Ok(false);
                }
                if let Some(detection) = self.acquisition.find(&self.pending) {
                    self.origin = self.consumed + detection.start as u64;
                    let followed = self.last_frame.is_some();
                    self.count_lost_frames();
                    self.frequency = self.next_frequency(detection.frequency, followed);
                    self.phase = 0.0;
                    self.consume(detection.start + 2048);
                    if detection.preamble.fft().is_ok() {
                        self.state = State::P2(detection.preamble);
                    }
                    Ok(true)
                } else {
                    let consumed = self.pending.len() - 2047;
                    self.consume(consumed);
                    self.locked = false;
                    Ok(false)
                }
            }
            State::P2(preamble) => self.preamble(preamble, packets),
            State::Data {
                pre,
                symbol,
                address,
            } => self.data(pre, symbol, address, packets),
        }
    }

    fn preamble(
        &mut self,
        preamble: Preamble,
        packets: &mut Vec<[u8; PACKET]>,
    ) -> Result<bool, DecodeError> {
        let fft = preamble.fft()?;
        let count = preamble.p2_symbols()?;
        if self.pending.len() < count * (fft + fft / 4) {
            return Ok(false);
        }
        let mut last = DecodeError::Acquisition;
        let mut clock_measured = false;
        for code in [0, 1, 2, 3, 4, 5, 6] {
            let guard = match code {
                0 => fft / 32,
                1 => fft / 16,
                2 => fft / 8,
                3 => fft / 4,
                4 => fft / 128,
                5 => fft * 19 / 128,
                _ => fft * 19 / 256,
            };
            if !guard_allowed(preamble, code) {
                continue;
            }
            let (quality, residual, lag) = lagged_prefix(&self.pending, fft, guard, self.frequency);
            if quality < 0.3 {
                continue;
            }
            if !clock_measured {
                clock_measured = true;
                self.trim_clock_by_prefix(lag, fft)?;
            }
            let frequency = self.frequency;
            self.frequency += residual;
            match self.decode_preamble(preamble, guard, code, packets) {
                Ok(pre) => {
                    self.parameters = Some(pre);
                    self.frames = self.frames.saturating_add(1);
                    let used = count * (fft + guard);
                    self.consume(used);
                    let p2_count = self.modes[fft.ilog2() as usize - 10].map.data;
                    let address = count * p2_count - 1840 - pre.post_cells;
                    self.state = State::Data {
                        pre,
                        symbol: count,
                        address,
                    };
                    return Ok(true);
                }
                Err(error) => {
                    last = error;
                    self.frequency = frequency;
                }
            }
        }
        Err(last)
    }

    fn decode_preamble(
        &mut self,
        preamble: Preamble,
        guard: usize,
        code: u8,
        packets: &mut Vec<[u8; PACKET]>,
    ) -> Result<Pre, DecodeError> {
        let fft = preamble.fft()?;
        let mode = fft.ilog2() as usize - 10;
        let count = preamble.p2_symbols()?;
        self.equalizer.reset();
        for symbol in 0..count {
            self.modes[mode].map.p2(preamble, symbol)?;
            self.transform(mode, symbol * (fft + guard) + guard - backoff(guard));
            self.equalizer.decode(
                &self.spectrum[..fft],
                &self.modes[mode].map,
                Shape {
                    miso: preamble.miso(),
                    history: 0,
                    guard,
                },
                &mut self.equalized,
                &mut self.gains,
            )?;
            let map = &self.modes[mode].map;
            map.deinterleave(
                &self.equalized[..map.data],
                symbol,
                &mut self.p2[symbol * map.data..(symbol + 1) * map.data],
            )?;
            map.deinterleave(
                &self.gains[..map.data],
                symbol,
                &mut self.p2_gains[symbol * map.data..(symbol + 1) * map.data],
            )?;
        }
        let capacity = self.modes[mode].map.data;
        for i in 0..1840 {
            self.l1[i] = self.p2[i % count * capacity + i / count];
        }
        let pre = self.signalling.pre(&self.l1[..1840], preamble)?;
        if pre.guard_code != code
            || 1840 + pre.post_cells > capacity * count
            || !pre.post_cells.is_multiple_of(count)
        {
            return Err(DecodeError::Signalling);
        }
        for i in 0..pre.post_cells {
            self.l1[i] = self.p2[i % count * capacity + (1840 + i) / count];
        }
        let post = self.signalling.post(&self.l1[..pre.post_cells], pre)?;
        self.scheduler
            .begin(pre, &post, self.selected, self.origin)?;
        self.fef = if post.fef_interval > 0 && (post.frame + 1).is_multiple_of(post.fef_interval) {
            post.fef_length
        } else {
            0
        };
        let start = (1840 + pre.post_cells) / count;
        let mut address = 0;
        let previous = self.scheduler.report().packets;
        for symbol in 0..count {
            self.scheduler.push(
                address,
                &self.p2[symbol * capacity + start..(symbol + 1) * capacity],
                &self.p2_gains[symbol * capacity + start..(symbol + 1) * capacity],
                self.equalizer.noise,
                packets,
            )?;
            address += capacity - start;
        }
        if self.scheduler.report().packets > previous {
            self.locked = true;
        }
        self.last_error = None;
        Ok(pre)
    }

    fn data(
        &mut self,
        pre: Pre,
        symbol: usize,
        address: usize,
        packets: &mut Vec<[u8; PACKET]>,
    ) -> Result<bool, DecodeError> {
        let fft = pre.preamble.fft()?;
        let guard = pre.guard()?;
        let length = fft + guard;
        if self.pending.len() < length + 16 {
            return Ok(false);
        }
        let (quality, residual, adjustment) =
            sliding_prefix(&self.pending, fft, guard, self.frequency);
        let (residual, mut adjustment) = if quality < SMEARED_PREFIX {
            (0.0, 0)
        } else {
            (residual, adjustment)
        };
        adjustment += self.follow_drift(fft);
        self.trim_clock(fft, length)?;
        self.frequency += residual * 0.2;
        let mode = fft.ilog2() as usize - 10;
        self.modes[mode].map.data(pre, symbol)?;
        self.transform(
            mode,
            (guard as isize + adjustment) as usize - backoff(guard),
        );
        self.equalizer.shift(fft, adjustment);
        let map = &self.modes[mode].map;
        let history = match pre.pilots {
            1 | 3 | 5 | 7 => 3,
            8 => 15,
            _ => 1,
        };
        self.equalizer.decode(
            &self.spectrum[..fft],
            map,
            Shape {
                miso: pre.preamble.miso(),
                history,
                guard,
            },
            &mut self.equalized,
            &mut self.gains,
        )?;
        map.deinterleave(&self.equalized[..map.data], symbol, &mut self.cells)?;
        map.deinterleave(&self.gains[..map.data], symbol, &mut self.cell_gains)?;
        let active = map.active;
        self.cells_per_sample = active as f64 / length as f64;
        let previous = self.scheduler.report().packets;
        self.scheduler.push(
            address,
            &self.cells[..active],
            &self.cell_gains[..active],
            self.equalizer.noise,
            packets,
        )?;
        if self.scheduler.report().packets > previous {
            self.locked = true;
        }
        let step = (length as isize + adjustment) as usize;
        if symbol + 1 == pre.preamble.p2_symbols()? + pre.data_symbols {
            self.consume(step - P1_LEAD);
            self.scheduler.end()?;
            let end = self.consumed + P1_LEAD as u64;
            self.last_frame = Some((end + self.fef as u64, end - self.origin));
            self.state = if self.fef > 0 {
                State::Skip(self.fef)
            } else {
                State::Search
            };
        } else {
            self.consume(step);
            self.state = State::Data {
                pre,
                symbol: symbol + 1,
                address: address + active,
            };
        }
        Ok(true)
    }

    /// Leans the resampler against the clock error the pilots measured, so symbols are cut from
    /// a stream at the transmitter's rate and no carrier slides toward its neighbour.
    fn trim_clock(&mut self, fft: usize, length: usize) -> Result<(), DecodeError> {
        let error = self.equalizer.drift_samples(fft) / length as f64;
        self.clock_error =
            (self.clock_error + TRIM_GAIN * error).clamp(-MAX_CLOCK_ERROR, MAX_CLOCK_ERROR);
        self.clock
            .set_ratio((1.0 + self.clock_error).recip())
            .map_err(|_| DecodeError::Parameters)
    }

    /// Trims the clock by how far the P2 symbol's tail sat from its prefix. A clock far enough
    /// off to blur the 32K symbols the pilots track would never let them start.
    fn trim_clock_by_prefix(&mut self, lag: f64, fft: usize) -> Result<(), DecodeError> {
        if lag.abs() < PREFIX_LAG_FLOOR {
            return Ok(());
        }
        let error = lag / fft as f64;
        self.clock_error = (self.clock_error + error * (1.0 + self.clock_error))
            .clamp(-MAX_CLOCK_ERROR, MAX_CLOCK_ERROR);
        self.clock
            .set_ratio((1.0 + self.clock_error).recip())
            .map_err(|_| DecodeError::Parameters)
    }

    /// The frequency to open a frame on. A frame that follows a decoded one keeps the offset its
    /// pilots tracked to a fraction of a carrier, unless the P1 says it moved; the P1 alone can
    /// miss by half a 32K carrier in noise and echoes, enough to slip the P2 a whole carrier.
    fn next_frequency(&self, detected: f32, followed: bool) -> f32 {
        if followed && (detected - self.frequency).abs() < P1_FREQUENCY_SLACK {
            self.frequency
        } else {
            detected
        }
    }

    /// Counts the frames that passed between the last one decoded and this P1 as bad, so a frame
    /// whose P1 went unseen is not lost in silence.
    fn count_lost_frames(&mut self) {
        let Some((end, length)) = self.last_frame.take() else {
            return;
        };
        let lost = (self.origin.saturating_sub(end) + length / 2) / length.max(1);
        if lost > 0 {
            self.errors = self
                .errors
                .saturating_add(u32::try_from(lost).unwrap_or(u32::MAX));
            self.last_error = Some(DecodeError::Discontinuity);
        }
    }

    /// Whole samples to move this symbol's window by, so a sample clock that runs off the
    /// transmitter's keeps the window on the symbol and the next P1 where it is looked for.
    fn follow_drift(&mut self, fft: usize) -> isize {
        self.lag += self.equalizer.drift_samples(fft);
        let step = self.lag.trunc();
        self.lag -= step;
        step as isize
    }

    fn transform(&mut self, mode: usize, start: usize) {
        let fft = self.modes[mode].map.fft;
        let turns = -f64::from(self.frequency) / std::f64::consts::TAU;
        self.derotation.set_turns(turns);
        self.derotation
            .set_phase(turns * start as f64 - f64::from(self.phase) / std::f64::consts::TAU);
        self.derotation
            .mix_into(&self.pending[start..start + fft], &mut self.spectrum[..fft]);
        self.modes[mode].fft.process(&mut self.spectrum[..fft]);
    }

    fn consume(&mut self, count: usize) {
        self.consumed = self.consumed.saturating_add(count as u64);
        self.pending.drain(..count);
        self.phase = (self.phase + self.frequency * count as f32).rem_euclid(TAU);
    }
}

/// How far into the guard interval each FFT window starts, so the ringing of the filters on
/// the way, and pre-echoes in a single-frequency network, stay out of the next symbol.
const fn backoff(guard: usize) -> usize {
    guard / 4
}

fn guard_allowed(preamble: Preamble, code: u8) -> bool {
    let field = preamble.s2 >> 1;
    let special = matches!(field, 6 | 7) || (preamble.lite() && field == 3);
    if matches!(field, 1 | 5 | 6 | 7) || (preamble.lite() && matches!(field, 3 | 4)) {
        return (code >= 4) == special && !(field == 5 && code == 3);
    }
    if preamble.fft().is_ok_and(|fft| fft <= 4096) {
        code <= 3 && !(field == 3 && code == 0)
    } else {
        true
    }
}

/// The normalized cyclic prefix correlation at every start a few samples either side of the
/// guard, its sums slid one sample at a time. A start replaces the nominal one only by beating it clearly. Returns the quality,
/// frequency residual and the start's offset.
fn sliding_prefix(
    iq: &[Complex<f32>],
    fft: usize,
    guard: usize,
    frequency: f32,
) -> (f32, f32, isize) {
    let edge = (guard / 8).max(1);
    let margin = (guard / 8).clamp(1, 8) - 1;
    let width = guard - 2 * edge;
    let first = edge - margin;
    let pair = |at: usize| (iq[at], iq[at + fft]);
    let (mut correlation, mut head, mut tail) = (Complex::<f32>::default(), 0.0_f32, 0.0_f32);
    for (a, b) in (first..first + width).map(pair) {
        correlation += b * a.conj();
        head += a.norm_sqr();
        tail += b.norm_sqr();
    }
    let score = |correlation: Complex<f32>, head: f32, tail: f32| {
        correlation.norm_sqr() / (head * tail).max(1e-20)
    };
    let mut scores = [(0.0_f32, Complex::<f32>::default()); 15];
    for (index, slot) in scores[..=2 * margin].iter_mut().enumerate() {
        *slot = (score(correlation, head, tail), correlation);
        if index < 2 * margin {
            let ((a, b), (c, d)) = (pair(first + index), pair(first + index + width));
            correlation += d * c.conj() - b * a.conj();
            head += c.norm_sqr() - a.norm_sqr();
            tail += d.norm_sqr() - b.norm_sqr();
        }
    }
    let mut best = margin;
    for (index, &(quality, _)) in scores[..=2 * margin].iter().enumerate() {
        if quality > scores[best].0 + 0.002 {
            best = index;
        }
    }
    let (quality, correlation) = scores[best];
    let residual =
        (correlation * Complex::from_polar(1.0, -frequency * fft as f32)).arg() / fft as f32;
    (quality, residual, best as isize - margin as isize)
}

/// Lags [`lagged_prefix`] can try: the 32K tail moved as far as [`MAX_CLOCK_ERROR`] allows.
const MAX_LAGS: usize = 21;

/// The normalized cyclic prefix correlation with the symbol's tail searched a few samples either side of `fft`, as far as
/// [`MAX_CLOCK_ERROR`] can move it. Returns the quality and frequency residual at the best lag,
/// and that lag to a fraction of a sample.
pub(super) fn lagged_prefix(
    iq: &[Complex<f32>],
    fft: usize,
    guard: usize,
    frequency: f32,
) -> (f32, f32, f64) {
    let reach = ((fft as f64 * MAX_CLOCK_ERROR).ceil() as usize).min(MAX_LAGS / 2);
    let edge = (guard / 8).max(1);
    let window = &iq[edge..guard - edge];
    let head: f32 = window.iter().map(Complex::norm_sqr).sum();
    let mut scores = [(0.0_f32, Complex::<f32>::default()); MAX_LAGS];
    for (offset, score) in scores[..=2 * reach].iter_mut().enumerate() {
        let (mut correlation, mut power) = (Complex::default(), 0.0_f32);
        for (a, b) in window.iter().zip(&iq[edge + fft + offset - reach..]) {
            correlation += b * a.conj();
            power += b.norm_sqr();
        }
        *score = (
            correlation.norm_sqr() / (head * power).max(1e-20),
            correlation,
        );
    }
    let mut best = reach;
    for (offset, score) in scores[..=2 * reach].iter().enumerate() {
        if score.0 > scores[best].0 {
            best = offset;
        }
    }
    let fraction = if best > 0 && best < 2 * reach {
        let (before, here, after) = (scores[best - 1].0, scores[best].0, scores[best + 1].0);
        let curve = before - 2.0 * here + after;
        if curve < 0.0 {
            (0.5 * (before - after) / curve).clamp(-0.5, 0.5)
        } else {
            0.0
        }
    } else {
        0.0
    };
    let (quality, correlation) = scores[best];
    let span = (fft + best - reach) as f32;
    let residual = (correlation * Complex::from_polar(1.0, -frequency * span)).arg() / span;
    (
        quality,
        residual,
        best as f64 - reach as f64 + f64::from(fraction),
    )
}

/// The four PLP grids, indexed by bits per cell over two, less one.
fn tables() -> Result<Vec<Table>, DecodeError> {
    let mut tables = Vec::with_capacity(4);
    for constellation in [
        Constellation::Qpsk,
        Constellation::Qam16,
        Constellation::Qam64,
        Constellation::Qam256,
    ] {
        let size = 1 << constellation.bits();
        let points = (0..size)
            .map(|word| bicm::point(word, constellation))
            .collect();
        tables.push(
            Table::from_points(points, (0..size as u32).collect())
                .map_err(|_| DecodeError::Parameters)?,
        );
    }
    Ok(tables)
}
