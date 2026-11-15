/// Shifts that fold a 32-bit Gray word back to binary as a prefix XOR.
const PREFIX_SHIFTS: [u32; 5] = [16, 8, 4, 2, 1];

#[must_use]
pub fn gray(i: u32) -> u32 {
    i ^ (i >> 1)
}

#[must_use]
pub fn gray_inverse(g: u32) -> u32 {
    PREFIX_SHIFTS.iter().fold(g, |b, &shift| b ^ (b >> shift))
}
