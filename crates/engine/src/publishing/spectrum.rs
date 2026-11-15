use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use super::Publisher;
use crate::{
    runtime::{SpectrumSnapshot, SpectrumTap, Watching},
    spectrum::SpectrumFrame,
};

/// Publisher polls between looks at who reads the tap: about every 20 ms.
const POLLS_PER_WATCH: u32 = 20;

struct SpectrumPacket {
    seq: u32,
    frame: SpectrumFrame,
    db: Vec<f32>,
}

pub(crate) struct SpectrumPublisher {
    queue: Publisher<SpectrumPacket>,
    dropped: Arc<AtomicU64>,
    watching: Watching,
}

impl SpectrumPublisher {
    pub(crate) fn new(tap: SpectrumTap, size: usize) -> std::io::Result<Self> {
        Self::with_metrics(tap, size, Arc::new(crate::metrics::QueueMetrics::default()))
    }

    pub(crate) fn with_metrics(
        tap: SpectrumTap,
        size: usize,
        metrics: Arc<crate::metrics::QueueMetrics>,
    ) -> std::io::Result<Self> {
        let dropped = Arc::new(AtomicU64::new(0));
        let count = dropped.clone();
        let mut seen = 0;
        let watching = Watching::default();
        let watched = watching.clone();
        let readers = tap.clone();
        let mut polls = 0u32;
        let queue = Publisher::with_metrics(
            "sdrmm-spectrum-publish",
            8,
            || SpectrumPacket {
                seq: 0,
                frame: SpectrumFrame {
                    timestamp: 0,
                    center_hz: 0.0,
                    span_hz: 0.0,
                },
                db: vec![0.0; size],
            },
            move |packet| {
                tap.send(SpectrumSnapshot {
                    seq: packet.seq,
                    timestamp: packet.frame.timestamp,
                    center_hz: packet.frame.center_hz,
                    span_hz: packet.frame.span_hz,
                    db: Arc::from(packet.db.as_slice()),
                });
            },
            move || {
                if polls == 0 {
                    watched.set(readers.watch());
                }
                polls = (polls + 1) % POLLS_PER_WATCH;
                let now = count.load(Ordering::Relaxed);
                if now != seen {
                    tracing::warn!(
                        dropped = now - seen,
                        total = now,
                        "spectrum publication queue overflow"
                    );
                    seen = now;
                }
            },
            metrics,
        )?;
        Ok(Self {
            queue,
            dropped,
            watching,
        })
    }

    /// Who reads the spectrum, as the publishing thread last saw it.
    pub(crate) fn watching(&self) -> Watching {
        self.watching.clone()
    }

    pub(crate) fn publish(&mut self, seq: u32, frame: SpectrumFrame, db: &[f32]) {
        if !self.queue.submit(|packet| {
            packet.seq = seq;
            packet.frame = frame;
            packet.db.copy_from_slice(db);
        }) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}
