use num_complex::Complex;
use sdrmm_dsp::fft::FftPair;

use super::sync::Samples;

/// Tile height in field lines and width in samples; tiles overlap by half each way.
const LINES: usize = 16;
const SAMPLES: usize = 32;
const HOP_LINES: usize = LINES / 2;
const HOP_SAMPLES: usize = SAMPLES / 2;
/// The subcarrier's bin across: a quarter of the rate.
const CARRIER: usize = SAMPLES / 4;
/// Bins either side of the subcarrier that may hold chroma, about 2.2 MHz.
const BAND: usize = 4;
/// Bins across kept for each tile line: the chroma band, nothing else can be chroma.
const BINS: usize = 2 * BAND + 1;
/// A bin and its mirror whose magnitudes part by more than this ratio are luma.
const LIKENESS: f32 = 0.6;
/// Samples a row reaches past its line's stretch on each side, so every kept sample sees two
/// full tiles across.
pub(super) const ROW_MARGIN: u64 = 20;
/// Subcarrier quarter turns a 4x-subcarrier PAL line and frame advance, modulo a whole turn:
/// 1135 and 709379 samples. Rows keep these steps so the subcarrier sits on one lattice.
const LINE_TURNS: i64 = 3;
const FRAME_TURNS: i64 = 3;
/// Frames a tile spans in depth, and the frames it moves on by.
const DEEP_FRAMES: usize = 4;
const DEEP_HOP: usize = 2;

/// How far the transform reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Depth {
    /// Each field alone: lines and samples.
    Field,
    /// Frames of the same field parity as well.
    Frames,
}

impl Depth {
    fn frames(self) -> usize {
        match self {
            Self::Field => 1,
            Self::Frames => DEEP_FRAMES,
        }
    }

    fn hop(self) -> usize {
        match self {
            Self::Field => 1,
            Self::Frames => DEEP_HOP,
        }
    }
}

/// Where one line sits in the transform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RowKey {
    parity: usize,
    frame: u64,
    line: usize,
}

/// One field line as the transform holds it.
#[derive(Clone, Debug)]
struct Row {
    present: bool,
    start: u64,
    raw: Vec<f32>,
    chroma: Vec<f32>,
}

#[derive(Clone, Debug)]
struct Frame {
    number: Option<u64>,
    rows: Vec<Row>,
}

/// The frames of one field parity, and the lattice their rows keep.
#[derive(Clone, Debug)]
struct Parity {
    frames: Vec<Frame>,
    next: u64,
    /// Frames before this have been through every window that covers them.
    complete_before: u64,
    /// Frames before this are handed out, one a frame, so pictures keep an even pace.
    split_before: u64,
    anchor: Option<(u64, usize, u64)>,
}

/// PAL chroma split from luma in the frequency domain: each tile of samples, lines and, in
/// depth, frames is transformed, and chroma, made of real colour differences on carriers a
/// quarter of the way along every axis, mirrors itself about each carrier. A bin whose mirror
/// matches it is chroma; one that does not is luma, however fine.
pub(super) struct TransformPal {
    depth: Depth,
    width: usize,
    first_line: usize,
    parities: [Parity; 2],
    current: Option<(usize, u64)>,
    across: FftPair,
    down: FftPair,
    through: FftPair,
    window_across: [f32; SAMPLES],
    window_down: [f32; LINES],
    window_through: Vec<f32>,
    pair: Vec<Complex<f32>>,
    spectrum: Vec<Complex<f32>>,
    column: Vec<Complex<f32>>,
    deep: Vec<Complex<f32>>,
    keep: Vec<bool>,
    /// Each bin's mirror, as an index into the spectrum.
    mirrors: Vec<usize>,
}

