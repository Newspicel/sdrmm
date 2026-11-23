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
    last_guard: Option<u8>,
    last_p2_frequency: Option<(Preamble, f32)>,
    phase: f32,
    correction: Nco,
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
            last_guard: None,
            last_p2_frequency: None,
            phase: 0.0,
            correction: Nco::new(0.0, 1.0),
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
        let _ = self.clock.set_ratio(1.0);
        self.clock_error = 0.0;
        self.equalizer.reset();
        self.scheduler.reset();
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
                    self.count_lost_frames();
                    self.frequency = detection.frequency;
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
        let last_guard = self.last_guard;
        for code in last_guard
            .into_iter()
            .chain((0..=6).filter(|code| Some(*code) != last_guard))
        {
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
            let (quality, residual) = prefix(&self.pending, 0, fft, guard, self.frequency);
            if quality < 0.3 {
                continue;
            }
            let frequency = self.frequency;
            let measured = frequency + residual;
            let spacing = TAU / fft as f32;
            let tracked = self
                .last_p2_frequency
                .filter(|(previous, _)| previous.s1 == preamble.s1 && previous.s2 == preamble.s2)
                .map_or(measured, |(_, previous)| {
                    measured - ((measured - previous) / spacing).round() * spacing
                });
            for candidate in [Some(tracked), (tracked != measured).then_some(measured)]
                .into_iter()
                .flatten()
            {
                self.frequency = candidate;
                match self.decode_preamble(preamble, guard, code, packets) {
                    Ok(pre) => {
                        self.parameters = Some(pre);
                        self.last_guard = Some(code);
                        self.last_p2_frequency = Some((preamble, self.frequency));
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
        let (quality, residual, mut adjustment) =
            tracked_prefix(&self.pending, fft, guard, self.frequency);
        if quality < 0.12 {
            return Err(DecodeError::Acquisition);
        }
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
        let last = symbol + 1 == pre.preamble.p2_symbols()? + pre.data_symbols;
        let consumed = (length as isize + adjustment) as usize;
        let overlap = if last && self.fef == 0 { 64 } else { 0 };
        self.consume(consumed - overlap);
        if last {
            self.scheduler.end()?;
            let end = self.consumed + overlap as u64;
            self.last_frame = Some((end + self.fef as u64, end - self.origin));
            self.state = if self.fef > 0 {
                State::Skip(self.fef)
            } else {
                State::Search
            };
        } else {
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
        self.correction.set_turns(turns);
        self.correction
            .set_phase(turns * start as f64 - f64::from(self.phase) / std::f64::consts::TAU);
        self.correction
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

fn prefix(
    iq: &[Complex<f32>],
    start: isize,
    fft: usize,
    guard: usize,
    frequency: f32,
) -> (f32, f32) {
    let mut correlation = Complex::<f32>::default();
    let mut power = [0.0_f32; 2];
    let edge = (guard / 8).max(1);
    for i in (start + edge as isize) as usize..(start + (guard - edge) as isize) as usize {
        correlation += iq[i + fft] * iq[i].conj();
        power[0] += iq[i].norm_sqr();
        power[1] += iq[i + fft].norm_sqr();
    }
    let quality = correlation.norm_sqr() / (power[0] * power[1]).max(1e-20);
    let residual =
        (correlation * Complex::from_polar(1.0, -frequency * fft as f32)).arg() / fft as f32;
    (quality, residual)
}

fn tracked_prefix(
    iq: &[Complex<f32>],
    fft: usize,
    guard: usize,
    frequency: f32,
) -> (f32, f32, isize) {
    let edge = (guard / 8).max(1);
    let margin = (guard / 8).clamp(1, 8) - 1;
    let width = guard - 2 * edge;
    let first = edge - margin;
    let pair = |at| (iq[at], iq[at + fft]);
    let (mut corr, mut left, mut right) = (Complex::default(), 0.0, 0.0);
    let mut windows = [(Complex::new(0.0, 0.0), 0.0); 15];
    for at in first..first + width {
        let (a, b) = pair(at);
        corr += b * a.conj();
        left += a.norm_sqr();
        right += b.norm_sqr();
    }
    for (i, window) in windows[..2 * margin + 1].iter_mut().enumerate() {
        *window = (corr, corr.norm_sqr() / (left * right).max(1e-20));
        if i < 2 * margin {
            let (a, b) = pair(first + i);
            let (c, d) = pair(first + i + width);
            corr += d * c.conj() - b * a.conj();
            left += c.norm_sqr() - a.norm_sqr();
            right += d.norm_sqr() - b.norm_sqr();
        }
    }
    let (mut correlation, mut quality) = windows[margin];
    let mut adjustment = 0;
    for (i, &(candidate, score)) in windows[..2 * margin + 1].iter().enumerate() {
        if score > quality + 0.002 {
            correlation = candidate;
            quality = score;
            adjustment = i as isize - margin as isize;
        }
    }
    let residual =
        (correlation * Complex::from_polar(1.0, -frequency * fft as f32)).arg() / fft as f32;
    (quality, residual, adjustment)
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
