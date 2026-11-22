use std::time::{Duration, Instant};

use sdrmm_decoder::{
    ChannelCtx, ChannelFront, ChannelOutputs,
    broadcast::{BroadcastStatus, latest_status},
    create, input_rate,
};
use sdrmm_wire::ChannelParams;

use super::{HdRadioParams, fixture::kut, receiver::Receiver};

const MEDIA_WAIT: Duration = Duration::from_secs(10);

fn receive() -> Receiver {
    let iq = kut();
    let mut receiver = Receiver::new();
    let mut rest = iq.as_slice();
    while !rest.is_empty() {
        let (taken, _) = receiver.push(rest);
        rest = &rest[taken..];
    }
    receiver
}

#[test]
fn the_station_names_itself_in_crc_valid_pids_frames() {
    let receiver = receive();
    let station = &receiver.station;
    assert!(receiver.locked());
    assert_eq!(station.frames_bad, 0);
    assert!(station.frames_ok >= 20, "{} PIDS frames", station.frames_ok);
    assert_eq!(station.call_sign.as_deref(), Some("KUT"));
    assert_eq!(
        station.long_name.as_deref(),
        Some("The University of Texas at Austin")
    );
    assert_eq!(station.facility_id, Some(21));
    assert!(receiver.mer_db() > 10.0, "MER {} dB", receiver.mer_db());
}

#[test]
fn a_p1_frame_carries_both_programs_hdc_packets() {
    let receiver = receive();
    assert_eq!((receiver.p1_ok, receiver.p1_bad), (1, 0));
    assert!(receiver.bit_error_rate() < 1e-3);
    let frame = &receiver.frame;
    assert_eq!(frame.packets_bad, 0);
    let packets: Vec<(usize, &[u8])> = frame.packets().collect();
    assert_eq!(
        packets.iter().filter(|(program, _)| *program == 0).count(),
        32
    );
    assert!(packets.iter().filter(|(program, _)| *program == 1).count() >= 30);
    assert!(
        packets
            .iter()
            .all(|(_, packet)| sdrmm_codec::aac::hdc::stereo(packet) == Ok(true))
    );
    assert_eq!(frame.programs[1].program_type, 22);
}

fn take(out: &mut ChannelOutputs, pcm: &mut Vec<f32>, status: &mut Option<BroadcastStatus>) {
    pcm.extend_from_slice(&out.audio_pcm);
    if let Some(latest) = latest_status(out) {
        *status = Some(latest);
    }
    out.reset();
}

fn play(params: HdRadioParams, frames: u32) -> (BroadcastStatus, Vec<f32>) {
    let params = ChannelParams::new("hd_radio", &params).expect("hd radio settings");
    let settings = sdrmm_decoder_test_support::settings(params);
    let rate = input_rate(&settings.params);
    let mut front = ChannelFront::new(&settings.params, rate, 0.0).expect("front");
    let mut channel = create(ChannelCtx { input_rate: rate }, &settings).expect("builds");
    let mut filtered = Vec::new();
    let mut out = ChannelOutputs::default();
    let mut pcm = Vec::new();
    let mut status = None;
    for block in kut().chunks(4_096) {
        front.process(block, &mut filtered);
        channel.process(&filtered, &mut out);
        take(&mut out, &mut pcm, &mut status);
    }
    let silence = vec![num_complex::Complex::default(); 51_840];
    let started = Instant::now();
    while started.elapsed() < MEDIA_WAIT {
        channel.process(&silence, &mut out);
        take(&mut out, &mut pcm, &mut status);
        if status
            .as_ref()
            .is_some_and(|status| status.audio_frames_ok + status.audio_frames_bad >= frames)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    (status.expect("a broadcast status"), pcm)
}

#[test]
fn the_scope_sees_four_qpsk_clusters_off_air() {
    let params = ChannelParams::new("hd_radio", &HdRadioParams::default()).expect("settings");
    let settings = sdrmm_decoder_test_support::settings(params);
    let rate = input_rate(&settings.params);
    let mut front = ChannelFront::new(&settings.params, rate, 0.0).expect("front");
    let mut channel = create(ChannelCtx { input_rate: rate }, &settings).expect("builds");
    let mut filtered = Vec::new();
    let mut out = ChannelOutputs::default();
    out.symbols.set_wanted(true);
    for block in kut().chunks(4_096) {
        front.process(block, &mut filtered);
        channel.process(&filtered, &mut out);
    }
    let tap = &out.symbols;
    assert_eq!(tap.plane, Some(sdrmm_wire::SymbolPlane::Carriers));
    assert_eq!(tap.reference.len(), 8);
    assert!(
        tap.symbols.len() >= 2 * 4 * 360,
        "{} values",
        tap.symbols.len()
    );
    assert!(tap.evm < 0.3, "EVM {}", tap.evm);
    assert!(
        (tap.symbol_rate - 360.0 * rate / 1620.0).abs() < 1.0,
        "{}",
        tap.symbol_rate
    );
}

#[test]
fn hd1_plays_as_aac_through_fdk() {
    let (status, pcm) = play(HdRadioParams::default(), 32);
    assert_eq!(status.audio_error, None);
    assert_eq!(status.audio_frames_bad, 0);
    assert!(
        status.audio_frames_ok >= 32,
        "{} frames",
        status.audio_frames_ok
    );
    let rms = (pcm.iter().map(|v| v * v).sum::<f32>() / pcm.len().max(1) as f32).sqrt();
    assert!(rms > 0.01, "audio RMS {rms}");
    assert_eq!(status.ensemble_label.as_deref(), Some("KUT"));
    assert_eq!(status.label.as_deref(), Some("HD1"));
    assert!(
        status
            .bitrate_kbps
            .is_some_and(|kbps| (48..=96).contains(&kbps)),
        "{:?} kbit/s",
        status.bitrate_kbps
    );
}

#[test]
fn hd2_plays_as_its_own_program() {
    let (status, pcm) = play(HdRadioParams { service: Some(1) }, 31);
    assert_eq!(status.audio_error, None);
    assert!(
        status.audio_frames_ok >= 31,
        "{} frames",
        status.audio_frames_ok
    );
    assert!(pcm.iter().any(|v| v.abs() > 0.01));
    assert_eq!(status.label.as_deref(), Some("HD2 Public"));
}
