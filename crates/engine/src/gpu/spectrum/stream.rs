use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle, Thread},
    time::{Duration, Instant},
};

use num_complex::Complex;
use rtrb::{Consumer, Producer, PushError, RingBuffer};

use super::{Batch, Context, batch_for};
use crate::spectrum::{Cadence, SpectrumFrame, Tuning};

/// Batches of samples the ring holds while the GPU works on earlier ones.
const RING_BATCHES: usize = 4;
const MARKERS: usize = 64;
const FRAMES: usize = 3;
const IDLE: Duration = Duration::from_millis(1);
const DROP_LOG_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug)]
enum Marker {
    Segment {
        at: u64,
        origin: u64,
        tuning: Tuning,
        cadence: Cadence,
    },
    Flush {
        at: u64,
    },
    Settle {
        at: u64,
    },
}

impl Marker {
    const fn at(&self) -> u64 {
        match *self {
            Self::Segment { at, .. } | Self::Flush { at } | Self::Settle { at } => at,
        }
    }
}

enum Completion {
    Frame(SpectrumFrame, Vec<f32>),
    Failed(String),
}

struct DropLog {
    what: &'static str,
    count: u64,
    last: Option<Instant>,
}

impl DropLog {
    const fn new(what: &'static str) -> Self {
        Self {
            what,
            count: 0,
            last: None,
        }
    }

    fn record(&mut self, count: u64) {
        self.count += count;
        let now = Instant::now();
        if self
            .last
            .is_none_or(|last| now.duration_since(last) >= DROP_LOG_INTERVAL)
        {
            tracing::warn!(dropped = self.count, "GPU spectrum dropped {}", self.what);
            self.last = Some(now);
        }
    }
}

/// Hands contiguous samples to a GPU worker that forms the overlapping windows itself, so the
/// DSP thread pays one copy per sample however many transforms overlap.
pub(crate) struct Stream {
    samples: Producer<Complex<f32>>,
    markers: Producer<Marker>,
    completions: Consumer<Completion>,
    spare: Producer<Vec<f32>>,
    worker: Thread,
    handle: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    written: u64,
    next_index: Option<u64>,
    segment: Option<(Tuning, Cadence)>,
    open: bool,
    settled: bool,
    dropped: DropLog,
}

impl Stream {
    pub(crate) fn new(context: Arc<Context>, size: usize) -> Result<Self, String> {
        Self::with_batch(context, size, batch_for(size))
    }

    pub(crate) fn with_batch(
        context: Arc<Context>,
        size: usize,
        transforms: usize,
    ) -> Result<Self, String> {
        let batch = Batch::new(context, size, transforms)?;
        let (samples, sample_rx) = RingBuffer::new(batch.capacity() * RING_BATCHES);
        let (markers, marker_rx) = RingBuffer::new(MARKERS);
        let (completion_tx, completions) = RingBuffer::new(FRAMES);
        let (mut spare, spare_rx) = RingBuffer::new(FRAMES);
        for _ in 0..FRAMES {
            spare
                .push(vec![0.0; size])
                .map_err(|_| "GPU spectrum frame pool is full".to_owned())?;
        }
        let worker = Worker::new(batch, sample_rx, marker_rx, completion_tx, spare_rx);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let handle = thread::Builder::new()
            .name("sdrmm-gpu-spectrum".to_owned())
            .spawn(move || worker.run(&worker_stop))
            .map_err(|error| format!("start GPU spectrum worker: {error}"))?;
        Ok(Self {
            samples,
            markers,
            completions,
            spare,
            worker: handle.thread().clone(),
            handle: Some(handle),
            stop,
            written: 0,
            next_index: None,
            segment: None,
            open: false,
            settled: false,
            dropped: DropLog::new("samples"),
        })
    }

    pub(crate) fn push(
        &mut self,
        samples: &[Complex<f32>],
        index: u64,
        tuning: Tuning,
        cadence: Cadence,
        publish: &mut impl FnMut(SpectrumFrame, &[f32]),
    ) -> Result<(), String> {
        self.deliver(publish)?;
        if !self.follow(index, tuning, cadence) {
            self.dropped.record(samples.len() as u64);
            return Ok(());
        }
        let len = samples.len().min(self.samples.slots());
        if let Ok(chunk) = self.samples.write_chunk_uninit(len) {
            chunk.fill_from_iter(samples[..len].iter().copied());
        }
        self.written += len as u64;
        self.next_index = Some(index + len as u64);
        if len < samples.len() {
            self.dropped.record((samples.len() - len) as u64);
            self.restart();
        }
        self.worker.unpark();
        Ok(())
    }

