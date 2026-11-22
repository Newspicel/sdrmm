use num_complex::Complex;
use sdrmm_decoder::ChannelFront;
use sdrmm_decoder_test_support::{at_snr, settings};
use sdrmm_wire::ChannelParams;

use super::*;
use crate::synth::wenet::{
    self as wenet_synth, ALTITUDE_M, CALLSIGN, HEIGHT, IMAGE_ID, LAT, LON, MESSAGE, WIDTH,
};

const RATE: f64 = INPUT_RATE_HZ;
const BLOCKS: [usize; 4] = [4_096, 1, 9_973, 65_536];

fn params() -> ChannelParams {
    ChannelParams::new("wenet", &WenetParams::default()).expect("wenet settings")
}

fn channel() -> WenetChannel {
    WenetChannel::new(ChannelCtx { input_rate: RATE }, settings(params())).expect("builds")
}

#[derive(Default)]
struct Received {
    telemetry: Vec<WenetTelemetry>,
    pictures: Vec<WenetImage>,
    images: Vec<DecodedImage>,
    progress: usize,
}

impl Received {
    fn take(&mut self, out: &mut ChannelOutputs) {
        for event in out.events.drain(..) {
            if let Some(telemetry) = event.read::<WenetTelemetry>() {
                self.telemetry.push(telemetry);
            } else if let Some(picture) = event.read::<WenetImage>() {
                self.pictures.push(picture);
            } else {
                panic!("unexpected event {event:?}");
            }
        }
        self.progress += out.video.len();
        self.images.append(&mut out.images);
    }
}

fn decode(iq: &[Complex<f32>], blocks: &[usize]) -> Received {
    let mut chan = channel();
    let mut received = Received::default();
    let mut out = ChannelOutputs::default();
    let mut at = 0;
    for len in blocks.iter().cycle() {
        if at >= iq.len() {
            break;
        }
        let end = (at + len).min(iq.len());
        out.reset();
        chan.process(&iq[at..end], &mut out);
        received.take(&mut out);
        at = end;
    }
    received
}

fn mean_error(a: &[u8], b: &[u8]) -> f64 {
    let total: u64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| u64::from(x.abs_diff(*y)))
        .sum();
    total as f64 / a.len() as f64
}

fn assert_full_decode(received: &Received, mode: WenetMode) {
    let text = received
        .telemetry
        .iter()
        .find(|event| event.packet == WenetPacket::Text)
        .unwrap_or_else(|| panic!("{mode:?}: no text"));
    assert_eq!(text.text.as_deref(), Some(MESSAGE));
    assert_eq!(text.mode, mode);
    let fix = received
        .telemetry
        .iter()
        .find(|event| event.packet == WenetPacket::Gps)
        .unwrap_or_else(|| panic!("{mode:?}: no fix"));
    assert_eq!(fix.lat, Some(f64::from(LAT)));
    assert_eq!(fix.lon, Some(f64::from(LON)));
    assert_eq!(fix.altitude_m, Some(f64::from(ALTITUDE_M)));
    let [picture] = received.pictures.as_slice() else {
        panic!("{mode:?}: pictures {:?}", received.pictures);
    };
    assert_eq!(picture.callsign, CALLSIGN);
    assert_eq!(picture.image_id, IMAGE_ID);
    assert_eq!((picture.width, picture.height), (WIDTH, HEIGHT));
    assert!(picture.complete, "{picture:?}");
    assert_eq!(picture.packets as usize, wenet_synth::ssdv_packets().len());
    assert_eq!(picture.packets_lost, 0);
    assert_eq!(picture.mode, mode);
    let [image] = received.images.as_slice() else {
        panic!("{mode:?}: {} images", received.images.len());
    };
    assert!(image.complete);
    assert_eq!(image.lines, HEIGHT);
    let error = mean_error(&image.picture.rgb, &wenet_synth::scene());
    assert!(error < 6.0, "{mode:?} mean error {error}");
}

#[test]
fn both_framings_decode_picture_text_and_fix_through_noise() {
    for mode in WenetMode::ALL {
        let iq = at_snr(&wenet_synth::transmission(mode, RATE), 12.0, 7);
        assert_full_decode(&decode(&iq, &BLOCKS), mode);
    }
}

#[test]
fn a_frequency_offset_still_decodes() {
    let iq = wenet_synth::transmission(WenetMode::Uart, RATE);
    let shifted: Vec<Complex<f32>> = iq
        .iter()
        .enumerate()
        .map(|(n, &s)| {
            let phase = std::f64::consts::TAU * 15_000.0 * n as f64 / RATE;
            s * Complex::new(phase.cos() as f32, phase.sin() as f32)
        })
        .collect();
    assert_full_decode(&decode(&shifted, &[4_096]), WenetMode::Uart);
}

