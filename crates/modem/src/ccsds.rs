//! CCSDS telemetry links (131.0-B, 132.0-B, 133.0-B): ASM-marked CADUs under the rate 1/2 K=7
//! code, the PN randomizer and RS(255,223) four deep; VCDUs; `M_PDU`s and space packets. Meteor
//! LRPT, `MetOp` AHRPT, GOES HRIT and GK-2A LRIT all use it.

mod frame;
mod link;
mod packet;
mod transmit;

pub use frame::{Deframer, FrameOutcome, SOFT_CAPACITY};
pub use link::*;
pub use packet::{Depacketizer, MAX_PACKET};
pub use sdrmm_dsp::coding::fec::ccsds_rs::Basis;
pub use transmit::{Cadu, CaduWriter, coded_bits, coded_bits_with, idle_packet, space_packet};

#[cfg(test)]
mod tests {
    use super::*;

    const LAYOUT: MpduLayout = MpduLayout { insert_zone: 0 };

    fn soft(cadus: &[Cadu], map: PairMap, skew: usize) -> Vec<i16> {
        let coded = coded_bits(cadus);
        let mut out: Vec<i16> = vec![25; skew];
        for &[first, second] in coded.as_chunks::<2>().0 {
            let level = |bit: bool| if bit { 40i16 } else { -40 };
            let (a, b) = map.apply(level(first), level(second));
            out.push(a);
            out.push(b);
        }
        out
    }

    fn packets(apid: u16, count: usize) -> Vec<Vec<u8>> {
        (0..count)
            .map(|index| {
                let data: Vec<u8> = (0..300 + 37 * index).map(|byte| byte as u8).collect();
                space_packet(apid, Segment::Unsegmented, index as u16, &data)
            })
            .collect()
    }

    fn deframe(soft: &[i16], basis: Basis) -> Vec<(FrameOutcome, [u8; VCDU_BYTES])> {
        let mut deframer = Deframer::new(basis);
        let mut out = Vec::new();
        for chunk in soft.chunks(2_999) {
            deframer.push(chunk);
            while let Some(outcome) = deframer.next_frame() {
                let mut vcdu = [0u8; VCDU_BYTES];
                vcdu.copy_from_slice(deframer.vcdu());
                out.push((outcome, vcdu));
            }
        }
        out
    }

    #[test]
    fn both_bases_survive_every_rotation_and_slip() {
        for basis in [Basis::Dual, Basis::Conventional] {
            let writer = CaduWriter::new(7, LAYOUT, basis);
            let mut cadus = vec![writer.fill(0)];
            cadus.extend(writer.channel(3, 1, &packets(100, 6)));
            cadus.push(writer.fill(9));
            for map in [PairMap::ALL[0], PairMap::ALL[3], PairMap::ALL[6]] {
                for skew in [0, 1] {
                    let frames = deframe(&soft(&cadus, map, skew), basis);
                    assert_eq!(frames.len(), cadus.len() - 1, "{basis:?} {map:?} {skew}");
                    assert!(frames.iter().all(|(outcome, _)| !outcome.failed));
                    let header = VcduHeader::parse(&frames[1].1).map(|h| (h.spacecraft, h.vcid));
                    assert_eq!(header, Some((7, 3)));
                }
            }
        }
    }

    #[test]
    fn the_wrong_basis_fails_every_frame() {
        let writer = CaduWriter::new(7, LAYOUT, Basis::Dual);
        let cadus = writer.channel(3, 0, &packets(100, 6));
        let frames = deframe(&soft(&cadus, PairMap::ALL[0], 0), Basis::Conventional);
        assert!(frames.is_empty());
    }

    fn unpack(vcdus: &[[u8; VCDU_BYTES]], capacity: usize) -> (Vec<Vec<u8>>, u32) {
        unpack_with(Depacketizer::new(LAYOUT, capacity), vcdus)
    }

    fn unpack_with(
        mut depacketizer: Depacketizer,
        vcdus: &[[u8; VCDU_BYTES]],
    ) -> (Vec<Vec<u8>>, u32) {
        let mut out = Vec::new();
        for vcdu in vcdus {
            depacketizer.load(vcdu);
            while let Some(packet) = depacketizer.next_packet() {
                if PacketHeader::parse(packet).is_some_and(|header| header.apid != IDLE_APID) {
                    out.push(packet.to_vec());
                }
            }
        }
        (out, depacketizer.dropped())
    }

    fn vcdus(cadus: &[Cadu]) -> Vec<[u8; VCDU_BYTES]> {
        let frames = deframe(&soft(cadus, PairMap::ALL[0], 0), Basis::Dual);
        frames.into_iter().map(|(_, vcdu)| vcdu).collect()
    }

    #[test]
    fn packets_come_back_whole_across_vcdus() {
        let writer = CaduWriter::new(7, LAYOUT, Basis::Dual);
        let sent = packets(100, 8);
        let mut cadus = writer.channel(3, 0, &sent);
        cadus.push(writer.fill(0));
        let (got, dropped) = unpack(&vcdus(&cadus), MAX_PACKET);
        assert_eq!(got, sent);
        assert_eq!(dropped, 0);
    }

    #[test]
    fn a_counter_gap_and_an_oversized_packet_are_counted() {
        let writer = CaduWriter::new(7, LAYOUT, Basis::Dual);
        let sent = packets(100, 8);
        let mut cadus = writer.channel(3, 0, &sent);
        cadus.push(writer.fill(0));
        let mut frames = vcdus(&cadus);
        frames.remove(2);
        let (got, dropped) = unpack(&frames, MAX_PACKET);
        assert!(got.len() < sent.len());
        assert!(dropped >= 1);
        assert!(got.iter().all(|packet| sent.contains(packet)));
        let (small, dropped) = unpack(&vcdus(&cadus), 500);
        assert_eq!(small.len(), 6);
        assert_eq!(dropped, 2);
    }

    fn derandomized(cadus: &[Cadu]) -> Vec<[u8; VCDU_BYTES]> {
        cadus
            .iter()
            .map(|cadu| {
                let mut body = *cadu;
                randomize(&mut body[ASM_BYTES..]);
                std::array::from_fn(|index| body[ASM_BYTES + index])
            })
            .collect()
    }

    #[test]
    fn a_filtered_depacketizer_passes_over_other_channels() {
        let layout = MpduLayout { insert_zone: 2 };
        let writer = CaduWriter::new(11, layout, Basis::Dual);
        let sent = packets(103, 8);
        let mut units = derandomized(&writer.channel(9, 0, &sent));
        units.insert(
            2,
            derandomized(&writer.channel(10, 40, &packets(104, 2)))[0],
        );
        let filtered = Depacketizer::new(layout, MAX_PACKET).with_vcid(9);
        assert_eq!(unpack_with(filtered, &units), (sent, 0));
        let (_, dropped) = unpack_with(Depacketizer::new(layout, MAX_PACKET), &units);
        assert!(dropped > 0);
    }

    #[test]
    fn a_short_vcdu_is_passed_over_and_its_gap_counted() {
        let writer = CaduWriter::new(7, LAYOUT, Basis::Dual);
        let units = derandomized(&writer.channel(3, 0, &packets(100, 8)));
        let mut depacketizer = Depacketizer::new(LAYOUT, MAX_PACKET);
        let mut whole = 0;
        for (index, unit) in units.iter().enumerate() {
            let unit: &[u8] = if index == 2 { &unit[..100] } else { unit };
            depacketizer.load(unit);
            while depacketizer.next_packet().is_some() {
                whole += 1;
            }
        }
        assert!(whole < 9);
        assert_eq!(depacketizer.dropped(), 1);
    }
}
