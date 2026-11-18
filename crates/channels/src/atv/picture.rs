use num_complex::Complex;

use super::{
    AtvColor,
    chroma::{Burst, COMB_PAD, Comb, Turn, demodulate_line, modulate, phasor, split},
    luma::{Sharpness, repair_clicks},
    secam::{Difference, Secam},
    standard::{Geometry, comb_lines},
    sync::{Levels, Samples},
    transform::{Depth, ROW_MARGIN, RowKey, TransformPal},
};
use sdrmm_decoder::VideoPicture;

/// Luma noise, as a share of black to white, above which narrow bumps are taken for clicks.
const REPAIR_NOISE: f32 = 0.008;
const REPAIR_SIGMAS: f32 = 6.0;
const REPAIR_REACH: f64 = 2.5;
const MARGIN: usize = 4;
const BURST_GUARD_US: f64 = 0.1;
/// Lines held at once: three frames waiting on the 3D transform, and the newest.
const HELD: usize = 4096;
/// Drawn lines kept for colour partners: a line, the one before and the one after.
const DRAWN: usize = 4;
/// Field lines the transform holds above the first picture line and below the last, so
/// every picture line sees full tiles.
const TRANSFORM_MARGIN: usize = 8;

/// Saturation and hue as one linear map of the colour plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Adjust {
    cos: f32,
    sin: f32,
}

impl Adjust {
    pub fn new(saturation_pct: f64, hue_deg: f64) -> Self {
        let gain = (saturation_pct / 100.0).max(0.0);
        let (sin, cos) = hue_deg.to_radians().sin_cos();
        Self {
            cos: (gain * cos) as f32,
            sin: (gain * sin) as f32,
        }
    }

    fn apply(self, u: f32, v: f32) -> (f32, f32) {
        (self.cos * u - self.sin * v, self.sin * u + self.cos * v)
    }
}

/// How a held line's colour demodulates.
#[derive(Clone, Copy, Debug)]
enum Tint {
    None,
    Qam(Turn),
    Secam(Option<Difference>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Empty,
    Gap,
    Waiting,
    Separated,
    Done,
}

/// One received line, held until what it borrows from has come.
#[derive(Clone, Copy, Debug)]
struct Held {
    seq: u64,
    stage: Stage,
    zero_h: f64,
    levels: Levels,
    row: Option<usize>,
    tint: Tint,
    key: Option<RowKey>,
}

impl Held {
    const EMPTY: Self = Self {
        seq: 0,
        stage: Stage::Empty,
        zero_h: 0.0,
        levels: Levels {
            tip: 0.0,
            blank: 1.0,
        },
        row: None,
        tint: Tint::None,
        key: None,
    };

