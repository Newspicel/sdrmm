use num_complex::Complex;
use sdrmm_modem::wifi::{Frame, Receiver, Sink, WifiPhy, append_fcs, transmit};

use super::Measurement;
use crate::ber::sweep::Link;

pub const PAYLOAD_BYTES: usize = 100;
pub const PAYLOAD_BITS: usize = PAYLOAD_BYTES * 8;
pub const GUARD_SAMPLES: usize = 4_000;
pub const FULL_CAP: u64 = 3_000_000;
const HEADER: [u8; 24] = [0x80; 24];

pub const DSSS_GRID: &[f64] = &[11.0, 12.0, 13.0, 14.0, 15.0, 16.0];
pub const DSSS_SEED: u64 = 0x1f11;
pub const DSSS_AWGN: &str = "wifi/dsss1m_awgn";
pub const OFDM_GRID: &[f64] = &[11.0, 12.0, 13.0, 14.0, 15.0, 16.0];
pub const OFDM_SEED: u64 = 0x1f16;
pub const OFDM_AWGN: &str = "wifi/ofdm6m_awgn";

fn bytes_of(bits: &[bool]) -> Vec<u8> {
    bits.chunks(8)
        .map(|byte| {
            byte.iter()
                .enumerate()
                .fold(0u8, |acc, (bit, &value)| acc | u8::from(value) << bit)
        })
        .collect()
}

#[derive(Default)]
struct First {
    payload: Option<Vec<u8>>,
}

impl Sink for First {
    fn accepts(&self, frame_control: u8) -> bool {
        frame_control == HEADER[0]
    }

    fn frame(&mut self, frame: Frame<'_>) {
        if self.payload.is_none() {
            let end = frame.mpdu.len().saturating_sub(4);
            self.payload = frame.mpdu.get(HEADER.len()..end).map(<[u8]>::to_vec);
        }
    }
}

fn link(phy: WifiPhy, label: &str) -> Link {
    Link {
        label: label.to_owned(),
        bits_per_trial: PAYLOAD_BITS,
        modulate: Box::new(move |bits| {
            let mut mpdu = HEADER.to_vec();
            mpdu.extend(bytes_of(bits));
            let mpdu = append_fcs(mpdu);
            let mut wave = vec![Complex::new(0.0, 0.0); GUARD_SAMPLES];
            wave.extend(match phy {
                WifiPhy::Ofdm { mbps } => transmit::ofdm(&mpdu, mbps),
                _ => transmit::dsss(&mpdu, phy, false).unwrap_or_default(),
            });
            wave.extend(vec![Complex::new(0.0, 0.0); GUARD_SAMPLES]);
            wave
        }),
        demodulate: Box::new(|wave| {
            let Ok(mut receiver) = Receiver::new() else {
                return Vec::new();
            };
            let mut first = First::default();
            receiver.process(wave, &mut first);
            first
                .payload
                .map(|payload| {
                    payload
                        .iter()
                        .flat_map(|&byte| (0..8).map(move |bit| byte >> bit & 1 == 1))
                        .collect()
                })
                .unwrap_or_default()
        }),
    }
}

#[must_use]
pub fn dsss_link() -> Link {
    link(
        WifiPhy::Dsss1m,
        "802.11b dsss 1 mbit/s at 20 MS/s: barker correlation detector, differential \
         dbpsk, long preamble, plcp crc, 100-byte payload with fcs, a missed frame counts \
         every bit",
    )
}

#[must_use]
pub fn ofdm_link() -> Link {
    link(
        WifiPhy::Ofdm { mbps: 6 },
        "802.11a/g ofdm 6 mbit/s at 20 MS/s: stf plateau detector, ltf timing and channel \
         estimate, pilot phase tracking, k=7 soft viterbi, 100-byte payload with fcs, a \
         missed frame counts every bit",
    )
}

pub const MEASUREMENTS: &[Measurement] = &[
    Measurement::committed(DSSS_AWGN, dsss_link, DSSS_GRID, DSSS_SEED, FULL_CAP),
    Measurement::committed(OFDM_AWGN, ofdm_link, OFDM_GRID, OFDM_SEED, FULL_CAP),
];
