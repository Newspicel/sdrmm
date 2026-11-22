use super::*;
use sdrmm_decoder_test_support::{cf32_le, settings};
use sdrmm_modem::synth::{fm_modulate, silence};
use sdrmm_wire::ChannelParams;

const LEVEL: f32 = 0.7;
const ANI: [u8; 6] = [0xFE, 0x80, 0x08, 0x30, 0x53, 0x05];

fn messages(iq: &[Complex<f32>]) -> (Vec<FleetSyncMessage>, u32) {
    let mut channel = FleetSyncChannel::new(
        ChannelCtx { input_rate: RATE },
        settings(ChannelParams::of(&FleetSyncParams::default()).expect("fleetsync settings")),
    )
    .expect("fleetsync channel");
    let mut filtered = Vec::new();
    sdrmm_decoder_test_support::built_filter(channel_filter(), RATE).process(iq, &mut filtered);
    let mut out = ChannelOutputs::default();
    for block in filtered.chunks(1_021) {
        channel.process(block, &mut out);
    }
    let messages = out
        .events
        .iter()
        .filter_map(DecoderEvent::read::<FleetSyncMessage>)
        .collect();
    (messages, channel.rejected())
}

fn msb(word: u64, bits: u32) -> impl Iterator<Item = bool> {
    (0..bits).rev().map(move |k| word >> k & 1 == 1)
}

fn transmission(tones: FfskTones, blocks: &[(u32, u32)]) -> Vec<Complex<f32>> {
    let mut bits: Vec<bool> = (0..32).map(|k| k % 2 == 0).collect();
    bits.extend(msb(SYNC_WORDS[0], SYNC_BITS));
    for &(first, second) in blocks {
        bits.extend(msb(u64::from(first), 32));
        bits.extend(msb(u64::from(second), 32));
    }
    bits.extend((0..160).map(|k| k % 3 == 0));
    let mut iq = silence(4_800);
    iq.extend(fm_modulate(
        &tones.audio(&bits, LEVEL, RATE),
        DEVIATION_HZ,
        RATE,
    ));
    iq.extend(silence(9_600));
    iq
}

fn assert_ani(messages: &[FleetSyncMessage], version: FleetSyncVersion) {
    let ends: Vec<Option<bool>> = messages.iter().map(FleetSyncMessage::end).collect();
    assert_eq!(ends, [Some(false), Some(true)], "{messages:?}");
    for message in messages {
        assert_eq!(message.version, version);
        assert_eq!(message.baud, 1_200);
        assert_eq!(message.kind(), "ANI");
        assert_eq!(
            (message.from_fleet, message.from_unit),
            (Some(107), Some(1772))
        );
        assert!(!message.truncated);
    }
}

#[test]
fn decodes_fleetsync_i_ani_off_air() {
    let iq = cf32_le(include_bytes!(
        "../../../../fixtures/fleetsync1_fleet107_offair_48k.sigmf-data"
    ));
    assert_ani(&messages(&iq).0, FleetSyncVersion::One);
}

#[test]
fn decodes_fleetsync_ii_ani_off_air() {
    let iq = cf32_le(include_bytes!(
        "../../../../fixtures/fleetsync2_fleet107_offair_48k.sigmf-data"
    ));
    assert_ani(&messages(&iq).0, FleetSyncVersion::Two);
}

#[test]
fn hears_fleetsync_ii_at_2400_baud() {
    let blocks = frame::encode_fs2(
        u32::from_be_bytes([ANI[0], ANI[1], ANI[2], ANI[3]]),
        frame::encode(ANI).1,
    );
    let (messages, _) = messages(&transmission(FfskTones::MSK_2400, &blocks));
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert_eq!(messages[0].baud, 2_400);
    assert_eq!(messages[0].version, FleetSyncVersion::Two);
    assert_eq!(messages[0].sender(), "107-1772");
}

#[test]
fn the_scope_follows_the_lane_that_found_a_2400_baud_sync() {
    let blocks = frame::encode_fs2(
        u32::from_be_bytes([ANI[0], ANI[1], ANI[2], ANI[3]]),
        frame::encode(ANI).1,
    );
    let iq = transmission(FfskTones::MSK_2400, &blocks);
    let mut channel = FleetSyncChannel::new(
        ChannelCtx { input_rate: RATE },
        settings(ChannelParams::of(&FleetSyncParams::default()).expect("fleetsync settings")),
    )
    .expect("fleetsync channel");
    let mut out = ChannelOutputs::default();
    out.symbols.set_wanted(true);
    let mut best = f32::INFINITY;
    for block in iq[4_800..iq.len() - 9_600].chunks(1_021) {
        out.reset();
        channel.process(block, &mut out);
        let tap = &out.symbols;
        assert_eq!(tap.plane, Some(sdrmm_wire::SymbolPlane::Level));
        assert_eq!(tap.reference.len(), 2);
        assert!(!tap.symbols.is_empty());
        if tap.symbol_rate == 2_400.0 {
            best = best.min(tap.evm);
        }
    }
    assert!(best < 0.2, "best 2400 baud EVM {best}");
}

#[test]
fn a_text_message_reads_through_its_blocks() {
    let blocks = [
        frame::encode([0x00, 0x42, 0x08, 0x30, 0x53, 0x05]),
        frame::encode([0x00, 0x00, 0x00, 0x00, 0x05, 0x00]),
        frame::encode(*b"OLLEH "),
    ];
    let (messages, rejected) = messages(&transmission(FfskTones::CCIR_1200, &blocks));
    assert_eq!(rejected, 0);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert_eq!(messages[0].kind(), "Message");
    assert_eq!(messages[0].text.as_deref(), Some(" HELL"));
}

#[test]
fn a_cut_message_surfaces_as_truncated() {
    let blocks = [
        frame::encode([0x00, 0x42, 0x08, 0x30, 0x53, 0x05]),
        frame::encode([0x00, 0x00, 0x00, 0x00, 0x09, 0x00]),
        frame::encode(*b"OLLEH "),
    ];
    let (messages, _) = messages(&transmission(FfskTones::CCIR_1200, &blocks));
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].truncated);
    assert_eq!(messages[0].text.as_deref(), Some(" HELLO"));
}

#[test]
fn noise_produces_nothing() {
    let iq = sdrmm_decoder_test_support::complex_noise(11, 0.5, 96_000);
    let (messages, _) = messages(&iq);
    assert!(messages.is_empty(), "{messages:?}");
}
