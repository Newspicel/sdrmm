pub(super) const ROWS: usize = 50;
pub(super) const ROW_BYTES: usize = 128;
const ROW_BITS: usize = ROW_BYTES * 8;

pub(super) struct Bits {
    rows: [[u8; ROW_BYTES]; ROWS],
    lens: [u16; ROWS],
    count: usize,
}

impl Bits {
    pub(super) fn new() -> Box<Self> {
        Box::new(Self {
            rows: [[0; ROW_BYTES]; ROWS],
            lens: [0; ROWS],
            count: 0,
        })
    }

    pub(super) fn clear(&mut self) {
        for row in 0..self.count {
            let used = usize::from(self.lens[row]).div_ceil(8);
            self.rows[row][..used].fill(0);
            self.lens[row] = 0;
        }
        self.count = 0;
    }

    pub(super) fn push(&mut self, bit: bool) {
        if self.count == 0 {
            self.count = 1;
        }
        let row = self.count - 1;
        let len = usize::from(self.lens[row]);
        if len >= ROW_BITS {
            return;
        }
        if bit {
            self.rows[row][len / 8] |= 0x80 >> (len % 8);
        }
        self.lens[row] += 1;
    }

    pub(super) fn new_row(&mut self) {
        if self.count == 0 {
            self.count = 1;
        }
        if self.count < ROWS {
            self.count += 1;
        } else {
            let row = self.count - 1;
            let used = usize::from(self.lens[row]).div_ceil(8);
            self.rows[row][..used].fill(0);
            self.lens[row] = 0;
        }
    }

    pub(super) fn sync(&mut self) {
        if self.count > 0 && self.lens[self.count - 1] > 0 {
            self.new_row();
        } else if self.count == 0 {
            self.count = 1;
        }
    }

    pub(super) fn rows(&self) -> usize {
        self.count
    }

    pub(super) fn len(&self, row: usize) -> usize {
        usize::from(self.lens[row])
    }

    pub(super) fn row(&self, row: usize) -> &[u8; ROW_BYTES] {
        &self.rows[row]
    }

    pub(super) fn bit(&self, row: usize, at: usize) -> bool {
        bit_at(&self.rows[row], at)
    }

    pub(super) fn invert(&mut self) {
        for row in 0..self.count {
            let len = usize::from(self.lens[row]);
            let bytes = &mut self.rows[row];
            for byte in &mut bytes[..len.div_ceil(8)] {
                *byte = !*byte;
            }
            if len % 8 != 0 {
                bytes[len / 8] &= 0xFF << (8 - len % 8);
            }
        }
    }

    pub(super) fn same(&self, a: usize, b: usize) -> bool {
        self.lens[a] == self.lens[b]
            && self.rows[a][..usize::from(self.lens[a]).div_ceil(8)]
                == self.rows[b][..usize::from(self.lens[b]).div_ceil(8)]
    }

    fn same_prefix(&self, a: usize, b: usize, bits: usize) -> bool {
        if self.len(a) < bits || self.len(b) < bits {
            return self.same(a, b);
        }
        let whole = bits / 8;
        let mask = 0xFF00u16 >> (bits % 8);
        self.rows[a][..whole] == self.rows[b][..whole]
            && (u16::from(self.rows[a][whole]) & mask) == (u16::from(self.rows[b][whole]) & mask)
    }

    pub(super) fn repeats(&self, row: usize) -> usize {
        (0..self.count)
            .filter(|&other| self.same(row, other))
            .count()
    }

    pub(super) fn repeated_row(&self, min_repeats: usize, min_bits: usize) -> Option<usize> {
        (0..self.count).find(|&row| self.len(row) >= min_bits && self.repeats(row) >= min_repeats)
    }

    pub(super) fn repeated_prefix(&self, min_repeats: usize, min_bits: usize) -> Option<usize> {
        (0..self.count).find(|&row| {
            self.len(row) >= min_bits
                && (0..self.count)
                    .filter(|&other| self.same_prefix(row, other, min_bits))
                    .count()
                    >= min_repeats
        })
    }

    pub(super) fn search(&self, row: usize, start: usize, pattern: &[u8], bits: usize) -> usize {
        let len = self.len(row);
        let mut at = start;
        while at + bits <= len {
            if (0..bits).all(|k| self.bit(row, at + k) == bit_at(pattern, k)) {
                return at;
            }
            at += 1;
        }
        len
    }

    pub(super) fn extract(&self, row: usize, start: usize, out: &mut [u8], bits: usize) {
        extract(&self.rows[row], start, out, bits);
    }

    pub(super) fn manchester(&self, row: usize, start: usize, max: usize, out: &mut Packed) {
        let len = self.len(row).min(start + 2 * max);
        let mut at = start;
        while at + 1 < len {
            let (first, second) = (self.bit(row, at), self.bit(row, at + 1));
            at += 2;
            if first == second {
                break;
            }
            out.push(second);
        }
    }
}

