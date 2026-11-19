use num_complex::Complex;
use sdrmm_modem::ccsds::{Basis, Cadu, CaduWriter, MpduLayout};
use sdrmm_modem::synth::{Render, Signal};
use sdrmm_modem::xrit::{
    CdsTime, HeaderWriter, ImageStructure, Noaa, Rice, RiceParams, SegmentId, file_packets, header,
    modulate, rice_encode, rice_image_packets,
};

use super::{INPUT_RATE_HZ, PRECODING, ROLL_OFF, SPS};

pub const IMAGE_NAME: &str = "OR_ABI-L2-CMIPF-M6C13_G19_s20251772230205_e20251772239525.lrit";
pub const ADMIN_NAME: &str = "GOES_EAST_Admin_message.lrit";
pub const ADMIN_TEXT: &str = "SDR-- GOES HRIT TEST MESSAGE";
pub const WIDTH: u16 = 320;
pub const SEGMENT_LINES: u16 = 60;
pub const SEGMENTS: u16 = 2;
pub const IMAGE_ID: u16 = 43_643;
const IMAGE_VCID: u8 = 13;
const IMAGE_APID: u16 = 420;
const ADMIN_VCID: u8 = 0;
const ADMIN_APID: u16 = 0;
const SPACECRAFT: u8 = 0x19;
const RICE_FLAGS: u16 = 49;
const PIXELS_PER_BLOCK: u8 = 32;
const LEAD_FILL: u32 = 3;
const TRAIL_FILL: u32 = 3;

/// The full-disk test pattern: a bright disk on black space.
#[must_use]
pub fn pixel(x: usize, y: usize) -> u8 {
    let half = f64::from(WIDTH) / 2.0;
    let (dx, dy) = (x as f64 - half, y as f64 - half);
    let radius = (dx * dx + dy * dy).sqrt();
    if radius > half * 0.9 {
        0
    } else {
        (90.0 + 60.0 * (x as f64 * 0.05).sin() + 40.0 * (y as f64 * 0.08).cos()) as u8
    }
}

fn params() -> RiceParams {
    RiceParams {
        flags: RICE_FLAGS,
        bits_per_pixel: 8,
        pixels_per_block: PIXELS_PER_BLOCK,
        columns: usize::from(WIDTH),
    }
}

fn segment_packets(sequence: u16, first_packet: u16) -> Vec<Vec<u8>> {
    let start_line = sequence * SEGMENT_LINES;
    let headers = HeaderWriter::new()
        .image(ImageStructure {
            bits_per_pixel: 8,
            columns: WIDTH,
            lines: SEGMENT_LINES,
            compression: 1,
        })
        .annotation(IMAGE_NAME)
        .time(CdsTime {
            days: 24_646,
            millis: 81_005_000,
        })
        .segment(SegmentId {
            image: IMAGE_ID,
            sequence,
            start_column: 0,
            start_line,
            max_segment: SEGMENTS,
            max_column: WIDTH,
            max_line: SEGMENT_LINES * SEGMENTS,
        })
        .noaa(Noaa {
            agency: *b"NOAA",
            product: 19,
            subproduct: 13,
            parameter: 0,
            compression: 1,
        })
        .rice(Rice {
            flags: RICE_FLAGS,
            pixels_per_block: PIXELS_PER_BLOCK,
            lines_per_packet: 1,
        });
    let mut file = headers.file(header::IMAGE, &[]);
    let data_bits = u64::from(WIDTH) * u64::from(SEGMENT_LINES) * 8;
    file[8..16].copy_from_slice(&data_bits.to_be_bytes());
    let lines: Vec<Vec<u8>> = (0..usize::from(SEGMENT_LINES))
        .map(|row| {
            let y = usize::from(start_line) + row;
            let line: Vec<u8> = (0..usize::from(WIDTH)).map(|x| pixel(x, y)).collect();
            rice_encode(params(), &line)
        })
        .collect();
    rice_image_packets(IMAGE_APID, first_packet, sequence + 1, &file, &lines)
}

fn admin_packets() -> Vec<Vec<u8>> {
    let file = HeaderWriter::new()
        .annotation(ADMIN_NAME)
        .noaa(Noaa {
            agency: *b"NOAA",
            product: 1,
            subproduct: 0,
            parameter: 0,
            compression: 0,
        })
        .file(header::TEXT, ADMIN_TEXT.as_bytes());
    let header_len = file.len() - ADMIN_TEXT.len();
    file_packets(ADMIN_APID, 0, 9, &file, header_len, 8_000)
}

#[must_use]
pub fn cadus() -> Vec<Cadu> {
    let writer = CaduWriter::new(SPACECRAFT, MpduLayout { insert_zone: 0 }, Basis::Dual);
    let mut out: Vec<Cadu> = (0..LEAD_FILL).map(|counter| writer.fill(counter)).collect();
    out.extend(writer.channel(ADMIN_VCID, 0, &admin_packets()));
    let mut image = segment_packets(0, 0);
    image.extend(segment_packets(1, SEGMENT_LINES + 1));
    out.extend(writer.channel(IMAGE_VCID, 0, &image));
    out.extend((0..TRAIL_FILL).map(|counter| writer.fill(LEAD_FILL + counter)));
    out
}

/// One pass of the test downlink at the channel rate.
#[must_use]
pub fn transmission() -> Vec<Complex<f32>> {
    modulate(&cadus(), PRECODING, SPS, ROLL_OFF)
}

/// The signals a generator offers for this decoder.
pub(crate) static SIGNALS: &[Signal] = &[Signal {
    id: "goes_hrit",
    label: "GOES HRIT",
    rate_hz: INPUT_RATE_HZ,
    render: Render::Always(transmission),
}];
