mod fit;
mod heading;
mod lag;
mod track;

use crate::math::special::norm_deg;
use fit::{TEMPLATE_POINTS, beam_template};
use heading::HeadingRing;
use lag::{PastSweep, START_LAG_S};
use track::{LEVEL_CAPACITY, Track};

pub const SWEEP_BINS: usize = 72;

const LIKELIHOOD_POINTS: usize = 360;
const FLIP_HOLD_S: f64 = 0.3;
const MAX_SPAN_DEG: f64 = 390.0;
const BIN_WIDTH_DEG: f64 = 360.0 / SWEEP_BINS as f64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SweepConfig {
    pub beamwidth_deg: f64,
    pub front_back_db: f64,
    pub min_span_deg: f64,
    pub min_contrast_db: f32,
    pub min_fit: f32,
    pub max_heading_sigma_deg: f32,
    pub stop_s: f64,
    pub min_rate_dps: f64,
    pub max_rate_dps: f64,
    pub sample_correlation: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LevelSample {
    pub t_s: f64,
    pub level_db: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeadingSample {
    pub t_s: f64,
    pub heading_deg: f64,
    pub sigma_deg: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotation {
    Clockwise,
    Counterclockwise,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SweepReject {
    ShortSpan,
    LowContrast,
    PoorFit,
    HeadingPoor,
    TooFast,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SweepPhase {
    Idle,
    Sweeping { span_deg: f32, rate_dps: f32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct SweepBearing {
    pub bearing_deg: f64,
    pub sigma_deg: f32,
    pub confidence: f32,
    pub fit: f32,
    pub contrast_db: f32,
    pub span_deg: f32,
    pub rate_dps: f32,
    pub rotation: Rotation,
    pub samples: u16,
    pub lag_s: f32,
    pub likelihood: [f32; LIKELIHOOD_POINTS],
}

pub struct SweepEstimator {
    config: SweepConfig,
    template: [f32; TEMPLATE_POINTS],
    headings: Box<HeadingRing>,
    track: Box<Track>,
    motion: Motion,
    bins: [f32; SWEEP_BINS],
    lag_s: f64,
    last: [Option<PastSweep>; 2],
    scratch: Box<[f32; LEVEL_CAPACITY]>,
    correlation: [f64; LIKELIHOOD_POINTS],
    dropped_levels: u64,
}

/// One turn in a single direction, with how long it has stood still or turned back.
#[derive(Clone, Copy)]
struct Segment {
    sign: f64,
    still_since: Option<f64>,
    flip_since: Option<f64>,
}

#[derive(Clone, Copy)]
enum Motion {
    Idle,
    Sweeping(Segment),
    Spinning,
}

#[derive(Clone, Copy)]
enum Close {
    Stop,
    Flip,
    Span,
}

impl Default for SweepConfig {
    fn default() -> Self {
        Self {
            beamwidth_deg: 60.0,
            front_back_db: 15.0,
            min_span_deg: 180.0,
            min_contrast_db: 6.0,
            min_fit: 0.5,
            max_heading_sigma_deg: 30.0,
            stop_s: 0.7,
            min_rate_dps: 10.0,
            max_rate_dps: 360.0,
            sample_correlation: 4.0,
        }
    }
}

impl Default for SweepBearing {
    fn default() -> Self {
        Self {
            bearing_deg: 0.0,
            sigma_deg: 0.0,
            confidence: 0.0,
            fit: 0.0,
            contrast_db: 0.0,
            span_deg: 0.0,
            rate_dps: 0.0,
            rotation: Rotation::Clockwise,
            samples: 0,
            lag_s: 0.0,
            likelihood: [0.0; LIKELIHOOD_POINTS],
        }
    }
}

impl Segment {
    const fn turning(sign: f64) -> Self {
        Self {
            sign,
            still_since: None,
            flip_since: None,
        }
    }
}

impl SweepEstimator {
    #[must_use]
    pub fn new(config: SweepConfig) -> Self {
        let mut estimator = Self {
            config,
            template: [0.0; TEMPLATE_POINTS],
            headings: Box::new(HeadingRing::new()),
            track: Box::new(Track::new()),
            motion: Motion::Idle,
            bins: [f32::NEG_INFINITY; SWEEP_BINS],
            lag_s: START_LAG_S,
            last: [None; 2],
            scratch: Box::new([0.0; LEVEL_CAPACITY]),
            correlation: [0.0; LIKELIHOOD_POINTS],
            dropped_levels: 0,
        };
        estimator.configure(config);
        estimator
    }

    pub fn configure(&mut self, config: SweepConfig) {
        self.config = config;
        beam_template(&config, &mut self.template);
    }

    pub fn reset(&mut self) {
        self.headings.clear();
        self.track.len = 0;
        self.motion = Motion::Idle;
        self.bins = [f32::NEG_INFINITY; SWEEP_BINS];
        self.last = [None; 2];
    }

    pub fn push_heading(&mut self, sample: HeadingSample) {
        self.headings.push(sample);
    }

    pub fn push_level(
        &mut self,
        sample: LevelSample,
        out: &mut SweepBearing,
    ) -> Option<Result<(), SweepReject>> {
        let late = self.track.len > 0 && sample.t_s <= self.track.t[self.track.last()];
        if !sample.t_s.is_finite() || !sample.level_db.is_finite() || late {
            self.dropped_levels += 1;
            return None;
        }
        let Some((heading, sigma)) = self.headings.unwrapped_at(sample.t_s - self.lag_s) else {
            self.dropped_levels += 1;
            self.motion = Motion::Idle;
            self.track.len = 0;
            return None;
        };
        let overflow = if self.track.len == LEVEL_CAPACITY {
            self.make_room(out)
        } else {
            None
        };
        self.track.push(sample.t_s, sample.level_db, heading, sigma);
        overflow.or_else(|| self.advance(out))
    }

    #[must_use]
    pub fn phase(&self) -> SweepPhase {
        match self.motion {
            Motion::Sweeping(segment) if self.track.len > 0 => {
                let last = self.track.last();
                SweepPhase::Sweeping {
                    span_deg: (segment.sign * (self.track.heading[last] - self.track.heading[0]))
                        .max(0.0) as f32,
                    rate_dps: self.track.rate_dps().abs() as f32,
                }
            }
            _ => SweepPhase::Idle,
        }
    }

    #[must_use]
    pub const fn bins(&self) -> &[f32; SWEEP_BINS] {
        &self.bins
    }

    #[must_use]
    pub fn covered_deg(&self) -> f32 {
        let covered = self.bins.iter().filter(|value| value.is_finite()).count();
        (covered as f64 * BIN_WIDTH_DEG) as f32
    }

    #[must_use]
    pub const fn lag_s(&self) -> f64 {
        self.lag_s
    }

    #[must_use]
    pub const fn stale_headings(&self) -> u64 {
        self.headings.stale
    }

    #[must_use]
    pub const fn dropped_levels(&self) -> u64 {
        self.dropped_levels
    }

    #[must_use]
    pub fn heading_at(&self, t_s: f64) -> Option<HeadingSample> {
        self.headings
            .unwrapped_at(t_s)
            .map(|(heading, sigma)| HeadingSample {
                t_s,
                heading_deg: norm_deg(heading),
                sigma_deg: sigma,
            })
    }

    fn make_room(&mut self, out: &mut SweepBearing) -> Option<Result<(), SweepReject>> {
        match self.motion {
            Motion::Sweeping(segment) => Some(self.close(segment, Close::Span, out)),
            Motion::Idle | Motion::Spinning => {
                self.track.drop_front(1);
                None
            }
        }
    }

    fn advance(&mut self, out: &mut SweepBearing) -> Option<Result<(), SweepReject>> {
        let rate = self.track.rate_dps();
        let now = self.track.t[self.track.last()];
        let config = self.config;
        match self.motion {
            Motion::Idle | Motion::Spinning if rate.abs() > config.max_rate_dps => {
                let was_idle = matches!(self.motion, Motion::Idle);
                self.motion = Motion::Spinning;
                self.keep_window();
                was_idle.then_some(Err(SweepReject::TooFast))
            }
            Motion::Idle | Motion::Spinning => {
                self.keep_window();
                if rate.abs() >= config.min_rate_dps {
                    self.start_segment(rate.signum());
                } else {
                    self.motion = Motion::Idle;
                }
                None
            }
            Motion::Sweeping(_) if rate.abs() > config.max_rate_dps => {
                self.motion = Motion::Spinning;
                self.keep_window();
                Some(Err(SweepReject::TooFast))
            }
            Motion::Sweeping(segment) => self.follow(segment, rate, now, out),
        }
    }

    fn follow(
        &mut self,
        mut segment: Segment,
        rate: f64,
        now: f64,
        out: &mut SweepBearing,
    ) -> Option<Result<(), SweepReject>> {
        let last = self.track.last();
        self.add_to_bins(last);
        let along = segment.sign * rate;
        let close = if along >= self.config.min_rate_dps {
            segment.still_since = None;
            segment.flip_since = None;
            None
        } else if -along >= self.config.min_rate_dps {
            segment.still_since = None;
            let since = *segment.flip_since.get_or_insert(now);
            (now - since >= FLIP_HOLD_S).then_some(Close::Flip)
        } else {
            segment.flip_since = None;
            let since = *segment.still_since.get_or_insert(now);
            (now - since >= self.config.stop_s).then_some(Close::Stop)
        };
        let span = segment.sign * (self.track.heading[last] - self.track.heading[0]);
        let close = close.or((span > MAX_SPAN_DEG).then_some(Close::Span));
        self.motion = Motion::Sweeping(segment);
        close.map(|reason| self.close(segment, reason, out))
    }

    fn keep_window(&mut self) {
        let start = self.track.window_start();
        self.track.drop_front(start);
    }

    fn close(
        &mut self,
        segment: Segment,
        reason: Close,
        out: &mut SweepBearing,
    ) -> Result<(), SweepReject> {
        let end = match reason {
            Close::Stop | Close::Flip => self.track.extreme(segment.sign),
            Close::Span => self.track.last(),
        };
        let result = self.evaluate(end, segment.sign, out);
        match reason {
            Close::Stop => {
                self.motion = Motion::Idle;
                self.keep_window();
            }
            Close::Flip => {
                self.track.drop_front(end);
                self.start_segment(-segment.sign);
            }
            Close::Span => {
                self.track.drop_front(end);
                self.start_segment(segment.sign);
            }
        }
        result
    }

    fn start_segment(&mut self, sign: f64) {
        self.motion = Motion::Sweeping(Segment::turning(sign));
        self.rebuild_bins();
    }

    fn rebuild_bins(&mut self) {
        self.bins = [f32::NEG_INFINITY; SWEEP_BINS];
        for index in 0..self.track.len {
            self.add_to_bins(index);
        }
    }

    fn add_to_bins(&mut self, index: usize) {
        let bin = (norm_deg(self.track.heading[index]) / BIN_WIDTH_DEG) as usize % SWEEP_BINS;
        self.bins[bin] = self.bins[bin].max(self.track.level[index]);
    }
}

#[cfg(test)]
mod tests;
