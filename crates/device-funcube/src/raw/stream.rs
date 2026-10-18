use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError},
    },
    thread::JoinHandle,
    time::Duration,
};

use sdrmm_device::{BlockGap, CaptureStream, Latency, Next, StopHandle, StreamFailure, lock};

use super::{Assembler, Chunk, QUEUE};
use crate::error::{Error, Result};

#[derive(Debug, Default)]
pub(crate) struct PumpState {
    stop: AtomicBool,
    gone: AtomicBool,
    failure: Mutex<Option<String>>,
}

impl PumpState {
    pub(crate) fn stopping(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }

    fn fail(&self, error: &Error) {
        self.gone.store(error.is_disconnected(), Ordering::Release);
        *lock(&self.failure) = Some(error.to_string());
    }
}

pub(crate) trait Pump: Send + 'static {
    fn run(self, assembler: &mut Assembler, state: &PumpState) -> Result<()>;
}

#[derive(Clone, Debug)]
pub(crate) struct RawStop(Arc<PumpState>);

impl StopHandle for RawStop {
    fn stop(&self) {
        self.0.stop.store(true, Ordering::Release);
    }
}

pub(crate) struct RawStream {
    chunks: Receiver<Chunk>,
    state: Arc<PumpState>,
    pump: Option<JoinHandle<()>>,
}

impl RawStream {
    pub(crate) fn spawn(pump: impl Pump) -> Result<Self> {
        let (out, chunks) = mpsc::sync_channel(QUEUE);
        let state = Arc::new(PumpState::default());
        let shared = state.clone();
        let pump = std::thread::Builder::new()
            .name("sdrmm-funcube-usb".to_string())
            .spawn(move || {
                sdrmm_device::schedule::claim(Latency::Critical);
                let mut assembler = Assembler::new(out);
                if let Err(error) = pump.run(&mut assembler, &shared) {
                    shared.fail(&error);
                }
                assembler.flush();
            })
            .map_err(|_| Error::protocol("start stream", "no thread for the USB pump"))?;
        Ok(Self {
            chunks,
            state,
            pump: Some(pump),
        })
    }
}

impl CaptureStream for RawStream {
    type Block = Chunk;
    type Stop = RawStop;

    fn stop_handle(&self) -> RawStop {
        RawStop(self.state.clone())
    }

    fn next_block(&self, timeout: Duration) -> Next<Chunk> {
        match self.chunks.recv_timeout(timeout) {
            Ok(chunk) => Next::Block(chunk),
            Err(RecvTimeoutError::Timeout) => Next::Idle,
            Err(RecvTimeoutError::Disconnected) => Next::Ended,
        }
    }

    fn dropped(&self) -> u64 {
        0
    }

    fn block_gap(&self, chunk: &Chunk, _bytes_per_sample: u64) -> Option<BlockGap> {
        Some(chunk.gap)
    }

    fn failure(&self) -> StreamFailure {
        StreamFailure {
            reason: lock(&self.state.failure)
                .clone()
                .unwrap_or_else(|| "funcube stream ended".to_string()),
            gone: self.state.gone.load(Ordering::Acquire),
        }
    }
}

impl Drop for RawStream {
    fn drop(&mut self) {
        self.state.stop.store(true, Ordering::Release);
        if let Some(pump) = self.pump.take()
            && pump.join().is_err()
        {
            tracing::error!("funcube USB pump panicked");
        }
    }
}
