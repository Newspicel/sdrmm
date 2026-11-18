/// A block interleaver as an index table built once: `gather` reads `out[i] = input[p[i]]`,
/// `scatter` writes `out[p[i]] = input[i]`, so one table serves both directions without
/// allocating per block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Permutation {
    table: Vec<usize>,
}

impl Permutation {
    /// Every index in `0..len` must come back exactly once.
    #[must_use]
    pub fn from_fn(len: usize, index: impl FnMut(usize) -> usize) -> Self {
        let table: Vec<usize> = (0..len).map(index).collect();
        let mut seen = vec![false; len];
        for &at in &table {
            assert!(
                at < len && !seen[at],
                "index {at} repeats or leaves 0..{len}"
            );
            seen[at] = true;
        }
        Self { table }
    }

    /// Splits the sequence into columns of `lengths` symbols and reads them row by row,
    /// `group` symbols at a time; a longer column carries its rest into later rows.
    #[must_use]
    pub fn columns(lengths: &[usize], group: usize) -> Self {
        assert!(group > 0, "a group holds at least one symbol");
        let len = lengths.iter().sum();
        let starts: Vec<usize> = lengths
            .iter()
            .scan(0, |start, &column| {
                let at = *start;
                *start += column;
                Some(at)
            })
            .collect();
        let mut table = Vec::with_capacity(len);
        let mut row = 0;
        while table.len() < len {
            for (&start, &column) in starts.iter().zip(lengths) {
                let first = row * group;
                table.extend((first..column.min(first + group)).map(|offset| start + offset));
            }
            row += 1;
        }
        let mut indices = table.into_iter();
        Self::from_fn(len, |_| indices.next().unwrap_or(usize::MAX))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.table.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    #[must_use]
    pub fn index(&self, at: usize) -> usize {
        self.table[at]
    }

    #[must_use]
    pub fn inverse(&self) -> Self {
        let mut table = vec![0; self.table.len()];
        for (from, &to) in self.table.iter().enumerate() {
            table[to] = from;
        }
        Self { table }
    }

    /// `out[i] = input[p[i]]` over the table's length.
    pub fn gather<T: Copy>(&self, input: &[T], out: &mut [T]) {
        assert!(
            input.len() >= self.len() && out.len() >= self.len(),
            "short block"
        );
        for (slot, &from) in out.iter_mut().zip(&self.table) {
            *slot = input[from];
        }
    }

    /// `out[p[i]] = input[i]` over the table's length.
    pub fn scatter<T: Copy>(&self, input: &[T], out: &mut [T]) {
        assert!(
            input.len() >= self.len() && out.len() >= self.len(),
            "short block"
        );
        for (&value, &to) in input.iter().zip(&self.table) {
            out[to] = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gather_undoes_scatter() {
        let p = Permutation::from_fn(10, |i| i * 3 % 10);
        let input: Vec<u16> = (100..110).collect();
        let mut spread = [0; 10];
        p.scatter(&input, &mut spread);
        let mut back = [0; 10];
        p.gather(&spread, &mut back);
        assert_eq!(back[..], input[..]);
    }

    #[test]
    fn the_inverse_gathers_what_the_table_scatters() {
        let p = Permutation::from_fn(12, |i| (i * 5 + 7) % 12);
        let input: Vec<u8> = (0..12).collect();
        let mut scattered = [0; 12];
        p.scatter(&input, &mut scattered);
        let mut gathered = [0; 12];
        p.inverse().gather(&input, &mut gathered);
        assert_eq!(scattered, gathered);
    }

    #[test]
    #[should_panic(expected = "repeats")]
    fn a_table_that_misses_an_index_is_refused() {
        let _ = Permutation::from_fn(4, |i| i / 2);
    }

    #[test]
    fn ragged_columns_read_row_by_row_in_groups() {
        let p = Permutation::columns(&[5, 4], 2);
        let order: Vec<usize> = (0..p.len()).map(|i| p.index(i)).collect();
        assert_eq!(order, [0, 1, 5, 6, 2, 3, 7, 8, 4]);
    }

    #[test]
    fn equal_columns_of_single_symbols_are_a_matrix_transpose() {
        let p = Permutation::columns(&[3, 3, 3, 3], 1);
        let order: Vec<usize> = (0..p.len()).map(|i| p.index(i)).collect();
        assert_eq!(order, [0, 3, 6, 9, 1, 4, 7, 10, 2, 5, 8, 11]);
    }
}
