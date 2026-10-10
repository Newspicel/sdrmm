use num_complex::Complex;

use super::{
    BlePhy, Lane, Packet, RATE_HZ, Sink, Whitener, channel_index, crc_ok, crc24, rf_channel_at,
    rf_channel_hz, transmit,
};

#[derive(Default)]
pub(crate) struct Collect {
    pub packets: Vec<(Vec<u8>, BlePhy)>,
}

impl Sink for Collect {
    fn packet(&mut self, packet: Packet<'_>) {
        self.packets.push((packet.pdu.to_vec(), packet.phy));
    }
}

pub(crate) fn pdu(length: usize) -> Vec<u8> {
    let mut pdu = vec![0x42, length as u8];
    pdu.extend((0..length).map(|k| (k * 29 + 7) as u8));
    pdu
}

pub(crate) fn noise(iq: &mut [Complex<f32>], seed: u64, sigma: f32) {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut uniform = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        ((state >> 11) as f64 / (1u64 << 53) as f64).max(1e-300)
    };
    for sample in iq {
        let radius = (-2.0 * uniform().ln()).sqrt();
        let angle = std::f64::consts::TAU * uniform();
        *sample += Complex::new(
            (radius * angle.cos()) as f32 * sigma,
            (radius * angle.sin()) as f32 * sigma,
        );
    }
}

pub(crate) fn on_air(
    pdu: &[u8],
    rf: u8,
    phy: BlePhy,
    offset_hz: f64,
    snr_db: f64,
    seed: u64,
) -> Vec<Complex<f32>> {
    let mut iq = vec![Complex::new(0.0, 0.0); 3_000];
    iq.extend(transmit::packet(pdu, channel_index(rf), phy, RATE_HZ));
    iq.extend(vec![Complex::new(0.0, 0.0); 3_000]);
    let step = std::f64::consts::TAU * offset_hz / RATE_HZ;
    for (n, sample) in iq.iter_mut().enumerate() {
        *sample *= Complex::from_polar(1.0, (step * n as f64) as f32);
    }
    let sigma = (10f64.powf(-snr_db / 10.0) / 2.0).sqrt() as f32;
    noise(&mut iq, seed, sigma);
    iq
}

pub(crate) fn receive(iq: &[Complex<f32>], rf: u8) -> Collect {
    let mut lane = Lane::new(Some(rf), channel_index(rf)).unwrap();
    let mut sink = Collect::default();
    for block in iq.chunks(2_048) {
        lane.process(block, &mut sink);
    }
    sink
}

#[test]
fn every_phy_loops_back_with_an_offset_carrier() {
    for phy in [BlePhy::Le1m, BlePhy::CodedS8, BlePhy::CodedS2] {
        let pdu = pdu(37);
        let found = receive(&on_air(&pdu, 12, phy, 61e3, 25.0, 3), 12);
        assert_eq!(found.packets, vec![(pdu.clone(), phy)], "{phy:?}");
    }
}

#[test]
fn whitening_follows_the_spec_register_for_every_channel() {
    for channel in 0..40u8 {
        let mut register: [bool; 7] =
            std::array::from_fn(|position| position == 0 || channel >> (6 - position) & 1 == 1);
        let mut whitener = Whitener::new(channel);
        for _ in 0..64 {
            let out = register[6];
            register = [
                out,
                register[0],
                register[1],
                register[2],
                register[3] ^ out,
                register[4],
                register[5],
            ];
            assert_eq!(whitener.bit(), out, "channel {channel}");
        }
    }
}

#[test]
fn the_crc_covers_the_pdu_least_significant_byte_first() {
    let pdu = pdu(10);
    let crc = crc24(&pdu);
    let mut framed = pdu.clone();
    framed.extend_from_slice(&crc.to_le_bytes()[..3]);
    assert!(crc_ok(&framed));
    framed[4] ^= 0x10;
    assert!(!crc_ok(&framed));
}

#[test]
fn channels_map_between_frequency_and_index() {
    assert_eq!(rf_channel_at(2_402e6), Some(0));
    assert_eq!(rf_channel_at(2_426.3e6), Some(12));
    assert_eq!(rf_channel_at(2_403e6), None);
    assert_eq!(rf_channel_at(145e6), None);
    assert_eq!(channel_index(0), 37);
    assert_eq!(channel_index(12), 38);
    assert_eq!(channel_index(39), 39);
    assert_eq!(channel_index(13), 11);
    assert_eq!(rf_channel_hz(39), 2_480e6);
}

