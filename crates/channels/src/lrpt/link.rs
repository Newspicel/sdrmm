use sdrmm_modem::ccsds::{Basis, Deframer, Depacketizer, MAX_PACKET, MpduLayout};

pub use sdrmm_modem::ccsds::{IDLE_APID, PACKET_HEADER, PacketHeader, SEQUENCE_MODULO};

/// Meteor puts a two-byte insert zone ahead of the `M_PDU`.
pub const LAYOUT: MpduLayout = MpduLayout { insert_zone: 2 };
/// Meteor's RS symbols are in the conventional basis, not CCSDS's dual one.
pub const BASIS: Basis = Basis::Conventional;
#[cfg(any(test, feature = "synth"))]
pub const SPACECRAFT_ID: u8 = 0x93;
pub const IMAGE_VCID: u8 = 5;

#[must_use]
pub fn deframer() -> Deframer {
    Deframer::new(BASIS)
}

#[must_use]
pub fn depacketizer() -> Depacketizer {
    Depacketizer::new(LAYOUT, MAX_PACKET).with_vcid(IMAGE_VCID)
}
