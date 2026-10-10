const MASK: u64 = (1 << 51) - 1;

#[cfg(any(test, feature = "synth"))]
const P_MINUS_2: [u8; 32] = exponent(0xeb, 0x7f);
const P_MINUS_5_OVER_8: [u8; 32] = exponent(0xfd, 0x0f);

const fn exponent(lowest: u8, highest: u8) -> [u8; 32] {
    let mut bytes = [0xff; 32];
    bytes[0] = lowest;
    bytes[31] = highest;
    bytes
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Field([u64; 5]);

impl Field {
    pub(super) const ZERO: Self = Self([0; 5]);
    pub(super) const ONE: Self = Self([1, 0, 0, 0, 0]);

    pub(super) fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self([
            load(bytes, 0) & MASK,
            (load(bytes, 6) >> 3) & MASK,
            (load(bytes, 12) >> 6) & MASK,
            (load(bytes, 19) >> 1) & MASK,
            (load(bytes, 24) >> 12) & MASK,
        ])
    }

    pub(super) fn to_bytes(self) -> [u8; 32] {
        let mut limbs = self.reduce().0;
        let mut quotient = (limbs[0] + 19) >> 51;
        for limb in &limbs[1..] {
            quotient = (limb + quotient) >> 51;
        }
        limbs[0] += 19 * quotient;
        for index in 0..4 {
            limbs[index + 1] += limbs[index] >> 51;
            limbs[index] &= MASK;
        }
        limbs[4] &= MASK;
        pack(limbs)
    }

    fn reduce(self) -> Self {
        let limbs = self.0;
        let carries = limbs.map(|limb| limb >> 51);
        let mut reduced = limbs.map(|limb| limb & MASK);
        reduced[0] += carries[4] * 19;
        for index in 1..5 {
            reduced[index] += carries[index - 1];
        }
        Self(reduced)
    }

    pub(super) fn add(self, other: Self) -> Self {
        Self(std::array::from_fn(|index| self.0[index] + other.0[index])).reduce()
    }

    pub(super) fn sub(self, other: Self) -> Self {
        const FOUR_P: [u64; 5] = [4 * (MASK - 18), 4 * MASK, 4 * MASK, 4 * MASK, 4 * MASK];
        Self(std::array::from_fn(|index| {
            self.0[index] + FOUR_P[index] - other.0[index]
        }))
        .reduce()
    }

    pub(super) fn neg(self) -> Self {
        Self::ZERO.sub(self)
    }

    pub(super) fn mul(self, other: Self) -> Self {
        let [a0, a1, a2, a3, a4] = self.0.map(u128::from);
        let [b0, b1, b2, b3, b4] = other.0.map(u128::from);
        let [b1_19, b2_19, b3_19, b4_19] = [b1, b2, b3, b4].map(|limb| limb * 19);
        carry_wide([
            a0 * b0 + a1 * b4_19 + a2 * b3_19 + a3 * b2_19 + a4 * b1_19,
            a0 * b1 + a1 * b0 + a2 * b4_19 + a3 * b3_19 + a4 * b2_19,
            a0 * b2 + a1 * b1 + a2 * b0 + a3 * b4_19 + a4 * b3_19,
            a0 * b3 + a1 * b2 + a2 * b1 + a3 * b0 + a4 * b4_19,
            a0 * b4 + a1 * b3 + a2 * b2 + a3 * b1 + a4 * b0,
        ])
    }

    pub(super) fn square(self) -> Self {
        self.mul(self)
    }

    fn pow(self, exponent: &[u8; 32]) -> Self {
        let mut result = Self::ONE;
        for byte in exponent.iter().rev() {
            for bit in (0..8).rev() {
                result = result.square();
                if (byte >> bit) & 1 == 1 {
                    result = result.mul(self);
                }
            }
        }
        result
    }

    #[cfg(any(test, feature = "synth"))]
    pub(super) fn invert(self) -> Self {
        self.pow(&P_MINUS_2)
    }

    pub(super) fn pow_p_minus_5_over_8(self) -> Self {
        self.pow(&P_MINUS_5_OVER_8)
    }

    pub(super) fn is_negative(self) -> bool {
        self.to_bytes()[0] & 1 == 1
    }

    pub(super) fn equals(self, other: Self) -> bool {
        self.to_bytes() == other.to_bytes()
    }
}

fn load(bytes: &[u8; 32], offset: usize) -> u64 {
    let mut word = [0u8; 8];
    word.copy_from_slice(&bytes[offset..offset + 8]);
    u64::from_le_bytes(word)
}

fn carry_wide(mut wide: [u128; 5]) -> Field {
    for index in 0..4 {
        wide[index + 1] += wide[index] >> 51;
        wide[index] &= u128::from(MASK);
    }
    let overflow = wide[4] >> 51;
    wide[4] &= u128::from(MASK);
    wide[0] += overflow * 19;
    wide[1] += wide[0] >> 51;
    wide[0] &= u128::from(MASK);
    Field(wide.map(|limb| limb as u64))
}

fn pack(limbs: [u64; 5]) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    let mut accumulator = 0u128;
    let mut bits = 0;
    let mut index = 0;
    for limb in limbs {
        accumulator |= u128::from(limb) << bits;
        bits += 51;
        while bits >= 8 {
            bytes[index] = accumulator as u8;
            accumulator >>= 8;
            bits -= 8;
            index += 1;
        }
    }
    bytes[index] = accumulator as u8;
    bytes
}

#[cfg(test)]
mod tests {
    use super::Field;

    fn small(value: u64) -> Field {
        Field([value, 0, 0, 0, 0])
    }

    #[test]
    fn round_trips_canonical_bytes() {
        let mut bytes = [0x5au8; 32];
        bytes[31] = 0x3c;
        assert_eq!(Field::from_bytes(&bytes).to_bytes(), bytes);
    }

    #[test]
    fn reduces_p_to_zero() {
        let mut p = [0xffu8; 32];
        p[0] = 0xed;
        p[31] = 0x7f;
        assert_eq!(Field::from_bytes(&p).to_bytes(), [0; 32]);
    }

    #[test]
    fn inverse_times_value_is_one() {
        let value = small(121_666);
        assert!(value.mul(value.invert()).equals(Field::ONE));
    }

    #[test]
    fn negation_adds_to_zero() {
        let value = small(7).mul(small(1 << 50));
        assert!(value.add(value.neg()).equals(Field::ZERO));
        assert!(small(3).sub(small(5)).equals(small(2).neg()));
    }
}
