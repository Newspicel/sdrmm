use super::*;

const WIDTH: u16 = 64;
const HEIGHT: u16 = 48;

fn scene(width: u16, height: u16) -> Vec<u8> {
    let (w, h) = (usize::from(width), usize::from(height));
    (0..w * h)
        .flat_map(|index| {
            let (x, y) = ((index % w) as f32, (index / w) as f32);
            let ring = ((x - 30.0).hypot(y - 20.0) / 6.0).sin();
            [
                (60.0 + 2.5 * x) as u8,
                (128.0 + 90.0 * ring) as u8,
                (200.0 - 3.0 * y) as u8,
            ]
        })
        .collect()
}

fn settings(packet_type: PacketType, mcu_mode: McuMode) -> EncoderSettings {
    EncoderSettings {
        callsign: "VK5QI".to_owned(),
        image_id: 7,
        quality: 6,
        packet_type,
        mcu_mode,
        packet_len: MAX_PACKET_LEN,
    }
}

fn assemble(packets: &mut [Vec<u8>]) -> Assembler {
    let checker = PacketChecker::new();
    let mut assembler = Assembler::new();
    for packet in packets {
        let checked = checker.check(packet).expect("packet passes its checks");
        if assembler.header().is_none() {
            assembler.start(&checked.header);
        }
        assembler.feed(packet, &checked.header);
    }
    assembler
}

fn mean_error(a: &[u8], b: &[u8]) -> f64 {
    let total: u64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| u64::from(x.abs_diff(*y)))
        .sum();
    total as f64 / a.len() as f64
}

#[test]
fn callsigns_round_trip() {
    assert_eq!(decode_callsign(encode_callsign("VK5QI")), "VK5QI");
    assert_eq!(decode_callsign(encode_callsign("m0abc1")), "M0ABC1");
    assert_eq!(decode_callsign(encode_callsign("TOOLONGCALL")), "TOOLON");
    assert_eq!(decode_callsign(u32::MAX), "");
}

#[test]
fn pictures_round_trip_in_every_unit_shape() {
    let rgb = scene(WIDTH, HEIGHT);
    for packet_type in [PacketType::Fec, PacketType::NoFec] {
        for mode in [
            McuMode::Square,
            McuMode::Tall,
            McuMode::Wide,
            McuMode::Single,
        ] {
            let mut packets =
                encode(&settings(packet_type, mode), WIDTH, HEIGHT, &rgb).expect("encodes");
            let assembler = assemble(&mut packets);
            let header = assembler.header().expect("a picture");
            assert_eq!(header.callsign(), "VK5QI");
            assert_eq!((header.width, header.height), (WIDTH, HEIGHT));
            assert_eq!(header.mcu_mode, mode);
            assert!(assembler.is_complete(), "{packet_type:?} {mode:?}");
            assert_eq!(assembler.lines(), HEIGHT);
            let error = mean_error(assembler.rgb(), &rgb);
            assert!(error < 6.0, "{packet_type:?} {mode:?} error {error}");
        }
    }
}

#[test]
fn headers_carry_what_the_encoder_wrote() {
    let rgb = scene(WIDTH, HEIGHT);
    let packets = encode(
        &settings(PacketType::NoFec, McuMode::Square),
        WIDTH,
        HEIGHT,
        &rgb,
    )
    .expect("encodes");
    let first = Header::parse(&packets[0]).expect("header");
    assert_eq!(first.packet_id, 0);
    assert_eq!(first.image_id, 7);
    assert_eq!(first.quality, 6);
    assert_eq!(first.first_mcu, Some((0, 0)));
    assert_eq!(first.mcu_count(), 12);
    let last = Header::parse(packets.last().expect("packets")).expect("header");
    assert!(last.eoi);
    assert!(
        packets[..packets.len() - 1]
            .iter()
            .all(|packet| !Header::parse(packet).expect("header").eoi)
    );
}

#[test]
fn reed_solomon_repairs_sixteen_bytes() {
    let rgb = scene(WIDTH, HEIGHT);
    let mut packets = encode(
        &settings(PacketType::Fec, McuMode::Square),
        WIDTH,
        HEIGHT,
        &rgb,
    )
    .expect("encodes");
    let clean = packets[0].clone();
    for index in 0..16 {
        packets[0][2 + index * 15] ^= 0xA5;
    }
    let checked = PacketChecker::new()
        .check(&mut packets[0])
        .expect("repaired");
    assert_eq!(checked.corrected, 16);
    assert_eq!(packets[0], clean);
    for index in 0..17 {
        packets[1][2 + index * 14] ^= 0x5A;
    }
    assert_eq!(
        PacketChecker::new().check(&mut packets[1]),
        Err(PacketError::Corrupt)
    );
}

#[test]
fn a_bad_crc_refuses_a_no_fec_packet() {
    let rgb = scene(WIDTH, HEIGHT);
    let mut packets = encode(
        &settings(PacketType::NoFec, McuMode::Square),
        WIDTH,
        HEIGHT,
        &rgb,
    )
    .expect("encodes");
    packets[0][40] ^= 1;
    assert_eq!(
        PacketChecker::new().check(&mut packets[0]),
        Err(PacketError::Corrupt)
    );
    assert_eq!(
        PacketChecker::new().check(&mut packets[0][..10]),
        Err(PacketError::Length)
    );
}

#[test]
fn a_lost_packet_leaves_a_gap_and_drawing_resumes() {
    let (width, height) = (160, 96);
    let rgb = scene(width, height);
    let mut packets = encode(
        &settings(PacketType::NoFec, McuMode::Square),
        width,
        height,
        &rgb,
    )
    .expect("encodes");
    assert!(packets.len() > 4, "{} packets", packets.len());
    packets.remove(2);
    let assembler = assemble(&mut packets);
    let progress = assembler.progress();
    assert_eq!(progress.lost, 1);
    assert_eq!(progress.broken, 0);
    assert!(!assembler.is_complete());
    assert!(progress.drawn > progress.units / 2, "{progress:?}");
    let last_row = (usize::from(height) - 1) * usize::from(width) * 3;
    let tail = &assembler.rgb()[last_row..];
    assert!(mean_error(tail, &rgb[last_row..]) < 8.0);
}

#[test]
fn a_repeated_packet_is_ignored() {
    let rgb = scene(WIDTH, HEIGHT);
    let mut packets = encode(
        &settings(PacketType::NoFec, McuMode::Square),
        WIDTH,
        HEIGHT,
        &rgb,
    )
    .expect("encodes");
    let again = packets[0].clone();
    packets.insert(1, again);
    let assembler = assemble(&mut packets);
    assert_eq!(assembler.progress().repeated, 1);
    assert!(assembler.is_complete());
}

#[test]
fn shortened_packets_round_trip() {
    let rgb = scene(WIDTH, HEIGHT);
    let mut short = settings(PacketType::Fec, McuMode::Square);
    short.packet_len = 128;
    let mut packets = encode(&short, WIDTH, HEIGHT, &rgb).expect("encodes");
    assert!(packets.iter().all(|packet| packet.len() == 128));
    packets[1][20] ^= 0xFF;
    let assembler = assemble(&mut packets);
    assert!(assembler.is_complete());
}

#[test]
fn bad_sizes_are_refused() {
    let rgb = scene(WIDTH, HEIGHT);
    let mode = settings(PacketType::NoFec, McuMode::Square);
    assert_eq!(encode(&mode, 60, HEIGHT, &rgb), Err(EncodeError::Size));
    assert_eq!(
        encode(&mode, WIDTH, HEIGHT, &rgb[3..]),
        Err(EncodeError::Pixels)
    );
}
