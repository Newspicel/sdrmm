use sdrmm_dsp::crc8_msb;
use sdrmm_wire::{IsmCoding, IsmIntegrity, IsmMessage};

use crate::ism::{
    bits::Bits,
    device::{Device, Found, Outcome, ook, reading},
    slicer::Timing,
};

pub(super) const SCHRADER: Device =
    ook(IsmCoding::Manchester, Timing::new(120, 0, 480), schrader).integrity(IsmIntegrity::Crc);

fn schrader(bits: &mut Bits, found: &mut Found) -> Outcome {
    if bits.len(0) != 68 {
        return Outcome::Skip;
    }
    let mut b = [0u8; 8];
    bits.extract(0, 4, &mut b, 64);
    Outcome::checked(crc8_msb(0x07, 0xf0, &b[..7]) == b[7], || {
        let serial = u32::from(b[1] & 0x0f) << 24
            | u32::from(b[2]) << 16
            | u32::from(b[3]) << 8
            | u32::from(b[4]);
        Outcome::emit(
            found,
            IsmMessage {
                pressure_kpa: Some(f32::from(b[5]) * 2.5),
                temperature_c: Some(f32::from(b[6]) - 50.0),
                ..reading("Schrader", format!("{serial:07X}"), &b, 64)
            },
        )
    })
}
