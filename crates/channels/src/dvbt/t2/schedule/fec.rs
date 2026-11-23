use std::thread::{self, Thread};
use std::time::Duration;

use num_complex::Complex;
use rtrb::{Consumer, Producer, RingBuffer};

use super::{Coding, DecodeError, Decoded, Decoder, TIME_CAPACITY};

const LANE_CAPACITY: usize = TIME_CAPACITY / 3 + 32_400;

pub(super) struct Job {
    coding: Option<Coding>,
    first: usize,
    noise: f32,
    cells: Vec<Complex<f32>>,
    gains: Vec<f32>,
    pub(super) words: Vec<bool>,
    pub(super) results: Vec<Result<Decoded, DecodeError>>,
}

impl Job {
    fn new() -> Self {
        Self {
            coding: None,
            first: 0,
            noise: 0.0,
            cells: Vec::with_capacity(LANE_CAPACITY),
            gains: Vec::with_capacity(LANE_CAPACITY),
            words: Vec::new(),
            results: Vec::new(),
        }
    }

    fn decode(&mut self, cached: &mut Option<(Coding, Decoder)>) {
        self.results.clear();
        self.words.clear();
        let Some(coding) = self.coding else { return };
        let cells = coding.cells();
        let blocks = self.cells.len() / cells;
        let message = coding.message();
        self.words.resize(blocks * message, false);
        if cached.as_ref().is_none_or(|(held, _)| *held != coding) {
            *cached = Decoder::new(coding).ok().map(|decoder| (coding, decoder));
        }
        let Some((_, decoder)) = cached else {
            self.results.resize(blocks, Err(DecodeError::Parameters));
            return;
        };
        for (index, word) in self.words.chunks_exact_mut(message).enumerate() {
            let range = index * cells..(index + 1) * cells;
            self.results.push(decoder.decode_weighted(
                &self.cells[range.clone()],
                &self.gains[range],
                self.first + index,
                self.noise,
                word,
            ));
        }
    }
}

pub(super) struct FecLane {
    incoming: Producer<Job>,
    completed: Consumer<Job>,
    worker: Thread,
    idle: Option<Job>,
}

impl FecLane {
    pub(super) fn new() -> Option<Self> {
        let (incoming, mut jobs) = RingBuffer::<Job>::new(1);
        let (mut results, completed) = RingBuffer::new(1);
        let worker = thread::Builder::new()
            .name("dvbt2-fec".to_owned())
            .spawn(move || {
                let mut decoder = None;
                loop {
                    match jobs.pop() {
                        Ok(mut job) => {
                            job.decode(&mut decoder);
                            if results.push(job).is_err() {
                                return;
                            }
                        }
                        Err(_) if jobs.is_abandoned() => return,
                        Err(_) => thread::park_timeout(Duration::from_millis(20)),
                    }
                }
            })
            .ok()?;
        Some(Self {
            incoming,
            completed,
            worker: worker.thread().clone(),
            idle: Some(Job::new()),
        })
    }

    pub(super) fn submit(
        &mut self,
        coding: Coding,
        first: usize,
        cells: &[Complex<f32>],
        gains: &[f32],
        noise: f32,
    ) -> bool {
        if cells.len() > LANE_CAPACITY || self.incoming.is_abandoned() {
            return false;
        }
        let Some(mut job) = self.idle.take() else {
            return false;
        };
        job.coding = Some(coding);
        job.first = first;
        job.noise = noise;
        job.cells.clear();
        job.cells.extend_from_slice(cells);
        job.gains.clear();
        job.gains.extend_from_slice(gains);
        if let Err(rtrb::PushError::Full(job)) = self.incoming.push(job) {
            self.idle = Some(job);
            return false;
        }
        self.worker.unpark();
        true
    }

    pub(super) fn finish(&mut self) -> Option<&Job> {
        loop {
            match self.completed.pop() {
                Ok(job) => {
                    self.idle = Some(job);
                    return self.idle.as_ref();
                }
                Err(_) if self.completed.is_abandoned() => return None,
                Err(_) => thread::yield_now(),
            }
        }
    }
}
