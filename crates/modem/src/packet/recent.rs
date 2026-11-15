/// Frames heard within the last `window` samples, to pass each transmission on once.
pub struct RecentFrames {
    window: u64,
    clock: u64,
    seen: Vec<(u64, Vec<u8>)>,
}

impl RecentFrames {
    #[must_use]
    pub fn new(window: u64) -> Self {
        Self {
            window,
            clock: 0,
            seen: Vec::new(),
        }
    }

    pub fn advance(&mut self, samples: usize) {
        self.clock += samples as u64;
        let (clock, window) = (self.clock, self.window);
        self.seen
            .retain(|(at, _)| clock.saturating_sub(*at) < window);
    }

    /// True the first time `frame` turns up inside the window, which it then remembers.
    pub fn first(&mut self, frame: &[u8]) -> bool {
        if self.seen.iter().any(|(_, seen)| seen == frame) {
            return false;
        }
        self.seen.push((self.clock, frame.to_vec()));
        true
    }

    pub fn clear(&mut self) {
        self.seen.clear();
    }
}
