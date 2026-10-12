use std::f64::consts::TAU;

use num_complex::Complex;

use super::{
    burst::{FRAME_SAMPLES, HYPERFRAME, INPUT_RATE_HZ, SLOT_SAMPLES, SPS, Training, sync_data},
    coding::Decoder,
    equalizer::{Demodulated, Equalizer},
    fcch::{Tone, ToneSearch, tone_offset},
    frames::{ControlSlot, SyncInfo, control_slot, parse_sync},
};

const ACQUIRE_SEARCH: usize = 24 * SPS;
const SYNC_SEARCH: usize = 4 * SPS;
const NORMAL_SEARCH: usize = 3 * SPS;
const MAX_SYNC_MISSES: u8 = 6;
const SYNC_TIMING_GAIN: f64 = 0.5;
const BURST_TIMING_GAIN: f64 = 0.05;
const TIMING_MIN_SNR_DB: f32 = 6.0;
const FREQUENCY_GAIN: f64 = 0.5;
const FREQUENCY_MIN_COHERENCE: f64 = 0.6;
const MAX_FREQUENCY_STEP_HZ: f64 = 2_000.0;
const SLOTS: u8 = 8;

pub(crate) trait Sink {
    fn training(&self, frame_number: u32, timeslot: u8) -> Option<u8>;
    fn locked(&mut self, info: SyncInfo);
    fn lost(&mut self);
    fn sync_missed(&mut self);
    fn burst(&mut self, frame_number: u32, timeslot: u8, burst: &Demodulated, offset_hz: f32);
}

#[derive(Clone, Copy, Debug)]
struct Lock {
    frame_number: u32,
    frame_start: f64,
    offset_hz: f64,
    bsic: u8,
    misses: u8,
}

#[derive(Clone, Copy, Debug)]
enum State {
    Hunting,
    Pending(Tone),
    Locked(Lock),
}

pub(crate) struct Receiver {
    samples: Vec<Complex<f32>>,
    scan: usize,
    search: ToneSearch,
    state: State,
    equalizer: Equalizer,
    decoder: Decoder,
}

impl Receiver {
    pub(crate) fn new() -> Self {
        Self {
            samples: Vec::with_capacity(8 * FRAME_SAMPLES as usize),
            scan: 0,
            search: ToneSearch::new(),
            state: State::Hunting,
            equalizer: Equalizer::new(),
            decoder: Decoder::new(),
        }
    }

    pub(crate) fn reset(&mut self) {
        self.samples.clear();
        self.scan = 0;
        self.search.reset();
        self.state = State::Hunting;
    }

    pub(crate) fn process(&mut self, iq: &[Complex<f32>], sink: &mut impl Sink) {
        self.samples.extend_from_slice(iq);
        while self.advance(sink) {}
        self.compact();
    }

    fn advance(&mut self, sink: &mut impl Sink) -> bool {
        match self.state {
            State::Hunting => self.hunt(),
            State::Pending(tone) => self.acquire(tone, sink),
            State::Locked(lock) => self.track(lock, sink),
        }
    }

    fn hunt(&mut self) -> bool {
        while self.scan < self.samples.len() {
            let at = self.scan;
            self.scan += 1;
            if let Some(tone) = self.search.step(&self.samples, at) {
                self.state = State::Pending(tone);
                return true;
            }
        }
        false
    }

    fn acquire(&mut self, tone: Tone, sink: &mut impl Sink) -> bool {
        let start = tone.start + FRAME_SAMPLES as usize;
        if !self.covers(start, ACQUIRE_SEARCH) {
            return false;
        }
        self.state = State::Hunting;
        if let Some((info, burst)) = self.sync(start, ACQUIRE_SEARCH, tone.offset_hz) {
            self.state = State::Locked(Lock {
                frame_number: (info.frame_number + 1) % HYPERFRAME,
                frame_start: burst.start as f64 + FRAME_SAMPLES,
                offset_hz: tone.offset_hz,
                bsic: info.bsic,
                misses: 0,
            });
            sink.locked(info);
        }
        true
    }

    fn track(&mut self, mut lock: Lock, sink: &mut impl Sink) -> bool {
        let last = (lock.frame_start + SLOT_SAMPLES * f64::from(SLOTS - 1)).round() as usize;
        if !self.covers(last, NORMAL_SEARCH.max(SYNC_SEARCH)) {
            return false;
        }
        if !self.frame(&mut lock, sink) {
            self.state = State::Hunting;
            self.search.reset();
            self.scan = (lock.frame_start.max(0.0) as usize).min(self.samples.len());
            sink.lost();
            return true;
        }
        lock.frame_start += FRAME_SAMPLES;
        lock.frame_number = (lock.frame_number + 1) % HYPERFRAME;
        self.state = State::Locked(lock);
        true
    }

