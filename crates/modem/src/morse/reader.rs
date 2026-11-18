use sdrmm_dsp::{Envelope, KeyingSlicer};

use super::decode;

const ENV_ATTACK_S: f64 = 2e-3;
const ENV_RELEASE_S: f64 = 2e-3;
const MIN_RUN_S: f64 = 5e-3;
const MIN_SNR: f32 = 6.0;
const DASH_MIN_DOTS: f32 = 2.0;
const LETTER_GAP_DOTS: f32 = 2.0;
const WORD_GAP_DOTS: f32 = 4.5;
const TRACK_ALPHA: f32 = 0.2;
const CALIB_MARKS: usize = 9;
const WPM_MIN: f32 = 3.0;
const WPM_MAX: f32 = 80.0;
const IDLE_FLUSH_S: f64 = 3.0;
const MAX_CHUNK_CHARS: usize = 64;
const MAX_ELEMENTS: u8 = 8;
const FIT_ALPHA: f32 = 0.15;
const UNKNOWN: &str = "*";

/// Timing this far off the one-dot and three-dot lengths came from a slicer chewing on noise, not
/// from a hand or a keyer.
pub const FIT_MAX: f32 = 0.35;

/// Marks that have to land before their timing says anything about who sent them.
const FIT_MARKS: u32 = 3;

/// Text read off the air, with the speed it was sent at.
#[derive(Clone, Debug, PartialEq)]
pub struct MorseChunk {
    pub text: String,
    pub wpm: f32,
}

#[derive(Clone, Copy)]
struct Run {
    on: bool,
    len: u32,
    clean: bool,
}

/// Reads Morse from the magnitude of a keyed carrier, one sample at a time.
///
/// An envelope follower and a keying slicer cut the signal into marks and spaces. Without a
/// fixed speed the first marks are held until their lengths give the dot, which then follows
/// the sender. Text comes out in chunks at word gaps, after three idle seconds, or when a
/// chunk grows long; a chunk whose timing does not fit hand-sent Morse is held back until
/// some that does vouches for it.
#[derive(Clone)]
pub struct MorseReader {
    rate: f64,
    fixed_wpm: Option<f32>,
    env: Envelope,
    slicer: KeyingSlicer,
    min_run: u32,
    idle_flush: u32,
    dot_bounds: (f32, f32),

    key: bool,
    run: u32,
    mark_clean: bool,
    idle: u32,
    pending: Option<Run>,

    dot: Option<f32>,
    hold: Vec<Run>,
    replay: Vec<Run>,
    marks_held: usize,

    pattern: u16,
    elements: u8,
    overflow: bool,
    started: bool,
    pending_space: bool,
    fit: f32,
    scored: u32,
    unproven: String,
    text: String,
}

fn dot_samples(wpm: f32, rate: f64) -> f32 {
    (1.2 * rate / f64::from(wpm)) as f32
}

impl MorseReader {
    /// `wpm` fixes the speed; `None` learns it.
    #[must_use]
    pub fn new(rate: f64, wpm: Option<f32>) -> Self {
        Self {
            rate,
            fixed_wpm: wpm,
            env: Envelope::new(rate, ENV_ATTACK_S, ENV_RELEASE_S),
            slicer: KeyingSlicer::new(rate),
            min_run: (MIN_RUN_S * rate).round().max(1.0) as u32,
            idle_flush: (IDLE_FLUSH_S * rate).round().max(1.0) as u32,
            dot_bounds: (dot_samples(WPM_MAX, rate), dot_samples(WPM_MIN, rate)),
            key: false,
            run: 0,
            mark_clean: true,
            idle: 0,
            pending: None,
            dot: wpm.map(|wpm| dot_samples(wpm, rate)),
            hold: Vec::new(),
            replay: Vec::new(),
            marks_held: 0,
            pattern: 1,
            elements: 0,
            overflow: false,
            started: false,
            pending_space: false,
            fit: 1.0,
            scored: 0,
            unproven: String::new(),
            text: String::new(),
        }
    }

    /// Fixes the speed, or lets it follow the sender from where it is.
    pub fn set_wpm(&mut self, wpm: Option<f32>) {
        self.fixed_wpm = wpm;
        if let Some(wpm) = wpm {
            self.dot = Some(dot_samples(wpm, self.rate));
        }
    }

