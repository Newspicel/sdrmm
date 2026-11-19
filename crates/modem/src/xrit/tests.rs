use super::header::{IMAGE, TEXT};
use super::*;
use crate::ccsds::PacketHeader;

const PARAMS: RiceParams = RiceParams {
    flags: 49,
    bits_per_pixel: 8,
    pixels_per_block: 32,
    columns: 200,
};

fn line(seed: u32, columns: usize) -> Vec<u8> {
    (0..columns)
        .map(|x| {
            let x = x as u32;
            match seed % 4 {
                0 => 0,
                1 => (128 + ((x * 7 + seed) % 23)) as u8,
                2 => ((x * x + seed * 31) % 256) as u8,
                _ => {
                    if x < 60 {
                        0
                    } else {
                        (x / 3 + seed) as u8
                    }
                }
            }
        })
        .collect()
}

#[test]
fn rice_lines_round_trip_for_every_block_size_and_mode() {
    for pixels_per_block in [8, 16, 32, 64] {
        for flags in [49, 17, 1] {
            for columns in [200, 5424 / 8, 64] {
                let params = RiceParams {
                    flags,
                    pixels_per_block,
                    columns,
                    ..PARAMS
                };
                let mut decoder = RiceLine::new(params).expect("supported");
                for seed in 0..8 {
                    let sent = line(seed, columns);
                    let coded = rice_encode(params, &sent);
                    let mut got = Vec::new();
                    decoder.decode(&coded, &mut got).expect("decodes");
                    assert_eq!(
                        got, sent,
                        "block {pixels_per_block} flags {flags} seed {seed}"
                    );
                }
            }
        }
    }
}

#[test]
fn smooth_lines_compress() {
    let sent: Vec<u8> = (0..2_000).map(|x| (100 + x / 50) as u8).collect();
    let params = RiceParams {
        columns: sent.len(),
        ..PARAMS
    };
    assert!(rice_encode(params, &sent).len() < sent.len() / 4);
}

#[test]
fn a_truncated_rice_line_is_an_error() {
    let sent = line(2, 200);
    let coded = rice_encode(PARAMS, &sent);
    let mut decoder = RiceLine::new(PARAMS).expect("supported");
    let mut got = Vec::new();
    assert_eq!(
        decoder.decode(&coded[..coded.len() / 2], &mut got),
        Err(RiceError::Truncated)
    );
    assert!(
        RiceLine::new(RiceParams {
            bits_per_pixel: 10,
            ..PARAMS
        })
        .is_err()
    );
}

fn image_headers(name: &str, columns: u16, lines: u16) -> Vec<u8> {
    let writer = HeaderWriter::new()
        .image(ImageStructure {
            bits_per_pixel: 8,
            columns,
            lines,
            compression: 1,
        })
        .annotation(name)
        .time(CdsTime {
            days: 24_646,
            millis: 81_000_000,
        })
        .segment(SegmentId {
            image: 43_643,
            sequence: 1,
            start_column: 0,
            start_line: 339,
            max_segment: 16,
            max_column: 5_424,
            max_line: 5_424,
        })
        .noaa(Noaa {
            agency: *b"NOAA",
            product: 19,
            subproduct: 13,
            parameter: 0,
            compression: 1,
        })
        .rice(Rice {
            flags: 49,
            pixels_per_block: 32,
            lines_per_packet: 1,
        });
    let mut file = writer.file(IMAGE, &[]);
    let data_bits = u64::from(columns) * u64::from(lines) * 8;
    file[8..16].copy_from_slice(&data_bits.to_be_bytes());
    file
}

#[test]
fn headers_read_back() {
    let file = image_headers("OR_ABI-L2-CMIPF-M6C13_G19.lrit", 200, 5);
    let headers = Headers::parse(&file).expect("headers");
    assert_eq!(headers.primary.file_type, IMAGE);
    assert_eq!(headers.primary.data_len(), 1_000);
    assert_eq!(headers.annotation(), Some("OR_ABI-L2-CMIPF-M6C13_G19.lrit"));
    assert_eq!(headers.image().map(|image| image.columns), Some(200));
    assert_eq!(
        headers.segment().map(|segment| segment.start_line),
        Some(339)
    );
    assert_eq!(headers.noaa().map(|noaa| noaa.subproduct), Some(13));
    assert_eq!(headers.rice().map(|rice| rice.pixels_per_block), Some(32));
    assert_eq!(headers.key(), None);
    let time = headers.time().expect("time");
    assert_eq!(time.unix_seconds(), 1_750_804_200);
    let keyed = HeaderWriter::new().key(0x0102).file(TEXT, b"x");
    assert_eq!(Headers::parse(&keyed).and_then(Headers::key), Some(0x0102));
}

fn image_lines(count: usize) -> Vec<Vec<u8>> {
    (0..count)
        .map(|index| line(index as u32 + 1, 200))
        .collect()
}

fn rice_file(apid: u16, sequence: u16, lines: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let headers = image_headers("IMG", 200, lines.len() as u16);
    let coded: Vec<Vec<u8>> = lines.iter().map(|line| rice_encode(PARAMS, line)).collect();
    rice_image_packets(apid, sequence, 7, &headers, &coded)
}

fn collect(assembler: &mut SessionAssembler, vcid: u8, packets: &[Vec<u8>]) -> Vec<Vec<u8>> {
    packets
        .iter()
        .filter_map(|packet| assembler.push(vcid, packet).map(|file| file.bytes.to_vec()))
        .collect()
}

