use num_complex::Complex;
use sdrmm_decoder::{Channel, ChannelCtx, ChannelOutputs, NativeChannel};
use sdrmm_decoder_test_support::settings;
use sdrmm_wire::ChannelParams;

use crate::weak_signal::{Jt9Channel, Jt65Channel, WsjtMessage, WsjtParams, synth};

const RATE: usize = 12_000;

/// The WSJT-X JT9 sample `130418_1742.wav`: the seven decodes its user guide shows.
const JT9_SAMPLE: [&str; 7] = [
    "K1JT KF4RWA 73",
    "CQ GM7GAX IO75",
    "TF3G N7MQ CN84",
    "CQ M0WAY IO82",
    "K1JT N5KDV EM41",
    "G7CNF N4HFA EL89",
    "JA1KAU PD0JAC -23",
];

/// The WSJT-X JT65 and JT9 sample `130610_2343.wav`: the nine JT65 decodes the guide lists.
/// Its screenshot flags K2OI's at 2065 Hz as JT9, but the guide counts nine JT65 signals and
/// decodes JT9 only above 2500 Hz. Once those are subtracted a weaker `WU7B K9EEI 73` shows at
/// 1064 Hz; both calls are in WSJT-X's `CALL3.TXT`, so it is a station the guide does not list.
const MIXED_JT65: [&str; 9] = [
    "BG THX JOE 73",
    "VE6WQ SQ2NIJ -14",
    "KK4DSD W7VP -16",
    "CQ DL7ACA JO40",
    "N2SU W0JMW R-14",
    "YV6BFE F6GUU R-08",
    "VA3UG F1HMR 73",
    "RA3Y VE3NLS 73",
    "K2OI AJ4UU R-20",
];

/// The eight JT9 decodes of the same sample. TA4A's starts before the recording does, its first
/// third missing or buried, and is not read.
const MIXED_JT9: [&str; 8] = [
    "WB8QPG IZ0MIT -11",
    "KK4HEG KE0CO CN87",
    "CQ AG4M EM75",
    "CQ TA4A KM37",
    "CT1FBK IK5YZT R+02",
    "KF5SLN KB1SUA FN42",
    "CQ M0ABA JO01",
    "EI3HGB DD2EE JO31",
];

/// A committed recording padded with silence to the end of its minute, so one slot closes.
fn minute(bytes: &[u8]) -> Vec<Complex<f32>> {
    let mut iq: Vec<Complex<f32>> = bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|sample| {
            Complex::new(
                f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]),
                f32::from_le_bytes([sample[4], sample[5], sample[6], sample[7]]),
            )
        })
        .collect();
    iq.resize(60 * RATE, Complex::default());
    iq
}

fn params(type_id: &str) -> ChannelParams {
    let band = WsjtParams {
        audio_high_hz: 4_000.0,
        ..WsjtParams::default()
    };
    ChannelParams::new(type_id, &band).expect("settings")
}

macro_rules! heard {
    ($channel:ty, $type_id:expr, $iq:expr) => {{
        let ctx = ChannelCtx {
            input_rate: RATE as f64,
        };
        let mut channel = <$channel>::new(ctx, settings(params($type_id))).expect("builds");
        let mut out = ChannelOutputs::default();
        for block in $iq.chunks(4_096) {
            channel.process(block, &mut out);
        }
        channel.settle(&mut out);
        out.events
            .iter()
            .map(|event| {
                assert_eq!(event.kind, $type_id);
                event.payload::<WsjtMessage>().expect("a WSJT message")
            })
            .collect::<Vec<WsjtMessage>>()
    }};
}

fn texts(messages: &[WsjtMessage]) -> Vec<&str> {
    messages
        .iter()
        .map(|message| message.text.as_str())
        .collect()
}

fn missing<'a>(published: &[&'a str], heard: &[WsjtMessage]) -> Vec<&'a str> {
    published
        .iter()
        .filter(|wanted| !heard.iter().any(|message| message.text == **wanted))
        .copied()
        .collect()
}

