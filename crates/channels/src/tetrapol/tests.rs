use sdrmm_wire::{
    DecoderEvent, TetrapolAddressKind, TetrapolCallType, TetrapolLogicalChannel, TetrapolMessage,
    TetrapolPayload,
};

use super::{
    coding::{Block, DataBlock, Decoder, FRAME_BITS, encode_data, encode_voice, frame_bits},
    scrambling::Scrambling,
    *,
};
use crate::{
    synth::tetrapol as tx,
    testutil::{at_snr, cf32_le, complex_noise, frequency_shift, sample_clock_offset, settings},
};

const RATE: f64 = INPUT_RATE_HZ;

fn soft_frame(bits: &[u8; FRAME_BITS], previous: u8) -> [f32; FRAME_BITS] {
    std::array::from_fn(|k| {
        let before = if k == 0 { previous } else { bits[k - 1] };
        if bits[k] ^ before == 0 { 1.0 } else { -1.0 }
    })
}

fn sample_block() -> DataBlock {
    DataBlock {
        flag: 1,
        octets: [0x7F, 0xFF, 0x03, 0x35, 0x09, 0x55, 0x11, 0x89],
        asb: [0, 1],
    }
}

fn channel(params: TetrapolParams) -> TetrapolChannel {
    TetrapolChannel::new(
        ChannelCtx { input_rate: RATE },
        settings(ChannelParams::Tetrapol(params)),
    )
    .unwrap()
}

fn run(params: TetrapolParams, iq: &[Complex<f32>]) -> (Vec<TetrapolMessage>, u32) {
    let mut channel = channel(params);
    let mut out = ChannelOutputs::default();
    let mut filter = channel_filter();
    let mut filtered = Vec::new();
    for chunk in iq.chunks(2048) {
        filtered.clear();
        filter.process(chunk, &mut filtered);
        channel.process(&filtered, &mut out);
    }
    let messages = out
        .events
        .into_iter()
        .filter_map(|event| match event {
            DecoderEvent::Tetrapol(message) => Some(message),
            _ => None,
        })
        .collect();
    (messages, channel.rejected())
}

#[test]
fn a_data_block_survives_coding_on_both_bands() {
    for band in [TetrapolBand::Uhf, TetrapolBand::Vhf] {
        let bits = frame_bits(&encode_data(&sample_block(), band), band, 70);
        let mut frame = soft_frame(&bits, 0);
        frame[40] = -frame[40] * 0.2;
        let mut decoder = Decoder::new();
        let mut scrambling = Scrambling::new(None);
        let (decoded, code) = scrambling.decode(&mut decoder, &frame, band).unwrap();
        assert_eq!(code, 70, "{band:?}");
        assert_eq!(decoded.block, Block::Data(sample_block()), "{band:?}");
    }
}

#[test]
fn a_uhf_block_corrects_scattered_errors() {
    let bits = frame_bits(
        &encode_data(&sample_block(), TetrapolBand::Uhf),
        TetrapolBand::Uhf,
        115,
    );
    let mut frame = soft_frame(&bits, 0);
    for k in [20, 61, 103, 140] {
        frame[k] = -frame[k] * 0.3;
    }
    let mut scrambling = Scrambling::new(Some(115));
    let (decoded, _) = scrambling
        .decode(&mut Decoder::new(), &frame, TetrapolBand::Uhf)
        .unwrap();
    assert_eq!(decoded.block, Block::Data(sample_block()));
}

#[test]
fn voice_frames_need_a_settled_scrambling_code() {
    let e = encode_voice(&[1; 22], &[0; 100], TetrapolBand::Uhf);
    let frame = soft_frame(&frame_bits(&e, TetrapolBand::Uhf, 118), 0);
    let mut decoder = Decoder::new();
    let mut scrambling = Scrambling::new(None);
    assert!(
        scrambling
            .decode(&mut decoder, &frame, TetrapolBand::Uhf)
            .is_none()
    );
    let (decoded, code) = scrambling
        .decode(&mut decoder, &frame, TetrapolBand::Uhf)
        .unwrap();
    assert_eq!(code, 118);
    assert!(matches!(decoded.block, Block::Voice { .. }));
}

#[test]
fn noise_never_passes_as_a_block() {
    let noise = complex_noise(7, 1.0, 200 * FRAME_BITS);
    let mut decoder = Decoder::new();
    let mut scrambling = Scrambling::new(None);
    let passes = noise
        .as_chunks::<FRAME_BITS>()
        .0
        .iter()
        .filter(|chunk| {
            let frame: [f32; FRAME_BITS] = std::array::from_fn(|k| chunk[k].re);
            scrambling
                .decode(&mut decoder, &frame, TetrapolBand::Uhf)
                .is_some()
        })
        .count();
    assert_eq!(passes, 0);
}