    /// Ends the running average at a gap; the next push starts a fresh segment.
    pub(crate) fn restart(&mut self) {
        if self.open && self.announce(Marker::Flush { at: self.written }) {
            self.worker.unpark();
        }
        self.open = false;
        self.next_index = None;
    }

    /// Publishes a partial frame once after a segment starts, so a stalled source still shows.
    pub(crate) fn idle(
        &mut self,
        publish: &mut impl FnMut(SpectrumFrame, &[f32]),
    ) -> Result<(), String> {
        self.deliver(publish)?;
        if self.open && !self.settled && self.announce(Marker::Settle { at: self.written }) {
            self.settled = true;
            self.worker.unpark();
        }
        Ok(())
    }

    fn follow(&mut self, index: u64, tuning: Tuning, cadence: Cadence) -> bool {
        if self.next_index != Some(index) {
            self.restart();
        } else if self.segment == Some((tuning, cadence)) {
            return true;
        }
        let segment = Marker::Segment {
            at: self.written,
            origin: index,
            tuning,
            cadence,
        };
        if !self.announce(segment) {
            self.next_index = None;
            return false;
        }
        self.segment = Some((tuning, cadence));
        self.open = true;
        self.settled = false;
        true
    }

    fn announce(&mut self, marker: Marker) -> bool {
        self.markers.push(marker).is_ok()
    }

