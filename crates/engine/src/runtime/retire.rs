use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

use rtrb::{Producer, RingBuffer, chunks::WriteChunkUninit};

pub(crate) struct Reclaimer<T> {
    queue: Producer<T>,
    stopped: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

pub(crate) const QUEUE: usize = 64;

impl<T: Send + 'static> Reclaimer<T> {
    pub(crate) fn new(mut release: impl FnMut(T) + Send + 'static) -> std::io::Result<Self> {
        let (queue, mut receiver) = RingBuffer::new(QUEUE);
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let worker = std::thread::Builder::new()
            .name("sdrmm-retire".into())
            .spawn(move || {
                loop {
                    while let Ok(value) = receiver.pop() {
                        release(value);
                    }
                    if stop.load(Ordering::Acquire) {
                        while let Ok(value) = receiver.pop() {
                            release(value);
                        }
                        break;
                    }
                    std::thread::park_timeout(Duration::from_millis(20));
                }
            })?;
        Ok(Self {
            queue,
            stopped,
            worker: Some(worker),
        })
    }

    pub(crate) fn slot(&mut self) -> Option<Slot<'_, T>> {
        let worker = self.worker.as_ref()?;
        if worker.is_finished() {
            return None;
        }
        let chunk = self.queue.write_chunk_uninit(1).ok()?;
        Some(Slot { chunk, worker })
    }

    pub(crate) fn room(&self) -> usize {
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            self.queue.slots()
        } else {
            0
        }
    }
}

pub(crate) struct Slot<'a, T> {
    chunk: WriteChunkUninit<'a, T>,
    worker: &'a JoinHandle<()>,
}

impl<T> Slot<'_, T> {
    pub(crate) fn retire(self, value: T) {
        let Self { chunk, worker } = self;
        chunk.fill_from_iter(std::iter::once(value));
        worker.thread().unpark();
    }
}

impl<T> Drop for Reclaimer<T> {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            if worker.join().is_err() {
                tracing::error!("resource retirement worker panicked");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    struct BlockingDrop(mpsc::Sender<()>, mpsc::Receiver<()>);
    impl Drop for BlockingDrop {
        fn drop(&mut self) {
            self.0.send(()).expect("entered");
            self.1
                .recv_timeout(Duration::from_secs(5))
                .expect("release");
        }
    }

    #[test]
    fn blocking_cleanup_does_not_block_the_producer() {
        let (entered, waiting) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let mut reclaimer = Reclaimer::new(drop).expect("worker");
        let slot = reclaimer.slot().expect("a free slot");
        slot.retire(BlockingDrop(entered, resume));
        waiting
            .recv_timeout(Duration::from_secs(5))
            .expect("cleanup started");
        assert!(reclaimer.slot().is_some());
        release.send(()).expect("release cleanup");
        drop(reclaimer);
    }

    #[test]
    fn a_full_queue_hands_out_no_slot_so_nothing_is_released_by_the_caller() {
        let (entered, waiting) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let mut reclaimer = Reclaimer::new(drop).expect("worker");
        reclaimer
            .slot()
            .expect("a free slot")
            .retire(Some(BlockingDrop(entered, resume)));
        waiting
            .recv_timeout(Duration::from_secs(5))
            .expect("cleanup started");
        let mut queued = 0;
        while let Some(slot) = reclaimer.slot() {
            slot.retire(None);
            queued += 1;
            assert!(queued <= QUEUE, "the queue never filled");
        }
        assert_eq!(reclaimer.room(), 0);
        assert!(reclaimer.slot().is_none());
        release.send(()).expect("release cleanup");
        drop(reclaimer);
    }
}