    /// Takes the carrier's magnitude for one sample; finished chunks land in `out`.
    pub fn push(&mut self, magnitude: f32, out: &mut Vec<MorseChunk>) {
        let key = self.slicer.push(self.env.push(magnitude));
        if key {
            self.mark_clean &= self.slicer.snr() >= MIN_SNR;
            self.idle = 0;
        } else {
            self.idle = self.idle.saturating_add(1);
        }
        if key != self.key {
            let run = Run {
                on: self.key,
                len: self.run,
                clean: self.mark_clean,
            };
            self.push_run(run, out);
            self.key = key;
            self.run = 0;
            self.mark_clean = true;
        }
        self.run = self.run.saturating_add(1);
        if self.idle == self.idle_flush {
            self.on_idle(out);
        }
    }

    #[must_use]
    pub fn marks_scored(&self) -> u32 {
        self.scored
    }

    /// How closely the marks seen so far land on the one-dot and three-dot lengths a hand or a
    /// keyer produces. Filtered noise sliced into runs lands anywhere, so this separates a station
    /// from a patch of band that merely crossed the detection threshold.
    #[must_use]
    pub fn element_fit(&self) -> f32 {
        if self.scored < FIT_MARKS || self.speed_ran_into_its_limit() {
            1.0
        } else {
            self.fit
        }
    }

    fn push_run(&mut self, run: Run, out: &mut Vec<MorseChunk>) {
        match self.pending.take() {
            None => self.pending = Some(run),
            Some(prev) if prev.on == run.on || run.len < self.min_run => {
                self.pending = Some(Run {
                    on: prev.on,
                    len: prev.len.saturating_add(run.len),
                    clean: prev.clean && run.clean,
                });
            }
            Some(prev) => {
                self.consume(prev, out);
                self.pending = Some(run);
            }
        }
    }

    fn consume(&mut self, run: Run, out: &mut Vec<MorseChunk>) {
        if self.dot.is_none() {
            self.hold.push(run);
            if run.on {
                self.marks_held += 1;
                if self.marks_held >= CALIB_MARKS {
                    self.calibrate(out);
                }
            }
            return;
        }
        self.classify(run, out);
    }

    fn calibrate(&mut self, out: &mut Vec<MorseChunk>) {
        let marks = self.hold.iter().filter(|r| r.on).count();
        let seed = self
            .hold
            .iter()
            .filter(|r| r.on)
            .skip(usize::from(marks > 1))
            .map(|r| r.len)
            .min();
        if let Some(seed) = seed {
            let (lo, hi) = self.dot_bounds;
            self.dot = Some((seed as f32).clamp(lo, hi));
            std::mem::swap(&mut self.hold, &mut self.replay);
            let mut i = 0;
            while let Some(&run) = self.replay.get(i) {
                self.classify(run, out);
                i += 1;
            }
            self.replay.clear();
            std::mem::swap(&mut self.hold, &mut self.replay);
        }
        self.hold.clear();
        self.marks_held = 0;
    }

    fn classify(&mut self, run: Run, out: &mut Vec<MorseChunk>) {
        let Some(dot) = self.dot else { return };
        let len = run.len as f32;
        if run.on {
            if !run.clean {
                self.reset_character();
                return;
            }
            self.started = true;
            self.score_fit(len);
            let dash = len >= DASH_MIN_DOTS * dot;
            if self.elements < MAX_ELEMENTS {
                self.pattern = self.pattern * 2 + u16::from(dash);
                self.elements += 1;
            } else {
                self.overflow = true;
            }
            self.track(if dash { len / 3.0 } else { len });
        } else if len >= WORD_GAP_DOTS * dot {
            self.finish_character(out);
            self.pending_space |= self.started;
            self.flush(out);
        } else if len >= LETTER_GAP_DOTS * dot {
            self.finish_character(out);
            self.track(len / 3.0);
        } else {
            self.track(len);
        }
    }

    fn score_fit(&mut self, len: f32) {
        let Some(dot) = self.dot else { return };
        let units = len / dot;
        let nearest = if units < DASH_MIN_DOTS { 1.0 } else { 3.0 };
        let error = (units - nearest).abs() / nearest;
        self.scored = self.scored.saturating_add(1);
        let alpha = FIT_ALPHA.max(1.0 / self.scored as f32);
        self.fit += alpha * (error.min(1.0) - self.fit);
    }

