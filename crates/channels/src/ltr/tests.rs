use sdrmm_decoder_test_support::frames::{decode, decode_with_audio};
use sdrmm_decoder_test_support::{complex_noise, settings};
use sdrmm_modem::synth::{add_noise, fm_modulate};
use sdrmm_wire::ChannelParams;

use super::*;
use crate::synth::ltr as tx;

fn channel() -> LtrChannel {
    LtrChannel::new(
        ChannelCtx {
            input_rate: INPUT_RATE_HZ,
        },
        settings(ChannelParams::new("ltr", &LtrParams::default()).expect("ltr settings")),
    )
    .expect("ltr channel")
}

fn deviation() -> f64 {
    deviation_hz(&LtrParams::default())
}

#[test]
fn a_word_reads_back_as_it_was_built() {
    let call = tx::call();
    assert_eq!(Word::read(call.encode()), Some(call));
    assert_eq!(call.meaning(), Meaning::Call);
    assert_eq!(call.talkgroup_name(), "0-05-004");
    let corrupt = call.encode() ^ 1 << 15;
    assert_eq!(Word::read(corrupt), None);
}

#[test]
fn a_radio_sends_its_word_inverted() {
    let call = tx::call();
    let inverted = !call.encode() & ((1 << WORD_BITS) - 1);
    assert_eq!(
        Word::read(inverted),
        Some(Word {
            inbound: true,
            ..call
        })
    );
}

#[test]
fn idle_and_turn_off_words_mean_what_they_say() {
    let idle = Word {
        goto: 3,
        free: 3,
        group: 255,
        ..tx::call()
    };
    assert_eq!(idle.meaning(), Meaning::Idle);
    let turn_off = Word {
        goto: 31,
        ..tx::call()
    };
    assert_eq!(turn_off.meaning(), Meaning::TurnOff);
}

#[test]
fn the_scope_sees_the_subaudible_data_as_two_levels() {
    let iq = tx::transmission(tx::call(), 15, deviation(), INPUT_RATE_HZ);
    let mut chan = channel();
    let mut out = ChannelOutputs::default();
    out.symbols.set_wanted(true);
    for block in iq.chunks(4_800) {
        chan.process(block, &mut out);
    }
    let tap = &out.symbols;
    assert_eq!(tap.plane, Some(sdrmm_wire::SymbolPlane::Level));
    assert_eq!(tap.reference.len(), 2);
    assert!(tap.symbols.len() > 500, "{} symbols", tap.symbols.len());
    assert_eq!(tap.symbol_rate, BAUD);
    assert!(tap.evm < 0.3, "EVM {}", tap.evm);
}

#[test]
fn decodes_a_call_its_voice_and_its_end() {
    let mut iq = tx::transmission(tx::call(), 15, deviation(), INPUT_RATE_HZ);
    add_noise(&mut iq, 5, 0.2);
    let mut chan = channel();
    let (frames, audio) = decode_with_audio(&mut chan, &iq);
    let kinds: Vec<DvFrameKind> = frames.iter().map(|frame| frame.kind).collect();
    assert_eq!(kinds, vec![DvFrameKind::Header, DvFrameKind::Terminator]);
    let header = &frames[0];
    assert_eq!(header.destination, Some(1_284));
    assert_eq!(header.destination_call.as_deref(), Some("0-05-004"));
    assert_eq!(header.channel, Some(5));
    assert_eq!(header.rest_channel, Some(2));
    assert_eq!(header.trunk_protocol.as_deref(), Some("ltr"));
    let voiced = audio.iter().filter(|s| s.abs() > 0.1).count();
    assert!(voiced > 48_000, "{voiced} voiced samples");
    assert_eq!(chan.rejected(), 0);
}

#[test]
fn a_call_that_stops_without_turn_off_still_ends() {
    let words = vec![tx::call(); 10];
    let data = tx::data_audio(&words, INPUT_RATE_HZ);
    let mut iq = fm_modulate(&data, deviation(), INPUT_RATE_HZ);
    iq.extend(complex_noise(9, 0.5, 48_000));
    let frames = decode(&mut channel(), &iq);
    assert_eq!(
        frames.last().map(|frame| frame.kind),
        Some(DvFrameKind::Terminator)
    );
}

#[test]
fn noise_decodes_to_nothing() {
    let noise = complex_noise(69, 0.5, 960_000);
    assert!(decode(&mut channel(), &noise).is_empty());
}

#[test]
fn the_free_repeater_changing_does_not_start_a_new_call() {
    let words: Vec<Word> = (1..=12)
        .map(|n| Word {
            free: n % 3 + 1,
            ..tx::call()
        })
        .collect();
    let data = tx::data_audio(&words, INPUT_RATE_HZ);
    let iq = fm_modulate(&data, deviation(), INPUT_RATE_HZ);
    let headers = decode(&mut channel(), &iq)
        .iter()
        .filter(|frame| frame.kind == DvFrameKind::Header)
        .count();
    assert_eq!(headers, 1);
}

#[test]
fn a_repeater_is_followed_by_watching_it() {
    let trunking = trunk::trunking();
    let spec = (trunking.spec)();
    assert_eq!(spec.schemes[0].kind, sdrmm_wire::TrunkKind::Distributed);
    let calls = ChannelParams::bare("ltr");
    assert!((trunking.follow)(None, &calls).is_ok());
    assert!((trunking.follow)(Some(1), &calls).is_err());
}

fn recording(dir: &std::path::Path, stem: &str) -> Vec<Complex<f32>> {
    let bytes = sdrmm_test_support::reference_data::bytes(dir, &format!("{stem}.f32"));
    let audio: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&b| f32::from_le_bytes(b))
        .collect();
    let peak = audio.iter().fold(0f32, |peak, s| peak.max(s.abs()));
    let unit: Vec<f32> = audio.iter().map(|s| s / peak).collect();
    fm_modulate(&unit, deviation(), INPUT_RATE_HZ)
}

#[test]
#[ignore = "needs LTR_SIGID_DIR=<SigIDWiki LTR samples as 48 kHz mono f32le>"]
fn decodes_the_sigidwiki_recording() {
    let Some(dir) = sdrmm_test_support::reference_data::ltr_sigid_dir() else {
        return;
    };
    let frames = decode(&mut channel(), &recording(&dir, "Ltrtx"));
    for frame in &frames {
        println!(
            "{:?} {:?} {:?} {:?}",
            frame.kind, frame.destination_call, frame.channel, frame.rest_channel
        );
    }
    let header = frames
        .iter()
        .find(|frame| frame.kind == DvFrameKind::Header)
        .expect("a call");
    assert_eq!(header.destination_call.as_deref(), Some("0-05-004"));
    assert_eq!(header.channel, Some(5));
    assert_eq!(
        frames.last().map(|frame| frame.kind),
        Some(DvFrameKind::Terminator)
    );
}
