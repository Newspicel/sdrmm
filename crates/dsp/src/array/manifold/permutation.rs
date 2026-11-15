use super::MAX_ELEMENTS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Permutation {
    pub(super) len: usize,
    pub(super) map: [u8; MAX_ELEMENTS],
}

impl Permutation {
    #[must_use]
    pub fn identity(len: usize) -> Self {
        Self::from_fn(len, |i| i)
    }

    #[must_use]
    pub fn reversal(len: usize) -> Self {
        let len = len.min(MAX_ELEMENTS);
        Self::from_fn(len, |i| len - 1 - i)
    }

    #[must_use]
    pub fn get(&self, index: usize) -> usize {
        usize::from(self.map[index])
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.map[..self.len].iter().map(|&index| usize::from(index))
    }

    fn from_fn(len: usize, f: impl Fn(usize) -> usize) -> Self {
        let len = len.min(MAX_ELEMENTS);
        let mut map = [0u8; MAX_ELEMENTS];
        for (i, slot) in map.iter_mut().enumerate().take(len) {
            *slot = f(i) as u8;
        }
        Self { len, map }
    }
}
