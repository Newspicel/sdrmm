use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use crate::error::{Result, StreamError};

const POLL: Duration = Duration::from_millis(1);

#[derive(Debug, Default)]
pub(crate) struct PauseState {
    wanted: AtomicBool,
    held: AtomicBool,
    ended: AtomicBool,
    missing_bytes: AtomicU64,
}

impl PauseState {
    pub(crate) fn wanted(&self) -> bool {
        self.wanted.load(Ordering::Acquire)
    }

    pub(crate) fn hold(&self) {
        self.held.store(true, Ordering::Release);
    }

    pub(crate) fn release(&self) -> u64 {
        self.held.store(false, Ordering::Release);
        self.missing_bytes.swap(0, Ordering::AcqRel)
    }

    pub(crate) fn end(&self) {
        self.ended.store(true, Ordering::Release);
    }

    pub(crate) const fn poll() -> Duration {
        POLL
    }
}

#[derive(Clone, Debug)]
pub struct Pauser {
    state: Arc<PauseState>,
}

impl Pauser {
    pub(crate) const fn new(state: Arc<PauseState>) -> Self {
        Self { state }
    }

    pub fn hold(&self, bytes_per_second: f64, timeout: Duration) -> Result<Paused> {
        self.state.wanted.store(true, Ordering::Release);
        let asked = Instant::now();
        while !self.state.held.load(Ordering::Acquire) {
            if self.state.ended.load(Ordering::Acquire) {
                self.state.wanted.store(false, Ordering::Release);
                return Err(StreamError::Pause("the stream ended"));
            }
            if asked.elapsed() >= timeout {
                self.state.wanted.store(false, Ordering::Release);
                return Err(StreamError::Pause("transfers still in flight"));
            }
            std::thread::sleep(POLL);
        }
        Ok(Paused {
            state: self.state.clone(),
            since: asked,
            bytes_per_second,
        })
    }
}

#[derive(Debug)]
pub struct Paused {
    state: Arc<PauseState>,
    since: Instant,
    bytes_per_second: f64,
}

impl Drop for Paused {
    fn drop(&mut self) {
        let lost = (self.since.elapsed().as_secs_f64() * self.bytes_per_second).ceil();
        self.state
            .missing_bytes
            .fetch_add(lost.max(0.0) as u64, Ordering::AcqRel);
        self.state.wanted.store(false, Ordering::Release);
    }
}