#[test]
fn recorded_signalling_parses() {
    let activation = message::payload(&[0x55, 0x11, 0x89, 0xC8, 0x0B, 0x4A, 0x7D, 0x59, 0x4A]);
    let Some(TetrapolPayload::GroupActivation(call)) = activation else {
        panic!("{activation:?}");
    };
    assert_eq!(
        (call.group, call.coverage, call.channel),
        (393, 200, Some(2890))
    );
    assert_eq!(call.call_type, TetrapolCallType::Internal);
    assert_eq!(
        (call.uplink_scrambling, call.downlink_scrambling),
        (Some(125), Some(89))
    );
    assert!(!call.key.clear());
    let info = message::payload(&[
        0x90, 0x00, 0x40, 0x03, 0x35, 0x00, 0x1C, 0x07, 0x22, 0x80, 0x41, 0x00, 0x63, 0xBB, 0x00,
        0x08, 0x46,
    ]);
    let Some(TetrapolPayload::SystemInfo(info)) = info else {
        panic!("{info:?}");
    };
    assert_eq!(info.cell.label(), "28.2.7");
    assert_eq!((info.country, info.network), (Some(3), Some(3)));
    assert_eq!(info.superframe, Some(2118));
    assert_eq!(info.uplink_scrambling, 65);
    assert_eq!(
        message::payload(&[0x58, 0x12]),
        Some(TetrapolPayload::GroupIdle { cause: 0x12 })
    );
    assert_eq!(
        message::payload(&[0x94, 0x07]),
        Some(TetrapolPayload::Other {
            codop: 0x94,
            name: "neighbouring cells".to_owned()
        })
    );
    let all = message::address(0x7F, 0xFF).unwrap();
    assert_eq!(all.kind, TetrapolAddressKind::AllStations);
    assert!(message::address(0x70, 0x00).is_none());
    assert_eq!(
        message::address(0x01, 0x8F).map(|a| (a.kind, a.id)),
        Some((TetrapolAddressKind::Group, 399))
    );
}

fn control(superframes: u16) -> Vec<Complex<f32>> {
    tx::control_channel(RATE, superframes)
}

#[test]
fn a_control_channel_decodes_off_centre_in_noise() {
    let iq = control(2);
    let iq = frequency_shift(&iq, 600.0, RATE);
    let iq = sample_clock_offset(&iq, 20.0);
    let iq = at_snr(&iq, 14.0, 0x7e7a);
    let (messages, rejected) = run(TetrapolParams::default(), &iq);
    let info = messages
        .iter()
        .find_map(|m| match &m.payload {
            TetrapolPayload::SystemInfo(info) => Some((m, info)),
            _ => None,
        })
        .expect("system info");
    assert_eq!(info.1.cell.label(), "28.2.7");
    assert_eq!(info.0.scrambling, 70);
    let groups: Vec<(u16, Option<u16>)> = messages
        .iter()
        .filter_map(|m| match &m.payload {
            TetrapolPayload::GroupActivation(call) => Some((call.group, call.channel)),
            _ => None,
        })
        .collect();
    assert!(groups.contains(&(393, Some(2890))), "{groups:?}");
    assert!(groups.contains(&(171, Some(2862))), "{groups:?}");
    let paging = messages
        .iter()
        .find(|m| matches!(m.payload, TetrapolPayload::Paging { .. }))
        .expect("paging");
    assert_eq!(paging.channel, TetrapolLogicalChannel::Paging);
    assert!(matches!(paging.frame, Some(98 | 198)), "{paging:?}");
    assert!(rejected <= 2, "{rejected}");
}

#[test]
fn repeated_messages_are_reported_once() {
    let (messages, _) = run(TetrapolParams::default(), &control(3));
    let activations = messages
        .iter()
        .filter(
            |m| matches!(&m.payload, TetrapolPayload::GroupActivation(call) if call.group == 393),
        )
        .count();
    assert_eq!(activations, 1);
}