    fn is_line(&self) -> bool {
        matches!(self.stage, Stage::Waiting | Stage::Separated | Stage::Done)
    }
}

/// A separated line's luma and colour, kept until it and its partners are written.
#[derive(Clone, Debug)]
struct Drawn {
    seq: Option<u64>,
    tint: Tint,
    luma: Vec<f32>,
    u: Vec<f32>,
    v: Vec<f32>,
}

/// Where one line's drawn stretch sits in the sample stream.
#[derive(Clone, Copy, Debug)]
struct Stretch {
    from: u64,
    to: u64,
    first: u64,
    frac: f64,
}

/// Builds woven frames line by line, each drawn once the lines it borrows from have come.
pub(super) struct Picture {
    g: Geometry,
    color: AtvColor,
    /// Lines between a line and the comb's neighbours.
    reach: u64,
    /// Lines each side a line's colour borrows from: PAL's averaging, SECAM's other half.
    pair: u64,
    adjust: Adjust,
    sharpness: Sharpness,
    burst: Burst,
    secam: Option<Secam>,
    comb: Comb,
    /// PAL's splitter; NTSC uses the comb.
    transform: Option<TransformPal>,
    held: Vec<Held>,
    drawn: Vec<Drawn>,
    next: u64,
    /// The next line to separate and the next to write.
    separate_at: u64,
    finish_at: u64,
    luma: Vec<u8>,
    rgb: Vec<u8>,
    written: u32,
    carrier: Vec<f32>,
    even: Vec<f32>,
    odd: Vec<f32>,
    chroma: Vec<f32>,
    plain: Vec<f32>,
    mix_u: Vec<f32>,
    mix_v: Vec<f32>,
    blurred: Vec<f32>,
    scratch: Vec<f32>,
}

impl Picture {
    pub fn new(
        g: Geometry,
        color: AtvColor,
        bandwidth_hz: Option<f64>,
        sound_hz: Option<f64>,
        adjust: Adjust,
        depth: Depth,
    ) -> Self {
        let width = usize::from(g.width);
        let pixels = width * usize::from(g.field_lines) * 2;
        let secam = color == AtvColor::Secam;
        let sharpness = Sharpness::new(g.rate, bandwidth_hz, sound_hz, secam);
        let stretch = width + 2 * sharpness.widest() + 3 * MARGIN;
        Self {
            g,
            color,
            reach: comb_lines(color) as u64,
            pair: u64::from(matches!(color, AtvColor::Pal | AtvColor::Secam)),
            adjust,
            sharpness,
            burst: Burst::new(color == AtvColor::Pal),
            secam: secam.then(|| Secam::new(g.rate)),
            comb: Comb::new(g.rate),
            transform: (color == AtvColor::Pal).then(|| {
                let first = usize::from(g.picture_delay).saturating_sub(TRANSFORM_MARGIN);
                let lines = usize::from(g.field_lines) + 2 * TRANSFORM_MARGIN;
                TransformPal::new(stretch, first, lines, depth)
            }),
            held: vec![Held::EMPTY; HELD],
            drawn: (0..DRAWN)
                .map(|_| Drawn {
                    seq: None,
                    tint: Tint::None,
                    luma: vec![0.0; width],
                    u: vec![0.0; width],
                    v: vec![0.0; width],
                })
                .collect(),
            next: 0,
            separate_at: 0,
            finish_at: 0,
            luma: vec![0; pixels],
            rgb: if color == AtvColor::Monochrome {
                Vec::new()
            } else {
                vec![0; pixels * 3]
            },
            written: 0,
            carrier: Vec::new(),
            even: Vec::new(),
            odd: Vec::new(),
            chroma: Vec::new(),
            plain: Vec::new(),
            mix_u: Vec::new(),
            mix_v: Vec::new(),
            blurred: Vec::new(),
            scratch: Vec::new(),
        }
    }

    #[cfg(test)]
    pub fn width(&self) -> u16 {
        self.g.width
    }

    pub fn height(&self) -> u16 {
        self.g.field_lines * 2
    }

    pub fn set_adjust(&mut self, adjust: Adjust) {
        self.adjust = adjust;
    }

    /// The finished field, if it drew anything.
    pub fn take(&mut self) -> Option<VideoPicture> {
        self.sharpness.settle();
        if self.written == 0 {
            return None;
        }
        self.written = 0;
        Some(VideoPicture {
            width: self.g.width,
            height: self.height(),
            luma: self.luma.clone(),
            rgb: self.rgb.clone(),
        })
    }

    /// The oldest sample a held line still reads; the transform keeps its own copy.
    pub fn keep_from(&self) -> Option<u64> {
        if self.transform.is_some() {
            return None;
        }
        (self.separate_at.saturating_sub(self.reach)..self.next)
            .filter_map(|seq| self.get(seq))
            .find(|h| h.is_line())
            .map(|h| h.zero_h.max(0.0) as u64)
    }

    fn span(levels: Levels, g: &Geometry) -> (f32, f32) {
        let sync = levels.sync();
        let black = levels.blank + g.setup * sync;
        let white = levels.blank + g.white * sync;
        (black, (white - black).max(1e-6))
    }

    /// A line without line sync: the colour flywheels run on, and lines split by neighbours
    /// finish without one across it.
    pub fn gap(&mut self, samples: &Samples) {
        self.burst.coast();
        if let Some(secam) = &mut self.secam {
            secam.coast();
        }
        if self.transform.is_none() {
            self.flush(samples);
        }
        self.push(
            Stage::Gap,
            0.0,
            Levels {
                tip: 0.0,
                blank: 1.0,
            },
            None,
            Tint::None,
            None,
        );
    }

    /// A new field of `parity` begins: what the last one completed is drawn.
    pub fn field(&mut self, samples: &Samples, parity: u16) {
        match &mut self.transform {
            Some(transform) => {
                transform.begin_field(usize::from(parity));
                self.advance(samples, false);
            }
            None => self.flush(samples),
        }
    }

