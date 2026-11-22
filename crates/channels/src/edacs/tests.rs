use std::path::Path;

use sdrmm_decoder::TrunkGrant;
use sdrmm_decoder_test_support::frames::{decode, decode_with_audio};
use sdrmm_decoder_test_support::{complex_noise, settings};
use sdrmm_modem::synth::{add_noise, bits, fm_modulate, silence};
use sdrmm_wire::{ChannelParams, DvFrame};

use super::message::{Call, Message, Read, read};
use super::*;
use crate::synth::edacs as tx;

fn channel(params: EdacsParams) -> EdacsChannel {
    EdacsChannel::new(
        ChannelCtx {
            input_rate: INPUT_RATE_HZ,
        },
        settings(ChannelParams::new("edacs", &params).expect("edacs settings")),
    )
    .expect("edacs channel")
}

fn control(frames: &[DvFrame]) -> Vec<&DvFrame> {
    frames
        .iter()
        .filter(|frame| frame.kind == DvFrameKind::Control)
        .collect()
}

fn grant_of(frames: &[DvFrame]) -> Option<&DvFrame> {
    frames
        .iter()
        .find(|frame| frame.opcode.as_deref() == Some("Group call"))
}

#[test]
fn the_check_matches_codewords_heard_off_air() {
    for word in [
        0xFD41_1028C3u64,
        0xFC40_0039AD,
        0xEE48_2FD804,
        0xFC21_430735,
    ] {
        assert_eq!(tx::codeword((word >> 12) as u32), bits(word, MESSAGE_BITS));
    }
}

#[test]
fn reads_a_standard_site_and_its_options_heard_off_air() {
    assert_eq!(
        read(0xFD4_1102, Some(0xFC4_0003), false, false),
        Some(Read {
            first: Message::Site {
                site: 2,
                control_lcn: 1,
            },
            second: Some(Message::Other("Site options")),
        })
    );
}

#[test]
fn reads_a_group_channel_update_heard_off_air() {
    let read = read(0xFC2_1430, Some(0xEE4_82FD), false, false).expect("read");
    assert_eq!(read.first, Message::AdjacentSite { site: 3, lcn: 1 });
    assert_eq!(
        read.second,
        Some(Message::GroupCall(Call {
            target: 765,
            lcn: 9,
            update: true,
            ..Call::default()
        }))
    );
}

#[test]
fn a_two_word_assignment_takes_its_caller_from_the_second_word() {
    let (first, second) = tx::group_assignment(765, 0x1234, 9);
    let read = read(first, Some(second), false, false).expect("read");
    assert_eq!(read.second, None);
    let Message::GroupCall(call) = read.first else {
        panic!("{read:?}");
    };
    assert_eq!((call.target, call.source, call.lcn), (765, Some(0x1234), 9));
}

#[test]
fn an_esk_site_is_read_once_its_key_is_lifted() {
    let keyed = tx::site(2, 1) ^ 0xA0 << 20;
    let read = read(keyed, Some(keyed), false, true).expect("read");
    assert_eq!(
        read.first,
        Message::Site {
            site: 2,
            control_lcn: 1
        }
    );
}

#[test]
fn a_status_lcn_is_named_and_never_a_channel() {
    let frame = Message::GroupCall(Call {
        target: 7,
        lcn: 30,
        ..Call::default()
    })
    .frame();
    assert_eq!(frame.channel, None);
    assert_eq!(frame.opcode.as_deref(), Some("Group call, system busy"));
}

fn decodes_a_control_channel_at(baud: EdacsBaud) {
    let grant = tx::group_assignment(765, 0x1234, 9);
    let mut iq = tx::control_channel(&[grant], baud, INPUT_RATE_HZ);
    add_noise(&mut iq, 7, 0.3);
    let mut chan = channel(EdacsParams {
        baud,
        ..EdacsParams::default()
    });
    let frames = decode(&mut chan, &iq);
    let control = control(&frames);
    assert!(control.iter().any(|frame| frame.site_id == Some(1)));
    let grant = grant_of(&frames).expect("the grant");
    assert_eq!(grant.destination, Some(765));
    assert_eq!(grant.source, Some(0x1234));
    assert_eq!(grant.channel, Some(9));
    assert_eq!(grant.trunk_protocol.as_deref(), Some("edacs"));
}

#[test]
fn decodes_a_9600_baud_control_channel() {
    decodes_a_control_channel_at(EdacsBaud::B9600);
}

#[test]
fn decodes_a_4800_baud_control_channel() {
    decodes_a_control_channel_at(EdacsBaud::B4800);
}