fn drifted(iq: &[Complex<f32>], ppm: f64) -> Vec<Complex<f32>> {
    let step = 1.0 + ppm * 1e-6;
    let mut out = Vec::with_capacity(iq.len());
    let mut t = 0.0f64;
    while (t as usize) + 1 < iq.len() {
        let index = t.floor() as usize;
        let weight = (t - index as f64) as f32;
        out.push(iq[index] * (1.0 - weight) + iq[index + 1] * weight);
        t += step;
    }
    out
}

fn decoded_share(phy: BlePhy, length: usize, snr_db: f64, offset_hz: f64, ppm: f64) -> f64 {
    let trials = 24u64;
    let pdu = pdu(length);
    let good = (0..trials)
        .filter(|&seed| {
            let air = drifted(&on_air(&pdu, 12, phy, offset_hz, snr_db, 7_000 + seed), ppm);
            receive(&air, 12)
                .packets
                .iter()
                .any(|packet| packet.0 == pdu)
        })
        .count();
    good as f64 / trials as f64
}

#[test]
fn each_phy_meets_its_sensitivity_with_offset_and_clock_error() {
    for (phy, snr_db) in [
        (BlePhy::Le1m, 5.0),
        (BlePhy::CodedS2, 0.0),
        (BlePhy::CodedS8, -3.0),
    ] {
        for (offset_hz, ppm) in [(-120e3, 40.0), (60e3, -40.0)] {
            let share = decoded_share(phy, 37, snr_db, offset_hz, ppm);
            assert!(share >= 0.9, "{phy:?} at {snr_db} dB: {share}");
        }
    }
}

#[test]
fn long_range_reaches_further_than_one_megabit() {
    assert!(decoded_share(BlePhy::Le1m, 37, -1.0, 30e3, 0.0) < 0.2);
    assert!(decoded_share(BlePhy::CodedS2, 37, -1.0, 30e3, 0.0) >= 0.7);
    assert!(decoded_share(BlePhy::CodedS8, 37, -4.0, 30e3, 0.0) >= 0.7);
}

#[test]
fn a_full_length_long_range_packet_survives_clock_error() {
    for ppm in [-40.0, 40.0] {
        let share = decoded_share(BlePhy::CodedS8, 250, -2.0, 80e3, ppm);
        assert!(share >= 0.8, "{ppm} ppm: {share}");
    }
}

#[test]
fn each_packet_is_reported_once() {
    for phy in [BlePhy::Le1m, BlePhy::CodedS8, BlePhy::CodedS2] {
        for seed in 0..20 {
            let pdu = pdu(20);
            let found = receive(&on_air(&pdu, 12, phy, 0.0, 15.0, seed), 12);
            assert_eq!(found.packets.len(), 1, "{phy:?} seed {seed}");
        }
    }
}

#[test]
fn a_packet_that_fails_its_crc_is_counted_once() {
    for (phy, damage) in [(BlePhy::Le1m, 400), (BlePhy::CodedS8, 3_000)] {
        let mut iq = on_air(&pdu(60), 12, phy, 0.0, 30.0, 5);
        let clean = {
            let mut lane = Lane::new(Some(12), channel_index(12)).unwrap();
            let mut sink = Collect::default();
            lane.process(&iq, &mut sink);
            lane.process(&vec![Complex::new(0.0, 0.0); 40_000], &mut sink);
            assert_eq!(sink.packets.len(), 1, "{phy:?}");
            lane.rejected()
        };
        assert_eq!(clean, 0, "{phy:?}");
        let middle = iq.len() / 2;
        for sample in &mut iq[middle..middle + damage] {
            *sample = sample.conj();
        }
        let mut lane = Lane::new(Some(12), channel_index(12)).unwrap();
        let mut sink = Collect::default();
        lane.process(&iq, &mut sink);
        lane.process(&vec![Complex::new(0.0, 0.0); 40_000], &mut sink);
        assert!(sink.packets.is_empty(), "{phy:?}");
        assert_eq!(lane.rejected(), 1, "{phy:?}");
    }
}