    /// Draws every held line with what has come so far.
    pub fn flush(&mut self, samples: &Samples) {
        if let Some(transform) = &mut self.transform {
            transform.finish();
        }
        self.advance(samples, true);
    }

    /// Reads the burst and the line's noise; runs on every line with a line sync.
    pub fn observe(&mut self, samples: &Samples, zero_h: f64, levels: Levels) {
        let (_, span) = Self::span(levels, &self.g);
        let g = self.g;
        let tip = samples.span(
            (zero_h + 0.2 * g.sync).max(0.0) as u64,
            (zero_h + 0.85 * g.sync).max(0.0) as u64,
        );
        self.scratch.clear();
        self.scratch.extend(tip.iter().map(|v| v / span));
        self.sharpness
            .sample_noise(&self.scratch, &mut self.blurred);
        let guard = g.us(BURST_GUARD_US);
        let from = (zero_h + g.burst.0 + guard).max(0.0) as u64;
        let to = (zero_h + g.burst.1 - guard).max(0.0) as u64;
        match self.color {
            AtvColor::Monochrome => {}
            AtvColor::Secam => {
                let to = (zero_h + g.active.0 - guard).max(0.0) as u64;
                if let Some(secam) = &mut self.secam {
                    secam.observe(samples.span(from, to), from, span);
                }
            }
            AtvColor::Pal | AtvColor::Ntsc => {
                let burst = self.demodulate(samples, from, to);
                self.burst.update(burst / span);
            }
        }
    }

    /// Holds one line with a line sync, `field_line` down its field, landing on frame row
    /// `row` if it is a picture line.
    pub fn line(
        &mut self,
        samples: &Samples,
        (zero_h, levels): (f64, &Levels),
        field_line: Option<u32>,
        row: Option<usize>,
    ) {
        let (_, span) = Self::span(*levels, &self.g);
        let tint = match self.color {
            AtvColor::Monochrome => Tint::None,
            AtvColor::Secam => Tint::Secam(self.secam.as_ref().and_then(Secam::difference)),
            AtvColor::Pal | AtvColor::Ntsc => Tint::Qam(self.burst.turn(span)),
        };
        let row = row.filter(|&r| (r + 1) * usize::from(self.g.width) <= self.luma.len());
        let wanted = self.stretch(zero_h).from.saturating_sub(ROW_MARGIN);
        let key = self
            .transform
            .as_mut()
            .zip(field_line)
            .and_then(|(transform, line)| transform.push(line, samples, wanted));
        self.push(Stage::Waiting, zero_h, *levels, row, tint, key);
        self.advance(samples, false);
    }

    fn push(
        &mut self,
        stage: Stage,
        zero_h: f64,
        levels: Levels,
        row: Option<usize>,
        tint: Tint,
        key: Option<RowKey>,
    ) {
        let seq = self.next;
        self.next += 1;
        self.held[slot(seq)] = Held {
            seq,
            stage,
            zero_h,
            levels,
            row,
            tint,
            key,
        };
    }

    fn get(&self, seq: u64) -> Option<&Held> {
        held_at(&self.held, seq)
    }

    /// Whether line `seq` has what it needs to be separated.
    fn ready(&self, seq: u64, held: &Held) -> bool {
        match (&self.transform, held.key) {
            (Some(transform), Some(key)) => transform.row(key).is_some(),
            (Some(_), None) => true,
            (None, _) => seq + self.reach < self.next,
        }
    }

    /// Separates lines in order while they are ready, writing each once its colour partner
    /// is separated too; `all` takes every held line.
    fn advance(&mut self, samples: &Samples, all: bool) {
        self.separate_at = self.separate_at.max(self.next.saturating_sub(HELD as u64));
        while self.separate_at < self.next {
            let seq = self.separate_at;
            if let Some(held) = self.get(seq).copied()
                && held.stage == Stage::Waiting
            {
                if !(all || self.ready(seq, &held)) {
                    break;
                }
                self.separate(samples, seq);
            }
            self.separate_at += 1;
            self.finish_ready(all);
        }
        self.finish_ready(all);
    }

    /// Writes separated lines in order while their partner below has been separated.
    fn finish_ready(&mut self, all: bool) {
        self.finish_at = self.finish_at.max(self.next.saturating_sub(HELD as u64));
        while self.finish_at < self.separate_at {
            let seq = self.finish_at;
            if self.get(seq).is_some_and(|h| h.stage == Stage::Separated) {
                if !(all || seq + self.pair < self.separate_at) {
                    return;
                }
                self.finish(seq);
            }
            self.finish_at += 1;
        }
    }

