use num_complex::Complex;
use sdrmm_crypto::Des;
use sdrmm_modem::ccsds::{Basis, Cadu, CaduWriter, MpduLayout, Precoding};
use sdrmm_modem::jpeg::encode_grey;
use sdrmm_modem::synth::{Render, Signal};
use sdrmm_modem::xrit::{HeaderWriter, ImageStructure, file_packets, header, modulate};

use super::{INPUT_RATE_HZ, ROLL_OFF, SPS};

pub const SPACECRAFT: u8 = 195;
pub const WIDTH: usize = 160;
pub const SEGMENT_LINES: usize = 16;
pub const SEGMENTS: usize = 10;
/// The segment sent encrypted, and the key it is sent under.
pub const LOCKED_SEGMENT: usize = 4;
pub const KEY_INDEX: u16 = 0x0007;
pub const KEY: [u8; 8] = [0x13, 0x34, 0x57, 0x79, 0x9B, 0xBC, 0xDF, 0xF1];
pub const TEXT_NAME: &str = "ANT_LRIT_047_20190713_022006.lrit";
pub const TEXT: &str = "GK-2A SDR-- TEST BULLETIN";
const IMAGE_VCID: u8 = 0;
const TEXT_VCID: u8 = 4;
const IMAGE_APID: u16 = 0;
const TEXT_APID: u16 = 4;
const PACKET_DATA: usize = 4_000;
const DES_BLOCK: usize = 8;
const LEAD_FILL: u32 = 3;
const TRAIL_FILL: u32 = 3;

#[must_use]
pub fn segment_name(segment: usize) -> String {
    format!("IMG_FD_047_IR105_20190713_022006_{segment:02}.lrit")
}

/// The full disk test pattern.
#[must_use]
pub fn pixel(x: usize, y: usize) -> u8 {
    (100.0 + 60.0 * (x as f64 * 0.07).sin() + 50.0 * (y as f64 * 0.05).cos()) as u8
}

/// KMA's four-byte segment record: segment, segments, first line counted from one.
fn kma_segment(segment: usize) -> [u8; 4] {
    let [high, low] = (((segment - 1) * SEGMENT_LINES + 1) as u16).to_be_bytes();
    [segment as u8, SEGMENTS as u8, high, low]
}

fn segment_file(segment: usize) -> Vec<u8> {
    let first = (segment - 1) * SEGMENT_LINES;
    let pixels: Vec<u8> = (first..first + SEGMENT_LINES)
        .flat_map(|y| (0..WIDTH).map(move |x| pixel(x, y)))
        .collect();
    let mut data = encode_grey(&pixels, WIDTH, SEGMENT_LINES, 0);
    let locked = segment == LOCKED_SEGMENT;
    if locked {
        data.resize(data.len().next_multiple_of(DES_BLOCK), 0);
        let des = Des::new(KEY);
        for block in data.as_chunks_mut::<DES_BLOCK>().0 {
            des.encrypt(block);
        }
    }
    HeaderWriter::new()
        .image(ImageStructure {
            bits_per_pixel: 8,
            columns: WIDTH as u16,
            lines: SEGMENT_LINES as u16,
            compression: 2,
        })
        .annotation(&segment_name(segment))
        .key(if locked { KEY_INDEX } else { 0 })
        .record(header::SEGMENT, &kma_segment(segment))
        .file(header::IMAGE, &data)
}

fn packets_of(file: &[u8], apid: u16, sequence: &mut u16, counter: u16) -> Vec<Vec<u8>> {
    let header_len = sdrmm_modem::xrit::Headers::parse(file)
        .map_or(file.len(), |headers| headers.primary.header_len);
    let packets = file_packets(apid, *sequence, counter, file, header_len, PACKET_DATA);
    *sequence = sequence.wrapping_add(packets.len() as u16) % (1 << 14);
    packets
}

#[must_use]
pub fn cadus() -> Vec<Cadu> {
    let writer = CaduWriter::new(SPACECRAFT, MpduLayout { insert_zone: 0 }, Basis::Dual);
    let mut out: Vec<Cadu> = (0..LEAD_FILL).map(|counter| writer.fill(counter)).collect();
    let text = HeaderWriter::new()
        .annotation(TEXT_NAME)
        .key(0)
        .file(header::TEXT, TEXT.as_bytes());
    let mut sequence = 0;
    out.extend(writer.channel(
        TEXT_VCID,
        0,
        &packets_of(&text, TEXT_APID, &mut sequence, 1),
    ));
    let mut sequence = 0;
    let image: Vec<Vec<u8>> = (1..=SEGMENTS)
        .flat_map(|segment| {
            packets_of(
                &segment_file(segment),
                IMAGE_APID,
                &mut sequence,
                segment as u16 + 1,
            )
        })
        .collect();
    out.extend(writer.channel(IMAGE_VCID, 0, &image));
    out.extend((0..TRAIL_FILL).map(|counter| writer.fill(LEAD_FILL + counter)));
    out
}

/// One pass of the test downlink at the channel rate.
#[must_use]
pub fn transmission() -> Vec<Complex<f32>> {
    modulate(&cadus(), Precoding::Nrzl, SPS, ROLL_OFF)
}

/// The signals a generator offers for this decoder.
pub(crate) static SIGNALS: &[Signal] = &[Signal {
    id: "gk2a_lrit",
    label: "GK-2A LRIT",
    rate_hz: INPUT_RATE_HZ,
    render: Render::Always(transmission),
}];
