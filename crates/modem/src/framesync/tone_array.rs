use num_complex::Complex;

/// The known tones an MFSK frame carries for sync, such as FT8's Costas arrays: each block
/// is its first symbol and its tones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToneArray {
    blocks: &'static [(usize, &'static [u8])],
}

impl ToneArray {
    #[must_use]
    pub const fn new(blocks: &'static [(usize, &'static [u8])]) -> Self {
        Self { blocks }
    }

    #[must_use]
    pub const fn blocks(&self) -> &'static [(usize, &'static [u8])] {
        self.blocks
    }

    #[must_use]
    pub fn sync_symbols(&self) -> usize {
        self.blocks.iter().map(|(_, tones)| tones.len()).sum()
    }

    /// The sync tone at `symbol`, `None` for a data symbol.
    #[must_use]
    pub fn tone(&self, symbol: usize) -> Option<u8> {
        self.blocks.iter().find_map(|&(start, tones)| {
            symbol
                .checked_sub(start)
                .and_then(|offset| tones.get(offset).copied())
        })
    }

    /// Each run of data symbols between two sync blocks, as its first symbol and length.
    pub fn data_blocks(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.blocks.windows(2).map(|pair| {
            let start = pair[0].0 + pair[0].1.len();
            (start, pair[1].0 - start)
        })
    }

    /// The sync energy from `correlate(symbol, tone)`, one symbol's matched filter output at
    /// one tone. With `coherent`, a block sums its symbols rotated by that step per symbol
    /// before squaring; without it each symbol's power adds.
    pub fn score(
        &self,
        coherent: Option<Complex<f32>>,
        mut correlate: impl FnMut(usize, u8) -> Complex<f32>,
    ) -> f32 {
        let mut total = 0.0;
        for &(first, tones) in self.blocks {
            let mut block = Complex::<f32>::default();
            let mut phase = Complex::new(1.0f32, 0.0);
            for (index, &tone) in tones.iter().enumerate() {
                let sum = correlate(first + index, tone);
                match coherent {
                    Some(step) => {
                        block += sum * phase;
                        phase *= step;
                    }
                    None => total += sum.norm_sqr(),
                }
            }
            total += block.norm_sqr();
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COSTAS: &[u8] = &[3, 1, 4, 0, 6, 5, 2];
    const ARRAY: ToneArray = ToneArray::new(&[(0, COSTAS), (36, COSTAS), (72, COSTAS)]);

    #[test]
    fn tones_and_data_blocks_tile_the_frame() {
        assert_eq!(ARRAY.tone(0), Some(3));
        assert_eq!(ARRAY.tone(38), Some(4));
        assert_eq!(ARRAY.tone(7), None);
        assert_eq!(
            ARRAY.data_blocks().collect::<Vec<_>>(),
            vec![(7, 29), (43, 29)]
        );
        assert_eq!(ARRAY.sync_symbols(), 21);
    }

    #[test]
    fn the_score_peaks_on_the_pattern() {
        let on = |symbol: usize, tone: u8| {
            let hit = ARRAY.tone(symbol) == Some(tone);
            Complex::new(if hit { 1.0 } else { 0.1 }, 0.0)
        };
        let shifted = |symbol: usize, tone: u8| on(symbol + 1, tone);
        assert_eq!(ARRAY.score(None, on), 21.0);
        assert_eq!(ARRAY.score(Some(Complex::new(1.0, 0.0)), on), 3.0 * 49.0);
        assert!(ARRAY.score(None, shifted) < 21.0);
    }
}