#[test]
fn a_traffic_channel_reports_idle_and_voice() {
    let bursts = tx::traffic(60, 150);
    let iq = tx::transmission(&bursts, TetrapolBand::Uhf, 118, RATE);
    let iq = at_snr(&iq, 16.0, 0x118);
    let (messages, rejected) = run(TetrapolParams::default(), &iq);
    assert!(
        messages
            .iter()
            .any(|m| m.payload == TetrapolPayload::GroupIdle { cause: 0x12 }),
        "{messages:?}"
    );
    let voice = messages
        .iter()
        .find_map(|m| match m.payload {
            TetrapolPayload::Voice { seconds } => Some((m, seconds)),
            _ => None,
        })
        .expect("voice");
    assert_eq!(voice.0.channel, TetrapolLogicalChannel::Traffic);
    assert_eq!(voice.0.scrambling, 118);
    assert!((voice.1 - 3.0).abs() < 0.1, "{}", voice.1);
    assert_eq!(rejected, 0);
}

#[test]
fn a_vhf_channel_with_a_fixed_code_decodes() {
    let bursts = tx::control_superframe(&tx::Cell::default(), &[tx::GroupCall::default()], 7);
    let iq = tx::transmission(&bursts, TetrapolBand::Vhf, 21, RATE);
    let iq = at_snr(&iq, 18.0, 0x21);
    let params = TetrapolParams {
        band: TetrapolBand::Vhf,
        scrambling: Some(21),
    };
    let (messages, _) = run(params, &iq);
    assert!(
        messages.iter().any(
            |m| matches!(&m.payload, TetrapolPayload::GroupActivation(call) if call.group == 393)
        ),
        "{messages:?}"
    );
}

#[test]
fn noise_alone_stays_silent() {
    let iq = complex_noise(0x5eed, 1.0, RATE as usize * 4);
    let (messages, rejected) = run(TetrapolParams::default(), &iq);
    assert!(messages.is_empty(), "{messages:?}");
    assert_eq!(rejected, 0);
}

#[test]
fn an_out_of_range_scrambling_code_is_refused() {
    let result = TetrapolChannel::new(
        ChannelCtx { input_rate: RATE },
        settings(ChannelParams::Tetrapol(TetrapolParams {
            scrambling: Some(200),
            ..TetrapolParams::default()
        })),
    );
    assert!(result.is_err());
}

#[test]
fn a_recorded_control_channel_decodes() {
    let iq = cf32_le(include_bytes!(
        "../../../../fixtures/tetrapol_control_48k.sigmf-data"
    ));
    let (messages, rejected) = run(TetrapolParams::default(), &iq);
    let info = messages
        .iter()
        .find_map(|m| match &m.payload {
            TetrapolPayload::SystemInfo(info) => Some((m, info)),
            _ => None,
        })
        .expect("system info");
    assert_eq!(info.0.channel, TetrapolLogicalChannel::Broadcast);
    assert_eq!(info.0.frame, Some(0));
    assert_eq!(info.0.scrambling, 70);
    assert_eq!(info.1.cell.label(), "28.2.7");
    assert_eq!(info.1.superframe, Some(2118));
    assert!(messages.iter().any(|m| matches!(
        &m.payload,
        TetrapolPayload::GroupActivation(call) if call.group == 393 && call.channel == Some(2890)
    )));
    assert!(
        messages
            .iter()
            .any(|m| matches!(&m.payload, TetrapolPayload::Other { codop: 0x94, .. }))
    );
    let paged = messages
        .iter()
        .find_map(|m| match &m.payload {
            TetrapolPayload::Paging { terminals } => Some(terminals),
            _ => None,
        })
        .expect("paging");
    assert_eq!(
        paged.iter().map(|a| a.id).collect::<Vec<_>>(),
        vec![125, 399]
    );
    assert!(rejected <= 2, "{rejected}");
}

#[test]
fn a_recorded_traffic_channel_reports_idle_and_voice() {
    let mut iq = cf32_le(include_bytes!(
        "../../../../fixtures/tetrapol_traffic_48k.sigmf-data"
    ));
    iq.extend(complex_noise(0x7c4, 0.01, RATE as usize));
    let (messages, rejected) = run(TetrapolParams::default(), &iq);
    let idle = messages
        .iter()
        .find(|m| m.payload == TetrapolPayload::GroupIdle { cause: 0x12 })
        .expect("group idle");
    assert_eq!(idle.scrambling, 115);
    let voice = messages
        .iter()
        .find_map(|m| match m.payload {
            TetrapolPayload::Voice { seconds } => Some((m.scrambling, seconds)),
            _ => None,
        })
        .expect("voice");
    assert_eq!(voice.0, 118);
    assert!(voice.1 > 1.0, "{}", voice.1);
    assert!(rejected <= 3, "{rejected}");
}