fn invented<'a>(published: &[&str], heard: &'a [WsjtMessage]) -> Vec<&'a str> {
    heard
        .iter()
        .map(|message| message.text.as_str())
        .filter(|text| !published.contains(text))
        .collect()
}

#[test]
fn a_recorded_jt9_minute_reads_what_wsjtx_published() {
    const FIXTURE: &[u8] = include_bytes!("../../../../../fixtures/jt9_20m_12k.sigmf-data");
    let heard = heard!(Jt9Channel, "jt9", minute(FIXTURE));
    assert_eq!(missing(&JT9_SAMPLE, &heard), [""; 0], "{:?}", texts(&heard));
    assert_eq!(invented(&JT9_SAMPLE, &heard), [""; 0]);
    let kf4rwa = heard
        .iter()
        .find(|message| message.text == "K1JT KF4RWA 73")
        .expect("KF4RWA");
    assert!((kf4rwa.audio_hz - 1_224.0).abs() < 2.0, "{kf4rwa:?}");
    assert!((kf4rwa.snr_db + 19.0).abs() < 5.0, "{kf4rwa:?}");
    assert!(kf4rwa.time_offset_s.abs() < 0.3, "{kf4rwa:?}");
}

#[test]
fn a_recorded_mixed_minute_reads_its_jt65_stations() {
    const FIXTURE: &[u8] = include_bytes!("../../../../../fixtures/jt65_jt9_20m_12k.sigmf-data");
    let heard = heard!(Jt65Channel, "jt65", minute(FIXTURE));
    assert_eq!(missing(&MIXED_JT65, &heard), [""; 0], "{:?}", texts(&heard));
    assert_eq!(
        invented(&MIXED_JT65, &heard),
        ["WU7B K9EEI 73"],
        "{heard:?}"
    );
    let w7vp = heard
        .iter()
        .find(|message| message.text == "KK4DSD W7VP -16")
        .expect("W7VP");
    assert!((w7vp.audio_hz - 815.0).abs() < 2.0, "{w7vp:?}");
}

#[test]
fn a_recorded_mixed_minute_reads_its_jt9_stations() {
    const FIXTURE: &[u8] = include_bytes!("../../../../../fixtures/jt65_jt9_20m_12k.sigmf-data");
    let heard = heard!(Jt9Channel, "jt9", minute(FIXTURE));
    assert_eq!(
        missing(&MIXED_JT9, &heard),
        ["CQ TA4A KM37"],
        "{:?}",
        texts(&heard)
    );
    assert_eq!(invented(&MIXED_JT9, &heard), [""; 0]);
}

#[test]
fn synthetic_slots_decode_on_both_channels() {
    let jt65 = heard!(
        Jt65Channel,
        "jt65",
        synth::jt65_slot("K1ABC", "FN42", 1_200.0).expect("a synthetic signal")
    );
    assert_eq!(texts(&jt65), ["CQ K1ABC FN42"], "{jt65:?}");
    assert!((jt65[0].audio_hz - 1_200.0).abs() < 1.0, "{jt65:?}");
    assert!(jt65[0].time_offset_s.abs() < 0.1, "{jt65:?}");
    let jt9 = heard!(
        Jt9Channel,
        "jt9",
        synth::jt9_slot("K1ABC", "FN42", 1_200.0).expect("a synthetic signal")
    );
    assert_eq!(texts(&jt9), ["CQ K1ABC FN42"], "{jt9:?}");
    assert!((jt9[0].audio_hz - 1_200.0).abs() < 1.0, "{jt9:?}");
    assert!(jt9[0].time_offset_s.abs() < 0.1, "{jt9:?}");
}

#[test]
fn noise_alone_reads_nothing() {
    let mut iq = vec![Complex::default(); 60 * RATE];
    sdrmm_modem::synth::add_noise(&mut iq, 11, 0.1);
    for sample in &mut iq {
        sample.im = 0.0;
    }
    assert!(heard!(Jt65Channel, "jt65", iq.clone()).is_empty());
    assert!(heard!(Jt9Channel, "jt9", iq).is_empty());
}
