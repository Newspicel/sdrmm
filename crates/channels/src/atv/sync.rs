use super::standard::Geometry;

/// Video samples at the line rate, addressed by their index since the channel opened.
#[derive(Clone, Debug, Default)]
pub(super) struct Samples {
    data: Vec<f32>,
    base: u64,
}

impl Samples {
    pub fn extend(&mut self, samples: &[f32]) {
        self.data.extend_from_slice(samples);
    }

    pub fn end(&self) -> u64 {
        self.base + self.data.len() as u64
    }

    /// Samples `from..to`, cut to what is held.
    pub fn span(&self, from: u64, to: u64) -> &[f32] {
        let start = from.saturating_sub(self.base).min(self.data.len() as u64) as usize;
        let stop = to.saturating_sub(self.base).min(self.data.len() as u64) as usize;
        &self.data[start..stop.max(start)]
    }

    pub fn forget_before(&mut self, index: u64) {
        let drop = index.saturating_sub(self.base).min(self.data.len() as u64);
        self.data.drain(..drop as usize);
        self.base += drop;
    }
}

/// Where a window of `width` samples has the lowest mean in a folded line of `len`, read
/// round.
fn fold_sync(profile: impl Fn(usize) -> f32, len: usize, width: usize) -> usize {
    let at = |k: usize| profile(k % len);
    let mut sum: f32 = (0..width).map(at).sum();
    let (mut best, mut found) = (sum, 0);
    for start in 1..len {
        sum += at(start + width - 1) - at(start - 1);
        if sum < best {
            (best, found) = (sum, start);
        }
    }
    found
}

fn index(at: f64) -> u64 {
    at.max(0.0) as u64
}

/// Sync tip and blanking level of the detected video, in its own units, so every threshold
/// follows the signal whatever its deviation or gain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Levels {
    pub tip: f32,
    pub blank: f32,
}

impl Levels {
    pub fn sync(self) -> f32 {
        self.blank - self.tip
    }

