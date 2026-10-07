use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_wire::LoraCodingRate;

use crate::lora::{
    lorawan, meshcore, meshtastic,
    phy::code::{Header, Shape, encode, low_data_rate},
};

pub const MESHTASTIC_NODE: u32 = 0x5d12_a7c3;
pub const LORAWAN_DEV_ADDR: u32 = 0x2601_1bda;
pub const LORAWAN_NWK_S_KEY: [u8; 16] = [0x44; 16];
pub const LORAWAN_APP_S_KEY: [u8; 16] = [0xec; 16];
pub const LORAWAN_APP_KEY: [u8; 16] = [0x2b; 16];
pub const LORAWAN_JOIN_EUI: u64 = 0x70b3_d57e_d000_0001;
pub const LORAWAN_DEV_EUI: u64 = 0x0004_a30b_001c_0530;
const MESHCORE_SEED: [u8; 32] = [0x5a; 32];
const MESHCORE_TIME: u32 = 1_760_000_000;
const SCENE_GAP_S: f64 = 0.3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoraWaveform {
    pub spreading_factor: u8,
    pub bandwidth_hz: f64,
    pub coding_rate: LoraCodingRate,
    pub sync_word: u8,
    pub preamble: usize,
    pub implicit_header: bool,
    pub crc: bool,
    pub inverted: bool,
}

