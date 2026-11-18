/// The codeword a diagonal interleaver of `rows` codewords places at bit `row` of column
/// `column`, and, being its own inverse within a column, where that bit goes back to. Each
/// column turns the rows one further, so a burst in one column touches every codeword once.
#[must_use]
pub const fn diagonal(row: usize, column: usize, rows: usize) -> usize {
    (column + 2 * rows - row - 1) % rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_column_holds_each_codeword_once() {
        let rows = 7;
        for column in 0..8 {
            let mut seen = [false; 7];
            for row in 0..rows {
                seen[diagonal(row, column, rows)] = true;
            }
            assert!(seen.iter().all(|&hit| hit), "{column}");
        }
        assert_eq!(diagonal(0, 0, 5), 4);
        assert_eq!(diagonal(0, 1, 5), 0);
    }
}
