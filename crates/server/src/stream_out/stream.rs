use std::sync::{
    Mutex, PoisonError,
    atomic::{AtomicU32, AtomicU64, Ordering},
};

use axum::body::Bytes;
use sdrmm_wire::{StreamOutNode, StreamOutStatus, stream_out_path};
use tokio::sync::broadcast;

/// Chunks a slow HTTP viewer may fall behind before it skips ahead.
const VIEWER_QUEUE: usize = 512;

/// What a node's encoder shares with its viewers and its status.
pub(super) struct Stream {
    node: String,
    viewers: broadcast::Sender<Bytes>,
    pictures: AtomicU64,
    bytes: AtomicU64,
    dropped: AtomicU64,
    size: AtomicU32,
    error: Mutex<Option<String>>,
}

impl Stream {
    pub(super) fn new(node: String) -> Self {
        Self {
            node,
            viewers: broadcast::channel(VIEWER_QUEUE).0,
            pictures: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            size: AtomicU32::new(0),
            error: Mutex::new(None),
        }
    }

    pub(super) fn watch(&self) -> broadcast::Receiver<Bytes> {
        self.viewers.subscribe()
    }

    pub(super) fn viewers(&self) -> &broadcast::Sender<Bytes> {
        &self.viewers
    }

    pub(super) fn pictured(&self, width: u16, height: u16) {
        self.pictures.fetch_add(1, Ordering::Relaxed);
        self.size.store(
            u32::from(width) << 16 | u32::from(height),
            Ordering::Relaxed,
        );
    }

    pub(super) fn sent(&self, bytes: usize) {
        self.bytes.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub(super) fn dropped(&self, count: u64) {
        self.dropped.fetch_add(count, Ordering::Relaxed);
    }

    pub(super) fn fail(&self, message: String) {
        *self.error.lock().unwrap_or_else(PoisonError::into_inner) = Some(message);
    }

    pub(super) fn recovered(&self) {
        self.error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
    }

    pub(super) fn status(&self, settings: &StreamOutNode) -> StreamOutStatus {
        let size = self.size.load(Ordering::Relaxed);
        let serves = !settings.transport.sends();
        StreamOutStatus {
            node: self.node.clone(),
            path: serves.then(|| stream_out_path(&self.node)),
            clients: if serves {
                u32::try_from(self.viewers.receiver_count()).unwrap_or(u32::MAX)
            } else {
                0
            },
            pictures: self.pictures.load(Ordering::Relaxed),
            width: (size >> 16) as u16,
            height: size as u16,
            bytes: self.bytes.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            error: self
                .error
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
        }
    }
}