impl Default for LoraWaveform {
    fn default() -> Self {
        Self {
            spreading_factor: 7,
            bandwidth_hz: 125_000.0,
            coding_rate: LoraCodingRate::Cr45,
            sync_word: 0x12,
            preamble: 8,
            implicit_header: false,
            crc: true,
            inverted: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Chirp {
    Up(u16),
    Down,
}

#[derive(Clone, Copy, Debug)]
struct Segment {
    start: f64,
    length: f64,
    chirp: Chirp,
}

impl LoraWaveform {
    #[must_use]
    pub fn symbols(&self, payload: &[u8]) -> Vec<u16> {
        let header = (!self.implicit_header).then_some(Header {
            length: payload.len().min(255) as u8,
            coding_rate: self.coding_rate,
            crc: self.crc,
        });
        let nibbles = encode::nibbles(payload, header, self.crc);
        let shape = Shape {
            spreading_factor: self.spreading_factor,
            coding_rate: self.coding_rate,
            low_data_rate: low_data_rate(self.spreading_factor, self.bandwidth_hz),
        };
        encode::symbols(&nibbles, shape)
    }

    fn segments(&self, payload: &[u8]) -> Vec<Segment> {
        let n = f64::from(1u32 << self.spreading_factor);
        let mut chirps: Vec<(Chirp, f64)> = vec![(Chirp::Up(0), 1.0); self.preamble];
        chirps.push((Chirp::Up(u16::from(self.sync_word >> 4) << 3), 1.0));
        chirps.push((Chirp::Up(u16::from(self.sync_word & 0x0f) << 3), 1.0));
        chirps.extend([(Chirp::Down, 1.0), (Chirp::Down, 1.0), (Chirp::Down, 0.25)]);
        chirps.extend(
            self.symbols(payload)
                .into_iter()
                .map(|s| (Chirp::Up(s), 1.0)),
        );
        let mut start = 0.0;
        chirps
            .into_iter()
            .map(|(chirp, symbols)| {
                let segment = Segment {
                    start,
                    length: symbols * n,
                    chirp,
                };
                start += symbols * n;
                segment
            })
            .collect()
    }

    #[must_use]
    pub fn chips(&self, payload: &[u8]) -> f64 {
        self.segments(payload)
            .last()
            .map_or(0.0, |segment| segment.start + segment.length)
    }

    #[must_use]
    pub fn frame(&self, payload: &[u8], oversampling: usize) -> Vec<Complex<f32>> {
        self.frame_at(payload, self.bandwidth_hz * oversampling as f64, 0.0)
    }

    #[must_use]
    pub fn frame_at(&self, payload: &[u8], sample_rate_hz: f64, delay_s: f64) -> Vec<Complex<f32>> {
        let segments = self.segments(payload);
        let n = f64::from(1u32 << self.spreading_factor);
        let chips_per_sample = self.bandwidth_hz / sample_rate_hz;
        let delay_chips = delay_s * self.bandwidth_hz;
        let total = self.chips(payload) + delay_chips;
        let count = (total / chips_per_sample).ceil() as usize;
        let mut index = 0;
        (0..count)
            .map(|m| {
                let t = m as f64 * chips_per_sample - delay_chips;
                if t < 0.0 {
                    return Complex::new(0.0, 0.0);
                }
                while index + 1 < segments.len()
                    && t >= segments[index].start + segments[index].length
                {
                    index += 1;
                }
                let segment = segments[index];
                let local = t - segment.start;
                if local >= segment.length {
                    return Complex::new(0.0, 0.0);
                }
                let turns = phase(segment.chirp, local, n);
                let sample = Complex::from_polar(1.0, (TAU * (turns - turns.floor())) as f32);
                if self.inverted { sample.conj() } else { sample }
            })
            .collect()
    }
}

#[must_use]
pub fn meshtastic_text(text: &str) -> Vec<u8> {
    meshtastic::text(
        MESHTASTIC_NODE,
        u32::MAX,
        0x0000_0101,
        "LongFast",
        &[1],
        text,
    )
}

#[must_use]
pub fn meshtastic_position(lat: f64, lon: f64) -> Vec<u8> {
    meshtastic::position(
        MESHTASTIC_NODE,
        0x0000_0102,
        "LongFast",
        &[1],
        lat,
        lon,
        420,
    )
}

#[must_use]
pub fn meshcore_advert(name: &str, location: (f64, f64)) -> Vec<u8> {
    meshcore::advert(&MESHCORE_SEED, MESHCORE_TIME, 1, name, Some(location))
}

#[must_use]
pub fn meshcore_public_text(sender: &str, text: &str) -> Vec<u8> {
    meshcore::group_text(&meshcore::PUBLIC_SECRET, MESHCORE_TIME, sender, text)
}

#[must_use]
pub fn lorawan_uplink(f_cnt: u16, payload: &[u8]) -> Vec<u8> {
    lorawan::uplink(
        LORAWAN_DEV_ADDR,
        f_cnt,
        1,
        payload,
        &LORAWAN_NWK_S_KEY,
        &LORAWAN_APP_S_KEY,
        false,
    )
}

#[must_use]
pub fn lorawan_join_request(dev_nonce: u16) -> Vec<u8> {
    lorawan::join_request(
        LORAWAN_JOIN_EUI,
        LORAWAN_DEV_EUI,
        dev_nonce,
        &LORAWAN_APP_KEY,
    )
}

#[must_use]
pub fn scene(waveform: &LoraWaveform, packets: &[Vec<u8>], rate_hz: f64) -> Vec<Complex<f32>> {
    let gap = vec![Complex::new(0.0, 0.0); (SCENE_GAP_S * rate_hz) as usize];
    let mut iq = gap.clone();
    for packet in packets {
        iq.extend(waveform.frame_at(packet, rate_hz, 0.0));
        iq.extend_from_slice(&gap);
    }
    iq
}

#[must_use]
pub fn meshtastic_scene(rate_hz: f64) -> Vec<Complex<f32>> {
    let waveform = LoraWaveform {
        spreading_factor: 11,
        bandwidth_hz: 250_000.0,
        sync_word: 0x2b,
        preamble: 16,
        ..LoraWaveform::default()
    };
    let packets = [
        meshtastic_text("SDR-- MESH FIXTURE"),
        meshtastic_position(47.3769, 8.5417),
    ];
    scene(&waveform, &packets, rate_hz)
}

#[must_use]
pub fn meshcore_scene(rate_hz: f64) -> Vec<Complex<f32>> {
    let waveform = LoraWaveform {
        spreading_factor: 8,
        bandwidth_hz: 62_500.0,
        coding_rate: LoraCodingRate::Cr48,
        preamble: 16,
        ..LoraWaveform::default()
    };
    let packets = [
        meshcore_advert("SDR-- Repeater", (52.52, 13.405)),
        meshcore_public_text("sdrmm", "SDR-- MESHCORE FIXTURE"),
    ];
    scene(&waveform, &packets, rate_hz)
}

#[must_use]
pub fn lorawan_scene(rate_hz: f64) -> Vec<Complex<f32>> {
    let waveform = LoraWaveform {
        spreading_factor: 9,
        sync_word: 0x34,
        ..LoraWaveform::default()
    };
    let packets = [
        lorawan_join_request(0x1d2c),
        lorawan_uplink(1, b"21.5C"),
        lorawan_uplink(2, b"21.6C"),
    ];
    scene(&waveform, &packets, rate_hz)
}

fn phase(chirp: Chirp, t: f64, n: f64) -> f64 {
    match chirp {
        Chirp::Down => -(t * t / (2.0 * n) - t / 2.0),
        Chirp::Up(value) => {
            let s = f64::from(value);
            let wrapped = (t - (n - s)).max(0.0);
            t * t / (2.0 * n) + (s / n - 0.5) * t - wrapped
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lora::phy::code::HEADER_SYMBOLS;

    #[test]
    fn a_frame_spans_preamble_sync_sfd_and_payload() {
        let waveform = LoraWaveform::default();
        let payload = [0xde, 0xad, 0xbe, 0xef];
        let symbols = waveform.symbols(&payload);
        assert_eq!(symbols.len(), 18);
        assert!(symbols.len() >= HEADER_SYMBOLS);
        let chips = waveform.chips(&payload);
        assert_eq!(chips, (8.0 + 2.0 + 2.25 + 18.0) * 128.0);
        let iq = waveform.frame(&payload, 2);
        assert_eq!(iq.len(), (chips * 2.0) as usize);
        assert!(iq.iter().all(|s| (s.norm() - 1.0).abs() < 1e-4));
    }

    #[test]
    fn phase_is_continuous_across_every_boundary() {
        let waveform = LoraWaveform::default();
        let iq = waveform.frame(b"continuity", 8);
        let worst = iq
            .windows(2)
            .map(|pair| (pair[1] * pair[0].conj()).arg().abs())
            .fold(0.0f32, f32::max);
        assert!(worst < std::f32::consts::PI / 7.0, "phase step {worst}");
    }
}
