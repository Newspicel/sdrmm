use super::rs256::DVB_PRIMITIVE;

const GF64_PRIMITIVE: u16 = 0x43;

/// A binary extension field small enough for one symbol per byte, multiplied bit-serially so a
/// short code needs no log tables.
#[derive(Clone, Copy, Debug)]
pub(super) struct SmallField {
    bits: u32,
    primitive: u16,
}

impl SmallField {
    pub(super) const GF64: Self = Self {
        bits: 6,
        primitive: GF64_PRIMITIVE,
    };

    pub(super) const GF256: Self = Self {
        bits: 8,
        primitive: DVB_PRIMITIVE,
    };

    pub(super) const fn mask(self) -> u8 {
        (u16::MAX >> (u16::BITS - self.bits)) as u8
    }

    pub(super) const fn order(self) -> i32 {
        self.mask() as i32
    }

    pub(super) fn mul(self, a: u8, b: u8) -> u8 {
        let mask = self.mask();
        let top = 1u8 << (self.bits - 1);
        let reduction = self.primitive as u8 & mask;
        let (mut a, mut b, mut product) = (a & mask, b & mask, 0u8);
        while b != 0 {
            if b & 1 != 0 {
                product ^= a;
            }
            let high = a & top != 0;
            a = (a << 1) & mask;
            if high {
                a ^= reduction;
            }
            b >>= 1;
        }
        product
    }

    pub(super) fn pow(self, base: u8, exponent: i32) -> u8 {
        let exponent = exponent.rem_euclid(self.order());
        (0..exponent).fold(1, |value, _| self.mul(value, base))
    }

    pub(super) fn inv(self, value: u8) -> Option<u8> {
        (value != 0).then(|| self.pow(value, self.order() - 1))
    }

    pub(super) fn eval_ascending(self, poly: &[u8], x: u8) -> u8 {
        poly.iter()
            .rev()
            .fold(0, |value, &coefficient| self.mul(value, x) ^ coefficient)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_non_zero_symbol_has_an_inverse() {
        for field in [SmallField::GF64, SmallField::GF256] {
            assert_eq!(field.inv(0), None);
            for value in 1..=field.mask() {
                let inverse = field.inv(value).unwrap();
                assert_eq!(field.mul(value, inverse), 1, "{field:?} {value}");
            }
        }
    }
}
