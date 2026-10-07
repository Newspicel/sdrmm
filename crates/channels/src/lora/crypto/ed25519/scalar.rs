const ORDER: [u64; 4] = [
    0x5812_631a_5cf5_d3ed,
    0x14de_f9de_a2f7_9cd6,
    0,
    0x1000_0000_0000_0000,
];

pub(super) fn is_canonical(bytes: &[u8; 32]) -> bool {
    less_than(&to_limbs(bytes), &ORDER)
}

pub(super) fn reduce_wide(bytes: &[u8; 64]) -> [u8; 32] {
    let mut remainder = [0u64; 4];
    for byte in bytes.iter().rev() {
        for bit in (0..8).rev() {
            shift_in(&mut remainder, u64::from((byte >> bit) & 1));
            if !less_than(&remainder, &ORDER) {
                subtract(&mut remainder, &ORDER);
            }
        }
    }
    from_limbs(&remainder)
}

#[cfg(any(test, feature = "synth"))]
pub(super) fn multiply_add(left: &[u8; 32], right: &[u8; 32], addend: &[u8; 32]) -> [u8; 32] {
    let left = to_limbs(left);
    let right = to_limbs(right);
    let mut wide = [0u64; 8];
    for (index, limb) in to_limbs(addend).into_iter().enumerate() {
        wide[index] = limb;
    }
    for (left_index, &left_limb) in left.iter().enumerate() {
        let mut carry = 0u128;
        for (right_index, &right_limb) in right.iter().enumerate() {
            let slot = left_index + right_index;
            let sum =
                u128::from(left_limb) * u128::from(right_limb) + u128::from(wide[slot]) + carry;
            wide[slot] = sum as u64;
            carry = sum >> 64;
        }
        let mut slot = left_index + 4;
        while carry != 0 && slot < 8 {
            let sum = u128::from(wide[slot]) + carry;
            wide[slot] = sum as u64;
            carry = sum >> 64;
            slot += 1;
        }
    }
    let mut bytes = [0u8; 64];
    for (chunk, limb) in bytes.as_chunks_mut::<8>().0.iter_mut().zip(wide) {
        *chunk = limb.to_le_bytes();
    }
    reduce_wide(&bytes)
}

fn to_limbs(bytes: &[u8; 32]) -> [u64; 4] {
    let (words, _) = bytes.as_chunks::<8>();
    std::array::from_fn(|index| u64::from_le_bytes(words[index]))
}

fn from_limbs(limbs: &[u64; 4]) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    for (chunk, limb) in bytes.as_chunks_mut::<8>().0.iter_mut().zip(limbs) {
        *chunk = limb.to_le_bytes();
    }
    bytes
}

fn less_than(left: &[u64; 4], right: &[u64; 4]) -> bool {
    left.iter().rev().lt(right.iter().rev())
}

fn shift_in(value: &mut [u64; 4], bit: u64) {
    let mut carry = bit;
    for limb in value.iter_mut() {
        let next = *limb >> 63;
        *limb = (*limb << 1) | carry;
        carry = next;
    }
}

fn subtract(value: &mut [u64; 4], other: &[u64; 4]) {
    let mut borrow = false;
    for (limb, &other_limb) in value.iter_mut().zip(other) {
        let (difference, first) = limb.overflowing_sub(other_limb);
        let (difference, second) = difference.overflowing_sub(u64::from(borrow));
        *limb = difference;
        borrow = first || second;
    }
}

#[cfg(test)]
mod tests {
    use super::{ORDER, from_limbs, is_canonical, multiply_add, reduce_wide};

    #[test]
    fn order_is_not_canonical() {
        let order = from_limbs(&ORDER);
        assert!(!is_canonical(&order));
        let mut below = order;
        below[0] -= 1;
        assert!(is_canonical(&below));
    }

    #[test]
    fn reduces_order_multiples_to_zero() {
        let mut wide = [0u8; 64];
        wide[..32].copy_from_slice(&from_limbs(&ORDER));
        assert_eq!(reduce_wide(&wide), [0; 32]);
        let mut seven = [0u8; 32];
        seven[0] = 7;
        assert_eq!(multiply_add(&from_limbs(&ORDER), &seven, &seven), seven);
    }
}