impl TransformPal {
    /// A transform over field lines `first_line..first_line + lines`, each row covering
    /// `stretch` samples of its line.
    pub fn new(stretch: usize, first_line: usize, lines: usize, depth: Depth) -> Self {
        let width = (stretch + 2 * ROW_MARGIN as usize).div_ceil(HOP_SAMPLES) * HOP_SAMPLES;
        let frames = depth.frames();
        let parity = || Parity {
            frames: (0..frames + depth.hop())
                .map(|_| Frame {
                    number: None,
                    rows: (0..lines)
                        .map(|_| Row {
                            present: false,
                            start: 0,
                            raw: vec![0.0; width],
                            chroma: vec![0.0; width],
                        })
                        .collect(),
                })
                .collect(),
            next: 0,
            complete_before: 0,
            split_before: 0,
            anchor: None,
        };
        Self {
            depth,
            width,
            first_line,
            parities: [parity(), parity()],
            current: None,
            across: FftPair::new(SAMPLES),
            down: FftPair::new(LINES),
            through: FftPair::new(frames),
            window_across: sine_window(),
            window_down: sine_window(),
            window_through: if frames == 1 {
                vec![1.0]
            } else {
                sine_window::<DEEP_FRAMES>().to_vec()
            },
            pair: vec![Complex::new(0.0, 0.0); SAMPLES],
            spectrum: vec![Complex::new(0.0, 0.0); frames * LINES * BINS],
            column: vec![Complex::new(0.0, 0.0); LINES],
            deep: vec![Complex::new(0.0, 0.0); frames],
            keep: vec![false; frames * LINES * BINS],
            mirrors: (0..frames * LINES * BINS)
                .map(|at| {
                    let (time, rest) = (at / (LINES * BINS), at % (LINES * BINS));
                    let (m_time, m_down, m_bin) = mirror(frames, (time, rest / BINS, rest % BINS));
                    (m_time * LINES + m_down) * BINS + m_bin
                })
                .collect(),
        }
    }

    #[cfg(test)]
    pub fn width(&self) -> usize {
        self.width
    }

    /// Ends the field coming in, splitting what it completes, and starts one of `parity`.
    pub fn begin_field(&mut self, parity: usize) {
        self.end_field();
        let parity = parity % 2;
        let side = &mut self.parities[parity];
        let number = side.next;
        side.next += 1;
        let ring = side.frames.len() as u64;
        let frame = &mut side.frames[(number % ring) as usize];
        frame.number = Some(number);
        for row in &mut frame.rows {
            row.present = false;
        }
        self.current = Some((parity, number));
    }

    /// Adds field line `field_line`, whose stretch starts near `wanted`, to the field coming
    /// in; its key, if the line is one the transform holds.
    pub fn push(&mut self, field_line: u32, samples: &Samples, wanted: u64) -> Option<RowKey> {
        let (parity, number) = self.current?;
        let line = (field_line as usize).checked_sub(self.first_line)?;
        let side = &self.parities[parity];
        if line >= side.frames[0].rows.len() {
            return None;
        }
        let start = self.row_start(parity, number, field_line as usize, wanted);
        let side = &mut self.parities[parity];
        let ring = side.frames.len() as u64;
        let row = &mut side.frames[(number % ring) as usize].rows[line];
        row.present = true;
        row.start = start;
        row.raw.fill(0.0);
        row.chroma.fill(0.0);
        let raw = samples.span(start, start + row.raw.len() as u64);
        row.raw[..raw.len()].copy_from_slice(raw);
        Some(RowKey {
            parity,
            frame: number,
            line,
        })
    }

    /// Ends the field coming in and splits every held frame against blank frames after it;
    /// the next run starts clear of them.
    pub fn finish(&mut self) {
        self.end_field();
        let frames = self.depth.frames() as u64;
        for parity in 0..2 {
            let next = self.parities[parity].next;
            for end in next..next + frames - 1 {
                self.split_window_ending(parity, end);
            }
            let side = &mut self.parities[parity];
            side.complete_before = side.complete_before.max(next);
            side.split_before = side.complete_before;
            side.next = next + frames;
            side.anchor = None;
        }
    }

    /// A split row: its start, its samples and their chroma.
    pub fn row(&self, key: RowKey) -> Option<(u64, &[f32], &[f32])> {
        let side = &self.parities[key.parity];
        if key.frame >= side.split_before {
            return None;
        }
        let frame = &side.frames[(key.frame % side.frames.len() as u64) as usize];
        let row = frame.rows.get(key.line)?;
        (frame.number == Some(key.frame) && row.present).then_some((
            row.start,
            &row.raw[..],
            &row.chroma[..],
        ))
    }

    /// Splits what the field coming in completes and hands out the frame that is due.
    fn end_field(&mut self) {
        let Some((parity, number)) = self.current.take() else {
            return;
        };
        self.split_window_ending(parity, number);
        let due = (number + 2).saturating_sub(self.depth.frames() as u64);
        let side = &mut self.parities[parity];
        side.split_before = side.split_before.max(due.min(side.complete_before));
    }

    /// Splits the window whose last frame is `end`, if windows end there, and moves the
    /// split mark past every frame no later window reaches.
    fn split_window_ending(&mut self, parity: usize, end: u64) {
        let frames = self.depth.frames() as i64;
        let hop = self.depth.hop() as i64;
        let first = end as i64 + 1 - frames;
        if first.rem_euclid(hop) != 0 {
            return;
        }
        self.split_window(parity, first);
        let side = &mut self.parities[parity];
        side.complete_before = side.complete_before.max((first + hop).max(0) as u64);
    }