#[test]
fn the_scope_sees_two_clean_levels_at_9600_baud() {
    let iq = tx::control_channel(&[], EdacsBaud::B9600, INPUT_RATE_HZ);
    let mut chan = channel(EdacsParams {
        baud: EdacsBaud::B9600,
        ..EdacsParams::default()
    });
    let mut out = ChannelOutputs::default();
    out.symbols.set_wanted(true);
    for block in iq.chunks(2_048) {
        chan.process(block, &mut out);
    }
    let tap = &out.symbols;
    assert_eq!(tap.plane, Some(sdrmm_wire::SymbolPlane::Level));
    assert_eq!(tap.reference.len(), 2);
    assert!(tap.symbols.len() > 5_000, "{} symbols", tap.symbols.len());
    assert_eq!(tap.symbol_rate, 9_600.0);
    assert!(tap.evm < 0.15, "EVM {}", tap.evm);
}

#[test]
fn decodes_inverted_polarity() {
    let iq: Vec<Complex<f32>> = tx::control_channel(
        &[tx::group_assignment(42, 7, 3)],
        EdacsBaud::B9600,
        INPUT_RATE_HZ,
    )
    .iter()
    .map(Complex::conj)
    .collect();
    let frames = decode(&mut channel(EdacsParams::default()), &iq);
    assert_eq!(
        grant_of(&frames).and_then(|grant| grant.destination),
        Some(42)
    );
}

#[test]
fn decodes_an_extended_addressing_grant() {
    let grant = tx::extended_group(0x4321, 0x12345, 5);
    let iq = tx::control_channel(&[grant], EdacsBaud::B9600, INPUT_RATE_HZ);
    let frames = decode(
        &mut channel(EdacsParams {
            addressing: EdacsAddressing::Extended,
            ..EdacsParams::default()
        }),
        &iq,
    );
    let grant = grant_of(&frames).expect("the grant");
    assert_eq!(
        (grant.destination, grant.source, grant.channel),
        (Some(0x4321), Some(0x12345), Some(5))
    );
}

#[test]
fn one_spoilt_copy_is_outvoted_and_three_spoil_the_message() {
    let first = tx::codeword(tx::group_update(765, 9));
    let mut spoilt = tx::frame(&first, &first)[SYNC_BITS as usize..].to_vec();
    for bit in &mut spoilt[..MESSAGE_BITS] {
        *bit = !*bit;
    }
    let (voted, disagreements) = vote(&spoilt[..COPIES * MESSAGE_BITS]);
    assert_eq!(voted.to_vec(), first);
    assert_eq!(disagreements, MESSAGE_BITS as u32);

    for copy in 0..COPIES {
        for bit in [3, 17, 29] {
            let at = copy * MESSAGE_BITS + bit;
            spoilt[at] = !spoilt[at];
        }
    }
    let mut framer = Framer::new();
    framer.frame.copy_from_slice(&spoilt);
    assert!(
        framer
            .word(0)
            .is_none_or(|word| word.data != tx::group_update(765, 9))
    );
}

#[test]
fn an_unreadable_frame_is_counted() {
    let idle = tx::codeword(tx::site(1, 1));
    let idle = tx::frame(&idle, &idle);
    let mut keyed: Vec<bool> = (0..20).flat_map(|_| idle.clone()).collect();
    keyed.extend(bits(SYNC, SYNC_BITS as usize));
    keyed.extend(sdrmm_modem::synth::filler(FRAME_BITS, 11));
    keyed.extend((0..5).flat_map(|_| idle.clone()));
    let iq = sdrmm_modem::synth::fsk(&keyed, 9_600.0, 3_000.0, INPUT_RATE_HZ);
    let mut chan = channel(EdacsParams::default());
    decode(&mut chan, &iq);
    assert!(chan.rejected() > 0);
}

#[test]
fn noise_decodes_to_nothing() {
    let noise = complex_noise(69, 0.5, 400_000);
    let frames = decode(&mut channel(EdacsParams::default()), &noise);
    assert!(control(&frames).is_empty(), "{frames:?}");
}

#[test]
fn a_working_channel_plays_its_voice_and_frames_the_call() {
    let mut iq = tx::working_channel(EdacsBaud::B9600, 1.5, INPUT_RATE_HZ);
    let mut unkeyed = silence(48_000);
    add_noise(&mut unkeyed, 3, 1.0);
    iq.extend(unkeyed);
    let (frames, audio) = decode_with_audio(&mut channel(EdacsParams::default()), &iq);
    let kinds: Vec<DvFrameKind> = frames
        .iter()
        .filter(|frame| frame.control_channel == Some(false))
        .map(|frame| frame.kind)
        .collect();
    assert_eq!(kinds, vec![DvFrameKind::Voice, DvFrameKind::Terminator]);
    let voiced = audio.iter().filter(|s| s.abs() > 0.1).count();
    assert!(
        (48_000..72_000).contains(&voiced),
        "{voiced} voiced samples"
    );
}

