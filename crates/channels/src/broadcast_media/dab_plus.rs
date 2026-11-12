use crate::dab::superframe::AudioFormat;

struct Bits {
    bytes: Vec<u8>,
    used: usize,
}

impl Bits {
    fn push(&mut self, value: usize, width: usize) {
        for bit in (0..width).rev() {
            if self.used.is_multiple_of(8) {
                self.bytes.push(0);
            }
            self.bytes[self.used / 8] |= (((value >> bit) & 1) as u8) << (7 - self.used % 8);
            self.used += 1;
        }
    }
}

pub fn audio_specific_config(format: AudioFormat) -> Result<Vec<u8>, &'static str> {
    if format.surround != 0 || !matches!(format.sample_rate_hz, 32000 | 48000) {
        return Err("Unsupported DAB+ surround or sample rate");
    }
    let mut config = Bits {
        bytes: Vec::with_capacity(4),
        used: 0,
    };
    let extension = format.spectral_band_replication;
    config.push(
        if extension {
            if format.parametric_stereo { 29 } else { 5 }
        } else {
            2
        },
        5,
    );
    let index = |rate| match rate {
        16000 => 8,
        24000 => 6,
        32000 => 5,
        48000 => 3,
        _ => 15,
    };
    config.push(index(format.core_rate_hz()), 4);
    config.push(if format.stereo_core { 2 } else { 1 }, 4);
    if extension {
        config.push(index(format.output_rate_hz()), 4);
        config.push(2, 5);
    }
    config.push(1, 1);
    config.push(0, 2);
    Ok(config.bytes)
}

/// Strips leading PAD data elements and the zero fill after the block: a raw block ends in the
/// nonzero bits of `ID_END`, and FDK rejects bytes past it.
pub fn access_unit(mut unit: &[u8]) -> Result<&[u8], &'static str> {
    if unit.len() > 8000 {
        return Err("Unsupported DAB+ access-unit size");
    }
    while unit.first().is_some_and(|byte| byte >> 5 == 4) {
        let count = usize::from(*unit.get(1).ok_or("Truncated DAB+ data element")?);
        let offset = if count == 255 {
            3 + count + usize::from(*unit.get(2).ok_or("Truncated DAB+ data element")?)
        } else {
            2 + count
        };
        unit = unit.get(offset..).ok_or("Truncated DAB+ data element")?;
    }
    let end = unit
        .iter()
        .rposition(|&byte| byte != 0)
        .map_or(0, |last| last + 1);
    Ok(&unit[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_access_unit_drops_its_pad_element_and_trailing_fill() {
        let unit = [0x80, 2, 0xAA, 0xBB, 0x21, 0x1B, 0x0E, 0, 0, 0];
        assert_eq!(access_unit(&unit), Ok([0x21, 0x1B, 0x0E].as_slice()));
    }
}
