use num_complex::Complex;
use sdrmm_modem_test_support::ber::rng::Rng;

use super::{Frame, Receiver, Sink, WifiPhy, append_fcs, fcs_ok, transmit};

#[derive(Default)]
struct Collect {
    frames: Vec<(Vec<u8>, WifiPhy)>,
    only: Option<u8>,
}

impl Sink for Collect {
    fn accepts(&self, frame_control: u8) -> bool {
        self.only.is_none_or(|only| only == frame_control)
    }

    fn frame(&mut self, frame: Frame<'_>) {
        self.frames.push((frame.mpdu.to_vec(), frame.phy));
    }
}

fn mpdu(control: u8, len: usize) -> Vec<u8> {
    let mut body = vec![control, 0];
    body.extend((0..len).map(|k| (k * 7 + 3) as u8));
    append_fcs(body)
}

fn receive(wave: &[Complex<f32>], sink: &mut Collect) {
    let mut padded = vec![Complex::new(0.0, 0.0); 4_000];
    padded.extend_from_slice(wave);
    padded.extend(vec![Complex::new(0.0, 0.0); 30_000]);
    let mut receiver = Receiver::new().unwrap();
    for block in padded.chunks(4_096) {
        receiver.process(block, sink);
    }
}

#[test]
fn every_rate_loops_back() {
    let frame = mpdu(0x80, 180);
    let waves: Vec<(WifiPhy, Vec<Complex<f32>>)> = [
        (WifiPhy::Dsss1m, false),
        (WifiPhy::Dsss2m, true),
        (WifiPhy::Cck5m5, false),
        (WifiPhy::Cck11m, true),
    ]
    .into_iter()
    .map(|(phy, short)| (phy, transmit::dsss(&frame, phy, short).unwrap()))
    .chain(
        [6u8, 9, 12, 18, 24, 36, 48, 54]
            .into_iter()
            .map(|mbps| (WifiPhy::Ofdm { mbps }, transmit::ofdm(&frame, mbps))),
    )
    .collect();
    for (phy, wave) in waves {
        let mut sink = Collect::default();
        receive(&wave, &mut sink);
        assert_eq!(sink.frames, vec![(frame.clone(), phy)], "{phy:?}");
    }
}

#[test]
fn a_rejected_frame_control_is_never_handed_over() {
    for wave in [
        transmit::dsss(&mpdu(0x08, 400), WifiPhy::Dsss1m, false).unwrap(),
        transmit::ofdm(&mpdu(0x08, 400), 24),
    ] {
        let mut sink = Collect {
            only: Some(0x80),
            ..Collect::default()
        };
        receive(&wave, &mut sink);
        assert!(sink.frames.is_empty());
    }
}

#[test]
fn the_frame_check_sequence_is_the_ieee_crc() {
    let frame = mpdu(0x80, 10);
    assert!(fcs_ok(&frame));
    let mut broken = frame.clone();
    broken[3] ^= 1;
    assert!(!fcs_ok(&broken));
    assert!(!fcs_ok(&[1, 2]));
}

/// The wave with a carrier offset, a sample clock off by `ppm` and white noise of `sigma` per
/// component. A unit chip over `sigma` 0.95 is 0 dB chip SNR.
fn impaired(
    wave: &[Complex<f32>],
    cfo_hz: f64,
    ppm: f64,
    sigma: f32,
    seed: u64,
) -> Vec<Complex<f32>> {
    let mut rng = Rng::new(seed);
    let step = 1.0 + ppm * 1e-6;
    let len = ((wave.len() - 1) as f64 / step) as usize;
    (0..len)
        .map(|n| {
            let t = n as f64 * step;
            let index = t.floor() as usize;
            let weight = (t - index as f64) as f32;
            let sample = wave[index] * (1.0 - weight) + wave[index + 1] * weight;
            let turn = std::f64::consts::TAU * cfo_hz * n as f64 / super::RATE_HZ;
            let (a, b) = rng.normal_pair();
            sample * Complex::from_polar(1.0, turn as f32)
                + Complex::new(a as f32, b as f32) * sigma
        })
        .collect()
}

#[test]
fn a_long_cck_frame_survives_carrier_and_clock_offset() {
    let frame = mpdu(0x80, 1_500);
    for phy in [WifiPhy::Cck5m5, WifiPhy::Cck11m] {
        let clean = transmit::dsss(&frame, phy, false).unwrap();
        for (cfo_hz, ppm) in [(200_000.0, 100.0), (-200_000.0, -100.0)] {
            let mut sink = Collect::default();
            receive(&impaired(&clean, cfo_hz, ppm, 0.1, 7), &mut sink);
            assert_eq!(
                sink.frames,
                vec![(frame.clone(), phy)],
                "{phy:?} {cfo_hz} Hz {ppm} ppm"
            );
        }
    }
}

#[test]
fn every_rate_decodes_near_its_noise_floor() {
    let frame = mpdu(0x80, 100);
    for (phy, sigma) in [
        (WifiPhy::Dsss1m, 0.95),
        (WifiPhy::Dsss2m, 0.6),
        (WifiPhy::Cck5m5, 0.6),
        (WifiPhy::Cck11m, 0.4),
    ] {
        let clean = transmit::dsss(&frame, phy, false).unwrap();
        let decoded = (0..10)
            .filter(|&seed| {
                let mut sink = Collect::default();
                receive(&impaired(&clean, 30_000.0, 20.0, sigma, seed), &mut sink);
                sink.frames == vec![(frame.clone(), phy)]
            })
            .count();
        assert!(decoded >= 8, "{phy:?}: {decoded} of 10");
    }
}
