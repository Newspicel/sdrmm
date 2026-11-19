//! The sigidwiki MPT 1327 recordings: discriminator audio from receivers on real control
//! channels, put back on an FM carrier and decoded whole.

use std::path::Path;

use sdrmm_decoder_test_support::frames::decode;
use sdrmm_modem::synth::fm_modulate;

use super::*;

const PEAK: f32 = 0.8;

fn recording(dir: &Path, stem: &str) -> Vec<Complex<f32>> {
    let path = dir.join(format!("{stem}.f32"));
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let audio: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|sample| f32::from_le_bytes(*sample))
        .collect();
    let peak = audio
        .iter()
        .fold(0f32, |peak, sample| peak.max(sample.abs()));
    let scaled: Vec<f32> = audio.iter().map(|sample| sample * PEAK / peak).collect();
    fm_modulate(&scaled, DEVIATION_HZ, RATE)
}

fn heard(dir: &Path, stem: &str) -> Vec<DvFrame> {
    decode(&mut channel(), &recording(dir, stem))
}

fn data(frames: &[DvFrame]) -> Vec<&str> {
    frames
        .iter()
        .filter_map(|frame| frame.data.as_deref())
        .collect()
}

fn systems(frames: &[DvFrame]) -> Vec<Option<u16>> {
    let mut systems: Vec<Option<u16>> = frames.iter().map(|frame| frame.system_id).collect();
    systems.dedup();
    systems
}

/// A site's control channel: Alohas, both of its parameter broadcasts and a registration
/// acknowledgement, all under the system identity its system codewords carry.
#[test]
fn a_control_channel_names_its_system() {
    let Some(dir) = sdrmm_test_support::reference_data::mpt1327_sigid_dir() else {
        return;
    };
    let frames = heard(&dir, "Pitlaptop2");
    assert_eq!(systems(&frames), [Some(0x5009)]);
    assert!(
        frames
            .iter()
            .all(|frame| frame.control_channel == Some(true))
    );
    let data = data(&frames);
    for codeword in [
        "80000401B800",
        "80000401B80A",
        "8A804C7204B4",
        "8E804C701000",
        "80FFCC203B20",
    ] {
        assert!(data.contains(&codeword), "{codeword} missing from {data:?}");
    }
    let ack = frames
        .iter()
        .find(|frame| frame.opcode.as_deref() == Some("ACK"))
        .expect("the acknowledgement");
    assert_eq!(ack.destination_call.as_deref(), Some("0/8185"));
    assert_eq!(ack.source_call.as_deref(), Some("0/473"));
}

/// Another site's control channel, which moves units to channel 38 and asks for a status.
#[test]
fn a_second_control_channel_names_its_own_system() {
    let Some(dir) = sdrmm_test_support::reference_data::mpt1327_sigid_dir() else {
        return;
    };
    let frames = heard(&dir, "MPT1327_Sound");
    assert_eq!(systems(&frames), [None, Some(0x5109)]);
    let moved = frames
        .iter()
        .find(|frame| frame.opcode.as_deref() == Some("MOVE"))
        .expect("a move");
    assert_eq!(moved.channel, Some(38));
    assert!(data(&frames).contains(&"80FE745BFA3F"));
}

/// The two 423.6 MHz recordings are traffic channels: SYNT messages only, calls on channels 288
/// and 336 cleared back to control channel 264. No codeword on a traffic channel carries a
/// system identity, so none is read.
#[test]
fn traffic_channels_name_the_channels_and_no_system() {
    let Some(dir) = sdrmm_test_support::reference_data::mpt1327_sigid_dir() else {
        return;
    };
    for stem in ["MPT1327_423.6_1", "MPT1327_423.6_2"] {
        let frames = heard(&dir, stem);
        assert!(frames.len() >= 20, "{stem}: {} frames", frames.len());
        assert_eq!(systems(&frames), [None], "{stem}");
        assert!(
            frames
                .iter()
                .all(|frame| frame.control_channel == Some(false)),
            "{stem}"
        );
        assert!(data(&frames).contains(&"A40844680AAA"), "{stem}");
        let clear = frames
            .iter()
            .find(|frame| frame.opcode.as_deref() == Some("CLEAR"))
            .expect("a clear down");
        assert_eq!(clear.channel, Some(264));
        let maintained: Vec<u16> = frames
            .iter()
            .filter(|frame| frame.opcode.as_deref() == Some("MAINT"))
            .filter_map(|frame| frame.channel)
            .collect();
        assert!(maintained.contains(&288), "{stem}");
        assert!(maintained.contains(&336), "{stem}");
    }
}

#[test]
fn a_traffic_channel_moves_a_call() {
    let Some(dir) = sdrmm_test_support::reference_data::mpt1327_sigid_dir() else {
        return;
    };
    let frames = heard(&dir, "MPT1327_442MHz");
    assert!(!frames.is_empty());
    assert!(
        data(&frames)
            .iter()
            .all(|codeword| *codeword == "DA0300023301")
    );
    assert_eq!(frames[0].channel, Some(4));
    assert_eq!(frames[0].destination_call.as_deref(), Some("90/96"));
}
