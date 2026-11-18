/// A time interleaver: cell `i` of each frame leaves `delays[i % delays.len()]` frames later,
/// cells not yet filled read as the default. Frames have a fixed length set at build time.
#[derive(Clone, Debug)]
pub struct FrameDelay<T> {
    lines: Vec<T>,
    delays: Vec<usize>,
    len: usize,
    depth: usize,
    at: usize,
}

impl<T: Copy + Default> FrameDelay<T> {
    #[must_use]
    pub fn new(len: usize, delays: &[usize]) -> Self {
        assert!(!delays.is_empty(), "at least one delay");
        let depth = delays.iter().copied().max().unwrap_or(0).max(1);
        Self {
            lines: vec![T::default(); depth * len],
            delays: delays.to_vec(),
            len,
            depth,
            at: 0,
        }
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn reset(&mut self) {
        self.lines.fill(T::default());
        self.at = 0;
    }

    /// Takes one frame of `len` cells and writes the delayed frame into `out`.
    pub fn push(&mut self, frame: &[T], out: &mut [T]) {
        assert!(
            frame.len() == self.len && out.len() == self.len,
            "frame length"
        );
        let row = self.at * self.len;
        for (index, (&value, slot)) in frame.iter().zip(out.iter_mut()).enumerate() {
            let delay = self.delays[index % self.delays.len()];
            if delay == 0 {
                *slot = value;
                continue;
            }
            let line = (self.at + self.depth - delay) % self.depth;
            *slot = self.lines[line * self.len + index];
            self.lines[row + index] = value;
        }
        self.at = (self.at + 1) % self.depth;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_cell_leaves_after_its_own_delay() {
        let mut delay = FrameDelay::new(4, &[0, 1, 3, 2]);
        let mut out = [0u32; 4];
        let mut history = Vec::new();
        for frame in 1..=8u32 {
            let input = [frame * 10, frame * 10 + 1, frame * 10 + 2, frame * 10 + 3];
            delay.push(&input, &mut out);
            history.push(out);
        }
        assert_eq!(history[5], [60, 51, 32, 43]);
        assert_eq!(history[0], [10, 0, 0, 0]);
    }

    #[test]
    fn complementary_delays_line_every_cell_up_again() {
        let map = [0usize, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15];
        let back: Vec<usize> = map.iter().map(|&m| 16 - m).collect();
        let mut tx = FrameDelay::new(32, &map);
        let mut rx = FrameDelay::new(32, &back);
        let (mut sent, mut received) = ([0u16; 32], [0u16; 32]);
        for frame in 0..40u16 {
            let input: Vec<u16> = (0..32).map(|i| frame * 100 + i + 1).collect();
            tx.push(&input, &mut sent);
            rx.push(&sent, &mut received);
            if frame >= 16 {
                let expected: Vec<u16> = (0..32).map(|i| (frame - 16) * 100 + i + 1).collect();
                assert_eq!(received[..], expected[..], "frame {frame}");
            }
        }
    }
}