#[test]
fn ragged_blocks_give_the_same_picture() {
    let iq = wenet_synth::transmission(WenetMode::I2s, RATE);
    let whole = decode(&iq, &[4_096]);
    let ragged = decode(&iq, &BLOCKS);
    assert_eq!(whole.images, ragged.images);
    assert!(whole.progress >= 1);
}

#[test]
fn the_gps_fix_takes_the_picture_callsign_once_known() {
    let mut packets = wenet_synth::ssdv_packets();
    packets.push(packet::build::gps(2_400, 0, LAT, LON, 100.0, 0.0));
    let iq = wenet_synth::modulate(WenetMode::Uart, &packets, RATE);
    let received = decode(&iq, &[4_096]);
    let fix = received.telemetry.last().expect("a fix");
    assert_eq!(fix.callsign.as_deref(), Some(CALLSIGN));
}

#[test]
fn noise_alone_gives_nothing() {
    let noise = sdrmm_decoder_test_support::complex_noise(3, 0.5, RATE as usize);
    let received = decode(&noise, &[4_096]);
    assert!(received.telemetry.is_empty());
    assert!(received.pictures.is_empty());
}

/// Reads the cu8 capture, moves the signal centre to 0 Hz and filters it as the engine would.
fn off_air(path: &std::path::Path) -> Vec<Complex<f32>> {
    const SIGNAL_CENTRE_HZ: f64 = 196_000.0;
    let bytes = std::fs::read(path).expect("capture");
    let iq: Vec<Complex<f32>> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[i, q]| {
            Complex::new(
                (f32::from(i) - 127.5) / 127.5,
                (f32::from(q) - 127.5) / 127.5,
            )
        })
        .collect();
    let mut front = ChannelFront::new(&params(), RATE, SIGNAL_CENTRE_HZ).expect("front");
    let mut filtered = Vec::with_capacity(iq.len());
    let mut block = Vec::new();
    for chunk in iq.chunks(1 << 16) {
        front.process(chunk, &mut block);
        filtered.extend_from_slice(&block);
    }
    filtered
}

/// The capture holds the end of image 1 and all of image 2. The transmitter stalls on one tone
/// for a few milliseconds now and then, cutting the packet on air, so image 2 misses 58 of its
/// 1358 packets at the source.
#[test]
#[ignore = "needs WENET_CU8=<wenet_sample_fs921416Hz.cu8>"]
fn decodes_a_real_balloon_picture_off_air() {
    let Some(path) = sdrmm_test_support::reference_data::wenet_cu8() else {
        return;
    };
    let received = decode(&off_air(&path), &[1 << 16]);
    assert!(received.telemetry.iter().any(|event| {
        event.text.as_deref() == Some("PiCam Debug: Transmitting 1348 PiCam SSDV Packets.")
    }));
    let fixes = received
        .telemetry
        .iter()
        .filter(|event| event.packet == WenetPacket::Gps)
        .count();
    assert!(fixes > 30, "{fixes} GPS packets");
    let [first, second] = received.pictures.as_slice() else {
        panic!("pictures {:?}", received.pictures);
    };
    assert_eq!((first.callsign.as_str(), first.image_id), ("VK5ARG", 1));
    assert!(first.packets >= 420, "{first:?}");
    assert_eq!(second.callsign, "VK5ARG");
    assert_eq!(second.image_id, 2);
    assert_eq!((second.width, second.height), (2304, 1296));
    assert_eq!((second.quality, second.sampling.as_str()), (6, "4:2:0"));
    assert_eq!(second.packets + second.packets_lost, 1358);
    assert!(second.packets >= 1300, "{second:?}");
    assert_eq!(second.lines, 1296);
    let image = &received.images[1];
    assert_eq!(image.picture.rgb.len(), 2304 * 1296 * 3);
    if let Ok(out) = std::env::var("WENET_PPM_OUT") {
        let picture = &image.picture;
        let mut ppm = format!("P6\n{} {}\n255\n", picture.width, picture.height).into_bytes();
        ppm.extend_from_slice(&picture.rgb);
        std::fs::write(out, ppm).expect("ppm written");
    }
}

#[test]
fn the_scope_follows_the_framing_heard_and_sees_two_tones() {
    let iq = wenet_synth::transmission(WenetMode::I2s, RATE);
    let mut chan = channel();
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
    assert_eq!(tap.plane, Some(sdrmm_wire::SymbolPlane::Level));
    assert_eq!(tap.reference.len(), 2);
    assert!(tap.symbols.len() >= 40, "{} values", tap.symbols.len());
    assert_eq!(tap.symbol_rate, WenetMode::I2s.baud());
    assert!(tap.evm < 0.1, "EVM {}", tap.evm);
}