    fn track(&mut self, observed_dot: f32) {
        if self.fixed_wpm.is_some() || !observed_dot.is_finite() {
            return;
        }
        if let Some(dot) = self.dot {
            let (lo, hi) = self.dot_bounds;
            self.dot = Some((dot + TRACK_ALPHA * (observed_dot - dot)).clamp(lo, hi));
        }
    }

    fn reset_character(&mut self) {
        self.pattern = 1;
        self.elements = 0;
        self.overflow = false;
    }

    fn finish_character(&mut self, out: &mut Vec<MorseChunk>) {
        if self.elements == 0 && !self.overflow {
            return;
        }
        let decoded = if self.overflow {
            UNKNOWN
        } else {
            decode(self.pattern).unwrap_or(UNKNOWN)
        };
        if self.pending_space {
            self.text.push(' ');
            self.pending_space = false;
        }
        self.text.push_str(decoded);
        self.reset_character();
        if self.text.len() >= MAX_CHUNK_CHARS {
            self.flush(out);
        }
    }

    fn flush(&mut self, out: &mut Vec<MorseChunk>) {
        if self.text.is_empty() {
            return;
        }
        if self.element_fit() > FIT_MAX {
            if self.unproven.len() >= MAX_CHUNK_CHARS {
                self.unproven.clear();
            }
            self.unproven.push_str(&std::mem::take(&mut self.text));
            return;
        }
        let mut text = std::mem::take(&mut self.unproven);
        text.push_str(&std::mem::take(&mut self.text));
        out.push(MorseChunk {
            text,
            wpm: self.wpm(),
        });
    }

    fn speed_ran_into_its_limit(&self) -> bool {
        let (fastest, slowest) = self.dot_bounds;
        self.fixed_wpm.is_none() && self.dot.is_some_and(|dot| dot <= fastest || dot >= slowest)
    }

    fn wpm(&self) -> f32 {
        self.dot
            .filter(|d| *d > 0.0)
            .map_or(0.0, |dot| (1.2 * self.rate) as f32 / dot)
    }

    fn on_idle(&mut self, out: &mut Vec<MorseChunk>) {
        if let Some(run) = self.pending.take() {
            self.consume(run, out);
        }
        if self.dot.is_none() {
            self.calibrate(out);
        }
        self.finish_character(out);
        self.flush(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::morse::pattern;

    const RATE: f64 = 8_000.0;

    fn keyed(text: &str, wpm: f32) -> Vec<f32> {
        let dot = (1.2 * RATE / f64::from(wpm)) as usize;
        let mut out = vec![0.01; RATE as usize / 2];
        let mut level = |units: usize, on: bool| {
            out.extend(std::iter::repeat_n(
                if on { 1.0 } else { 0.01 },
                units * dot,
            ));
        };
        for word in text.split(' ') {
            for ch in word.chars() {
                for element in pattern(ch).unwrap_or_default().chars() {
                    level(if element == '-' { 3 } else { 1 }, true);
                    level(1, false);
                }
                level(2, false);
            }
            level(4, false);
        }
        out.extend(std::iter::repeat_n(
            0.01,
            (IDLE_FLUSH_S * RATE) as usize + 10,
        ));
        out
    }

    fn read(reader: &mut MorseReader, samples: &[f32]) -> Vec<MorseChunk> {
        let mut chunks = Vec::new();
        for &sample in samples {
            reader.push(sample, &mut chunks);
        }
        chunks
    }

    #[test]
    fn a_learned_speed_reads_the_whole_call() {
        let mut reader = MorseReader::new(RATE, None);
        let chunks = read(&mut reader, &keyed("CQ CQ DE DL1ABC K", 20.0));
        let text: String = chunks.iter().map(|chunk| chunk.text.as_str()).collect();
        assert_eq!(text.trim(), "CQ CQ DE DL1ABC K");
        let wpm = chunks.last().map_or(0.0, |chunk| chunk.wpm);
        assert!((wpm - 20.0).abs() < 2.0, "{wpm}");
        assert!(reader.element_fit() <= FIT_MAX);
        assert!(reader.marks_scored() > 20);
    }

    #[test]
    fn a_fixed_speed_reads_from_the_first_mark() {
        let mut reader = MorseReader::new(RATE, Some(25.0));
        let chunks = read(&mut reader, &keyed("TEST", 25.0));
        let text: String = chunks.iter().map(|chunk| chunk.text.as_str()).collect();
        assert_eq!(text.trim(), "TEST");
        reader.set_wpm(None);
        assert!(read(&mut reader, &[0.01; 100]).is_empty());
    }
}
