/// A Forney convolutional interleaver: symbols visit `branches` delay lines in turn, branch
/// `i` holding `i * depth` symbols. The deinterleaver reverses the delays, so the pair adds
/// `(branches - 1) * depth * branches` symbols of latency.
#[derive(Clone, Debug)]
pub struct ConvolutionalInterleaver<T> {
    lines: Vec<T>,
    starts: Vec<usize>,
    at: Vec<usize>,
    branch: usize,
}

impl<T: Copy + Default> ConvolutionalInterleaver<T> {
    #[must_use]
    pub fn interleaver(branches: usize, depth: usize) -> Self {
        Self::with_delays(branches, |branch| branch * depth)
    }

    #[must_use]
    pub fn deinterleaver(branches: usize, depth: usize) -> Self {
        Self::with_delays(branches, |branch| (branches - 1 - branch) * depth)
    }

    fn with_delays(branches: usize, delay: impl Fn(usize) -> usize) -> Self {
        assert!(branches > 0, "an interleaver has at least one branch");
        let mut starts = Vec::with_capacity(branches + 1);
        let mut total = 0;
        for branch in 0..branches {
            starts.push(total);
            total += delay(branch);
        }
        starts.push(total);
        Self {
            lines: vec![T::default(); total],
            starts,
            at: vec![0; branches],
            branch: 0,
        }
    }

    pub fn reset(&mut self) {
        self.lines.fill(T::default());
        self.at.fill(0);
        self.branch = 0;
    }

    pub fn push(&mut self, symbol: T) -> T {
        let branch = self.branch;
        self.branch = (branch + 1) % self.at.len();
        let (start, end) = (self.starts[branch], self.starts[branch + 1]);
        if start == end {
            return symbol;
        }
        let slot = &mut self.at[branch];
        let cell = &mut self.lines[start + *slot];
        let delayed = std::mem::replace(cell, symbol);
        *slot = (*slot + 1) % (end - start);
        delayed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BRANCHES: usize = 12;
    const DEPTH: usize = 17;

    #[test]
    fn the_pair_restores_the_order_after_its_latency() {
        let mut tx = ConvolutionalInterleaver::interleaver(BRANCHES, DEPTH);
        let mut rx = ConvolutionalInterleaver::deinterleaver(BRANCHES, DEPTH);
        let latency = (BRANCHES - 1) * DEPTH * BRANCHES;
        let input: Vec<u8> = (0..5_000).map(|i| (i * 7 + 3) as u8).collect();
        let output: Vec<u8> = input.iter().map(|&b| rx.push(tx.push(b))).collect();
        assert_eq!(output[latency..], input[..input.len() - latency]);
    }

    #[test]
    fn branch_zero_passes_straight_through() {
        let mut tx = ConvolutionalInterleaver::interleaver(4, 3);
        assert_eq!(tx.push(9u8), 9);
        assert_eq!(tx.push(5u8), 0);
    }

    #[test]
    fn reset_empties_every_line() {
        let mut rx = ConvolutionalInterleaver::deinterleaver(3, 2);
        for value in 1..=20u16 {
            rx.push(value);
        }
        rx.reset();
        assert_eq!(rx.push(77), 0);
    }
}
