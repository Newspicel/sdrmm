use super::*;
use crate::gk2a_lrit::synth;
use sdrmm_decoder_test_support::{at_snr, complex_noise, settings};
use sdrmm_wire::ChannelParams;

mod recorded;

const BLOCKS: [usize; 4] = [8_192, 1, 3_001, 40_000];

fn test_key() -> Gk2aKey {
    Gk2aKey {
        index: synth::KEY_INDEX,
        key: synth::KEY
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    }
}

pub(super) fn channel(keys: Vec<Gk2aKey>) -> Gk2aLritChannel {
    Gk2aLritChannel::new(
        ChannelCtx {
            input_rate: INPUT_RATE_HZ,
        },
        settings(ChannelParams::new("gk2a_lrit", &Gk2aLritParams { keys }).expect("settings")),
    )
    .expect("builds")
}

#[derive(Default)]
pub(super) struct Received {
    pub(super) images: Vec<DecodedImage>,
    pub(super) pictures: Vec<Gk2aLritImage>,
    pub(super) files: Vec<BroadcastData>,
    pub(super) progress: usize,
}

pub(super) fn run(
    chan: &mut Gk2aLritChannel,
    iq: &[Complex<f32>],
    lens: &[usize],
    received: &mut Received,
) {
    let mut out = ChannelOutputs::default();
    let mut at = 0;
    for len in lens.iter().cycle() {
        if at >= iq.len() {
            break;
        }
        let end = (at + len).min(iq.len());
        out.reset();
        chan.process(&iq[at..end], &mut out);
        received.progress += out.video.len();
        received.images.append(&mut out.images);
        for event in out.events.drain(..) {
            if let Some(picture) = event.read::<Gk2aLritImage>() {
                received.pictures.push(picture);
            } else if let Some(file) = event.read::<BroadcastData>() {
                received.files.push(file);
            } else {
                panic!("unexpected event {event:?}");
            }
        }
        at = end;
    }
}

fn mean_error(got: &[u8]) -> f64 {
    let height = synth::SEGMENT_LINES * synth::SEGMENTS;
    let want: Vec<u8> = (0..height)
        .flat_map(|y| (0..synth::WIDTH).map(move |x| synth::pixel(x, y)))
        .collect();
    assert_eq!(got.len(), want.len());
    want.iter()
        .zip(got)
        .map(|(&a, &b)| f64::from(a.abs_diff(b)))
        .sum::<f64>()
        / want.len() as f64
}

fn assert_text(received: &Received) {
    let text = received
        .files
        .iter()
        .find(|file| file.name == synth::TEXT_NAME)
        .expect("the bulletin");
    assert_eq!(text.media_type, "text/plain");
    assert_eq!(text.bytes, synth::TEXT.as_bytes());
}

#[test]
fn with_the_key_every_segment_draws() {
    let mut chan = channel(vec![test_key()]);
    let mut received = Received::default();
    run(&mut chan, &synth::transmission(), &BLOCKS, &mut received);
    assert_eq!(received.images.len(), 1);
    let image = &received.images[0];
    assert!(image.complete);
    assert_eq!(image.mode, "GK-2A IR105");
    assert_eq!(usize::from(image.picture.width), synth::WIDTH);
    let error = mean_error(&image.picture.luma);
    assert!(error < 3.0, "mean error {error}");
    let picture = &received.pictures[0];
    assert_eq!(picture.channel.as_deref(), Some("IR105"));
    assert_eq!((picture.segments, picture.segments_expected), (10, 10));
    assert_eq!(picture.name, synth::segment_name(1));
    assert_text(&received);
    assert_eq!(received.files.len(), 1);
    assert!(received.progress >= 9);
    assert_eq!(chan.rejected(), 0);
}