    fn stretch(&self, zero_h: f64) -> Stretch {
        let g = self.g;
        let width = usize::from(g.width);
        let lead = self.sharpness.blur().half + MARGIN;
        let start = zero_h + g.active.0 + 0.5;
        let first = start.floor().max(0.0) as u64;
        Stretch {
            from: first.saturating_sub(lead as u64),
            to: first + (width + self.sharpness.widest() + 2 * MARGIN) as u64,
            first,
            frac: start - start.floor(),
        }
    }

    /// The comb neighbour `lines` away, shifted by whole subcarrier half-turns to sit over
    /// this line: the offset in samples, if the neighbour is held.
    fn neighbour(&self, seq: u64, zero_h: f64, up: bool) -> Option<i64> {
        if self.reach == 0 {
            return None;
        }
        let other = if up {
            seq.checked_sub(self.reach)
        } else {
            Some(seq + self.reach)
        };
        let near = line_at(&self.held, other)?;
        let apart = near.zero_h - zero_h;
        Some(4 * ((apart - 2.0) / 4.0).round() as i64 + 2)
    }

    fn separate(&mut self, samples: &Samples, seq: u64) {
        let Some(held) = self.get(seq).copied() else {
            return;
        };
        self.held[slot(seq)].stage = Stage::Separated;
        let index = drawn_slot(seq);
        self.drawn[index].seq = None;
        if held.row.is_none() {
            return;
        }
        let s = self.stretch(held.zero_h);
        let len = (s.to - s.from) as usize;
        let (black, span) = Self::span(held.levels, &self.g);
        let mut plain = std::mem::take(&mut self.plain);
        plain.clear();
        let drawn = match held.tint {
            Tint::Qam(turn) if held.key.is_some() => {
                self.split_by_transform(held.key, s, len, &mut plain)
                    && self.draw(&plain, s, (black, span), index)
                    && self.demodulate_qam(s, turn, index)
            }
            Tint::Qam(turn) => {
                self.split_by_comb(samples, (seq, held.zero_h), s, span, &mut plain)
                    && self.draw(&plain, s, (black, span), index)
                    && self.demodulate_qam(s, turn, index)
            }
            Tint::Secam(difference) => {
                plain.extend_from_slice(samples.span(s.from, s.to));
                plain.len() == len
                    && self.draw(&plain, s, (black, span), index)
                    && self.demodulate_secam(samples, held.zero_h, s, difference, index)
            }
            Tint::None => {
                plain.extend_from_slice(samples.span(s.from, s.to));
                plain.len() == len && self.draw(&plain, s, (black, span), index)
            }
        };
        self.plain = plain;
        if drawn {
            self.drawn[index].seq = Some(seq);
            self.drawn[index].tint = held.tint;
        }
    }

    /// Luma of a transform-split line into `plain` and its chroma into `self.chroma`.
    fn split_by_transform(
        &mut self,
        key: Option<RowKey>,
        s: Stretch,
        len: usize,
        plain: &mut Vec<f32>,
    ) -> bool {
        let Some((start, raw, chroma)) = self
            .transform
            .as_ref()
            .zip(key)
            .and_then(|(transform, key)| transform.row(key))
        else {
            return false;
        };
        let Some(offset) = s.from.checked_sub(start).map(|o| o as usize) else {
            return false;
        };
        let (Some(raw), Some(chroma)) = (
            raw.get(offset..offset + len),
            chroma.get(offset..offset + len),
        ) else {
            return false;
        };
        plain.extend(raw.iter().zip(chroma).map(|(x, c)| x - c));
        self.chroma.clear();
        self.chroma.extend_from_slice(chroma);
        true
    }

    /// Luma of a comb-split line into `plain` and its chroma into `self.chroma`.
    fn split_by_comb(
        &mut self,
        samples: &Samples,
        (seq, zero_h): (u64, f64),
        s: Stretch,
        span: f32,
        plain: &mut Vec<f32>,
    ) -> bool {
        let pad = COMB_PAD as u64;
        let len = (s.to - s.from) as usize;
        let raw = samples.span(s.from.saturating_sub(pad), s.to + pad);
        if s.from < pad || raw.len() < len + 2 * COMB_PAD {
            return false;
        }
        let window = |offset: Option<i64>| {
            let from = s.from.checked_add_signed(offset?)?.checked_sub(pad)?;
            let near = samples.span(from, from + (len + 2 * COMB_PAD) as u64);
            (near.len() == len + 2 * COMB_PAD).then_some(near)
        };
        let up = window(self.neighbour(seq, zero_h, true));
        let down = window(self.neighbour(seq, zero_h, false));
        self.comb.separate(raw, up, down, span, &mut self.chroma);
        plain.extend(raw[COMB_PAD..].iter().zip(&self.chroma).map(|(x, c)| x - c));
        true
    }