    /// The row start nearest `wanted` that keeps the subcarrier on the parity's lattice.
    fn row_start(&mut self, parity: usize, frame: u64, line: usize, wanted: u64) -> u64 {
        let (anchor_frame, anchor_line, anchor_start) = *self.parities[parity]
            .anchor
            .get_or_insert((frame, line, wanted));
        let turns = anchor_start as i64
            + LINE_TURNS * (line as i64 - anchor_line as i64)
            + FRAME_TURNS * (frame as i64 - anchor_frame as i64);
        let off = (turns - wanted as i64).rem_euclid(4);
        wanted.saturating_add_signed(if off > 2 { off - 4 } else { off })
    }

    fn split_window(&mut self, parity: usize, first: i64) {
        let lines = self.parities[parity].frames[0].rows.len() as i64;
        let mut top = -(HOP_LINES as i64);
        while top < lines {
            let mut across = 0;
            while across + SAMPLES <= self.width {
                self.tile(parity, first, top, across);
                across += HOP_SAMPLES;
            }
            top += HOP_LINES as i64;
        }
    }

    fn row_at_mut(&mut self, parity: usize, frame: i64, line: i64) -> Option<&mut Row> {
        let frame = u64::try_from(frame).ok()?;
        let side = &mut self.parities[parity];
        let ring = side.frames.len() as u64;
        let held = &mut side.frames[(frame % ring) as usize];
        if held.number != Some(frame) {
            return None;
        }
        held.rows
            .get_mut(usize::try_from(line).ok()?)
            .filter(|row| row.present)
    }

    fn tile(&mut self, parity: usize, first: i64, top: i64, across: usize) {
        let frames = self.depth.frames();
        let mut any = false;
        for time in 0..frames {
            for down in (0..LINES).step_by(2) {
                any |= self.load_pair(parity, first + time as i64, top, (down, time), across);
            }
        }
        if !any {
            return;
        }
        self.transform_bins(true);
        self.mark_chroma();
        self.transform_bins(false);
        for time in 0..frames {
            for down in (0..LINES).step_by(2) {
                self.add_pair(parity, first + time as i64, top, (down, time), across);
            }
        }
    }

    /// Loads tile lines `down` and `down + 1` of frame `time` as one complex line and keeps
    /// their chroma bins; whether either was held.
    fn load_pair(
        &mut self,
        parity: usize,
        frame: i64,
        top: i64,
        (down, time): (usize, usize),
        across: usize,
    ) -> bool {
        let mut any = false;
        self.pair.fill(Complex::new(0.0, 0.0));
        for half in 0..2 {
            let Some(row) = row_at(&self.parities[parity], frame, top + (down + half) as i64)
            else {
                continue;
            };
            any = true;
            let w = self.window_down[down + half] * self.window_through[time];
            let raw = &row.raw[across..across + SAMPLES];
            for ((slot, &x), &across_w) in self.pair.iter_mut().zip(raw).zip(&self.window_across) {
                let value = x * across_w * w;
                if half == 0 {
                    slot.re = value;
                } else {
                    slot.im = value;
                }
            }
        }
        let base = (time * LINES + down) * BINS;
        if !any {
            self.spectrum[base..base + 2 * BINS].fill(Complex::new(0.0, 0.0));
            return false;
        }
        self.across.forward(&mut self.pair);
        for (b, x) in (CARRIER - BAND..=CARRIER + BAND).enumerate() {
            let (here, there) = (self.pair[x], self.pair[SAMPLES - x].conj());
            self.spectrum[base + b] = 0.5 * (here + there);
            self.spectrum[base + BINS + b] = Complex::new(0.0, -0.5) * (here - there);
        }
        true
    }

