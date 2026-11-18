use sdrmm_dsp::{GF256_PRIMITIVE, ReedSolomon};

use crate::dab::{
    msc::SubChannelEncoder,
    protection::{Eep, Protection},
};
use sdrmm_modem::synth::{Need, SynthError};

pub struct Data {
    encoder: SubChannelEncoder,
    stream: Vec<u8>,
    at: usize,
}

impl Data {
    pub fn new() -> Result<Self, SynthError> {
        let image = include_bytes!("../../../../../fixtures/broadcast_audio/slideshow.png");
        let groups = [
            super::pad::group(3, 0, true, &super::pad::header(image.len())),
            super::pad::group(4, 0, true, image),
        ];
        let mut application = Vec::new();
        let mut counter = 0;
        for group in groups {
            let count = group.len().div_ceil(19);
            for (i, part) in group.chunks(19).enumerate() {
                let flags = (u8::from(i == 0) * 8) | (u8::from(i + 1 == count) * 4);
                application.extend(packet(counter << 4 | flags, 17, part));
                counter = (counter + 1) & 3;
            }
        }
        while application.len() < 2256 {
            application.extend(packet(12, 0, &[]));
        }
        let rs = ReedSolomon::new(GF256_PRIMITIVE, 0, 16);
        let mut parity = [0u8; 198];
        for row in 0..12 {
            let data: Vec<_> = (0..188).map(|col| application[col * 12 + row]).collect();
            let mut word = Vec::new();
            rs.encode(&data, &mut word);
            for col in 0..16 {
                parity[col * 12 + row] = word[188 + col];
            }
        }
        for (i, bytes) in parity.as_chunks::<22>().0.iter().enumerate() {
            application.extend_from_slice(&[(i as u8) << 2 | 3, 254]);
            application.extend_from_slice(bytes);
        }
        Ok(Self {
            encoder: SubChannelEncoder::new(Protection::eep(64, Eep::A, 3).need("EEP-A 3")?),
            stream: application,
            at: 0,
        })
    }

    pub fn frame(&mut self, out: &mut Vec<bool>) {
        let bytes: Vec<_> = (0..192)
            .map(|i| self.stream[(self.at + i) % self.stream.len()])
            .collect();
        self.at = (self.at + 192) % self.stream.len();
        self.encoder.frame(&bytes, out);
    }
}

fn packet(header: u8, address: u8, data: &[u8]) -> Vec<u8> {
    let mut bytes = vec![header, address, data.len() as u8];
    bytes.extend_from_slice(data);
    bytes.resize(22, 0);
    sdrmm_dsp::push_crc16_genibus(&mut bytes);
    bytes
}
