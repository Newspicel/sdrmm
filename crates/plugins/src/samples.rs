//! Little-endian sample layouts shared with the guest.

use num_complex::Complex;

pub(crate) const SAMPLE_BYTES: usize = 8;

pub(crate) fn read_samples(bytes: &[u8], into: &mut Vec<Complex<f32>>) {
    into.clear();
    into.extend(
        bytes
            .as_chunks::<SAMPLE_BYTES>()
            .0
            .iter()
            .map(|[a, b, c, d, e, f, g, h]| {
                Complex::new(
                    f32::from_le_bytes([*a, *b, *c, *d]),
                    f32::from_le_bytes([*e, *f, *g, *h]),
                )
            }),
    );
}

pub(crate) fn write_samples(samples: &[Complex<f32>], into: &mut [u8]) {
    for (chunk, sample) in into
        .as_chunks_mut::<SAMPLE_BYTES>()
        .0
        .iter_mut()
        .zip(samples)
    {
        chunk[..4].copy_from_slice(&sample.re.to_le_bytes());
        chunk[4..].copy_from_slice(&sample.im.to_le_bytes());
    }
}

pub(crate) fn floats(bytes: &[u8]) -> impl Iterator<Item = f32> + '_ {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
}
