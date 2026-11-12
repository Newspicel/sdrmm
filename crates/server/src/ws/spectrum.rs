use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use sdrmm_dsp::{DbWindowSmoother, SpanFloor, adaptive_db_window, decimate_signal, quantize_db};
use sdrmm_engine::{Engine, SpectrumSnapshot};
use sdrmm_wire::{SPECTRUM_SIGNAL_MARGIN_DB, ServerEvent, SpectrumFrame, StreamKind};
use tokio::sync::broadcast;

use super::{FrameThrottle, MAX_BINS, MAX_FPS, MIN_BINS, Outbox, SpectrumLane, newest, text_event};

const LINES_HELD: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Reduction {
    device_set: u32,
    stream: u32,
    bins: usize,
    fps: u16,
}

struct Line {
    seq: u32,
    timestamp: u64,
    center_hz: f64,
    span_hz: f32,
    db_min: f32,
    db_max: f32,
    floor_db: f32,
    bins: Vec<u8>,
    skipped: u64,
}

/// Every reduction of a lane's spectrum now being sent, so clients that ask for the same size
/// and rate share one instead of each reducing the lane again.
#[derive(Default)]
pub(crate) struct SpectrumShare {
    running: Mutex<HashMap<Reduction, broadcast::Sender<Arc<Line>>>>,
}

impl SpectrumShare {
    fn running(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<Reduction, broadcast::Sender<Arc<Line>>>> {
        self.running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn join(
        self: &Arc<Self>,
        reduction: Reduction,
        snapshots: broadcast::Receiver<SpectrumSnapshot>,
        engine: Arc<Engine>,
    ) -> broadcast::Receiver<Arc<Line>> {
        let mut running = self.running();
        if let Some(lines) = running.get(&reduction) {
            return lines.subscribe();
        }
        let (lines, receiver) = broadcast::channel(LINES_HELD);
        running.insert(reduction, lines.clone());
        tokio::spawn(reduce(self.clone(), reduction, snapshots, lines, engine));
        receiver
    }

    /// Ends a reduction nobody listens to. A client that joins takes the same lock, so it
    /// either counts here or finds the reduction gone and starts its own.
    fn abandoned(&self, reduction: &Reduction, lines: &broadcast::Sender<Arc<Line>>) -> bool {
        let mut running = self.running();
        if lines.receiver_count() > 0 {
            return false;
        }
        self.forget(&mut running, reduction, lines);
        true
    }

    fn forget(
        &self,
        running: &mut HashMap<Reduction, broadcast::Sender<Arc<Line>>>,
        reduction: &Reduction,
        lines: &broadcast::Sender<Arc<Line>>,
    ) {
        if running
            .get(reduction)
            .is_some_and(|known| known.same_channel(lines))
        {
            running.remove(reduction);
        }
    }

    #[cfg(test)]
    pub(super) fn reductions(&self) -> usize {
        self.running().len()
    }
}

struct Reducer {
    decimated: Vec<f32>,
    quantized: Vec<u8>,
    window: Vec<f32>,
    smoother: DbWindowSmoother,
    span_floor: SpanFloor,
    throttle: FrameThrottle,
    skipped: u64,
}

impl Reducer {
    fn new(reduction: &Reduction) -> Self {
        Self {
            decimated: vec![0.0; reduction.bins],
            quantized: vec![0; reduction.bins],
            window: Vec::with_capacity(reduction.bins),
            smoother: DbWindowSmoother::default(),
            span_floor: SpanFloor::default(),
            throttle: FrameThrottle::new(reduction.fps),
            skipped: 0,
        }
    }

    fn line(&mut self, snapshot: &SpectrumSnapshot) -> Option<Line> {
        if !self.throttle.admit(snapshot.timestamp, snapshot.span_hz) {
            return None;
        }
        let floor_db = self
            .span_floor
            .read(&snapshot.db)
            .unwrap_or(f32::NEG_INFINITY);
        decimate_signal(
            &snapshot.db,
            floor_db + SPECTRUM_SIGNAL_MARGIN_DB,
            &mut self.decimated,
        );
        let (db_min, db_max) = self
            .smoother
            .follow(adaptive_db_window(&self.decimated, &mut self.window));
        quantize_db(&self.decimated, db_min, db_max, &mut self.quantized);
        Some(Line {
            seq: snapshot.seq,
            timestamp: snapshot.timestamp,
            center_hz: snapshot.center_hz,
            span_hz: snapshot.span_hz,
            db_min,
            db_max,
            floor_db,
            bins: self.quantized.clone(),
            skipped: std::mem::take(&mut self.skipped),
        })
    }
}

async fn reduce(
    share: Arc<SpectrumShare>,
    reduction: Reduction,
    mut snapshots: broadcast::Receiver<SpectrumSnapshot>,
    lines: broadcast::Sender<Arc<Line>>,
    engine: Arc<Engine>,
) {
    let mut reducer = Reducer::new(&reduction);
    loop {
        let received = tokio::select! {
            () = lines.closed() => {
                if share.abandoned(&reduction, &lines) {
                    return;
                }
                continue;
            }
            received = snapshots.recv() => received,
        };
        match received {
            Ok(mut snapshot) => {
                reducer.skipped += newest(&mut snapshots, &mut snapshot);
                if let Some(line) = reducer.line(&snapshot) {
                    let _ = lines.send(Arc::new(line));
                }
            }
            Err(broadcast::error::RecvError::Lagged(count)) => reducer.skipped += count,
            Err(broadcast::error::RecvError::Closed) => {
                let (again, device_set, stream) =
                    (engine.clone(), reduction.device_set, reduction.stream);
                let resubscribed = tokio::task::spawn_blocking(move || {
                    again.subscribe_spectrum(device_set, stream)
                })
                .await;
                let Ok(Ok(fresh)) = resubscribed else {
                    share.forget(&mut share.running(), &reduction, &lines);
                    return;
                };
                snapshots = fresh;
            }
        }
    }
}

pub(super) fn spawn_spectrum(
    lane: SpectrumLane,
    params: sdrmm_wire::StreamParams,
    snapshots: broadcast::Receiver<SpectrumSnapshot>,
    out_tx: Outbox,
    engine: Arc<Engine>,
    share: &Arc<SpectrumShare>,
) -> tokio::task::JoinHandle<()> {
    let reduction = Reduction {
        device_set: lane.device_set,
        stream: lane.stream,
        bins: usize::from(params.bins).clamp(MIN_BINS, MAX_BINS),
        fps: params.fps.clamp(1, MAX_FPS),
    };
    let mut lines = share.join(reduction, snapshots, engine);
    let stream_id = lane.stream_id;
    tokio::spawn(async move {
        loop {
            match lines.recv().await {
                Ok(line) => {
                    out_tx.dropped(line.skipped);
                    let frame = SpectrumFrame {
                        stream_id,
                        seq: line.seq,
                        timestamp: line.timestamp,
                        center_hz: line.center_hz,
                        span_hz: line.span_hz,
                        db_min: line.db_min,
                        db_max: line.db_max,
                        floor_db: line.floor_db,
                        bins: &line.bins,
                    }
                    .encode();
                    if out_tx.send_frame(frame).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(count)) => out_tx.dropped(count),
                Err(broadcast::error::RecvError::Closed) => {
                    let stopped = ServerEvent::StreamStopped {
                        stream_id,
                        kind: StreamKind::Spectrum,
                    };
                    let _ = out_tx.send(text_event(&stopped)).await;
                    break;
                }
            }
        }
    })
}