#[test]
fn a_control_channel_stays_silent() {
    let iq = tx::control_channel(&[], EdacsBaud::B9600, INPUT_RATE_HZ);
    let (_, audio) = decode_with_audio(&mut channel(EdacsParams::default()), &iq);
    assert!(audio.iter().all(|&s| s == 0.0));
}

#[test]
fn an_analog_grant_is_followed_and_a_digital_one_is_not() {
    let trunking = trunk::trunking();
    let analog = Message::GroupCall(Call {
        target: 765,
        source: Some(12),
        lcn: 9,
        ..Call::default()
    })
    .frame();
    assert_eq!(
        (trunking.grants)(&analog),
        vec![TrunkGrant {
            logical_channel: 9,
            slot: None,
            destination: 765,
            source: Some(12),
            group_call: Some(true),
        }]
    );
    let digital = Message::GroupCall(Call {
        target: 765,
        lcn: 9,
        digital: true,
        ..Call::default()
    })
    .frame();
    assert!((trunking.grants)(&digital).is_empty());
    let calls = ChannelParams::bare("edacs");
    assert!((trunking.follow)(None, &calls).is_ok());
    assert!((trunking.follow)(Some(1), &calls).is_err());
}

/// What one recording must decode to, and how many messages it must give at least.
struct Recording {
    stem: &'static str,
    baud: EdacsBaud,
    site: u16,
    control_lcn: u8,
    group: (u32, u16),
    min_frames: usize,
}

const RECORDINGS: [Recording; 3] = [
    Recording {
        stem: "EDACS96_Sound",
        baud: EdacsBaud::B9600,
        site: 2,
        control_lcn: 1,
        group: (765, 9),
        min_frames: 1_100,
    },
    Recording {
        stem: "EDACS_96",
        baud: EdacsBaud::B9600,
        site: 1,
        control_lcn: 4,
        group: (0, 0),
        min_frames: 500,
    },
    Recording {
        stem: "Edacs48",
        baud: EdacsBaud::B4800,
        site: 22,
        control_lcn: 3,
        group: (537, 1),
        min_frames: 60,
    },
];

/// Every call the site assigns shows again in the updates that hold it on its channel.
fn assignments_are_updated(control: &[&DvFrame]) -> bool {
    let named = |frame: &DvFrame, opcode: &str| frame.opcode.as_deref() == Some(opcode);
    control
        .iter()
        .filter(|frame| named(frame, "Group call"))
        .all(|assigned| {
            control.iter().any(|update| {
                named(update, "Group call update")
                    && update.destination == assigned.destination
                    && update.channel == assigned.channel
            })
        })
}

fn discriminator_audio(dir: &Path, stem: &str, deviation_hz: f64) -> Vec<Complex<f32>> {
    let bytes = sdrmm_test_support::reference_data::bytes(dir, &format!("{stem}.f32"));
    let audio: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&b| f32::from_le_bytes(b))
        .collect();
    let rms = (audio.iter().map(|s| s * s).sum::<f32>() / audio.len() as f32).sqrt();
    let unit: Vec<f32> = audio.iter().map(|s| s / rms).collect();
    fm_modulate(&unit, deviation_hz, INPUT_RATE_HZ)
}

#[test]
#[ignore = "needs EDACS_SIGID_DIR=<SigIDWiki EDACS samples as 48 kHz mono f32le>"]
fn decodes_the_sigidwiki_recordings() {
    let Some(dir) = sdrmm_test_support::reference_data::edacs_sigid_dir() else {
        return;
    };
    for recording in &RECORDINGS {
        let iq = discriminator_audio(&dir, recording.stem, air(recording.baud).data_deviation_hz);
        let mut chan = channel(EdacsParams::default());
        let frames = decode(&mut chan, &iq);
        let control = control(&frames);
        let sites = control
            .iter()
            .filter(|frame| frame.site_id == Some(recording.site))
            .filter(|frame| {
                frame.text.as_deref() == Some(&format!("control LCN {}", recording.control_lcn))
            })
            .count();
        let (group, lcn) = recording.group;
        let groups = control
            .iter()
            .filter(|frame| frame.destination == Some(group) && frame.channel == Some(lcn))
            .count();
        println!(
            "{:<16} {} messages, {sites} site, {groups} group {group} on {lcn}, rejected {}",
            recording.stem,
            control.len(),
            chan.rejected()
        );
        assert!(control.len() >= recording.min_frames);
        assert!(sites > 0);
        assert!(
            control
                .iter()
                .filter_map(|frame| frame.site_id)
                .all(|site| site == recording.site),
            "{} names another site",
            recording.stem
        );
        assert!(
            assignments_are_updated(&control),
            "{} assigns a call no update holds",
            recording.stem
        );
        assert!(lcn == 0 || groups > 0);
    }
}
