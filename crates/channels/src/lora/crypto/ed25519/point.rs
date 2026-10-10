use super::field::Field;

const CURVE_D: [u8; 32] = [
    0xa3, 0x78, 0x59, 0x13, 0xca, 0x4d, 0xeb, 0x75, 0xab, 0xd8, 0x41, 0x41, 0x4d, 0x0a, 0x70, 0x00,
    0x98, 0xe8, 0x79, 0x77, 0x79, 0x40, 0xc7, 0x8c, 0x73, 0xfe, 0x6f, 0x2b, 0xee, 0x6c, 0x03, 0x52,
];

const SQRT_MINUS_ONE: [u8; 32] = [
    0xb0, 0xa0, 0x0e, 0x4a, 0x27, 0x1b, 0xee, 0xc4, 0x78, 0xe4, 0x2f, 0xad, 0x06, 0x18, 0x43, 0x2f,
    0xa7, 0xd7, 0xfb, 0x3d, 0x99, 0x00, 0x4d, 0x2b, 0x0b, 0xdf, 0xc1, 0x4f, 0x80, 0x24, 0x83, 0x2b,
];

const BASE_X: [u8; 32] = [
    0x1a, 0xd5, 0x25, 0x8f, 0x60, 0x2d, 0x56, 0xc9, 0xb2, 0xa7, 0x25, 0x95, 0x60, 0xc7, 0x2c, 0x69,
    0x5c, 0xdc, 0xd6, 0xfd, 0x31, 0xe2, 0xa4, 0xc0, 0xfe, 0x53, 0x6e, 0xcd, 0xd3, 0x36, 0x69, 0x21,
];

const BASE_ENCODED: [u8; 32] = base_encoding();

const fn base_encoding() -> [u8; 32] {
    let mut bytes = [0x66; 32];
    bytes[0] = 0x58;
    bytes
}

pub(super) fn curve_d() -> Field {
    Field::from_bytes(&CURVE_D)
}

pub(super) fn sqrt_minus_one() -> Field {
    Field::from_bytes(&SQRT_MINUS_ONE)
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Point {
    x: Field,
    y: Field,
    z: Field,
    t: Field,
}

impl Point {
    const IDENTITY: Self = Self {
        x: Field::ZERO,
        y: Field::ONE,
        z: Field::ONE,
        t: Field::ZERO,
    };

    pub(super) fn base() -> Self {
        let x = Field::from_bytes(&BASE_X);
        let y = Field::from_bytes(&BASE_ENCODED);
        Self {
            x,
            y,
            z: Field::ONE,
            t: x.mul(y),
        }
    }

    pub(super) fn decode(bytes: &[u8; 32]) -> Option<Self> {
        let x_odd = bytes[31] >> 7 == 1;
        let mut y_bytes = *bytes;
        y_bytes[31] &= 0x7f;
        let y = Field::from_bytes(&y_bytes);
        if y.to_bytes() != y_bytes {
            return None;
        }
        let x = recover_x(y, x_odd)?;
        Some(Self {
            x,
            y,
            z: Field::ONE,
            t: x.mul(y),
        })
    }

    #[cfg(any(test, feature = "synth"))]
    pub(super) fn encode(self) -> [u8; 32] {
        let z_inverse = self.z.invert();
        let x = self.x.mul(z_inverse);
        let mut bytes = self.y.mul(z_inverse).to_bytes();
        bytes[31] |= u8::from(x.is_negative()) << 7;
        bytes
    }

    pub(super) fn add(self, other: Self) -> Self {
        let a = self.y.sub(self.x).mul(other.y.sub(other.x));
        let b = self.y.add(self.x).mul(other.y.add(other.x));
        let c = self.t.mul(curve_d().add(curve_d())).mul(other.t);
        let d = self.z.add(self.z).mul(other.z);
        let e = b.sub(a);
        let f = d.sub(c);
        let g = d.add(c);
        let h = b.add(a);
        Self {
            x: e.mul(f),
            y: g.mul(h),
            z: f.mul(g),
            t: e.mul(h),
        }
    }

    pub(super) fn multiply(self, scalar: &[u8; 32]) -> Self {
        let mut result = Self::IDENTITY;
        for byte in scalar.iter().rev() {
            for bit in (0..8).rev() {
                result = result.add(result);
                if (byte >> bit) & 1 == 1 {
                    result = result.add(self);
                }
            }
        }
        result
    }

    pub(super) fn equals(self, other: Self) -> bool {
        self.x.mul(other.z).equals(other.x.mul(self.z))
            && self.y.mul(other.z).equals(other.y.mul(self.z))
    }
}

fn recover_x(y: Field, x_odd: bool) -> Option<Field> {
    let y_squared = y.square();
    let u = y_squared.sub(Field::ONE);
    let v = curve_d().mul(y_squared).add(Field::ONE);
    let v_cubed = v.square().mul(v);
    let v_seventh = v_cubed.square().mul(v);
    let mut x = u.mul(v_cubed).mul(u.mul(v_seventh).pow_p_minus_5_over_8());
    let check = v.mul(x.square());
    if check.equals(u.neg()) {
        x = x.mul(sqrt_minus_one());
    } else if !check.equals(u) {
        return None;
    }
    if x.equals(Field::ZERO) && x_odd {
        return None;
    }
    if x.is_negative() != x_odd {
        x = x.neg();
    }
    Some(x)
}

#[cfg(test)]
mod tests {
    use super::{Point, curve_d, sqrt_minus_one};
    use crate::lora::crypto::ed25519::field::Field;

    fn small(value: u32) -> Field {
        let mut bytes = [0u8; 32];
        bytes[..4].copy_from_slice(&value.to_le_bytes());
        Field::from_bytes(&bytes)
    }

    #[test]
    fn curve_constant_is_minus_121665_over_121666() {
        assert!(curve_d().mul(small(121_666)).equals(small(121_665).neg()));
    }

    #[test]
    fn sqrt_minus_one_squares_to_minus_one() {
        assert!(sqrt_minus_one().square().equals(Field::ONE.neg()));
    }

    #[test]
    fn base_point_round_trips() {
        let base = Point::base();
        assert_eq!(base.encode(), super::BASE_ENCODED);
        assert!(Point::decode(&super::BASE_ENCODED).unwrap().equals(base));
        let doubled = base.add(base);
        assert!(doubled.equals(base.multiply(&{
            let mut two = [0u8; 32];
            two[0] = 2;
            two
        })));
        assert_eq!(
            Point::decode(&doubled.encode()).unwrap().encode(),
            doubled.encode()
        );
    }
}
