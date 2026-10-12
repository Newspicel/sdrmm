use num_complex::Complex;
use sdrmm_wire::{
    ChannelParams, DecoderEvent, GsmBand, GsmFrame, GsmLogicalChannel, GsmMessage, GsmParams, GsmSi,
};

use super::{DESCRIPTOR, GsmChannel, burst::INPUT_RATE_HZ};
use crate::{
    ChannelCtx, ChannelRx,
    synth::gsm::{Downlink, Scheduled, block},
    testutil::{
        at_snr, cf32_le, complex_noise, frequency_shift, multipath, realtime_budget, run_events,
        sample_clock_offset, settings,
    },
};

const FIXTURE: &[u8] = include_bytes!("../../../../fixtures/gsm_dcs1800_arfcn725_541k.sigmf-data");
const ARFCN_725_HZ: f64 = 1_847_800_000.0;
const BASE: u32 = 51 * 16_900;
const BSIC: u8 = 0o60;
const SI3: &str = "49061b2bd962f220013ec8070a156009b90000e81f461b";
const SI1: &str = "5506198f6a8000000000000000000000000000b9000083";
const SI2: &str = "59061a8f6d181080000000000000000000000078b90000";
const SI4: &str = "31061c62f220013e6009b90000e81f732b2b2b2b2b2b2b";
const IMSI_PAGING: &str = "310621200829435109280546128b2b2b2b2b2b2b2b2b2b";
const ASSIGNMENT: &str = "2d063f004902d52654c701002b2b2b2b2b2b2b2b2b2b2b";
const PAGING_RESPONSE: &str = "017335062700035018a005f4015c57032b2b2b2b2b2b2b";
const CIPHERING: &str = "03200d0635112b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b";
const SI5: &str = "0001030349061d9f6d1810800000000000000000000000";

fn channel(params: GsmParams) -> GsmChannel {
    let mut settings = settings(ChannelParams::Gsm(params));
    settings.frequency_hz = ARFCN_725_HZ;
    GsmChannel::new(
        ChannelCtx {
            input_rate: INPUT_RATE_HZ,
        },
        settings,
    )
    .expect("gsm channel")
}

fn frames(events: &[DecoderEvent]) -> Vec<GsmFrame> {
    events
        .iter()
        .filter_map(|event| match event {
            DecoderEvent::Gsm(frame) => Some(frame.clone()),
            _ => None,
        })
        .collect()
}

fn scheduled(timeslot: u8, frame_number: u32, data: &str) -> Scheduled {
    Scheduled {
        timeslot,
        frame_number: BASE + frame_number,
        tsc: BSIC & 7,
        data: block(data),
    }
}

fn cell() -> Downlink {
    Downlink {
        bsic: BSIC,
        first_frame: BASE,
        frames: 7 * 51,
        system_information: [SI3, SI1, SI2, SI4].map(block).to_vec(),
        blocks: vec![
            scheduled(0, 2 * 51 + 6, IMSI_PAGING),
            scheduled(0, 3 * 51 + 12, ASSIGNMENT),
            scheduled(1, 4 * 51 + 4, PAGING_RESPONSE),
            scheduled(1, 4 * 51 + 36, SI5),
            scheduled(1, 5 * 51 + 4, CIPHERING),
        ],
        seed: 7,
    }
}

fn decode(params: GsmParams, iq: &[Complex<f32>]) -> (Vec<GsmFrame>, u32) {
    let mut filtered = Vec::with_capacity(iq.len());
    super::channel_filter().process(iq, &mut filtered);
    let mut channel = channel(params);
    let frames = frames(&run_events(&mut channel, &filtered));
    (frames, channel.rejected())
}

fn system_information(frames: &[GsmFrame]) -> Vec<GsmSi> {
    frames
        .iter()
        .filter_map(|frame| match &frame.message {
            GsmMessage::SystemInformation { si, .. } => Some(*si),
            _ => None,
        })
        .collect()
}

