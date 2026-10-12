use num_complex::Complex;
use sdrmm_wire::{ChannelParams, DecoderEvent, DvFrame, DvFrameKind, DvMode, TetraParams};

use super::{INPUT_RATE_HZ, TetraChannel, cmce, sds::Payload};
use crate::{
    ChannelCtx, ChannelRx,
    synth::tetra::{self as tx, Cell},
    testutil::{at_snr, frequency_shift, run_events, sample_clock_offset, settings},
};

fn channel() -> TetraChannel {
    TetraChannel::new(
        ChannelCtx {
            input_rate: INPUT_RATE_HZ,
        },
        settings(ChannelParams::Tetra(TetraParams::default())),
    )
    .unwrap()
}

fn decode(iq: &[Complex<f32>]) -> (Vec<DvFrame>, u32) {
    let mut channel = channel();
    let frames = run_events(&mut channel, iq)
        .into_iter()
        .map(|event| match event {
            DecoderEvent::Dv(frame) => frame,
            other => panic!("unexpected event {other:?}"),
        })
        .collect();
    (frames, channel.rejected())
}

fn opcode<'a>(frames: &'a [DvFrame], name: &str) -> &'a DvFrame {
    frames
        .iter()
        .find(|frame| frame.opcode.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no {name} in {frames:#?}"))
}

fn sample() -> Vec<Complex<f32>> {
    tx::sample(Cell::default()).modulate()
}

fn assert_full_scenario(frames: &[DvFrame]) {
    let cell = Cell::default();
    assert!(frames.iter().all(|frame| frame.mode == DvMode::Tetra));
    let sysinfo = opcode(frames, "SYSINFO");
    assert_eq!(sysinfo.system_id, Some(cell.mcc));
    assert_eq!(sysinfo.network_id, Some(u32::from(cell.mnc)));
    assert_eq!(sysinfo.color_code, Some(u16::from(cell.colour)));
    assert_eq!(sysinfo.site_id, Some(cell.location_area));
    assert_eq!(sysinfo.encrypted, Some(false));
    let definition = sysinfo.channel_definition.clone().unwrap();
    assert_eq!(definition.tx_hz, cell.downlink_hz());
    assert_eq!(definition.rx_hz, cell.downlink_hz() - 10_000_000);

    let setup = opcode(frames, "D-SETUP");
    assert_eq!(setup.kind, DvFrameKind::Header);
    assert_eq!(setup.destination, Some(tx::GROUP));
    assert_eq!(setup.source, Some(tx::CALLER));
    assert_eq!(setup.group_call, Some(true));
    assert_eq!(setup.slot, Some(1));

    let voice = frames
        .iter()
        .find(|frame| frame.kind == DvFrameKind::Voice)
        .expect("voice");
    assert_eq!(voice.slot, Some(tx::TRAFFIC_SLOT));
    assert_eq!(voice.destination, Some(tx::GROUP));
    assert_eq!(voice.source, Some(tx::CALLER));

    let granted = opcode(frames, "D-TX GRANTED");
    assert_eq!(granted.source, Some(tx::TALKER));
    assert_eq!(granted.destination, Some(tx::GROUP));

    let texts: Vec<&str> = frames
        .iter()
        .filter_map(|frame| frame.text.as_deref())
        .collect();
    assert_eq!(texts, [tx::TEXT, tx::LONG_TEXT]);
    let position = frames
        .iter()
        .find(|frame| frame.lat.is_some())
        .expect("position");
    assert!((position.lat.unwrap() - tx::POSITION.0).abs() < 1e-4);
    assert!((position.lon.unwrap() - tx::POSITION.1).abs() < 1e-4);
    assert_eq!(position.position_error_m, Some(20));

    let release = opcode(frames, "D-RELEASE");
    assert_eq!(release.kind, DvFrameKind::Terminator);
    assert_eq!(release.source, Some(tx::TALKER));
    opcode(frames, "D-LOCATION UPDATE ACCEPT");
}

#[test]
fn a_clean_downlink_reports_cell_call_and_messages() {
    let (frames, rejected) = decode(&sample());
    assert_full_scenario(&frames);
    assert_eq!(rejected, 0);
    assert_eq!(
        frames
            .iter()
            .filter(|frame| frame.kind == DvFrameKind::Voice)
            .count(),
        1
    );
}

#[test]
fn survives_frequency_offset_clock_drift_and_noise() {
    let iq = sample_clock_offset(&frequency_shift(&sample(), 700.0, INPUT_RATE_HZ), 40.0);
    let iq = at_snr(&iq, 14.0, 7);
    let (frames, _) = decode(&iq);
    assert_full_scenario(&frames);
}

#[test]
fn an_inverted_spectrum_still_decodes() {
    let iq: Vec<Complex<f32>> = sample().iter().map(Complex::conj).collect();
    let (frames, _) = decode(&iq);
    assert_full_scenario(&frames);
}

#[test]
fn noise_yields_nothing() {
    let iq = crate::testutil::complex_noise(9, 0.3, 200_000);
    let (frames, rejected) = decode(&iq);
    assert!(frames.is_empty(), "{frames:#?}");
    assert_eq!(rejected, 0);
}

#[test]
fn broken_control_blocks_are_counted() {
    let mut iq = sample();
    let len = iq.len();
    for sample in &mut iq[len / 2..len / 2 + 600] {
        *sample = Complex::new(0.0, 0.0);
    }
    let (_, rejected) = decode(&iq);
    assert!(rejected > 0);
}

#[test]
fn a_group_setup_names_its_caller() {
    let bits = crate::synth::tetra::Bits::new()
        .field(7, 5)
        .field(77, 14)
        .field(0, 6)
        .field(0b0001_0100, 8)
        .field(0, 7)
        .field(1, 1)
        .field(0, 2)
        .field(1, 1)
        .field(1, 2)
        .field(4_711, 24)
        .into_vec();
    let setup = cmce::parse(&bits).unwrap();
    assert_eq!(setup.call_id, Some(77));
    assert_eq!(setup.group, Some(true));
    assert_eq!(setup.encrypted, Some(true));
    assert_eq!(setup.party, Some(4_711));
}

#[test]
fn a_status_message_carries_its_code() {
    let bits = crate::synth::tetra::Bits::new()
        .field(8, 5)
        .field(1, 2)
        .field(99, 24)
        .field(0x8002, 16)
        .into_vec();
    let status = cmce::parse(&bits).unwrap();
    assert_eq!(status.party, Some(99));
    assert_eq!(status.payload, Some(Payload::Status(0x8002)));
}

fn recording(bytes: &[u8], rate: f64, offset_hz: f64) -> Vec<Complex<f32>> {
    let iq: Vec<Complex<f32>> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|s| {
            Complex::new(
                f32::from(i16::from_le_bytes([s[0], s[1]])) / 32_768.0,
                f32::from(i16::from_le_bytes([s[2], s[3]])) / 32_768.0,
            )
        })
        .collect();
    let iq = crate::synth::resample(&iq, rate, INPUT_RATE_HZ);
    frequency_shift(&iq, -offset_hz, INPUT_RATE_HZ)
}