    /// Blurs, repairs and scales one line's luma into its drawn slot.
    fn draw(&mut self, plain: &[f32], s: Stretch, (black, span): (f32, f32), index: usize) -> bool {
        let blur = self.sharpness.blur();
        let skip = (s.first - s.from) as usize + 1 - blur.half;
        let out = &mut self.drawn[index].luma;
        blur.apply(&plain[skip..], s.frac, out);
        let noise = self.sharpness.noise();
        if noise > REPAIR_NOISE {
            let reach = (REPAIR_REACH * blur.sigma).round().max(2.0) as usize;
            repair_clicks(out, reach, REPAIR_SIGMAS * noise * span, &mut self.scratch);
        }
        let gain = 1.0 / span;
        for y in out.iter_mut() {
            *y = (*y - black) * gain;
        }
        true
    }

    fn demodulate_qam(&mut self, s: Stretch, turn: Turn, index: usize) -> bool {
        let first = s.first + u64::from(s.frac >= 0.5);
        let offset = (first - s.from) as usize;
        modulate(&self.chroma, s.from, &mut self.carrier);
        let drawn = &mut self.drawn[index];
        let width = drawn.u.len();
        demodulate_line(
            &self.carrier[offset - 3..],
            first,
            turn,
            (&mut drawn.u[..width], &mut drawn.v[..width]),
        );
        true
    }

    fn demodulate_secam(
        &mut self,
        samples: &Samples,
        zero_h: f64,
        s: Stretch,
        difference: Option<Difference>,
        index: usize,
    ) -> bool {
        let drawn = &mut self.drawn[index];
        let (Some(difference), Some(secam)) = (difference, &mut self.secam) else {
            drawn.u.fill(0.0);
            drawn.v.fill(0.0);
            return true;
        };
        let first = s.first + u64::from(s.frac >= 0.5);
        let from = (zero_h + self.g.burst.0).max(0.0) as u64;
        let width = drawn.u.len();
        let raw = samples.span(from, first + (width + Secam::REACH) as u64);
        let start = first.saturating_sub(from) as usize;
        let (out, other) = match difference {
            Difference::Red => (&mut drawn.v, &mut drawn.u),
            Difference::Blue => (&mut drawn.u, &mut drawn.v),
        };
        other.fill(0.0);
        secam.demodulate(raw, from, start, difference, out);
        true
    }

    /// Writes a separated line into the frame with its colour partners' help.
    fn finish(&mut self, seq: u64) {
        let index = slot(seq);
        self.held[index].stage = Stage::Done;
        let Some(row) = self.held[index].row else {
            return;
        };
        let Some(drawn) = drawn_line(&self.drawn, seq) else {
            return;
        };
        let width = usize::from(self.g.width);
        let base = row * width;
        for (pixel, &y) in self.luma[base..base + width].iter_mut().zip(&drawn.luma) {
            *pixel = byte(y);
        }
        self.written += 1;
        if matches!(drawn.tint, Tint::None) {
            return;
        }
        let partner = |other: Option<u64>| other.and_then(|o| drawn_line(&self.drawn, o));
        let (up, down) = if self.pair == 0 {
            (None, None)
        } else {
            (
                partner(seq.checked_sub(self.pair)),
                partner(Some(seq + self.pair)),
            )
        };
        let (mut u, mut v) = (
            std::mem::take(&mut self.mix_u),
            std::mem::take(&mut self.mix_v),
        );
        match drawn.tint {
            Tint::Secam(own) => secam_pair(drawn, own, [up, down], (&mut u, &mut v)),
            _ => blend(drawn, [up, down], (&mut u, &mut v)),
        }
        let pixels = self.rgb[3 * base..3 * (base + width)]
            .as_chunks_mut::<3>()
            .0;
        let adjust = self.adjust;
        for (((pixel, &y), &u), &v) in pixels.iter_mut().zip(&drawn.luma).zip(&u).zip(&v) {
            let (u, v) = adjust.apply(u, v);
            *pixel = rgb(y, u, v);
        }
        (self.mix_u, self.mix_v) = (u, v);
    }

