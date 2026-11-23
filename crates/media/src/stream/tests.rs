use sdrmm_modem::mpegts::{PesUnit, StreamKind, TsDemux};

use super::*;
use crate::{Codec, Payload, decoder::Decoder};

const AUDIO_BLOCK: usize = 960;

fn picture(width: u16, height: u16, shade: u8) -> VideoPicture {
    let rgb = (0..usize::from(width) * usize::from(height))
        .flat_map(|at| [shade, (at % 251) as u8, 255 - shade])
        .collect();
    VideoPicture {
        width,
        height,
        luma: Vec::new(),
        rgb,
    }
}

fn tone(start: usize) -> Vec<f32> {
    (start..start + AUDIO_BLOCK)
        .flat_map(|n| {
            let value =
                (n as f32 * 1_000.0 * std::f32::consts::TAU / AUDIO_RATE as f32).sin() * 0.5;
            [value, value]
        })
        .collect()
}

/// Two seconds of 25 pictures and 50 audio blocks a second.
fn broadcast(encoder: &mut TsEncoder) -> Vec<[u8; PACKET]> {
    let mut out = Vec::new();
    for step in 0..100u64 {
        let now = step * CLOCK_HZ / 50;
        encoder
            .audio(&tone(step as usize * AUDIO_BLOCK), now, &mut out)
            .expect("audio");
        if step % 2 == 0 {
            encoder
                .picture(&picture(64, 48, step as u8), now, &mut out)
                .expect("picture");
        }
    }
    out
}

fn demux(packets: &[[u8; PACKET]]) -> (TsDemux, Vec<PesUnit>) {
    let mut demux = TsDemux::new();
    let mut units = Vec::new();
    for packet in packets {
        demux.push(packet, &mut units);
    }
    (demux, units)
}

fn decoded(codec: &Codec, units: &[PesUnit], pid: u16) -> Vec<Payload> {
    let mut decoder = Decoder::new(codec).expect("decoder");
    let mut out = Vec::new();
    for unit in units.iter().filter(|unit| unit.pid == pid) {
        decoder
            .push(&unit.payload, unit.pts.map(|pts| pts as i64), &mut out)
            .expect("decodes");
    }
    decoder.finish(&mut out).expect("flush");
    out.into_iter().map(|(_, payload)| payload).collect()
}

#[test]
fn pictures_and_sound_play_back_from_the_stream() {
    let packets = broadcast(&mut TsEncoder::new(true));
    let (demux, units) = demux(&packets);
    assert_eq!(demux.discontinuities, 0);
    let program = demux.program().expect("a programme");
    let kinds: Vec<_> = program.streams.iter().map(|stream| stream.kind).collect();
    assert_eq!(kinds, [StreamKind::Mpeg2Video, StreamKind::Mpeg1Audio]);
    let pictures = decoded(&Codec::Mpeg2, &units, VIDEO_PID);
    assert!(pictures.len() >= 45, "{} pictures", pictures.len());
    assert!(pictures.iter().all(|payload| matches!(
        payload,
        Payload::Video(picture) if (picture.width, picture.height) == (64, 48)
    )));
    let sound: usize = decoded(&Codec::Mp2, &units, AUDIO_PID)
        .iter()
        .map(|payload| match payload {
            Payload::Audio(pcm) => pcm.len() / 2,
            _ => 0,
        })
        .sum();
    assert!(sound > AUDIO_RATE as usize * 3 / 2, "{sound} samples");
}

#[test]
fn presentation_runs_behind_the_clock_and_only_forward() {
    let packets = broadcast(&mut TsEncoder::new(true));
    let (_, units) = demux(&packets);
    let video: Vec<u64> = units
        .iter()
        .filter(|unit| unit.pid == VIDEO_PID)
        .filter_map(|unit| unit.pts)
        .collect();
    assert!(video.windows(2).all(|pair| pair[1] > pair[0]));
    assert!(video[0] >= DELAY);
}

#[test]
fn a_player_joining_late_finds_the_programme_and_a_picture() {
    let packets = broadcast(&mut TsEncoder::new(false));
    let (demux, units) = demux(&packets[packets.len() / 2..]);
    let program = demux.program().expect("a programme");
    assert_eq!(program.streams.len(), 1);
    assert!(!decoded(&Codec::Mpeg2, &units, VIDEO_PID).is_empty());
}

#[test]
fn pictures_faster_than_the_frame_rate_are_thinned() {
    let mut encoder = TsEncoder::new(false);
    let mut out = Vec::new();
    for step in 0..100u64 {
        encoder
            .picture(&picture(32, 32, 0), step * CLOCK_HZ / 100, &mut out)
            .expect("picture");
    }
    let (_, units) = demux(&out);
    let pictures = units.iter().filter(|unit| unit.pid == VIDEO_PID).count();
    assert!(
        (24..=26).contains(&pictures),
        "{pictures} pictures in a second"
    );
}

#[test]
fn grey_pictures_and_new_sizes_are_coded() {
    let mut encoder = TsEncoder::new(false);
    let mut out = Vec::new();
    let grey = VideoPicture {
        width: 33,
        height: 21,
        luma: vec![128; 33 * 21],
        rgb: Vec::new(),
    };
    encoder.picture(&grey, 0, &mut out).expect("grey");
    encoder
        .picture(&picture(48, 32, 9), FRAME_TICKS, &mut out)
        .expect("a new size");
    let (_, units) = demux(&out);
    let sizes: Vec<_> = decoded(&Codec::Mpeg2, &units, VIDEO_PID)
        .into_iter()
        .filter_map(|payload| match payload {
            Payload::Video(picture) => Some((picture.width, picture.height)),
            _ => None,
        })
        .collect();
    assert_eq!(sizes, [(32, 20), (48, 32)]);
}

#[test]
fn coded_sizes_are_even_and_within_high_level() {
    assert_eq!(coded_size(&picture(65, 49, 0)), Ok((64, 48)));
    let uhd = VideoPicture {
        width: 3840,
        height: 2160,
        ..VideoPicture::default()
    };
    assert_eq!(coded_size(&uhd), Ok((1920, 1080)));
    assert!(coded_size(&picture(8, 8, 0)).is_err());
}

#[test]
fn a_picture_short_of_its_bytes_is_refused() {
    let mut broken = picture(32, 32, 0);
    broken.rgb.truncate(100);
    let mut out = Vec::new();
    assert!(TsEncoder::new(false).picture(&broken, 0, &mut out).is_err());
}