#[test]
fn without_the_key_the_locked_segment_is_handed_on_whole() {
    let mut chan = channel(Vec::new());
    let mut received = Received::default();
    run(&mut chan, &synth::transmission(), &BLOCKS, &mut received);
    assert!(received.images.is_empty());
    assert_text(&received);
    let locked = received
        .files
        .iter()
        .find(|file| file.name == synth::segment_name(synth::LOCKED_SEGMENT))
        .expect("the locked segment");
    assert_eq!(locked.media_type, "application/octet-stream");
    assert_eq!(
        sdrmm_modem::xrit::Headers::parse(&locked.bytes).and_then(sdrmm_modem::xrit::Headers::key),
        Some(synth::KEY_INDEX)
    );
    chan.retuned();
    run(&mut chan, &[Complex::default(); 16], &BLOCKS, &mut received);
    assert_eq!(received.images.len(), 1);
    assert!(!received.images[0].complete);
    assert_eq!(received.pictures[0].segments, 9);
}

#[test]
fn noise_an_offset_and_an_inverted_carrier_still_decode() {
    let mut iq = synth::transmission();
    sdrmm_modem::synth::shift(&mut iq, -15_000.0, INPUT_RATE_HZ);
    for sample in &mut iq {
        *sample = -*sample;
    }
    let noisy = at_snr(&iq, 7.0, 5);
    let mut chan = channel(vec![test_key()]);
    let mut received = Received::default();
    run(&mut chan, &noisy, &BLOCKS, &mut received);
    assert_eq!(received.images.len(), 1);
    assert!(received.images[0].complete);
    assert_text(&received);
}

#[test]
fn pure_noise_emits_nothing() {
    let noise = complex_noise(9, 0.5, (2.0 * INPUT_RATE_HZ) as usize);
    let mut chan = channel(Vec::new());
    let mut received = Received::default();
    run(&mut chan, &noise, &BLOCKS, &mut received);
    assert!(received.images.is_empty() && received.files.is_empty());
    assert_eq!(chan.rejected(), 0);
}

#[test]
fn bad_keys_params_and_rates_are_rejected() {
    let ctx = ChannelCtx {
        input_rate: INPUT_RATE_HZ,
    };
    let short = Gk2aLritParams {
        keys: vec![Gk2aKey {
            index: 1,
            key: "1234".to_owned(),
        }],
    };
    let params = ChannelParams::new("gk2a_lrit", &short).expect("settings");
    assert!(Gk2aLritChannel::new(ctx, settings(params)).is_err());
    assert!(Gk2aLritChannel::new(ctx, settings(ChannelParams::bare("nfm"))).is_err());
    let slow = ChannelCtx {
        input_rate: 48_000.0,
    };
    let params = ChannelParams::new("gk2a_lrit", &Gk2aLritParams::default()).expect("settings");
    assert!(Gk2aLritChannel::new(slow, settings(params)).is_err());
}

#[test]
fn image_names_give_channel_and_segment() {
    assert_eq!(
        image_name("IMG_FD_047_IR105_20190713_022006_03.lrit"),
        (Some("IR105"), Some(3))
    );
    assert_eq!(image_name("ANT_LRIT_047_20190713.lrit").0, None);
}

#[test]
fn the_scope_sees_two_tight_bpsk_clusters() {
    let iq = synth::transmission();
    let mut chan = channel(vec![test_key()]);
    let mut out = ChannelOutputs::default();
    out.symbols.set_wanted(true);
    let mut tap = sdrmm_decoder::SymbolTap::default();
    for block in iq.as_chunks::<CHUNK>().0 {
        out.reset();
        chan.process(block, &mut out);
        if out.symbols.plane.is_some() {
            tap = std::mem::take(&mut out.symbols);
            out.symbols.set_wanted(true);
        }
    }
    assert_eq!(tap.plane, Some(sdrmm_wire::SymbolPlane::Complex));
    assert_eq!(tap.reference.len(), 4);
    assert!(tap.symbols.len() >= 2 * 400, "{} values", tap.symbols.len());
    assert_eq!(tap.symbol_rate, SYMBOL_RATE);
    assert!(tap.evm < 0.1, "EVM {}", tap.evm);
}
