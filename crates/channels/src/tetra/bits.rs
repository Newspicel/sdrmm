#[derive(Clone, Copy, Debug)]
pub(crate) struct Reader<'a> {
    bits: &'a [bool],
    at: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bits: &'a [bool]) -> Self {
        Self { bits, at: 0 }
    }

    pub(crate) fn take(&mut self, len: usize) -> Option<u32> {
        let field = self.bits.get(self.at..self.at + len)?;
        self.at += len;
        Some(field.iter().fold(0, |acc, &bit| acc << 1 | u32::from(bit)))
    }

    pub(crate) fn take_u8(&mut self, len: usize) -> Option<u8> {
        self.take(len).and_then(|value| u8::try_from(value).ok())
    }

    pub(crate) fn take_u16(&mut self, len: usize) -> Option<u16> {
        self.take(len).and_then(|value| u16::try_from(value).ok())
    }

    pub(crate) fn flag(&mut self) -> Option<bool> {
        self.take(1).map(|bit| bit == 1)
    }

    pub(crate) fn optional(&mut self, len: usize) -> Option<Option<u32>> {
        if self.flag()? {
            self.take(len).map(Some)
        } else {
            Some(None)
        }
    }

    pub(crate) fn skip(&mut self, len: usize) -> Option<()> {
        (self.at + len <= self.bits.len()).then(|| self.at += len)
    }

    pub(crate) fn position(&self) -> usize {
        self.at
    }

    pub(crate) fn remaining(&self) -> usize {
        self.bits.len() - self.at
    }

    pub(crate) fn rest(&self) -> &'a [bool] {
        &self.bits[self.at..]
    }
}

pub(crate) fn without_fill(bits: &[bool]) -> &[bool] {
    let end = bits.iter().rposition(|&bit| bit).unwrap_or(0);
    &bits[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(text: &str) -> Vec<bool> {
        text.chars()
            .filter(|c| !c.is_whitespace())
            .map(|c| c == '1')
            .collect()
    }

    #[test]
    fn fields_read_msb_first_and_stop_at_the_end() {
        let data = bits("1011 0010 1");
        let mut reader = Reader::new(&data);
        assert_eq!(reader.take(4), Some(0b1011));
        assert_eq!(reader.flag(), Some(false));
        assert_eq!(reader.take(3), Some(0b010));
        assert_eq!(reader.take(2), None);
        assert_eq!(reader.remaining(), 1);
    }

    #[test]
    fn fill_bits_end_at_the_last_one() {
        assert_eq!(without_fill(&bits("1101 1000")), bits("1101").as_slice());
        assert!(without_fill(&bits("0000")).is_empty());
    }
}