pub(super) fn bit_at(bytes: &[u8], at: usize) -> bool {
    bytes
        .get(at / 8)
        .is_some_and(|byte| byte >> (7 - at % 8) & 1 == 1)
}

pub(super) fn extract(bytes: &[u8], start: usize, out: &mut [u8], bits: usize) {
    for (k, byte) in out.iter_mut().enumerate().take(bits.div_ceil(8)) {
        *byte = (0..8).fold(0u8, |acc, bit| {
            let at = k * 8 + bit;
            acc << 1 | u8::from(at < bits && bit_at(bytes, start + at))
        });
    }
}

pub(super) struct Packed {
    bytes: [u8; 32],
    len: usize,
}

impl Packed {
    pub(super) fn new() -> Self {
        Self {
            bytes: [0; 32],
            len: 0,
        }
    }

    pub(super) fn push(&mut self, bit: bool) {
        if self.len >= self.bytes.len() * 8 {
            return;
        }
        if bit {
            self.bytes[self.len / 8] |= 0x80 >> (self.len % 8);
        }
        self.len += 1;
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }

    pub(super) fn bytes(&self) -> &[u8; 32] {
        &self.bytes
    }
}

pub(super) fn reflect_nibbles(bytes: &mut [u8]) {
    for byte in bytes {
        let hi = (*byte >> 4).reverse_bits() >> 4;
        let lo = (*byte & 0x0F).reverse_bits() >> 4;
        *byte = hi << 4 | lo;
    }
}

pub(super) fn add_bytes(bytes: &[u8]) -> u32 {
    bytes.iter().map(|&byte| u32::from(byte)).sum()
}

pub(super) fn parity(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .fold(0u32, |acc, &byte| acc ^ byte.count_ones())
        & 1
        == 1
}

pub(super) fn bcd(byte: u8) -> Option<u32> {
    let (hi, lo) = (byte >> 4, byte & 0x0F);
    (hi <= 9 && lo <= 9).then_some(u32::from(hi) * 10 + u32::from(lo))
}

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
pub(super) fn from_rows(rows: &[&str]) -> Box<Bits> {
    let mut bits = Bits::new();
    for (k, row) in rows.iter().enumerate() {
        if k > 0 {
            bits.new_row();
        }
        for c in row.chars() {
            bits.push(c == '1');
        }
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_fill_msb_first_and_clear_completely() {
        let mut bits = from_rows(&["1010000011", "11"]);
        assert_eq!(bits.rows(), 2);
        assert_eq!(bits.len(0), 10);
        assert_eq!(bits.row(0)[..2], [0xA0, 0xC0]);
        assert_eq!(bits.row(1)[0], 0xC0);
        bits.clear();
        assert_eq!(bits.rows(), 0);
        assert_eq!(bits.row(0)[..2], [0, 0]);
    }

    #[test]
    fn inverting_keeps_unused_bits_zero() {
        let mut bits = from_rows(&["101"]);
        bits.invert();
        assert_eq!(bits.row(0)[0], 0x40);
    }

    #[test]
    fn a_repeated_row_needs_identical_length_and_content() {
        let bits = from_rows(&["1100", "1101", "1101", "11010", "1101"]);
        assert_eq!(bits.repeated_row(3, 4), Some(1));
        assert_eq!(bits.repeated_row(4, 4), None);
        assert_eq!(bits.repeated_prefix(4, 4), Some(1));
    }

    #[test]
    fn search_finds_unaligned_patterns() {
        let bits = from_rows(&["0001011010"]);
        assert_eq!(bits.search(0, 0, &[0xB4], 6), 3);
        assert_eq!(bits.search(0, 0, &[0xFF], 8), 10);
        let mut out = [0u8; 1];
        bits.extract(0, 3, &mut out, 6);
        assert_eq!(out, [0xB4]);
    }

    #[test]
    fn manchester_pairs_take_their_second_bit_until_a_violation() {
        let bits = from_rows(&["0110011011"]);
        let mut out = Packed::new();
        bits.manchester(0, 0, 16, &mut out);
        assert_eq!(out.len(), 4);
        assert_eq!(out.bytes()[0], 0xA0);
    }

    #[test]
    fn manchester_drops_a_lone_trailing_half_bit() {
        let bits = from_rows(&["0110011"]);
        let mut out = Packed::new();
        bits.manchester(0, 0, 16, &mut out);
        assert_eq!((out.len(), out.bytes()[0]), (3, 0xA0));
    }

    #[test]
    fn nibbles_reflect_in_place() {
        let mut bytes = [0x12, 0x8F];
        reflect_nibbles(&mut bytes);
        assert_eq!(bytes, [0x84, 0x1F]);
    }

    #[test]
    fn rows_beyond_capacity_reuse_the_last_row() {
        let mut bits = Bits::new();
        for _ in 0..ROWS + 2 {
            bits.push(true);
            bits.new_row();
        }
        assert_eq!(bits.rows(), ROWS);
        assert_eq!(bits.len(ROWS - 1), 0);
    }
}
