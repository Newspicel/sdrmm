use super::*;
use crate::goes_hrit::synth;
use sdrmm_decoder_test_support::{at_snr, complex_noise, settings};
use sdrmm_wire::ChannelParams;

mod recorded;

const BLOCKS: [usize; 4] = [16_384, 1, 7_919, 65_536];

pub(super) fn channel(picture: GoesPicture) -> GoesHritChannel {
    GoesHritChannel::new(
        ChannelCtx {
            input_rate: INPUT_RATE_HZ,
        },
        settings(
            ChannelParams::new("goes_hrit", &GoesHritParams { picture }).expect("goes settings"),
        ),
    )
    .expect("builds")
}

#[derive(Default)]
pub(super) struct Received {
    pub(super) images: Vec<DecodedImage>,
    pub(super) pictures: Vec<GoesHritImage>,
    pub(super) files: Vec<BroadcastData>,
    pub(super) progress: usize,
}

pub(super) fn run(
    chan: &mut GoesHritChannel,
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
            if let Some(picture) = event.read::<GoesHritImage>() {
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

fn decode(iq: &[Complex<f32>], lens: &[usize]) -> (Received, u32) {
    let mut chan = channel(GoesPicture::FullDisk);
    let mut received = Received::default();
    run(&mut chan, iq, lens, &mut received);
    (received, chan.rejected())
}

fn expected_disk() -> Vec<u8> {
    let height = usize::from(synth::SEGMENT_LINES * synth::SEGMENTS);
    (0..height)
        .flat_map(|y| (0..usize::from(synth::WIDTH)).map(move |x| synth::pixel(x, y)))
        .collect()
}

fn assert_full_chain(received: &Received) {
    assert_eq!(received.images.len(), 1, "one picture");
    let image = &received.images[0];
    assert!(image.complete);
    assert_eq!(image.mode, "GOES-19 Full disk band 13");
    assert_eq!(image.picture.width, synth::WIDTH);
    assert_eq!(image.lines, synth::SEGMENT_LINES * synth::SEGMENTS);
    assert_eq!(image.picture.luma, expected_disk());
    let picture = &received.pictures[0];
    assert_eq!(picture.name, synth::IMAGE_NAME);
    assert_eq!(picture.band, Some(13));
    assert_eq!((picture.segments, picture.segments_expected), (2, 2));
    assert_eq!(picture.lines_missing, 0);
    assert_eq!(received.files.len(), 1, "one admin message");
    let admin = &received.files[0];
    assert_eq!(admin.name, synth::ADMIN_NAME);
    assert_eq!(admin.media_type, "text/plain");
    assert_eq!(admin.bytes, synth::ADMIN_TEXT.as_bytes());
}

#[test]
fn the_synthetic_downlink_decodes_losslessly() {
    let (received, rejected) = decode(&synth::transmission(), &BLOCKS);
    assert_full_chain(&received);
    assert!(received.progress >= 2);
    assert_eq!(rejected, 0);
}

#[test]
fn noise_and_a_frequency_offset_still_decode() {
    let mut iq = synth::transmission();
    sdrmm_modem::synth::shift(&mut iq, 40_000.0, INPUT_RATE_HZ);
    let noisy = at_snr(&iq, 7.0, 11);
    let (received, rejected) = decode(&noisy, &BLOCKS);
    assert_full_chain(&received);
    assert_eq!(rejected, 0);
}

#[test]
fn a_broken_frame_is_counted_and_its_lines_filled() {
    let mut cadus = synth::cadus();
    for byte in &mut cadus[7][100..700] {
        *byte ^= 0xA5;
    }
    let iq = sdrmm_modem::xrit::modulate(&cadus, PRECODING, SPS, ROLL_OFF);
    let (received, rejected) = decode(&iq, &BLOCKS);
    assert!(rejected >= 1);
    let picture = &received.pictures[0];
    assert!(picture.lines_missing > 0);
    assert!(picture.complete);
}

#[test]
fn mesoscale_only_skips_the_full_disk_but_keeps_files() {
    let mut chan = channel(GoesPicture::Mesoscale);
    let mut received = Received::default();
    run(&mut chan, &synth::transmission(), &BLOCKS, &mut received);
    assert!(received.images.is_empty());
    assert_eq!(received.files.len(), 1);
}

#[test]
fn a_retune_closes_the_picture_as_partial() {
    let iq = synth::transmission();
    let mut chan = channel(GoesPicture::FullDisk);
    let mut received = Received::default();
    run(&mut chan, &iq[..iq.len() * 4 / 5], &BLOCKS, &mut received);
    assert!(received.images.is_empty());
    chan.retuned();
    run(
        &mut chan,
        &vec![Complex::default(); 64],
        &BLOCKS,
        &mut received,
    );
    assert_eq!(received.images.len(), 1);
    assert!(!received.images[0].complete);
    assert_eq!(received.pictures[0].segments, 1);
}

#[test]
fn pure_noise_emits_nothing() {
    let noise = complex_noise(5, 0.5, (0.5 * INPUT_RATE_HZ) as usize);
    let (received, rejected) = decode(&noise, &BLOCKS);
    assert!(received.images.is_empty() && received.files.is_empty());
    assert_eq!(received.progress, 0);
    assert_eq!(rejected, 0);
}

#[test]
fn wrong_params_and_rates_are_rejected() {
    let ctx = ChannelCtx {
        input_rate: INPUT_RATE_HZ,
    };
    assert!(GoesHritChannel::new(ctx, settings(ChannelParams::bare("nfm"))).is_err());
    let slow = ChannelCtx {
        input_rate: 48_000.0,
    };
    let params = ChannelParams::new("goes_hrit", &GoesHritParams::default()).expect("settings");
    assert!(GoesHritChannel::new(slow, settings(params)).is_err());
}

const RECORDED: &[u8] = include_bytes!("../../../../fixtures/goes_hrit_offair_1m854.sigmf-data");

#[test]
fn a_recorded_goes19_second_gives_satdumps_files() {
    let iq = crate::builtin::fixtures::ci16_le(RECORDED);
    let mut chan = channel(GoesPicture::All);
    let mut received = Received::default();
    run(&mut chan, &iq, &BLOCKS, &mut received);
    let stats = chan.link.stats();
    assert!(stats.frames >= 68, "{stats:?}");
    assert_eq!(stats.frames_failed, 0);
    assert_eq!(chan.rejected(), 0);
    let got: Vec<(&str, &str, usize)> = received
        .files
        .iter()
        .map(|file| {
            (
                file.name.as_str(),
                file.media_type.as_str(),
                file.bytes.len(),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            (
                "A_WUCN13CWWG262231_C_KWIN_20250626223159_428276-1-SVRW13CN.lrit",
                "text/plain",
                1_813
            ),
            (
                "A_SACA33KWBC262200_C_KWIN_20250626223202_428280-2-SAHOURLY.lrit",
                "text/plain",
                158
            ),
            (
                "A_SAUS70KWBC262232_C_KWIN_20250626223202_428279-2-SAHOURLY.lrit",
                "text/plain",
                1_893
            ),
        ]
    );
    let warning = &received.files[0].bytes;
    assert!(warning.starts_with(
        b"WUCN13 CWWG 262231\r\r\nSEVERE THUNDERSTORM WARNING\r\r\nFOR SOUTHERN SASKATCHEWAN"
    ));
    assert!(received.files[1].bytes.starts_with(
        b"SACA33 KWBC 262200 RRB\r\r\nMETAR\r\r\nTFFF 262230Z AUTO 10013KT 9999 FEW028 28/23 Q1015"
    ));
}
