pub use sdrmm_modem::jpeg::{BLOCK_PIXELS as MCU_PIXELS, BLOCK_SIDE as MCU_SIDE, Block};
use sdrmm_modem::jpeg::{BitReader, Tables, ZIGZAG, decode_block, inverse_dct};

pub const MCUS_PER_PACKET: usize = 14;

const LUMA_QUANT: [u16; MCU_PIXELS] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56,
    14, 17, 22, 29, 51, 87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113,
    92, 49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
];

#[must_use]
pub fn quant_table(quality: u8) -> [u16; MCU_PIXELS] {
    let quality = f32::from(quality.max(1));
    let factor = if quality > 20.0 && quality < 50.0 {
        5_000.0 / quality
    } else {
        200.0 - 2.0 * quality
    };
    LUMA_QUANT.map(|base| ((factor / 100.0 * f32::from(base)).round() as u16).max(1))
}

pub struct McuDecoder<'a> {
    reader: BitReader<'a>,
    quant: [u16; MCU_PIXELS],
    previous_dc: i32,
}

impl<'a> McuDecoder<'a> {
    #[must_use]
    pub fn new(data: &'a [u8], quality: u8) -> Self {
        Self {
            reader: BitReader::new(data),
            quant: quant_table(quality),
            previous_dc: 0,
        }
    }

    pub fn next_block(&mut self, pixels: &mut Block) -> Option<()> {
        let coded = decode_block(&mut self.reader, Tables::luma())?;
        self.previous_dc += coded.dc_difference;
        let mut coefficients = [0f32; MCU_PIXELS];
        coefficients[0] = self.previous_dc as f32 * f32::from(self.quant[0]);
        for (k, &value) in coded.zigzag.iter().enumerate().skip(1) {
            let natural = ZIGZAG[k];
            coefficients[natural] = value as f32 * f32::from(self.quant[natural]);
        }
        inverse_dct(&coefficients, pixels);
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_scales_the_table() {
        assert_eq!(quant_table(50)[0], 16);
        assert!(quant_table(90)[0] < quant_table(30)[0]);
        assert!(quant_table(100).iter().all(|&q| q == 1));
    }
}
