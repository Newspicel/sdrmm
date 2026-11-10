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
    pub fn sync(&self) -> f32 {
        self.blank - self.tip
    }

    /// The sum of `span` on the tip-to-blanking scale, each sample clamped so a click
    /// cannot outweigh the rest; in lanes so it vectorizes.
    fn area(&self, span: &[f32]) -> f32 {
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
const AREA_LANES: usize = 8;
const LINE_TOLERANCE: f64 = 0.03;
const PULSE_WIDTH: (f64, f64) = (0.5, 1.6);
const TIP_ATTACK: f32 = 0.1;
const TIP_RELEASE: f32 = 1e-5;
const MEAN_TRACK: f32 = 2e-5;
const SLICE: f32 = 0.25;
const MAX_MISSES: u32 = 16;
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
    Locked { misses: u32 },
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
            Mode::Acquire(a) => a.scan.saturating_sub(self.boxcar as u64 + 1),
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
        if let Some((fall, levels)) = found {
            self.phase = fall + self.g.line;
            self.line = self.g.line;
            self.sync_width = self.g.sync;
            self.levels = levels;
            self.settled = 0;
            self.mode = Mode::Locked { misses: 0 };
        }
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
                if clean {
                    self.sync_width += SYNC_WIDTH_TRACK * (trail - lead);
                    0.5 * (lead + trail)
                } else {
                    trail
                }
            }
            Pulse::Equalizing | Pulse::Broad if clean => lead,
            Pulse::Equalizing | Pulse::Broad => 0.0,
        };
        if pulse == Pulse::Line {
            self.track_levels(samples, phi);
        }
        let zero_h = phi + PHASE_GAIN * error;
        let nominal = self.g.line;
        self.line = (self.line + RATE_GAIN * error)
            .clamp(nominal * (1.0 - RATE_LIMIT), nominal * (1.0 + RATE_LIMIT));
        self.phase = zero_h + self.line;
        if let Mode::Locked { misses } = &mut self.mode {
            *misses = if pulse == Pulse::None { *misses + 1 } else { 0 };
            if *misses > MAX_MISSES {
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
    broad_mid_before: bool,
    line: Option<u32>,
    parity: u16,
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
        if broad && !self.in_vertical {
            self.in_vertical = true;
            self.parity = u16::from(self.broad_mid_before);
            self.line = Some(0);
            new_field = true;
        } else if let Some(count) = &mut self.line {
            *count += 1;
            if *count > field_len + 8 {
                *count = 0;
                self.parity ^= 1;
                new_field = true;
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
