use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{cpu, iio::Buffer, loss::Chunk, session::SessionError};

const CHUNKS: usize = 4;
const COPY_CPU: usize = 0;
const POLL: Duration = Duration::from_millis(200);
const CHECK_EVERY: Duration = Duration::from_millis(50);

pub struct Lease {
    pub chunk: Chunk,
    pub bytes: Vec<u8>,
}

pub struct Ring {
    full: Receiver<Lease>,
    free: SyncSender<Vec<u8>>,
    filler: Option<JoinHandle<Result<(), SessionError>>>,
}

impl Ring {
    pub fn start(buffer: Buffer, stop: &Arc<AtomicBool>) -> Result<Self, SessionError> {
        let chunk_bytes = buffer.chunk_frames() * buffer.frame_bytes();
        let (free, free_rx) = sync_channel(CHUNKS);
        let (full_tx, full) = sync_channel(CHUNKS);
        for _ in 0..CHUNKS {
            free.send(vec![0u8; chunk_bytes])
                .map_err(|_| SessionError::Panicked)?;
        }
        let stop = stop.clone();
        let filler = thread::spawn(move || {
            cpu::pin(COPY_CPU);
            fill(buffer, &free_rx, &full_tx, &stop)
        });
        Ok(Self {
            full,
            free,
            filler: Some(filler),
        })
    }

    pub fn acquire(&mut self, timeout: Duration) -> Result<Option<Lease>, SessionError> {
        match self.full.recv_timeout(timeout) {
            Ok(lease) => Ok(Some(lease)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => {
                self.join_filler()?;
                Ok(None)
            }
        }
    }

    pub fn release(&mut self, lease: Lease) -> Result<(), SessionError> {
        if self.free.send(lease.bytes).is_err() {
            self.join_filler()?;
        }
        Ok(())
    }

    pub fn finish(self) -> Result<(), SessionError> {
        let Self {
            full, mut filler, ..
        } = self;
        drop(full);
        match filler.take() {
            Some(filler) => filler.join().map_err(|_| SessionError::Panicked)?,
            None => Ok(()),
        }
    }

    fn join_filler(&mut self) -> Result<(), SessionError> {
        match self.filler.take() {
            Some(filler) => filler.join().map_err(|_| SessionError::Panicked)?,
            None => Ok(()),
        }
    }
}

fn fill(
    mut buffer: Buffer,
    free: &Receiver<Vec<u8>>,
    full: &SyncSender<Lease>,
    stop: &AtomicBool,
) -> Result<(), SessionError> {
    let frame_bytes = buffer.frame_bytes().max(1);
    let mut status = Status::default();
    while !stop.load(Ordering::Acquire) {
        let mut bytes = match free.recv_timeout(POLL) {
            Ok(bytes) => bytes,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        let begun = Instant::now();
        if !buffer.fill(&mut bytes, stop).map_err(SessionError::Radio)? {
            break;
        }
        let done = Instant::now();
        let chunk = Chunk {
            frames: bytes.len() / frame_bytes,
            done,
            waited: done - begun,
            rate: status.rate(&buffer, done),
        };
        if full.send(Lease { chunk, bytes }).is_err() {
            break;
        }
    }
    Ok(())
}

#[derive(Default)]
struct Status {
    checked: Option<Instant>,
    rate: f64,
}

impl Status {
    fn rate(&mut self, buffer: &Buffer, now: Instant) -> f64 {
        if self
            .checked
            .is_none_or(|at| now.duration_since(at) >= CHECK_EVERY)
        {
            self.checked = Some(now);
            self.rate = buffer.rate();
        }
        self.rate
    }
}
