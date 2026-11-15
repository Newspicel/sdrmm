/// A running median over at most `N` values, kept sorted so each step costs one insert and one
/// remove instead of a fresh selection.
pub(super) struct SortedWindow<const N: usize> {
    values: [f32; N],
    len: usize,
}

impl<const N: usize> SortedWindow<N> {
    pub(super) const fn new() -> Self {
        Self {
            values: [0.0; N],
            len: 0,
        }
    }

    pub(super) fn insert(&mut self, value: f32) {
        let at = self.position(value);
        self.values.copy_within(at..self.len, at + 1);
        self.values[at] = value;
        self.len += 1;
    }

    pub(super) fn remove(&mut self, value: f32) {
        let at = self.position(value);
        self.values.copy_within(at + 1..self.len, at);
        self.len -= 1;
    }

    pub(super) fn median(&self) -> f32 {
        self.values[self.len / 2]
    }

    fn position(&self, value: f32) -> usize {
        self.values[..self.len].partition_point(|held| held.total_cmp(&value).is_lt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_tracks_the_median_of_what_it_holds() {
        let mut window = SortedWindow::<4>::new();
        for value in [5.0, 1.0, 3.0] {
            window.insert(value);
        }
        assert_eq!(window.median(), 3.0);
        window.remove(1.0);
        window.insert(9.0);
        assert_eq!(window.median(), 5.0);
    }
}