fn identities(frames: &[GsmFrame]) -> Vec<String> {
    frames
        .iter()
        .filter_map(|frame| match &frame.message {
            GsmMessage::Paging { identities } => Some(identities.iter().map(|id| id.label())),
            _ => None,
        })
        .flatten()
        .collect()
}

fn dedicated(frames: &[GsmFrame]) -> Vec<(u8, String)> {
    frames
        .iter()
        .filter_map(|frame| match &frame.message {
            GsmMessage::Dedicated(message) => Some((frame.timeslot, message.summary())),
            _ => None,
        })
        .collect()
}

fn assert_full_story(frames: &[GsmFrame]) {
    let si = system_information(frames);
    for kind in [GsmSi::Si1, GsmSi::Si2, GsmSi::Si3, GsmSi::Si4, GsmSi::Si5] {
        assert!(si.contains(&kind), "{kind:?} missing from {si:?}");
    }
    let last = frames.last().expect("frames");
    assert_eq!(last.cgi.as_deref(), Some("262-02-318-11225"));
    assert_eq!(last.bsic_label(), "6-0");
    assert_eq!(last.arfcn, Some(725));
    assert_eq!(last.band, Some(GsmBand::Dcs1800));
    assert_eq!(identities(frames), ["IMSI 234159082506421"]);
    let assignments: Vec<_> = frames
        .iter()
        .filter_map(|frame| match &frame.message {
            GsmMessage::ImmediateAssignment { assignments } => Some(assignments.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(assignments.len(), 1, "{assignments:?}");
    assert_eq!(assignments[0][0].timeslot, 1);
    assert_eq!(assignments[0][0].subchannel, Some(1));
    let dedicated = dedicated(frames);
    assert_eq!(dedicated.len(), 2, "{dedicated:?}");
    assert!(dedicated[0].1.contains("Paging Response"), "{dedicated:?}");
    assert!(dedicated[0].1.contains("TMSI 015C5703"), "{dedicated:?}");
    assert!(dedicated[1].1.contains("A5/1"), "{dedicated:?}");
    assert!(dedicated.iter().all(|(timeslot, _)| *timeslot == 1));
}

#[test]
fn descriptor_runs_at_two_samples_per_symbol() {
    assert_eq!(DESCRIPTOR.input_rate_hz, 1_625_000.0 / 3.0);
    assert_eq!(DESCRIPTOR.bandwidth_hz, 200_000.0);
}

#[test]
fn a_clean_cell_tells_the_whole_story() {
    let (frames, rejected) = decode(GsmParams::default(), &cell().modulate());
    assert_full_story(&frames);
    assert_eq!(rejected, 0);
    let si3 = frames
        .iter()
        .find(|frame| {
            matches!(
                frame.message,
                GsmMessage::SystemInformation { si: GsmSi::Si3, .. }
            )
        })
        .expect("si3");
    assert_eq!(si3.channel, GsmLogicalChannel::Bcch);
    assert_eq!(si3.raw, SI3);
    assert_eq!(si3.frame_number % 51, 2);
}

#[test]
fn an_impaired_cell_still_decodes() {
    let clean = cell().modulate();
    let shifted = frequency_shift(&clean, 9_800.0, INPUT_RATE_HZ);
    let drifted = sample_clock_offset(&shifted, 2.0);
    let paths = [(0, 1.0, 0.0), (3, 0.5, 0.0), (6, 0.3, 0.0)];
    let echoed = multipath(&drifted, &paths, INPUT_RATE_HZ);
    let noisy = at_snr(&echoed, 4.0, 11);
    let (frames, rejected) = decode(GsmParams::default(), &noisy);
    assert_full_story(&frames);
    assert!(rejected <= 2, "rejected {rejected}");
    let error = frames.last().expect("frames").frequency_error_hz;
    assert!((error - 9_800.0).abs() < 150.0, "{error}");
}

#[test]
fn a_weak_cell_still_decodes() {
    let noisy = at_snr(&cell().modulate(), 1.0, 23);
    let (frames, rejected) = decode(GsmParams::default(), &noisy);
    assert_full_story(&frames);
    assert!(rejected <= 2, "rejected {rejected}");
}

#[test]
fn paging_and_following_can_be_switched_off() {
    let iq = cell().modulate();
    let quiet = GsmParams {
        paging: false,
        follow: false,
    };
    let (frames, _) = decode(quiet, &iq);
    assert!(identities(&frames).is_empty());
    assert!(dedicated(&frames).is_empty());
    assert!(system_information(&frames).contains(&GsmSi::Si3));
}

#[test]
fn a_corrupted_block_is_counted() {
    let mut iq = cell().modulate();
    let first = (2 * 51 + 7) * 2_500;
    let noise = complex_noise(5, 1.0, 3 * 320);
    for (frame, burst) in noise.chunks(320).enumerate() {
        let at = first + frame * 2_500;
        iq[at..at + 320].copy_from_slice(burst);
    }
    let (frames, rejected) = decode(GsmParams::default(), &iq);
    assert!(identities(&frames).is_empty());
    assert!(rejected >= 1);
}

#[test]
fn noise_alone_decodes_nothing() {
    let (frames, rejected) = decode(GsmParams::default(), &complex_noise(3, 1.0, 200_000));
    assert!(frames.is_empty());
    assert_eq!(rejected, 0);
}

#[test]
fn decodes_the_recorded_cell() {
    let iq = cf32_le(FIXTURE);
    let started = std::time::Instant::now();
    let (frames, rejected) = decode(GsmParams::default(), &iq);
    let elapsed = started.elapsed().as_secs_f64();
    assert!(
        elapsed < realtime_budget(iq.len() as f64 / INPUT_RATE_HZ),
        "{elapsed} s"
    );
    assert_eq!(rejected, 0);
    assert_eq!(
        identities(&frames),
        [
            "TMSI 015C5703",
            "TMSI 0156DA8D",
            "TMSI 0156E498",
            "TMSI 015D0383",
            "TMSI 015AC4EB"
        ]
    );
    assert_eq!(
        system_information(&frames),
        [GsmSi::Si3, GsmSi::Si5, GsmSi::Si4, GsmSi::Si13]
    );
    let assignment = frames
        .iter()
        .find_map(|frame| match &frame.message {
            GsmMessage::ImmediateAssignment { assignments } => Some(assignments[0].label()),
            _ => None,
        })
        .expect("assignment");
    assert_eq!(assignment, "SDCCH/8 TS1 SC1 ARFCN 725 TA 1");
    assert_eq!(
        dedicated(&frames),
        [
            (1, "RR Paging Response · TMSI 015C5703".to_owned()),
            (1, "RR Ciphering Mode Command · A5/1".to_owned())
        ]
    );
    let last = frames.last().expect("frames");
    assert_eq!(last.frame_number, 862_312);
    assert_eq!(last.cgi.as_deref(), Some("262-02-318-11225"));
    assert_eq!(last.bsic_label(), "6-0");
    assert!(
        (9_500.0..10_100.0).contains(&last.frequency_error_hz),
        "{}",
        last.frequency_error_hz
    );
    assert!(frames.iter().all(|frame| frame.snr_db > 10.0));
}

#[test]
fn a_fading_cell_still_decodes() {
    let paths = [(0, 1.0, 0.0), (2, 0.8, 200.0), (5, 0.4, -140.0)];
    let faded = multipath(&cell().modulate(), &paths, INPUT_RATE_HZ);
    let (frames, rejected) = decode(GsmParams::default(), &at_snr(&faded, 8.0, 5));
    assert_full_story(&frames);
    assert_eq!(rejected, 0);
}
