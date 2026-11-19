use sdrmm_dsp::coding::fec::ccsds_rs::{Basis, CcsdsReedSolomon};

use super::link::{
    ASM, ASM_BYTES, CADU_BYTES, CODED_BYTES, FILL_VCID, IDLE_APID, INTERLEAVE, MpduLayout,
    NO_PACKET_START, PACKET_HEADER, PacketHeader, Precoding, Segment, VCDU_BYTES, VcduHeader,
    conv_code, randomize, reed_solomon,
};

pub type Cadu = [u8; CADU_BYTES];

/// Builds the CADUs a spacecraft sends: packets multiplexed into VCDUs, RS-coded, randomized and
/// marked. The test side of [`super::Deframer`] and [`super::Depacketizer`].
pub struct CaduWriter {
    spacecraft: u8,
    layout: MpduLayout,
    rs: CcsdsReedSolomon,
}

impl CaduWriter {
    #[must_use]
    pub fn new(spacecraft: u8, layout: MpduLayout, basis: Basis) -> Self {
        Self {
            spacecraft,
            layout,
            rs: reed_solomon(basis),
        }
    }

    #[must_use]
    pub fn cadu(&self, vcid: u8, counter: u32, first_header: u16, zone: &[u8]) -> Cadu {
        let mut vcdu = [0u8; VCDU_BYTES];
        VcduHeader {
            spacecraft: self.spacecraft,
            vcid,
            counter,
        }
        .write(&mut vcdu);
        self.layout.write_first_header(&mut vcdu, first_header);
        let zone_at = self.layout.zone_at();
        let len = zone.len().min(VCDU_BYTES - zone_at);
        vcdu[zone_at..zone_at + len].copy_from_slice(&zone[..len]);
        let mut coded = Vec::with_capacity(CODED_BYTES);
        self.rs.encode_interleaved(&vcdu, INTERLEAVE, &mut coded);
        let mut out = [0u8; CADU_BYTES];
        let (marker, body) = out.split_at_mut(ASM_BYTES);
        marker.copy_from_slice(&ASM.to_be_bytes());
        for (slot, &byte) in body.iter_mut().zip(&coded) {
            *slot = byte;
        }
        randomize(body);
        out
    }

    #[must_use]
    pub fn fill(&self, counter: u32) -> Cadu {
        self.cadu(FILL_VCID, counter, NO_PACKET_START, &[])
    }

    /// Packs `packets` back to back on `vcid`, padded with an idle packet to whole VCDUs.
    #[must_use]
    pub fn channel(&self, vcid: u8, first_counter: u32, packets: &[Vec<u8>]) -> Vec<Cadu> {
        let zone_len = self.layout.zone_len();
        let mut stream = Vec::new();
        let mut starts = Vec::new();
        for packet in packets {
            starts.push(stream.len());
            stream.extend_from_slice(packet);
        }
        let mut pad = zone_len - stream.len() % zone_len;
        if pad < PACKET_HEADER + 1 {
            pad += zone_len;
        }
        starts.push(stream.len());
        stream.extend_from_slice(&idle_packet(pad));
        stream
            .chunks(zone_len)
            .enumerate()
            .map(|(index, zone)| {
                let begin = index * zone_len;
                let first_header = starts
                    .iter()
                    .find(|&&start| (begin..begin + zone_len).contains(&start))
                    .map_or(NO_PACKET_START, |&start| (start - begin) as u16);
                self.cadu(vcid, first_counter + index as u32, first_header, zone)
            })
            .collect()
    }
}

#[must_use]
pub fn idle_packet(len: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; len];
    PacketHeader {
        secondary: false,
        apid: IDLE_APID,
        segment: Segment::Unsegmented,
        sequence: 0,
        length: len - PACKET_HEADER,
    }
    .write(&mut bytes);
    bytes
}

#[must_use]
pub fn space_packet(apid: u16, segment: Segment, sequence: u16, data: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0u8; PACKET_HEADER];
    PacketHeader {
        secondary: false,
        apid,
        segment,
        sequence,
        length: data.len(),
    }
    .write(&mut bytes);
    bytes.extend_from_slice(data);
    bytes
}

/// The rate 1/2 coded bits of `cadus`, MSB first, as the modulator takes them.
#[must_use]
pub fn coded_bits(cadus: &[Cadu]) -> Vec<bool> {
    coded_bits_with(cadus, Precoding::Nrzl)
}

#[must_use]
pub fn coded_bits_with(cadus: &[Cadu], precoding: Precoding) -> Vec<bool> {
    let mut bits: Vec<bool> = cadus
        .iter()
        .flat_map(|cadu| cadu.iter())
        .flat_map(|&byte| (0..8).rev().map(move |shift| byte >> shift & 1 == 1))
        .collect();
    precoding.encode(&mut bits, false);
    let mut coded = Vec::with_capacity(bits.len() * 2);
    conv_code().encode(&bits, &mut coded);
    coded
}
