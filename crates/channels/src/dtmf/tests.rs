use sdrmm_decoder_test_support::settings;
use sdrmm_wire::ChannelParams;

use super::*;

fn channel(params: DtmfParams) -> DtmfChannel {
    DtmfChannel::new(
        ChannelCtx {
            input_rate: INPUT_RATE_HZ,
        },
        settings(ChannelParams::of(&params).expect("dtmf settings")),
    )
    .expect("dtmf channel")
}

fn sequences(events: &[DecoderEvent]) -> Vec<DtmfSequence> {
    events
        .iter()
        .map(|event| event.read::<DtmfSequence>().expect("a dtmf sequence"))
        .collect()
}

fn received(params: DtmfParams, iq: &[Complex<f32>]) -> Vec<DtmfSequence> {
    let mut channel = channel(params);
    let mut out = ChannelOutputs::default();
    for block in iq.chunks(317) {
        channel.process(block, &mut out);
    }
    channel.process(&quiet_carrier(params.gap_ms), &mut out);
    sequences(&out.events)
}

fn heard(params: DtmfParams, audio: &[f32]) -> Vec<DtmfSequence> {
    let mut decoder = Decoder::new(params, INPUT_RATE_HZ);
    let mut out = ChannelOutputs::default();
    let tail = (INPUT_RATE_HZ * (f64::from(params.gap_ms) / 1_000.0 + 0.1)) as usize;
    for &sample in audio.iter().chain(std::iter::repeat_n(&0.0, tail)) {
        decoder.push(sample, &mut out);
    }
    sequences(&out.events)
}

fn quiet_carrier(gap_ms: u32) -> Vec<Complex<f32>> {
    vec![Complex::new(1.0, 0.0); (INPUT_RATE_HZ * (f64::from(gap_ms) / 1_000.0 + 0.1)) as usize]
}

fn digits(found: &[DtmfSequence]) -> Vec<&str> {
    found
        .iter()
        .map(|sequence| sequence.digits.as_str())
        .collect()
}

fn fast() -> DtmfParams {
    DtmfParams {
        pace: DtmfPace::Fast,
        ..DtmfParams::default()
    }
}

#[test]
fn all_sixteen_keys_cross_fm_with_their_timing() {
    let iq = synth::transmission("0123456789*#ABCD", 70, 50, INPUT_RATE_HZ).expect("dtmf iq");
    let found = received(DtmfParams::default(), &iq);
    let [sequence] = found.as_slice() else {
        panic!("expected one sequence, got {found:?}");
    };
    assert_eq!(sequence.digits, "0123456789*#ABCD");
    assert!(sequence.tone_ms.abs_diff(70) <= 8, "{sequence:?}");
    assert!(
        sequence.gap_ms.is_some_and(|gap| gap.abs_diff(50) <= 8),
        "{sequence:?}"
    );
}

#[test]
fn fast_pace_takes_twenty_millisecond_tones_that_q24_refuses() {
    let iq = synth::transmission("1646347904", 20, 5, INPUT_RATE_HZ).expect("dtmf iq");
    assert!(received(DtmfParams::default(), &iq).is_empty());
    assert_eq!(digits(&received(fast(), &iq)), ["1646347904"]);
}

#[test]
fn a_silence_of_the_gap_splits_sequences() {
    let mut audio = synth::audio("461", 40, 20, INPUT_RATE_HZ).expect("audio");
    audio.resize(audio.len() + INPUT_RATE_HZ as usize * 2, 0.0);
    audio.extend(synth::audio("333", 40, 20, INPUT_RATE_HZ).expect("audio"));
    assert_eq!(
        digits(&heard(DtmfParams::default(), &audio)),
        ["461", "333"]
    );
    let patient = DtmfParams {
        gap_ms: 3_000,
        ..DtmfParams::default()
    };
    assert_eq!(digits(&heard(patient, &audio)), ["461333"]);
}

#[test]
fn a_single_key_has_no_gap() {
    let audio = synth::audio("5", 100, 0, INPUT_RATE_HZ).expect("audio");
    let found = heard(DtmfParams::default(), &audio);
    assert!(
        matches!(found.as_slice(), [sequence] if sequence.digits == "5" && sequence.gap_ms.is_none()),
        "{found:?}"
    );
}

#[test]
fn a_run_past_the_buffer_goes_out_whole_in_parts() {
    let dialed = "0123456789".repeat(7);
    let audio = synth::audio(&dialed, 40, 40, INPUT_RATE_HZ).expect("audio");
    let found = heard(DtmfParams::default(), &audio);
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(found[0].digits.len(), MAX_DIGITS);
    assert_eq!(digits(&found).concat(), dialed);
}

#[test]
fn a_noisy_open_squelch_dials_nothing() {
    let mut iq = sdrmm_modem::synth::silence(INPUT_RATE_HZ as usize * 2);
    sdrmm_modem::synth::add_noise(&mut iq, 7, 1.0);
    assert!(received(fast(), &iq).is_empty());
}

#[test]
fn speech_dials_nothing() {
    let speech = sdrmm_modem::synth::speech_audio(INPUT_RATE_HZ, INPUT_RATE_HZ as usize * 3);
    assert!(heard(DtmfParams::default(), &speech).is_empty());
    assert!(heard(fast(), &speech).is_empty());
}

fn recording(dir: &std::path::Path, stem: &str) -> Vec<f32> {
    sdrmm_test_support::reference_data::bytes(dir, &format!("{stem}.f32"))
        .as_chunks::<4>()
        .0
        .iter()
        .map(|sample| f32::from_le_bytes(*sample))
        .collect()
}

const DEMO: [&str; 7] = [
    "0123456789ABCD*#",
    "0123456789ABCD*#",
    "#89042337744",
    "*006144378265",
    "045623333******",
    "041448727060",
    "041448727060",
];

#[test]
#[ignore = "needs DTMF_SIGID_DIR=<sigidwiki DTMF samples as 48 kHz mono f32le, one <stem>.f32 each>"]
fn decodes_the_sigidwiki_recordings() {
    let Some(dir) = sdrmm_test_support::reference_data::dtmf_sigid_dir() else {
        return;
    };
    let library: [(&str, DtmfParams, Vec<&str>); 6] = [
        (
            "Dtmf-genave-superfast-20-05-1646347904",
            fast(),
            vec!["1646347904"],
        ),
        (
            "Dtmf-genave-superfast-20-20-1646347904",
            fast(),
            vec!["1646347904"],
        ),
        (
            "Motorola_Rate_250ms_1646347904",
            DtmfParams::default(),
            vec!["1646347904"],
        ),
        (
            "Dtmf-whelen-40-20-461PPPP333",
            DtmfParams::default(),
            vec!["461####333"],
        ),
        (
            "Dtmf-page-normal-50-50-S00P0501P99",
            DtmfParams::default(),
            vec!["*C#4D34**"],
        ),
        ("Dtmf", DtmfParams::default(), [DEMO, DEMO].concat()),
    ];
    let mut misses = Vec::new();
    for (stem, params, expected) in library {
        let found = heard(params, &recording(&dir, stem));
        println!("{stem:<40} {found:?}");
        if digits(&found) != expected {
            misses.push((stem, found));
        }
    }
    assert!(misses.is_empty(), "{misses:#?}");
}
