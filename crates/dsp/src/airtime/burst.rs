const BRIDGED_BINS: usize = 1;
const HANGOVER_FRAMES: u64 = 2;
const MIN_FRAMES: u64 = 4;
const MIN_HITS: u32 = 8;
const TAIL_SHARE: f64 = 0.05;
const PAD_BINS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Burst {
    pub first: u64,
    pub frames: u64,
    pub low: usize,
    pub high: usize,
    pub centre: f32,
    pub mean: f32,
    pub peak: f32,
    pub edge: bool,
    pub cut: bool,
}

struct Track {
    live: bool,
    first: u64,
    last: u64,
    low: usize,
    high: usize,
    reach_low: usize,
    reach_high: usize,
    energy: Vec<f32>,
    hits: u32,
    on: u32,
    frame_power: f32,
    peak: f32,
    edge: bool,
}

impl Track {
    fn new(bins: usize) -> Self {
        Self {
            live: false,
            first: 0,
            last: 0,
            low: 0,
            high: 0,
            reach_low: 0,
            reach_high: 0,
            energy: vec![0.0; bins],
            hits: 0,
            on: 0,
            frame_power: 0.0,
            peak: 0.0,
            edge: false,
        }
    }

    fn open(&mut self, frame: u64, segment: &Segment) {
        self.live = true;
        self.first = frame;
        self.last = frame;
        self.low = segment.low;
        self.high = segment.high;
        self.reach_low = segment.low;
        self.reach_high = segment.high;
        self.energy.fill(0.0);
        self.hits = 0;
        self.on = 0;
        self.frame_power = 0.0;
        self.peak = 0.0;
        self.edge = false;
    }

    fn reaches(&self, segment: &Segment, frame: u64) -> bool {
        self.live
            && self.last + HANGOVER_FRAMES >= frame
            && segment.low <= self.high + 1
            && segment.high + 1 >= self.low
    }

    fn join(&mut self, segment: &Segment, frame: u64, power: &[f32], floor: &[f32]) {
        if self.last == frame && self.on > 0 {
            self.frame_power += segment.power;
        } else {
            self.peak = self.peak.max(self.frame_power);
            self.frame_power = segment.power;
            self.on += 1;
        }
        self.last = frame;
        self.low = self.low.min(segment.low);
        self.high = self.high.max(segment.high);
        self.hits += segment.hits;
        let from = segment.low.saturating_sub(PAD_BINS);
        let to = (segment.high + PAD_BINS).min(power.len() - 1);
        for ((total, &level), &noise) in self.energy[from..=to]
            .iter_mut()
            .zip(&power[from..=to])
            .zip(&floor[from..=to])
        {
            if noise.is_finite() {
                *total += level - noise;
            }
        }
        self.reach_low = self.reach_low.min(from);
        self.reach_high = self.reach_high.max(to);
        self.edge |= unusable(floor, segment.low.checked_sub(1))
            || unusable(floor, Some(segment.high + 1));
    }

    fn close(&mut self) -> Option<Burst> {
        self.live = false;
        let frames = self.last - self.first + 1;
        if frames < MIN_FRAMES || self.hits < MIN_HITS {
            return None;
        }
        let base = self.reach_low;
        let spread = &mut self.energy[base..=self.reach_high];
        spread.iter_mut().for_each(|bin| *bin = bin.max(0.0));
        let total: f64 = spread.iter().map(|&bin| f64::from(bin)).sum();
        let (low, high) = central(spread, total);
        let moment: f64 = spread
            .iter()
            .enumerate()
            .map(|(offset, &bin)| f64::from(bin) * offset as f64)
            .sum();
        Some(Burst {
            first: self.first,
            frames,
            low: base + low,
            high: base + high,
            centre: (base as f64 + moment / total.max(f64::MIN_POSITIVE)) as f32,
            mean: (total / f64::from(self.on.max(1))) as f32,
            peak: self.peak.max(self.frame_power),
            edge: self.edge,
            cut: false,
        })
    }
}

fn unusable(floor: &[f32], bin: Option<usize>) -> bool {
    bin.and_then(|bin| floor.get(bin))
        .is_none_or(|noise| !noise.is_finite())
}

fn central(spread: &[f32], total: f64) -> (usize, usize) {
    let tail = total * TAIL_SHARE;
    let mut running = 0.0;
    let low = spread
        .iter()
        .position(|&bin| {
            running += f64::from(bin);
            running > tail
        })
        .unwrap_or(0);
    running = 0.0;
    let high = spread
        .iter()
        .rposition(|&bin| {
            running += f64::from(bin);
            running > tail
        })
        .unwrap_or(spread.len().saturating_sub(1));
    (low, high.max(low))
}

#[derive(Clone, Copy)]
struct Segment {
    low: usize,
    high: usize,
    power: f32,
    hits: u32,
}

pub struct Bursts {
    tracks: Vec<Track>,
    segments: Vec<Segment>,
    frame: u64,
    longest: u64,
    dropped: u64,
}

impl Bursts {
    #[must_use]
    pub fn new(bins: usize, capacity: usize, longest: u64) -> Self {
        Self {
            tracks: (0..capacity.max(1)).map(|_| Track::new(bins)).collect(),
            segments: Vec::with_capacity(bins),
            frame: 0,
            longest: longest.max(MIN_FRAMES),
            dropped: 0,
        }
    }

    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn reset(&mut self) {
        self.tracks.iter_mut().for_each(|track| track.live = false);
        self.frame = 0;
    }

    pub fn frame(&mut self, power: &[f32], floor: &[f32], margin: f32, mut done: impl FnMut(Burst)) {
        self.find_segments(power, floor, margin);
        let frame = self.frame;
        for index in 0..self.segments.len() {
            let segment = self.segments[index];
            let track = match self
                .tracks
                .iter()
                .position(|track| track.reaches(&segment, frame))
            {
                Some(found) => found,
                None => match self.tracks.iter().position(|track| !track.live) {
                    Some(free) => {
                        self.tracks[free].open(frame, &segment);
                        free
                    }
                    None => {
                        self.dropped += 1;
                        continue;
                    }
                },
            };
            self.tracks[track].join(&segment, frame, power, floor);
        }
        for track in &mut self.tracks {
            if !track.live {
                continue;
            }
            if track.last + HANGOVER_FRAMES < frame {
                if let Some(burst) = track.close() {
                    done(burst);
                }
            } else if frame - track.first + 1 >= self.longest {
                if let Some(burst) = track.close() {
                    done(Burst { cut: true, ..burst });
                }
            }
        }
        self.frame += 1;
    }

    pub fn flush(&mut self, mut done: impl FnMut(Burst)) {
        for track in &mut self.tracks {
            if track.live
                && let Some(burst) = track.close()
            {
                done(burst);
            }
        }
    }

    fn find_segments(&mut self, power: &[f32], floor: &[f32], margin: f32) {
        self.segments.clear();
        let mut open: Option<Segment> = None;
        let mut quiet = 0;
        for (bin, (&level, &noise)) in power.iter().zip(floor).enumerate() {
            if level > noise * margin {
                let segment = open.get_or_insert(Segment {
                    low: bin,
                    high: bin,
                    power: 0.0,
                    hits: 0,
                });
                segment.high = bin;
                segment.power += level;
                segment.hits += 1;
                quiet = 0;
            } else if let Some(segment) = open {
                quiet += 1;
                if quiet > BRIDGED_BINS {
                    self.segments.push(segment);
                    open = None;
                }
            }
        }
        if let Some(segment) = open {
            self.segments.push(segment);
        }
    }
}
