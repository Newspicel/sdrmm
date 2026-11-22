use super::*;
use sdrmm_decoder_test_support::{cf32_le, settings};
use sdrmm_modem::synth::{fm_modulate, silence};
use sdrmm_wire::ChannelParams;

const LEADER_BYTES: usize = 7;
const LEVEL: f32 = 0.7;

fn channel() -> MdcChannel {
    MdcChannel::new(
        ChannelCtx { input_rate: RATE },
        settings(ChannelParams::of(&MdcParams::default()).expect("mdc settings")),
    )
    .expect("mdc channel")
}

fn packets(out: &ChannelOutputs) -> Vec<MdcPacket> {
    out.events
        .iter()
        .filter_map(DecoderEvent::read::<MdcPacket>)
        .collect()
}

fn decode_iq(iq: &[Complex<f32>]) -> (Vec<MdcPacket>, u32) {
    let mut channel = channel();
    let mut filtered = Vec::new();
    sdrmm_decoder_test_support::built_filter(channel_filter(), RATE).process(iq, &mut filtered);
    let mut out = ChannelOutputs::default();
    for block in filtered.chunks(1_021) {
        channel.process(block, &mut out);
    }
    (packets(&out), channel.rejected())
}

/// The data bits on air as the tones that send them: a space flips the line, a mark keeps it.
fn keyed(data: &[bool]) -> Vec<bool> {
    let mut level = false;
    data.iter()
        .map(|&bit| {
            let mark = bit == level;
            level = bit;
            mark
        })
        .collect()
}

fn msb(word: u64, bits: u32) -> impl Iterator<Item = bool> {
    (0..bits).rev().map(move |k| word >> k & 1 == 1)
}

fn transmission(blocks: &[[bool; BLOCK_BITS]]) -> Vec<Complex<f32>> {
    let mut data: Vec<bool> = (0..LEADER_BYTES * 8).map(|k| k % 2 == 1).collect();
    data.extend(msb(SYNC, SYNC_BITS));
    for block in blocks {
        data.extend_from_slice(block);
    }
    data.extend([false; 16]);
    let audio = FfskTones::CCIR_1200.audio(&keyed(&data), LEVEL, RATE);
    let mut iq = silence(4_800);
    iq.extend(fm_modulate(&audio, DEVIATION_HZ, RATE));
    iq.extend(silence(4_800));
    iq
}

#[test]
fn decodes_an_end_of_transmission_ptt_id_off_air() {
    let iq = cf32_le(include_bytes!(
        "../../../../fixtures/mdc1200_unit1777_offair_48k.sigmf-data"
    ));
    let (packets, _) = decode_iq(&iq);
    assert_eq!(packets.len(), 1, "{packets:?}");
    let packet = &packets[0];
    assert_eq!((packet.unit, packet.op, packet.arg), (0x1777, 0x01, 0x00));
    assert_eq!(packet.meaning(), Some("PTT ID end"));
    assert!(packet.extra.is_none() && !packet.truncated);
}

#[test]
fn decodes_the_reference_encoder_audio() {
    let audio: Vec<f32> = include_bytes!("../../../../fixtures/mdc1200_ref_01_80_1234_48k.s16")
        .chunks_exact(2)
        .map(|pair| f32::from(i16::from_le_bytes([pair[0], pair[1]])) / 32_768.0)
        .collect();
    let mut channel = channel();
    let mut out = ChannelOutputs::default();
    for block in audio.chunks(997) {
        channel.audio(block, &mut out);
    }
    let packets = packets(&out);
    assert_eq!(packets.len(), 1, "{packets:?}");
    assert_eq!(
        (packets[0].unit, packets[0].op, packets[0].arg),
        (0x1234, 0x01, 0x80)
    );
}

#[test]
fn the_scope_sees_two_clean_levels_at_1200_baud() {
    let iq = transmission(&[frame::encode(0x01, 0x80, 0x1234)]);
    let (warm_up, signal) = iq[4_800..iq.len() - 4_800].split_at(2_400);
    let mut channel = channel();
    let mut out = ChannelOutputs::default();
    out.symbols.set_wanted(true);
    channel.process(warm_up, &mut out);
    out.reset();
    for block in signal.chunks(1_021) {
        channel.process(block, &mut out);
    }
    let tap = &out.symbols;
    assert_eq!(tap.plane, Some(sdrmm_wire::SymbolPlane::Level));
    assert_eq!(tap.reference.len(), 2);
    assert!(tap.symbols.len() > 150, "{} symbols", tap.symbols.len());
    assert_eq!(tap.symbol_rate, 1_200.0);
    assert!(tap.evm < 0.2, "EVM {}", tap.evm);
}

#[test]
fn a_double_packet_carries_its_second_block() {
    let iq = transmission(&[
        frame::encode(0x35, 0x89, 0x5678),
        frame::encode(0x83, 0x0D, 0x1234),
    ]);
    let (packets, rejected) = decode_iq(&iq);
    assert_eq!(rejected, 0);
    assert_eq!(packets.len(), 1, "{packets:?}");
    assert_eq!(packets[0].extra, Some([0x83, 0x0D, 0x12, 0x34]));
    assert_eq!(packets[0].meaning(), Some("Call alert"));
    assert_eq!(packets[0].caller(), Some(0x1234));
}

#[test]
fn a_lost_second_block_surfaces_as_truncated() {
    let mut garbage = frame::encode(0x83, 0x0D, 0x1234);
    for bit in garbage.iter_mut().take(60) {
        *bit = !*bit;
    }
    let (packets, rejected) =
        decode_iq(&transmission(&[frame::encode(0x35, 0x89, 0x5678), garbage]));
    assert_eq!(rejected, 1);
    assert_eq!(packets.len(), 1, "{packets:?}");
    assert!(packets[0].truncated && packets[0].extra.is_none());
}

#[test]
fn a_damaged_block_is_counted_not_emitted() {
    let mut block = frame::encode(0x01, 0x80, 0x0042);
    for bit in block.iter_mut().skip(20).take(40) {
        *bit = !*bit;
    }
    let (packets, rejected) = decode_iq(&transmission(&[block]));
    assert!(packets.is_empty(), "{packets:?}");
    assert_eq!(rejected, 1);
}

#[test]
fn noise_produces_nothing() {
    let iq = sdrmm_modem_test_support::noise::complex_noise(7, 0.5, 48_000);
    let (packets, _) = decode_iq(&iq);
    assert!(packets.is_empty(), "{packets:?}");
}