    fn frame(&mut self, lock: &mut Lock, sink: &mut impl Sink) -> bool {
        let start = lock.frame_start.round() as usize;
        match control_slot(lock.frame_number, false) {
            ControlSlot::Frequency => self.refine_frequency(lock, start),
            ControlSlot::Sync => {
                if !self.check_sync(lock, start, sink) {
                    return false;
                }
            }
            ControlSlot::Block(_) => self.normal(lock, 0, NORMAL_SEARCH, lock.bsic & 7, sink),
            ControlSlot::Idle => {}
        }
        for timeslot in 1..SLOTS {
            if let Some(tsc) = sink.training(lock.frame_number, timeslot) {
                self.normal(lock, timeslot, NORMAL_SEARCH, tsc, sink);
            }
        }
        true
    }

    fn refine_frequency(&self, lock: &mut Lock, start: usize) {
        if let Some((offset, coherence)) = tone_offset(&self.samples, start)
            && coherence >= FREQUENCY_MIN_COHERENCE
        {
            let step =
                (offset - lock.offset_hz).clamp(-MAX_FREQUENCY_STEP_HZ, MAX_FREQUENCY_STEP_HZ);
            lock.offset_hz += FREQUENCY_GAIN * step;
        }
    }

    fn check_sync(&mut self, lock: &mut Lock, start: usize, sink: &mut impl Sink) -> bool {
        match self.sync(start, SYNC_SEARCH, lock.offset_hz) {
            Some((info, burst)) if info.bsic == lock.bsic => {
                lock.misses = 0;
                if info.frame_number != lock.frame_number {
                    lock.frame_number = info.frame_number;
                    sink.locked(info);
                }
                nudge(lock, start, &burst, SYNC_TIMING_GAIN);
                true
            }
            _ => {
                lock.misses += 1;
                sink.sync_missed();
                lock.misses < MAX_SYNC_MISSES
            }
        }
    }

    fn normal(
        &mut self,
        lock: &mut Lock,
        timeslot: u8,
        search: usize,
        tsc: u8,
        sink: &mut impl Sink,
    ) {
        let nominal = lock.frame_start + SLOT_SAMPLES * f64::from(timeslot);
        let start = nominal.round() as usize;
        let frequency = radians_per_sample(lock.offset_hz);
        let Some(burst) = self.equalizer.demodulate(
            &self.samples,
            start,
            search,
            frequency,
            &Training::normal(tsc),
        ) else {
            return;
        };
        if timeslot == 0 {
            nudge(lock, start, &burst, BURST_TIMING_GAIN);
        }
        sink.burst(lock.frame_number, timeslot, &burst, lock.offset_hz as f32);
    }

    fn sync(
        &mut self,
        start: usize,
        search: usize,
        offset_hz: f64,
    ) -> Option<(SyncInfo, Demodulated)> {
        let burst = self.equalizer.demodulate(
            &self.samples,
            start,
            search,
            radians_per_sample(offset_hz),
            &Training::sync(),
        )?;
        let mut coded = [0.0; 2 * super::burst::SYNC_DATA];
        sync_data(&burst.soft, &mut coded);
        let bits = self.decoder.sync(&coded)?;
        Some((parse_sync(&bits)?, burst))
    }

    fn covers(&self, start: usize, search: usize) -> bool {
        let (_, after) = Equalizer::reach(search);
        start + after <= self.samples.len()
    }

    fn compact(&mut self) {
        let keep_from = match self.state {
            State::Hunting => self.scan.saturating_sub(ToneSearch::history()),
            State::Pending(tone) => tone.start.saturating_sub(ToneSearch::history()),
            State::Locked(lock) => {
                let (before, _) = Equalizer::reach(ACQUIRE_SEARCH);
                (lock.frame_start.max(0.0) as usize).saturating_sub(before)
            }
        }
        .min(self.samples.len());
        if keep_from == 0 {
            return;
        }
        self.samples.drain(..keep_from);
        self.scan = self.scan.saturating_sub(keep_from);
        self.search.rebase(keep_from);
        match &mut self.state {
            State::Hunting => {}
            State::Pending(tone) => tone.start -= keep_from,
            State::Locked(lock) => lock.frame_start -= keep_from as f64,
        }
    }
}

fn nudge(lock: &mut Lock, start: usize, burst: &Demodulated, gain: f64) {
    if burst.snr_db < TIMING_MIN_SNR_DB {
        return;
    }
    let error = burst.start as f64 - start as f64;
    lock.frame_start += gain * error.clamp(-(SPS as f64), SPS as f64);
}

fn radians_per_sample(offset_hz: f64) -> f32 {
    (TAU * offset_hz / INPUT_RATE_HZ) as f32
}