    /// Returns tile lines `down` and `down + 1` of frame `time` to their rows, windowed again
    /// so overlapping tiles sum to one.
    fn add_pair(
        &mut self,
        parity: usize,
        frame: i64,
        top: i64,
        (down, time): (usize, usize),
        across: usize,
    ) {
        let base = (time * LINES + down) * BINS;
        self.pair.fill(Complex::new(0.0, 0.0));
        for (b, x) in (CARRIER - BAND..=CARRIER + BAND).enumerate() {
            let (upper, lower) = (self.spectrum[base + b], self.spectrum[base + BINS + b]);
            self.pair[x] = upper + Complex::new(0.0, 1.0) * lower;
            self.pair[SAMPLES - x] = upper.conj() + Complex::new(0.0, 1.0) * lower.conj();
        }
        self.across.inverse(&mut self.pair);
        let scale = self.window_through[time] / (SAMPLES * LINES * self.depth.frames()) as f32;
        let window_across = self.window_across;
        for half in 0..2 {
            let w = self.window_down[down + half] * scale;
            let pair = std::mem::take(&mut self.pair);
            if let Some(row) = self.row_at_mut(parity, frame, top + (down + half) as i64) {
                let out = &mut row.chroma[across..across + SAMPLES];
                for ((slot, value), &across_w) in out.iter_mut().zip(&pair).zip(&window_across) {
                    let value = if half == 0 { value.re } else { value.im };
                    *slot += value * across_w * w;
                }
            }
            self.pair = pair;
        }
    }

    /// Transforms each chroma bin down the lines and through the frames, or back.
    fn transform_bins(&mut self, forward: bool) {
        let frames = self.depth.frames();
        for time in 0..frames {
            for bin in 0..BINS {
                let at = |down: usize| (time * LINES + down) * BINS + bin;
                for (down, slot) in self.column.iter_mut().enumerate() {
                    *slot = self.spectrum[at(down)];
                }
                if forward {
                    self.down.forward(&mut self.column);
                } else {
                    self.down.inverse(&mut self.column);
                }
                for (down, value) in self.column.iter().enumerate() {
                    self.spectrum[at(down)] = *value;
                }
            }
        }
        if frames == 1 {
            return;
        }
        for down in 0..LINES {
            for bin in 0..BINS {
                let at = |time: usize| (time * LINES + down) * BINS + bin;
                for (time, slot) in self.deep.iter_mut().enumerate() {
                    *slot = self.spectrum[at(time)];
                }
                if forward {
                    self.through.forward(&mut self.deep);
                } else {
                    self.through.inverse(&mut self.deep);
                }
                for (time, value) in self.deep.iter().enumerate() {
                    self.spectrum[at(time)] = *value;
                }
            }
        }
    }

    /// Keeps each chroma bin whose mirror about its carrier has a like magnitude, and drops
    /// the rest.
    fn mark_chroma(&mut self) {
        let likeness = LIKENESS * LIKENESS;
        for (keep, (value, &other)) in self
            .keep
            .iter_mut()
            .zip(self.spectrum.iter().zip(&self.mirrors))
        {
            let (here, there) = (value.norm_sqr(), self.spectrum[other].norm_sqr());
            *keep = here.min(there) >= likeness * here.max(there);
        }
        for (value, &keep) in self.spectrum.iter_mut().zip(&self.keep) {
            if !keep {
                *value = Complex::new(0.0, 0.0);
            }
        }
    }
}

/// The row at frame `frame`, line `line` of a parity, if held.
fn row_at(side: &Parity, frame: i64, line: i64) -> Option<&Row> {
    let frame = u64::try_from(frame).ok()?;
    let held = &side.frames[(frame % side.frames.len() as u64) as usize];
    let row = held.rows.get(usize::try_from(line).ok()?)?;
    (held.number == Some(frame) && row.present).then_some(row)
}

/// A bin's mirror through the nearer carrier: one colour difference a quarter of the way
/// down and through, the other three quarters. Bins across count from the band's low edge.
fn mirror(frames: usize, (time, down, bin): (usize, usize, usize)) -> (usize, usize, usize) {
    let near = |at: usize, centre: usize, len: usize| {
        let d = at.abs_diff(centre);
        d.min(len - d) as f32 / len as f32
    };
    let carriers = [(frames / 4, LINES / 4), ((3 * frames) / 4, (3 * LINES) / 4)];
    let distance = |(t, d): (usize, usize)| {
        let (dt, dd) = (near(time, t, frames), near(down, d, LINES));
        dt * dt + dd * dd
    };
    let (t, d) = if distance(carriers[0]) <= distance(carriers[1]) {
        carriers[0]
    } else {
        carriers[1]
    };
    (
        (2 * t + frames - time) % frames,
        (2 * d + LINES - down) % LINES,
        BINS - 1 - bin,
    )
}

/// `sin(pi (k + 0.5) / N)`: squared, and overlapped by half, it sums to one.
fn sine_window<const N: usize>() -> [f32; N] {
    std::array::from_fn(|k| (std::f32::consts::PI * (k as f32 + 0.5) / N as f32).sin())
}

#[cfg(test)]
mod tests;
