use std::{collections::VecDeque, fmt};

pub const MAX_QUEUE_S: f64 = 5.0;

/// A transmitter was handed more than it may hold: nothing of the offer was taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueFull {
    pub mode: &'static str,
    pub queued: usize,
}

impl fmt::Display for QueueFull {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} transmit queue holds at most {MAX_QUEUE_S} s, {} items queued",
            self.mode, self.queued
        )
    }
}

impl std::error::Error for QueueFull {}

/// What a modulator still has to send, bounded so a stalled radio cannot grow it without end.
pub struct TxQueue<T> {
    mode: &'static str,
    capacity: usize,
    items: VecDeque<T>,
}

impl<T> TxQueue<T> {
    #[must_use]
    pub fn new(mode: &'static str, rate: f64) -> Self {
        Self {
            mode,
            capacity: (MAX_QUEUE_S * rate) as usize,
            items: VecDeque::new(),
        }
    }

    pub fn accept(&self, n: usize) -> Result<(), QueueFull> {
        if self.items.len() + n <= self.capacity {
            return Ok(());
        }
        Err(QueueFull {
            mode: self.mode,
            queued: self.items.len(),
        })
    }

    pub fn push(&mut self, item: T) {
        self.items.push_back(item);
    }

    pub fn extend(&mut self, items: impl IntoIterator<Item = T>) {
        self.items.extend(items);
    }

    pub fn pop(&mut self) -> Option<T> {
        self.items.pop_front()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_queue_refuses_a_backlog_past_the_bound_without_taking_any_of_it() {
        let mut queue = TxQueue::new("test", 10.0);
        let capacity = (MAX_QUEUE_S * 10.0) as usize;
        assert!(queue.accept(capacity).is_ok());
        queue.extend(std::iter::repeat_n(1u8, capacity));
        let full = queue.accept(1).expect_err("over the bound");
        assert_eq!(full.queued, capacity);
        assert!(full.to_string().contains("test transmit queue"));
        assert_eq!(queue.pop(), Some(1));
    }
}