fn assert_russian_cell(frames: &[DvFrame]) {
    let sysinfo = opcode(frames, "SYSINFO");
    assert_eq!(sysinfo.system_id, Some(250));
    assert_eq!(sysinfo.network_id, Some(13));
    assert_eq!(sysinfo.color_code, Some(0x2C));
    assert_eq!(sysinfo.site_id, Some(1_052));
    assert_eq!(sysinfo.channel, Some(2_716));
    assert_eq!(
        sysinfo
            .channel_definition
            .as_ref()
            .map(|definition| definition.tx_hz),
        Some(467_912_500)
    );
}

#[test]
fn a_recorded_control_channel_names_its_cell() {
    let iq = recording(
        include_bytes!("../../../../fixtures/tetra_scbs_offair_50k.sigmf-data"),
        50_000.0,
        -1_400.0,
    );
    let (frames, rejected) = decode(&iq);
    assert_russian_cell(&frames);
    assert_eq!(rejected, 0);
}

#[test]
fn a_weak_recorded_control_channel_still_decodes() {
    let iq = recording(
        include_bytes!("../../../../fixtures/tetra_weak_offair_144k.sigmf-data"),
        144_000.0,
        -900.0,
    );
    let (frames, rejected) = decode(&iq);
    assert_russian_cell(&frames);
    assert!(rejected <= 2, "{rejected} rejected");
}

#[test]
fn a_recorded_group_call_names_its_parties() {
    let iq = recording(
        include_bytes!("../../../../fixtures/tetra_call_offair_48k.sigmf-data"),
        48_000.0,
        300.0,
    );
    let (frames, rejected) = decode(&iq);
    assert_eq!(rejected, 0);
    let sysinfo = opcode(&frames, "SYSINFO");
    assert_eq!(
        (sysinfo.system_id, sysinfo.network_id),
        (Some(228), Some(8_889))
    );
    let setup = opcode(&frames, "D-SETUP");
    assert_eq!(
        (setup.source, setup.destination),
        (Some(100_701), Some(100_601))
    );
    assert_eq!(setup.group_call, Some(true));
    let granted = frames
        .iter()
        .find(|frame| {
            frame.opcode.as_deref() == Some("D-TX GRANTED") && frame.destination == Some(100_601)
        })
        .expect("grant");
    assert_eq!(granted.source, Some(100_702));
    assert!(frames.iter().any(|frame| frame.kind == DvFrameKind::Voice));
}
