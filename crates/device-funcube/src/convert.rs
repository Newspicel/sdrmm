use sdrmm_device::{Sample, SampleConverter};

const MAX_WIDTH: usize = 8;
#[cfg(any(test, not(target_os = "macos")))]
const INT16_SCALE: f32 = 1.0 / 32_768.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Format {
    #[cfg(any(test, target_os = "macos"))]
    Float32,
    #[cfg(any(test, not(target_os = "macos")))]
    Int16,
}

impl Format {
    pub(crate) const fn width(self) -> usize {
        match self {
            #[cfg(any(test, target_os = "macos"))]
            Self::Float32 => 8,
            #[cfg(any(test, not(target_os = "macos")))]
            Self::Int16 => 4,
        }
    }

    fn decode(self, frame: &[u8]) -> Sample {
        match self {
            #[cfg(any(test, target_os = "macos"))]
            Self::Float32 => Sample::new(
                f32::from_ne_bytes([frame[0], frame[1], frame[2], frame[3]]),
                f32::from_ne_bytes([frame[4], frame[5], frame[6], frame[7]]),
            ),
            #[cfg(any(test, not(target_os = "macos")))]
            Self::Int16 => Sample::new(
                f32::from(i16::from_le_bytes([frame[0], frame[1]])) * INT16_SCALE,
                f32::from(i16::from_le_bytes([frame[2], frame[3]])) * INT16_SCALE,
            ),
        }
    }
}

#[derive(Debug)]
pub(crate) struct FunCubeConverter {
    format: Format,
    out: Vec<Sample>,
    carry: Vec<u8>,
}

impl FunCubeConverter {
    pub(crate) fn new(format: Format, samples: usize) -> Self {
        Self {
            format,
            out: Vec::with_capacity(samples),
            carry: Vec::with_capacity(MAX_WIDTH),
        }
    }
}

impl SampleConverter for FunCubeConverter {
    fn convert(&mut self, bytes: &[u8]) -> &[Sample] {
        self.out.clear();
        let width = self.format.width();
        let mut rest = bytes;
        if !self.carry.is_empty() {
            let take = (width - self.carry.len()).min(rest.len());
            self.carry.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
            if self.carry.len() == width {
                self.out.push(self.format.decode(&self.carry));
                self.carry.clear();
            }
        }
        let whole = rest.len() - rest.len() % width;
        let format = self.format;
        self.out.extend(
            rest[..whole]
                .chunks_exact(width)
                .map(|frame| format.decode(frame)),
        );
        self.carry.extend_from_slice(&rest[whole..]);
        &self.out
    }

    fn reset(&mut self) {
        self.out.clear();
        self.carry.clear();
    }

    fn bytes_per_sample(&self) -> u64 {
        self.format.width() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn float(i: f32, q: f32) -> Vec<u8> {
        [i.to_ne_bytes(), q.to_ne_bytes()].concat()
    }

    fn int16(i: i16, q: i16) -> Vec<u8> {
        [i.to_le_bytes(), q.to_le_bytes()].concat()
    }

    #[test]
    fn the_left_channel_is_in_phase_and_the_right_quadrature() {
        let mut converter = FunCubeConverter::new(Format::Float32, 4);
        assert_eq!(
            converter.convert(&float(0.25, -0.5)),
            [Sample::new(0.25, -0.5)]
        );
        let mut converter = FunCubeConverter::new(Format::Int16, 4);
        assert_eq!(
            converter.convert(&int16(8_192, -16_384)),
            [Sample::new(0.25, -0.5)]
        );
    }

    #[test]
    fn sixteen_bit_codes_reach_full_scale_at_the_rails() {
        let mut converter = FunCubeConverter::new(Format::Int16, 4);
        let out = converter.convert(&int16(i16::MIN, i16::MAX));
        assert_eq!(out[0].re, -1.0);
        assert!((out[0].im - 1.0).abs() < 1e-4);
    }

    #[test]
    fn a_sample_takes_as_many_bytes_as_the_converter_claims() {
        for (format, bytes) in [
            (
                Format::Float32,
                (0..256)
                    .flat_map(|n| float(n as f32, -n as f32))
                    .collect::<Vec<u8>>(),
            ),
            (
                Format::Int16,
                (0..256).flat_map(|n| int16(n, -n)).collect::<Vec<u8>>(),
            ),
        ] {
            let mut converter = FunCubeConverter::new(format, 256);
            let samples = converter.convert(&bytes).len() as u64;
            assert_eq!(samples * converter.bytes_per_sample(), bytes.len() as u64);
        }
    }

    #[test]
    fn a_block_split_inside_a_sample_loses_none_of_it() {
        for format in [Format::Float32, Format::Int16] {
            let bytes: Vec<u8> = (0..512).flat_map(|n| float(n as f32, -n as f32)).collect();
            let expected = FunCubeConverter::new(format, 512).convert(&bytes).to_vec();
            let mut split = FunCubeConverter::new(format, 512);
            let mut got = Vec::new();
            let mut at = 0;
            for len in [1usize, 3, 7, 9, 13, 1021].iter().cycle() {
                if at >= bytes.len() {
                    break;
                }
                let end = (at + len).min(bytes.len());
                got.extend_from_slice(split.convert(&bytes[at..end]));
                at = end;
            }
            assert_eq!(expected, got, "{format:?}");
        }
    }

    #[test]
    fn a_reset_converter_forgets_a_part_sample() {
        let bytes = float(7.0, 9.0);
        let mut converter = FunCubeConverter::new(Format::Float32, 4);
        assert!(converter.convert(&bytes[..5]).is_empty());
        converter.reset();
        assert!(converter.convert(&bytes[..4]).is_empty());
        assert_eq!(converter.convert(&bytes[4..]).len(), 1);
    }
}
