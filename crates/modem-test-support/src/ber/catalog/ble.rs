use num_complex::Complex;
use sdrmm_modem::ble::{BlePhy, Lane, Packet, RATE_HZ, Sink, channel_index, transmit};

use super::Measurement;
use crate::ber::sweep::Link;

pub const PAYLOAD_BYTES: usize = 37;
pub const PAYLOAD_BITS: usize = PAYLOAD_BYTES * 8;
pub const RF_CHANNEL: u8 = 12;
pub const GUARD_SAMPLES: usize = 2_000;
pub const FULL_CAP: u64 = 3_000_000;

pub const LE1M_GRID: &[f64] = &[10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0];
pub const LE1M_SEED: u64 = 0xb1e1;
pub const LE1M_AWGN: &str = "ble/le1m_awgn";
pub const CODED_S2_GRID: &[f64] = &[9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0];
pub const CODED_S2_SEED: u64 = 0xb1e2;
pub const CODED_S2_AWGN: &str = "ble/coded_s2_awgn";
pub const CODED_S8_GRID: &[f64] = &[10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0];
pub const CODED_S8_SEED: u64 = 0xb1e8;
pub const CODED_S8_AWGN: &str = "ble/coded_s8_awgn";

fn bytes_of(bits: &[bool]) -> Vec<u8> {
    bits.chunks(8)
        .map(|byte| {
            byte.iter()
                .enumerate()
                .fold(0u8, |acc, (bit, &value)| acc | u8::from(value) << bit)
        })
        .collect()
}

fn bits_of(bytes: &[u8]) -> Vec<bool> {
    bytes
        .iter()
        .flat_map(|&byte| (0..8).map(move |bit| byte >> bit & 1 == 1))
        .collect()
}

#[derive(Default)]
struct First {
    payload: Option<Vec<u8>>,
}

impl Sink for First {
    fn packet(&mut self, packet: Packet<'_>) {
        if self.payload.is_none() {
            self.payload = packet.pdu.get(2..).map(<[u8]>::to_vec);
        }
    }
}

fn link(phy: BlePhy, label: &str) -> Link {
    let rf = RF_CHANNEL;
    Link {
        label: label.to_owned(),
        bits_per_trial: PAYLOAD_BITS,
        modulate: Box::new(move |bits| {
            let mut pdu = vec![0x42, PAYLOAD_BYTES as u8];
            pdu.extend(bytes_of(bits));
            let mut wave = vec![Complex::new(0.0, 0.0); GUARD_SAMPLES];
            wave.extend(transmit::packet(&pdu, channel_index(rf), phy, RATE_HZ));
            wave.extend(vec![Complex::new(0.0, 0.0); GUARD_SAMPLES]);
            wave
        }),
        demodulate: Box::new(move |wave| {
            let Ok(mut lane) = Lane::new(Some(rf), channel_index(rf)) else {
                return Vec::new();
            };
            let mut first = First::default();
            lane.process(wave, &mut first);
            first
                .payload
                .map(|payload| bits_of(&payload))
                .unwrap_or_default()
        }),
    }
}

#[must_use]
pub fn le1m_link() -> Link {
    link(
        BlePhy::Le1m,
        "ble le 1m: gfsk bt0.5 h0.5 at 4 sps, preamble + access address sync, laurent \
         matched filter, decision-fed carrier, whitening and crc-24, 37-byte payload, \
         a missed packet counts every bit",
    )
}

#[must_use]
pub fn coded_s2_link() -> Link {
    link(
        BlePhy::CodedS2,
        "ble le coded s=2: coded access address sync, laurent matched filter, decision-fed \
         carrier, k=4 soft viterbi, 37-byte payload, a missed packet counts every bit",
    )
}

#[must_use]
pub fn coded_s8_link() -> Link {
    link(
        BlePhy::CodedS8,
        "ble le coded s=8: coded access address sync, pilot-aided coherent pattern \
         detection, k=4 soft viterbi, 37-byte payload, a missed packet counts every bit",
    )
}

pub const MEASUREMENTS: &[Measurement] = &[
    Measurement::committed(LE1M_AWGN, le1m_link, LE1M_GRID, LE1M_SEED, FULL_CAP),
    Measurement::committed(
        CODED_S2_AWGN,
        coded_s2_link,
        CODED_S2_GRID,
        CODED_S2_SEED,
        FULL_CAP,
    ),
    Measurement::committed(
        CODED_S8_AWGN,
        coded_s8_link,
        CODED_S8_GRID,
        CODED_S8_SEED,
        FULL_CAP,
    ),
];