    fn deliver(&mut self, publish: &mut impl FnMut(SpectrumFrame, &[f32])) -> Result<(), String> {
        while let Ok(completion) = self.completions.pop() {
            match completion {
                Completion::Frame(frame, db) => {
                    publish(frame, &db);
                    let _ = self.spare.push(db);
                }
                Completion::Failed(error) => return Err(error),
            }
        }
        if self.handle.as_ref().is_some_and(JoinHandle::is_finished) {
            return Err("GPU spectrum worker stopped unexpectedly".to_owned());
        }
        Ok(())
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.unpark();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

struct Segment {
    origin: u64,
    tuning: Tuning,
    cadence: Cadence,
    /// Segment position of `pending[0]`.
    base: u64,
    skip: u64,
    averaged: u32,
    last_end: u64,
    completed: bool,
}

struct Worker {
    batch: Batch,
    samples: Consumer<Complex<f32>>,
    markers: Consumer<Marker>,
    completions: Producer<Completion>,
    spare: Consumer<Vec<f32>>,
    pending: Vec<Complex<f32>>,
    scratch: Vec<f32>,
    popped: u64,
    segment: Option<Segment>,
    dropped: DropLog,
}

impl Worker {
    fn new(
        batch: Batch,
        samples: Consumer<Complex<f32>>,
        markers: Consumer<Marker>,
        completions: Producer<Completion>,
        spare: Consumer<Vec<f32>>,
    ) -> Self {
        Self {
            pending: Vec::with_capacity(batch.capacity()),
            scratch: vec![0.0; batch.size()],
            batch,
            samples,
            markers,
            completions,
            spare,
            popped: 0,
            segment: None,
            dropped: DropLog::new("frames the DSP thread had no room for"),
        }
    }

    fn run(mut self, stop: &AtomicBool) {
        while !stop.load(Ordering::Acquire) {
            match self.step() {
                Ok(true) => {}
                Ok(false) => thread::park_timeout(IDLE),
                Err(error) => return self.fail(error, stop),
            }
        }
    }

    fn fail(&mut self, error: String, stop: &AtomicBool) {
        let mut failure = Completion::Failed(error);
        while let Err(PushError::Full(returned)) = self.completions.push(failure) {
            if stop.load(Ordering::Acquire) {
                return;
            }
            failure = returned;
            thread::park_timeout(IDLE);
        }
    }

    fn step(&mut self) -> Result<bool, String> {
        let marked = self.apply_markers()?;
        let read = self.read_samples();
        let ran = self.run_ready()?;
        Ok(marked || read || ran)
    }

    fn apply_markers(&mut self) -> Result<bool, String> {
        let mut applied = false;
        while let Ok(&marker) = self.markers.peek() {
            if marker.at() > self.popped {
                break;
            }
            let _ = self.markers.pop();
            applied = true;
            match marker {
                Marker::Segment {
                    origin,
                    tuning,
                    cadence,
                    ..
                } => self.begin(origin, tuning, cadence),
                Marker::Flush { .. } => self.flush(false)?,
                Marker::Settle { .. } => self.flush(true)?,
            }
        }
        Ok(applied)
    }

    fn begin(&mut self, origin: u64, tuning: Tuning, cadence: Cadence) {
        if self.segment.as_ref().is_some_and(|old| old.averaged > 0) {
            self.batch.clear();
        }
        self.pending.clear();
        self.segment = Some(Segment {
            origin,
            tuning,
            cadence,
            base: 0,
            skip: 0,
            averaged: 0,
            last_end: origin,
            completed: false,
        });
    }

    fn flush(&mut self, settling: bool) -> Result<(), String> {
        if settling
            && self
                .segment
                .as_ref()
                .is_none_or(|segment| segment.completed)
        {
            return Ok(());
        }
        while let Some(count) = self.available() {
            self.transform(count)?;
        }
        if self
            .segment
            .as_ref()
            .is_some_and(|segment| segment.averaged > 0)
        {
            self.finish()?;
        }
        Ok(())
    }

    fn target(&self, segment: &Segment) -> usize {
        let remaining = segment
            .cadence
            .averages
            .saturating_sub(segment.averaged)
            .max(1);
        self.batch
            .fitting(segment.cadence.hop)
            .min(remaining as usize)
    }

    fn span(&self, segment: &Segment, count: usize) -> usize {
        (count - 1) * segment.cadence.hop + self.batch.size()
    }

    fn available(&self) -> Option<usize> {
        let segment = self.segment.as_ref()?;
        let whole = self.pending.len().checked_sub(self.batch.size())? / segment.cadence.hop + 1;
        Some(whole.min(self.target(segment)))
    }

    fn read_samples(&mut self) -> bool {
        let limit = self
            .markers
            .peek()
            .map_or(u64::MAX, |marker| marker.at().saturating_sub(self.popped));
        let wanted = self.segment.as_ref().map_or(u64::MAX, |segment| {
            let span = self.span(segment, self.target(segment));
            segment.skip + span.saturating_sub(self.pending.len()) as u64
        });
        let count = (self.samples.slots() as u64).min(limit).min(wanted) as usize;
        if count == 0 {
            return false;
        }
        let Ok(chunk) = self.samples.read_chunk(count) else {
            return false;
        };
        let (head, tail) = chunk.as_slices();
        take(&mut self.segment, &mut self.pending, head);
        take(&mut self.segment, &mut self.pending, tail);
        chunk.commit_all();
        self.popped += count as u64;
        true
    }

    fn run_ready(&mut self) -> Result<bool, String> {
        let Some(segment) = self.segment.as_ref() else {
            return Ok(false);
        };
        let count = self.target(segment);
        if self.pending.len() < self.span(segment, count) {
            return Ok(false);
        }
        self.transform(count)?;
        Ok(true)
    }

    fn transform(&mut self, count: usize) -> Result<(), String> {
        let Some(segment) = self.segment.as_mut() else {
            return Ok(());
        };
        let hop = segment.cadence.hop;
        let span = (count - 1) * hop + self.batch.size();
        self.batch.add(&self.pending, hop, count)?;
        segment.averaged += count as u32;
        segment.last_end = segment.origin + segment.base + span as u64;
        let consumed = count * hop;
        let kept = self.pending.len().saturating_sub(consumed);
        segment.skip = (consumed - (self.pending.len() - kept)) as u64;
        segment.base += consumed as u64 - segment.skip;
        let len = self.pending.len();
        self.pending.copy_within(len - kept.., 0);
        self.pending.truncate(kept);
        if segment.averaged >= segment.cadence.averages.max(1) {
            self.finish()?;
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<(), String> {
        let Some(segment) = self.segment.as_mut() else {
            return Ok(());
        };
        let frame = segment.tuning.frame(segment.last_end);
        let averaged = std::mem::take(&mut segment.averaged);
        segment.completed = true;
        let Ok(mut db) = self.spare.pop() else {
            self.dropped.record(1);
            return self.batch.take_db(averaged, &mut self.scratch);
        };
        self.batch.take_db(averaged, &mut db)?;
        if self.completions.push(Completion::Frame(frame, db)).is_err() {
            self.dropped.record(1);
        }
        Ok(())
    }
}

fn take(segment: &mut Option<Segment>, pending: &mut Vec<Complex<f32>>, part: &[Complex<f32>]) {
    let Some(segment) = segment else {
        return;
    };
    let skipped = segment.skip.min(part.len() as u64);
    segment.skip -= skipped;
    segment.base += skipped;
    pending.extend_from_slice(&part[skipped as usize..]);
}