    /// The sum of `span` on the tip-to-blanking scale, each sample clamped so a click
    /// cannot outweigh the rest; in lanes so it vectorizes.
    fn area(self, span: &[f32]) -> f32 {
        let (tip, inv) = (self.tip, self.sync().recip());
        let level = |v: f32| ((v - tip) * inv).clamp(-0.5, 1.5);
        let (blocks, tail) = span.as_chunks::<AREA_LANES>();
        let mut lanes = [0.0f32; AREA_LANES];
        for block in blocks {
            for (sum, &v) in lanes.iter_mut().zip(block) {
                *sum += level(v);
            }
        }
        lanes.iter().sum::<f32>() + tail.iter().map(|&v| level(v)).sum::<f32>()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pulse {
    None,
    Line,
    Equalizing,
    Broad,
}

/// One line as the flywheel placed it.
#[derive(Clone, Copy, Debug)]
pub(super) struct Line {
    /// Sample index of the sync leading edge.
    pub zero_h: f64,
    pub pulse: Pulse,
    /// A broad pulse starts half a line in, so the field begins on the next line.
    pub broad_mid: bool,
    pub levels: Levels,
}

const ACQUIRE_LINES: u8 = 4;
/// Lines laid over each other when no clean run of syncs shows: noise falls by their root,
/// the sync stays where it is.
const FOLD_LINES: usize = 16;
/// Sync widths either way the two halves of a fold may place the sync and still agree.
const FOLD_AGREE: f64 = 0.25;
/// How far below the porch a folded sync tip must sit, in the noise of its own mean.
const FOLD_CONTRAST: f32 = 6.0;
/// Folds in a row that must find the sync upside down before the polarity turns: noise
/// passes for an upside-down sync now and then, a real one keeps showing.
const FLIP_FOLDS: u32 = 2;
/// How much clearer than upright an upside-down sync must stand to count toward a flip.
const FLIP_MARGIN: f32 = 2.0;
/// Share of the folded line's whole range a sync must drop by: a sync tip sits well below
/// black, a dark detail does not.
const FOLD_DEPTH: f32 = 0.15;
/// Samples per independent noise sample in detected video, which is oversampled.
const NOISE_SPAN: f32 = 4.0;
const AREA_LANES: usize = 8;
const LINE_TOLERANCE: f64 = 0.03;
const PULSE_WIDTH: (f64, f64) = (0.5, 1.6);
const TIP_ATTACK: f32 = 0.1;
const TIP_RELEASE: f32 = 1e-5;
const MEAN_TRACK: f32 = 2e-5;
const SLICE: f32 = 0.25;
const MAX_MISSES: u32 = 16;
/// Lines without a line sync past any vertical interval: the levels are wrong, start over.
const MAX_UNLINED: u32 = 32;
/// Edge jitter, in square samples, at which the timing loops run at half speed: past it a
/// line's own timing says less than the flywheel already knows.
const JITTER_REF: f64 = 1.0;
const JITTER_TRACK: f64 = 0.02;
/// How fast a noisy lock's line rate falls back to the standard's: broadcast timebases hold
/// it far better than noisy edges can measure it.
const RATE_LEAK: f64 = 0.01;
/// Lines a noisy lock may coast without a pulse before it starts over: a field.
const MAX_COAST: f64 = 312.0;
const SYNC_WIDTH_RANGE: (f64, f64) = (0.7, 1.4);
/// Lines before a field is due within which one broad pulse starts it.
const FIELD_SLACK: u32 = 8;
/// Lines after a coasted field start within which a real vertical sync pulls the field into
/// line instead of starting another.
const FIELD_REALIGN: u32 = 32;
/// Lines a measured field may differ from the standard's half frame and still set the pace.
const FIELD_SPREAD: u32 = 2;
const PHASE_GAIN: f64 = 0.15;
const RATE_GAIN: f64 = 0.004;
const RATE_LIMIT: f64 = 0.005;
const SYNC_WIDTH_TRACK: f64 = 0.05;
const PORCH_TOLERANCE: f32 = 0.15;
/// Edge windows span whole subcarrier cycles of four samples, so burst ringing sums to nothing.
const CYCLE: f64 = 4.0;

/// Looks for a run of evenly spaced line syncs to lock onto.
#[derive(Clone, Debug)]
struct Acquire {
    scan: u64,
    /// Where the next fold of lines starts.
    fold_from: u64,
    sum: f64,
    seen: u64,
    tip: f32,
    mean: f32,
    previous: f32,
    fall: Option<f64>,
    last: Option<f64>,
    run: u8,
}

impl Acquire {
    /// Feeds one boxcar mean centred on sample `at`; returns where the line sync that
    /// completes a run of evenly spaced ones starts, with rough levels.
    fn step(&mut self, s: f32, at: f64, g: &Geometry, width: usize) -> Option<(f64, Levels)> {
        self.seen += 1;
        if self.seen <= width as u64 {
            (self.tip, self.mean, self.previous) = (s, s, s);
            return None;
        }
        let attack = if s < self.tip {
            TIP_ATTACK
        } else {
            TIP_RELEASE
        };
        self.tip += attack * (s - self.tip);
        self.mean += MEAN_TRACK.max(1.0 / self.seen as f32) * (s - self.mean);
        let slice = self.tip + SLICE * (self.mean - self.tip);
        let previous = std::mem::replace(&mut self.previous, s);
        if (self.seen as f64) < g.line {
            return None;
        }
        let crossing = at - 1.0 + f64::from((previous - slice) / (previous - s));
        if previous >= slice && s < slice {
            self.fall = Some(crossing);
            return None;
        }
        if previous >= slice || s < slice {
            return None;
        }
        let fall = self.fall.take()?;
        if !(PULSE_WIDTH.0..PULSE_WIDTH.1).contains(&((crossing - fall) / g.sync)) {
            return None;
        }
        let spaced = self
            .last
            .is_some_and(|last| ((fall - last) / g.line - 1.0).abs() < LINE_TOLERANCE);
        self.run = if spaced { self.run + 1 } else { 1 };
        self.last = Some(fall);
        (self.run >= ACQUIRE_LINES).then(|| {
            let levels = Levels {
                tip: self.tip,
                blank: self.tip + 0.5 * (self.mean - self.tip),
            };
            (fall, levels)
        })
    }

    fn new(scan: u64) -> Self {
        Self {
            scan,
            fold_from: scan,
            sum: 0.0,
            seen: 0,
            tip: 0.0,
            mean: 0.0,
            previous: 0.0,
            fall: None,
            last: None,
            run: 0,
        }
    }
}

#[derive(Clone, Debug)]
enum Mode {
    Acquire(Acquire),
    /// `misses` counts lines without a pulse, `unlined` lines without a line sync.
    Locked {
        misses: u32,
        unlined: u32,
    },
}

#[derive(Clone, Debug)]
pub(super) struct LineSync {
    g: Geometry,
    boxcar: usize,
    window: usize,
    mode: Mode,
    phase: f64,
    line: f64,
    sync_width: f64,
    levels: Levels,
    settled: u32,
    /// Mean square of the line edge error, in samples: how noisy each line's timing reads.
    jitter: f64,
    /// A fold found the sync upside down.
    inverted: bool,
    /// Folds in a row that found it so.
    upside_down: u32,
    /// The two halves of a fold of lines, kept so acquiring does not allocate.
    folds: [Vec<f32>; 2],
}

impl LineSync {
    pub fn new(g: Geometry) -> Self {
        Self {
            g,
            boxcar: (g.us(1.0).round() as usize).max(1),
            window: ((g.us(0.6) / CYCLE).round() * CYCLE).max(CYCLE) as usize,
            mode: Mode::Acquire(Acquire::new(0)),
            phase: 0.0,
            line: g.line,
            sync_width: g.sync,
            levels: Levels {
                tip: 0.0,
                blank: 1.0,
            },
            settled: 0,
            jitter: 0.0,
            inverted: false,
            upside_down: 0,
            folds: [Vec::new(), Vec::new()],
        }
    }

    pub fn geometry(&self) -> &Geometry {
        &self.g
    }

    pub fn locked(&self) -> bool {
        matches!(self.mode, Mode::Locked { .. })
    }

    /// The oldest sample a later call still reads.
    pub fn keep_from(&self) -> u64 {
        match &self.mode {
            Mode::Acquire(a) => a
                .scan
                .saturating_sub(self.boxcar as u64 + 1)
                .min(a.fold_from),
            Mode::Locked { .. } => {
                index(self.phase - self.g.front_porch - self.window as f64 - 4.0)
            }
        }
    }

    /// The next whole line, once enough samples are held to measure it.
    pub fn next(&mut self, samples: &Samples) -> Option<Line> {
        if let Mode::Acquire(_) = self.mode {
            self.acquire(samples);
        }
        if !self.locked() || (samples.end() as f64) < self.phase + self.line + 2.0 {
            return None;
        }
        Some(self.measure(samples))
    }

    fn acquire(&mut self, samples: &Samples) {
        let Mode::Acquire(a) = &mut self.mode else {
            return;
        };
        let width = self.boxcar;
        let held = samples.span(a.scan.saturating_sub(width as u64), samples.end());
        let offset = a.scan.min(width as u64) as usize;
        let mut found = None;
        for k in offset..held.len() {
            a.sum += f64::from(held[k]);
            if k >= width {
                a.sum -= f64::from(held[k - width]);
            }
            let at = a.scan as f64 - (width as f64 - 1.0) / 2.0;
            a.scan += 1;
            found = a.step((a.sum / width as f64) as f32, at, &self.g, width);
            if found.is_some() {
                break;
            }
        }
        let (found, folded) = match found {
            Some(found) => (Some(found), false),
            None => (self.fold(samples), true),
        };
        if let Some((fall, rough)) = found {
            self.phase = fall + self.g.line;
            self.line = self.g.line;
            self.sync_width = self.g.sync;
            self.levels = if folded {
                rough
            } else {
                self.pulse_levels(samples, fall).unwrap_or(rough)
            };
            self.settled = 0;
            self.mode = Mode::Locked {
                misses: 0,
                unlined: 0,
            };
        }
    }

    /// Lays `FOLD_LINES` lines over each other at the nominal line rate and finds the sync in
    /// their mean, for signals too weak to show it line by line. Both halves of the fold must
    /// put it in the same place.
    fn fold(&mut self, samples: &Samples) -> Option<(f64, Levels)> {
        let Mode::Acquire(a) = &mut self.mode else {
            return None;
        };
        let g = self.g;
        let len = g.line.round() as usize;
        let span = (FOLD_LINES as f64 * g.line).ceil() as u64;
        if samples.end() < a.fold_from + span + len as u64 {
            return None;
        }
        let from = a.fold_from;
        a.fold_from += span / 2;
        let half = FOLD_LINES / 2;
        for fold in &mut self.folds {
            fold.clear();
            fold.resize(len, 0.0);
        }
        for line in 0..FOLD_LINES {
            let start = index(from as f64 + line as f64 * g.line);
            let row = samples.span(start, start + len as u64);
            if row.len() < len {
                return None;
            }
            for (sum, &v) in self.folds[line / half].iter_mut().zip(row) {
                *sum += v / half as f32;
            }
        }
        let (upright, found) = self.fold_lock(1.0, from, len);
        let (inverted, _) = self.fold_lock(-1.0, from, len);
        let upside_down = found.is_none() && inverted > FLIP_MARGIN * upright.max(FOLD_CONTRAST);
        self.upside_down = if upside_down { self.upside_down + 1 } else { 0 };
        if self.upside_down >= FLIP_FOLDS {
            self.upside_down = 0;
            self.inverted = true;
        }
        found
    }

    /// The sync in the folded lines read with `sign`: how far its tip stands below the porch,
    /// in the porch's noise, and, where the halves agree on it and it stands clear enough,
    /// where its leading edge falls and the levels.
    fn fold_lock(&self, sign: f32, from: u64, len: usize) -> (f32, Option<(f64, Levels)>) {
        let g = self.g;
        let at = |k: usize, i: usize| sign * self.folds[k][i];
        let width = (g.sync.round() as usize).max(1);
        let [early, late] = [0, 1].map(|k| fold_sync(|i| at(k, i), len, width));
        let apart = early.abs_diff(late).min(len - early.abs_diff(late)) as f64;
        let agree = apart <= FOLD_AGREE * g.sync;
        let both = |k: i64| {
            let i = k.rem_euclid(len as i64) as usize;
            f32::midpoint(at(0, i), at(1, i))
        };
        let span =
            |range: &std::ops::Range<f64>| range.start.round() as i64..range.end.round() as i64;
        let mean = |range: &std::ops::Range<f64>| {
            let r = span(range);
            r.clone().map(both).sum::<f32>() / (r.end - r.start).max(1) as f32
        };
        let edge = late as f64;
        let tip = edge + 0.2 * g.sync..edge + 0.85 * g.sync;
        let porch = edge + g.burst.1 + g.us(0.4)..edge + g.active.0 - g.us(0.4);
        let levels = Levels {
            tip: mean(&tip),
            blank: mean(&porch),
        };
        let r = span(&porch);
        let variance = r
            .clone()
            .map(|k| (both(k) - levels.blank).powi(2))
            .sum::<f32>()
            / (r.end - r.start).max(1) as f32;
        let independent = ((tip.end - tip.start) as f32 / NOISE_SPAN).max(1.0);
        let noise = (variance / independent).sqrt();
        let fall = from as f64 + (FOLD_LINES - 1) as f64 * g.line + edge;
        let depth = levels.sync();
        let (low, high) = (0..len as i64)
            .map(both)
            .fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
        let deep = depth > FOLD_DEPTH * (high - low);
        let score = if deep {
            depth / noise.max(f32::MIN_POSITIVE)
        } else {
            0.0
        };
        (
            score,
            (agree && score > FOLD_CONTRAST).then_some((fall, levels)),
        )
    }

    /// Whether a fold found the sync upside down since last asked: the video's polarity is
    /// the other way round.
    pub fn take_inverted(&mut self) -> bool {
        std::mem::take(&mut self.inverted)
    }

    /// Tip and blanking read off the found pulse and the porch before it, sounder than the
    /// scan's running estimates, which the stream's first samples can drag.
    fn pulse_levels(&self, samples: &Samples, fall: f64) -> Option<Levels> {
        let (g, sw) = (&self.g, self.g.sync);
        let (tip, tips) = Self::mean_raw(samples, fall + 0.2 * sw, fall + 0.85 * sw)?;
        let (blank, blanks) =
            Self::mean_raw(samples, fall - g.front_porch + g.us(0.3), fall - g.us(0.3))?;
        let levels = Levels {
            tip: tip / tips as f32,
            blank: blank / blanks as f32,
        };
        (levels.sync() > 0.0).then_some(levels)
    }

    fn mean_normalized(&self, samples: &Samples, from: f64, to: f64) -> f32 {
        let span = samples.span(index(from), index(to));
        if span.is_empty() {
            return 1.0;
        }
        self.levels.area(span) / span.len() as f32
    }

    fn mean_raw(samples: &Samples, from: f64, to: f64) -> Option<(f32, usize)> {
        let span = samples.span(index(from), index(to));
        (!span.is_empty()).then(|| (span.iter().sum::<f32>(), span.len()))
    }

    /// Where a step near `expected` really is, from the area it leaves in a window that
    /// reaches `before` samples ahead of it and `after` past it.
    fn edge(&self, samples: &Samples, expected: f64, falling: bool, after: f64) -> f64 {
        let w = self.window as f64;
        let start = (expected.round() - w).max(0.0);
        let reach = after.min(w).max(2.0).floor();
        let cycles = ((w + reach) / CYCLE).floor().max(1.0) * CYCLE;
        let stop = start
            + if cycles < w + 2.0 {
                cycles + CYCLE
            } else {
                cycles
            };
        let span = samples.span(index(start), index(stop));
        let area = self.levels.area(span);
        let edge = if falling {
            start + f64::from(area) - 0.5
        } else {
            stop - 0.5 - f64::from(area)
        };
        (edge - expected).clamp(-w / 2.0, w / 2.0)
    }

    /// Whether the front porch sits at blanking, so the leading edge is not dragged by the
    /// picture running into it.
    fn porch_is_blank(&self, samples: &Samples, phi: f64) -> bool {
        let g = &self.g;
        let porch =
            self.mean_normalized(samples, phi - g.front_porch + g.us(0.5), phi - g.us(0.25));
        (porch - 1.0).abs() < PORCH_TOLERANCE
    }

    fn classify(&self, samples: &Samples, phi: f64) -> (Pulse, bool) {
        let (g, sw) = (&self.g, self.g.sync);
        let lead = self.mean_normalized(samples, phi + 0.06 * sw, phi + 0.42 * sw);
        if lead >= 0.5 {
            return (Pulse::None, false);
        }
        let middle = self.mean_normalized(samples, phi + 0.64 * sw, phi + 0.94 * sw);
        let after = self.mean_normalized(samples, phi + 1.2 * sw, phi + 0.4 * g.line);
        let mid = phi + 0.5 * g.line;
        let broad_mid = self.mean_normalized(samples, mid + 1.2 * sw, phi + 0.9 * g.line) < 0.5;
        let pulse = if after < 0.5 {
            Pulse::Broad
        } else if middle < 0.5 {
            Pulse::Line
        } else {
            Pulse::Equalizing
        };
        (pulse, broad_mid)
    }

    fn measure(&mut self, samples: &Samples) -> Line {
        let phi = self.phase;
        let (pulse, broad_mid) = self.classify(samples, phi);
        let clean = self.porch_is_blank(samples, phi);
        let lead = self.edge(samples, phi, true, self.window as f64);
        let error = match pulse {
            Pulse::None => 0.0,
            Pulse::Line => {
                let quiet = self.g.burst.0 - self.sync_width - self.g.us(0.4);
                let trail = self.edge(samples, phi + self.sync_width, false, quiet);
                let error = if clean {
                    self.sync_width += self.calm() * SYNC_WIDTH_TRACK * (trail - lead);
                    self.sync_width = self.sync_width.clamp(
                        SYNC_WIDTH_RANGE.0 * self.g.sync,
                        SYNC_WIDTH_RANGE.1 * self.g.sync,
                    );
                    f64::midpoint(lead, trail)
                } else {
                    trail
                };
                self.jitter += JITTER_TRACK * (error * error - self.jitter);
                error
            }
            Pulse::Equalizing | Pulse::Broad if clean => lead,
            Pulse::Equalizing | Pulse::Broad => 0.0,
        };
        if pulse == Pulse::Line {
            self.track_levels(samples, phi);
        }
        let calm = self.calm();
        let zero_h = phi + PHASE_GAIN * calm * error;
        let nominal = self.g.line;
        self.line += RATE_LEAK * (1.0 - calm) * (nominal - self.line);
        self.line = (self.line + RATE_GAIN * calm * calm * error)
            .clamp(nominal * (1.0 - RATE_LIMIT), nominal * (1.0 + RATE_LIMIT));
        self.phase = zero_h + self.line;
        if let Mode::Locked { misses, unlined } = &mut self.mode {
            *misses = if pulse == Pulse::None { *misses + 1 } else { 0 };
            *unlined = if pulse == Pulse::Line {
                0
            } else {
                *unlined + 1
            };
            let coast = (1.0 + self.jitter / JITTER_REF).min(MAX_COAST / f64::from(MAX_MISSES));
            if f64::from(*misses) > f64::from(MAX_MISSES) * coast
                || f64::from(*unlined) > f64::from(MAX_UNLINED) * coast
            {
                self.mode = Mode::Acquire(Acquire::new(index(self.phase)));
            }
        }
        Line {
            zero_h,
            pulse,
            broad_mid,
            levels: self.levels,
        }
    }

    /// How far the timing loops trust one line: 1 on a clean signal, falling as the edges
    /// grow noisy.
    fn calm(&self) -> f64 {
        1.0 / (1.0 + self.jitter / JITTER_REF)
    }

    fn track_levels(&mut self, samples: &Samples, phi: f64) {
        let g = self.g;
        let sw = self.sync_width;
        let Some((tip_sum, tip_count)) = Self::mean_raw(samples, phi + 0.2 * sw, phi + 0.85 * sw)
        else {
            return;
        };
        let Some((blank_sum, blank_count)) = Self::mean_raw(
            samples,
            phi + g.burst.1 + g.us(0.4),
            phi + g.active.0 - g.us(0.4),
        ) else {
            return;
        };
        let tip = tip_sum / tip_count as f32;
        let blank = blank_sum / blank_count as f32;
        self.settled += 1;
        let gain = match self.settled {
            0..=4 => 0.5,
            5..=32 => 0.1,
            _ => 0.03,
        };
        let sync = self.levels.sync().abs().max(1e-6);
        let held = |value: f32, old: f32| old + (value - old).clamp(-sync, sync);
        let tip = if self.settled > 4 {
            held(tip, self.levels.tip)
        } else {
            tip
        };
        let blank = if self.settled > 4 {
            held(blank, self.levels.blank)
        } else {
            blank
        };
        self.levels.tip += gain * (tip - self.levels.tip);
        self.levels.blank += gain * (blank - self.levels.blank);
        if self.levels.sync() <= 0.0 {
            self.mode = Mode::Acquire(Acquire::new(index(self.phase)));
        }
    }
}

/// Counts lines from each field's vertical sync and names the field.
#[derive(Clone, Debug, Default)]
pub(super) struct Fields {
    in_vertical: bool,
    /// Broad pulses in a row; a lone one far from where a field is due is noise.
    broad_run: u32,
    broad_mid_before: bool,
    line: Option<u32>,
    parity: u16,
    /// Lines each field parity last ran from vertical sync to vertical sync: where the next
    /// field of that parity starts when its broad pulses are lost in noise.
    lengths: [Option<u32>; 2],
    /// The field coming in was started by the flywheel, so a broad pulse just after may
    /// still pull it into line.
    coasted: bool,
}

/// What a line means for the picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FieldLine {
    pub new_field: bool,
    /// Line of the field counted from its first broad pulse.
    pub line: Option<u32>,
    pub parity: u16,
}

impl Fields {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn on_line(&mut self, line: &Line, field_len: u32) -> FieldLine {
        let broad = line.pulse == Pulse::Broad;
        let mut new_field = false;
        let due = self
            .line
            .is_none_or(|count| count + FIELD_SLACK >= field_len);
        let late = self.coasted && self.line.is_some_and(|count| count < FIELD_REALIGN);
        let vertical = broad && (due || late || self.broad_run > 0);
        self.broad_run = if broad { self.broad_run + 1 } else { 0 };
        if vertical && !self.in_vertical {
            if let Some(count) = self
                .line
                .map(|count| count + 1)
                .filter(|&count| !late && count.abs_diff(field_len) <= FIELD_SPREAD)
            {
                self.lengths[usize::from(self.parity)] = Some(count);
            }
            self.in_vertical = true;
            self.parity = u16::from(self.broad_mid_before);
            self.line = Some(0);
            new_field = !late;
            self.coasted = false;
        } else if let Some(count) = &mut self.line {
            *count += 1;
            let expected = self.lengths[usize::from(self.parity)]
                .unwrap_or(field_len + u32::from(self.parity));
            if *count >= expected {
                *count = 0;
                self.parity ^= 1;
                new_field = true;
                self.coasted = true;
            }
        }
        if !broad && !line.broad_mid {
            self.in_vertical = false;
        }
        self.broad_mid_before = line.broad_mid && !broad;
        FieldLine {
            new_field,
            line: self.line,
            parity: self.parity,
        }
    }
}