    /// Mean subcarrier phasor over `from..to`, as `I + jQ` on the sample grid.
    fn demodulate(&mut self, samples: &Samples, from: u64, to: u64) -> Complex<f32> {
        let from_with = from.saturating_sub(3);
        let raw = samples.span(from_with, to + 3);
        if raw.len() < 8 {
            return Complex::new(0.0, 0.0);
        }
        modulate(raw, from_with, &mut self.carrier);
        split(&self.carrier, &mut self.even, &mut self.odd);
        let mut sum = Complex::new(0.0, 0.0);
        for k in 3..self.carrier.len() - 3 {
            sum += phasor(from_with + k as u64, self.even[k], self.odd[k]);
        }
        sum / (self.carrier.len() - 6) as f32
    }
}

fn slot(seq: u64) -> usize {
    (seq % HELD as u64) as usize
}

fn held_at(held: &[Held], seq: u64) -> Option<&Held> {
    let found = &held[slot(seq)];
    (found.seq == seq && found.stage != Stage::Empty).then_some(found)
}

fn line_at(held: &[Held], seq: Option<u64>) -> Option<&Held> {
    seq.and_then(|s| held_at(held, s)).filter(|h| h.is_line())
}

fn drawn_slot(seq: u64) -> usize {
    (seq % DRAWN as u64) as usize
}

/// Line `seq`'s drawn luma and colour, if still kept.
fn drawn_line(drawn: &[Drawn], seq: u64) -> Option<&Drawn> {
    let found = &drawn[drawn_slot(seq)];
    (found.seq == Some(seq)).then_some(found)
}

/// PAL's U and V averaged `[1 2 1]` with the lines either side, whose V switch is the
/// other way, so phase errors cancel without moving colour up or down.
fn blend(line: &Drawn, near: [Option<&Drawn>; 2], (u, v): (&mut Vec<f32>, &mut Vec<f32>)) {
    u.clone_from(&line.u);
    v.clone_from(&line.v);
    let count = near.iter().flatten().count();
    if count == 0 {
        return;
    }
    let scale = 1.0 / (2.0 + count as f32);
    for (out, own) in [(&mut *u, &line.u), (&mut *v, &line.v)] {
        for (o, &x) in out.iter_mut().zip(own) {
            *o = 2.0 * x;
        }
    }
    for other in near.into_iter().flatten() {
        for (o, &x) in u.iter_mut().zip(&other.u) {
            *o += x;
        }
        for (o, &x) in v.iter_mut().zip(&other.v) {
            *o += x;
        }
    }
    for o in u.iter_mut().chain(v.iter_mut()) {
        *o *= scale;
    }
}

/// A SECAM line's own difference and the other one averaged from the lines either side
/// that carry it.
fn secam_pair(
    line: &Drawn,
    own: Option<Difference>,
    near: [Option<&Drawn>; 2],
    (u, v): (&mut Vec<f32>, &mut Vec<f32>),
) {
    u.clone_from(&line.u);
    v.clone_from(&line.v);
    let Some(own) = own else {
        u.fill(0.0);
        v.fill(0.0);
        return;
    };
    let wanted = match own {
        Difference::Red => Difference::Blue,
        Difference::Blue => Difference::Red,
    };
    let others = near
        .into_iter()
        .flatten()
        .filter(|n| matches!(n.tint, Tint::Secam(Some(d)) if d == wanted))
        .map(|n| match wanted {
            Difference::Red => &n.v[..],
            Difference::Blue => &n.u[..],
        });
    let count = others.clone().count();
    let out = match wanted {
        Difference::Red => v,
        Difference::Blue => u,
    };
    out.fill(0.0);
    if count == 0 {
        return;
    }
    let scale = 1.0 / count as f32;
    for other in others {
        for (o, &x) in out.iter_mut().zip(other) {
            *o += x * scale;
        }
    }
}

fn rgb(y: f32, u: f32, v: f32) -> [u8; 3] {
    [
        byte(y + 1.140 * v),
        byte(y - 0.395 * u - 0.581 * v),
        byte(y + 2.032 * u),
    ]
}

fn byte(value: f32) -> u8 {
    (value * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}