#[test]
fn a_rice_image_comes_back_line_for_line() {
    let lines = image_lines(5);
    let mut assembler = SessionAssembler::new(4);
    let files = collect(&mut assembler, 13, &rice_file(420, 16_380, &lines));
    assert_eq!(files.len(), 1);
    let headers = Headers::parse(&files[0]).expect("headers");
    assert_eq!(&files[0][headers.primary.header_len..], lines.concat());
    assert_eq!(assembler.stats().files, 1);
}

#[test]
fn a_lost_rice_line_is_filled_and_counted() {
    let lines = image_lines(6);
    let mut packets = rice_file(420, 0, &lines);
    packets.remove(3);
    let mut assembler = SessionAssembler::new(4);
    let mut missing = 0;
    let mut file = Vec::new();
    for packet in &packets {
        if let Some(done) = assembler.push(13, packet) {
            missing = done.lines_missing;
            file = done.bytes.to_vec();
        }
    }
    assert_eq!(missing, 1);
    let data = &file[Headers::parse(&file).expect("headers").primary.header_len..];
    assert_eq!(data.len(), 6 * 200);
    assert!(data[400..600].iter().all(|&value| value == 0));
    assert_eq!(&data[600..], lines[3..].concat());
}

#[test]
fn a_damaged_text_file_is_lost_and_counted() {
    let text: Vec<u8> = (0..3_000).map(|index| b'A' + (index % 26) as u8).collect();
    let file = HeaderWriter::new()
        .annotation("A_TEXT.lrit")
        .file(TEXT, &text);
    let mut packets = file_packets(641, 5, 9, &file, file.len() - text.len(), 1_000);
    assert_eq!(packets.len(), 4);
    let last = packets[2].len() - 1;
    packets[2][last] ^= 0xFF;
    let mut assembler = SessionAssembler::new(4);
    assert!(collect(&mut assembler, 20, &packets).is_empty());
    let stats = assembler.stats();
    assert_eq!(stats.packets_failed, 1);
    assert_eq!(stats.files_lost, 1);
    let whole = file_packets(641, 9, 10, &file, 50, 1_000);
    let files = collect(&mut assembler, 20, &whole);
    assert_eq!(files, vec![file]);
}

#[test]
fn interleaved_channels_and_apids_stay_apart() {
    let a = HeaderWriter::new().annotation("A").file(TEXT, &[1; 2_500]);
    let b = HeaderWriter::new().annotation("B").file(TEXT, &[2; 1_700]);
    let first = file_packets(641, 0, 1, &a, 20, 800);
    let second = file_packets(642, 0, 2, &b, 20, 800);
    let mut assembler = SessionAssembler::new(2);
    let mut files = Vec::new();
    for index in 0..first.len().max(second.len()) {
        for (packets, vcid) in [(&first, 20u8), (&second, 22u8)] {
            if let Some(packet) = packets.get(index)
                && let Some(done) = assembler.push(vcid, packet)
            {
                files.push((done.vcid, done.apid, done.counter, done.bytes.to_vec()));
            }
        }
    }
    files.sort();
    assert_eq!(files, vec![(20, 641, 1, a), (22, 642, 2, b)]);
    assert_eq!(
        PacketHeader::parse(&first[0]).map(|header| header.segment),
        Some(crate::ccsds::Segment::First)
    );
}

fn transmission(files: &[Vec<Vec<u8>>], vcid: u8) -> Vec<num_complex::Complex<f32>> {
    use crate::ccsds::{Basis, CaduWriter, MpduLayout};
    let writer = CaduWriter::new(7, MpduLayout { insert_zone: 0 }, Basis::Dual);
    let mut cadus = vec![writer.fill(0), writer.fill(1)];
    cadus.extend(writer.channel(vcid, 100, &files.concat()));
    cadus.extend((2..5).map(|counter| writer.fill(counter)));
    modulate(&cadus, crate::ccsds::Precoding::Nrzm, 2, 0.3)
}

#[test]
fn the_link_brings_files_through_noise_and_offset() {
    let lines = image_lines(12);
    let text = HeaderWriter::new()
        .annotation("A_TEST.lrit")
        .file(TEXT, b"HELLO FROM ORBIT");
    let files = vec![
        rice_file(420, 0, &lines),
        file_packets(641, 0, 3, &text, text.len(), 8_000),
    ];
    let mut iq = transmission(&files, 13);
    for (index, sample) in iq.iter_mut().enumerate() {
        let phase = 0.004 * std::f32::consts::TAU * index as f32;
        *sample *= num_complex::Complex::from_polar(1.0, phase + 0.7);
    }
    let power = iq.iter().map(num_complex::Complex::norm_sqr).sum::<f32>() / iq.len() as f32;
    let amp = (1.5 * power / 10f32.powf(0.6)).sqrt();
    let noise = sdrmm_modem_test_support::noise::complex_noise(3, amp, iq.len());
    let noisy: Vec<_> = iq.iter().zip(&noise).map(|(a, b)| a + b).collect();
    let mut link = XritLink::new(LinkConfig {
        sps: 2,
        roll_off: 0.3,
        basis: crate::ccsds::Basis::Dual,
        precoding: crate::ccsds::Precoding::Nrzm,
        sessions: 4,
    })
    .expect("link");
    let mut got = Vec::new();
    for chunk in noisy.chunks(4_096) {
        link.process(chunk, &mut |file| {
            got.push((
                file.vcid,
                file.apid,
                file.lines_missing,
                file.bytes.to_vec(),
            ));
        });
    }
    assert_eq!(got.len(), 2, "{:?}", link.stats());
    let image = &got[0].3;
    let header_len = Headers::parse(image).expect("headers").primary.header_len;
    assert_eq!(&image[header_len..], lines.concat());
    assert_eq!(got[1].3, text);
    assert_eq!(link.stats().lost(), 0);
}
