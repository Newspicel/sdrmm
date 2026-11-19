use super::*;
use sdrmm_dsp::Ddc;
use sdrmm_test_support::reference_data;

const RECORDED_RATE: f64 = 176_400.0;
const BLOCK: usize = 1 << 16;

fn recording(dir: &std::path::Path) -> Vec<Complex<f32>> {
    let bytes = reference_data::bytes(dir, "gk2a_fulldisk.cf32");
    let samples: Vec<Complex<f32>> = bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|s| {
            Complex::new(
                f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
                f32::from_le_bytes([s[4], s[5], s[6], s[7]]),
            )
        })
        .collect();
    let mut ddc = Ddc::new(RECORDED_RATE, INPUT_RATE_HZ, 0.0).expect("resampler");
    let mut out = Vec::new();
    let mut block = Vec::new();
    for chunk in samples.chunks(BLOCK) {
        ddc.process(chunk, &mut block);
        out.extend_from_slice(&block);
    }
    out
}

/// What the recording's frames hold, read below the file layer: the recording drops a few
/// frames every second or so, too often for a whole image segment (about fifty frames) to
/// come through, so its files are checked by the headers of their first packets.
#[derive(Default)]
struct Frames {
    good: u32,
    failed: u32,
    vcids: Vec<u8>,
    spacecraft: Vec<u8>,
    names: Vec<String>,
    keys: Vec<u16>,
    images: Vec<(u16, u16, u8)>,
}

fn frames(iq: &[Complex<f32>]) -> Frames {
    use sdrmm_modem::ccsds::{Deframer, Depacketizer, MAX_PACKET, MpduLayout, PacketHeader};
    use sdrmm_modem::linear::{
        CarrierLoop, LinearDemod, LinearParams, LinearTiming, PhaseDetector,
    };
    use sdrmm_modem::pulse::{Norm, root_raised_cosine};
    let pulse = root_raised_cosine(2.0, ROLL_OFF, 8, Norm::Energy);
    let params = LinearParams::new(sdrmm_modem::constellation::tables::bpsk(), pulse.clone(), 2)
        .expect("params");
    let carrier = CarrierLoop::new(PhaseDetector::MthPower { m: 2 }, 0.004);
    let mut demod = LinearDemod::new(&params, &pulse, LinearTiming::CONTINUOUS, Some(carrier));
    let mut deframer = Deframer::new(sdrmm_modem::ccsds::Basis::Dual);
    let mut depacketizer = Depacketizer::new(MpduLayout { insert_zone: 0 }, MAX_PACKET);
    let mut symbols = Vec::new();
    let mut found = Frames::default();
    for chunk in iq.chunks(BLOCK) {
        symbols.clear();
        demod.process(chunk, &mut symbols);
        let soft: Vec<i16> = symbols
            .iter()
            .map(|s| (s.re * 48.0).round().clamp(-127.0, 127.0) as i16)
            .collect();
        deframer.push(&soft);
        while let Some(outcome) = deframer.next_frame() {
            if outcome.failed {
                found.failed += 1;
                continue;
            }
            found.good += 1;
            let vcdu = deframer.vcdu();
            let header = sdrmm_modem::ccsds::VcduHeader::parse(vcdu).expect("VCDU");
            found.spacecraft.push(header.spacecraft);
            found.vcids.push(header.vcid);
            if header.vcid != 0 {
                continue;
            }
            depacketizer.load(vcdu);
            while let Some(packet) = depacketizer.next_packet() {
                if !PacketHeader::parse(packet).is_some_and(|header| header.segment.starts()) {
                    continue;
                }
                let Some(headers) = packet
                    .get(6 + sdrmm_modem::xrit::TRANSPORT_HEADER..)
                    .and_then(sdrmm_modem::xrit::Headers::parse)
                else {
                    continue;
                };
                found.names.extend(headers.annotation().map(str::to_owned));
                found.keys.extend(headers.key());
                found.images.extend(
                    headers
                        .image()
                        .map(|image| (image.columns, image.lines, image.compression)),
                );
            }
        }
    }
    found
}

#[test]
fn the_sigidwiki_full_disk_pass_decodes_to_file_headers() {
    let Some(dir) = reference_data::gk2a_sigid_dir() else {
        return;
    };
    let found = frames(&recording(&dir));
    assert!(found.good >= 1_000, "{} frames", found.good);
    assert!(found.spacecraft.iter().all(|&id| id == synth::SPACECRAFT));
    assert!(found.vcids.iter().all(|&vcid| vcid == 0 || vcid == 63));
    let segments: Vec<&str> = found
        .names
        .iter()
        .filter_map(|name| name.strip_prefix("IMG_FD_066_IR105_20250616_110006_"))
        .collect();
    assert_eq!(segments.len(), found.names.len(), "{:?}", found.names);
    assert!(segments.len() >= 5, "{segments:?}");
    assert!(segments.contains(&"06.lrit"));
    assert!(
        found.keys.iter().all(|&key| key == 0x0070),
        "{:?}",
        found.keys
    );
    assert!(found.images.iter().all(|&image| image == (2_200, 220, 2)));
}
