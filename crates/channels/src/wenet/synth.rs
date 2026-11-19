use num_complex::Complex;
use sdrmm_modem::ssdv::{EncoderSettings, McuMode, PacketType, encode};
use sdrmm_modem::synth::{Render, Signal, WIDE_FM_HZ, fm_modulate, resample};

use crate::wenet::frame::{PACKET_LEN, code, frame_bits};
use crate::wenet::packet::build;
use crate::wenet::{INPUT_RATE_HZ, WenetMode};

pub const RATE: f64 = INPUT_RATE_HZ;
pub const CALLSIGN: &str = "VK5ARG";
pub const IMAGE_ID: u8 = 4;
pub const WIDTH: u16 = 96;
pub const HEIGHT: u16 = 64;
pub const MESSAGE: &str = "Wenet test";
pub const LAT: f32 = -34.95;
pub const LON: f32 = 138.52;
pub const ALTITUDE_M: f32 = 25_000.0;
const LEAD_BITS: usize = 2_000;
const TAIL_BITS: usize = 400;

pub(crate) static SIGNALS: &[Signal] = &[Signal {
    id: "wenet",
    label: "Wenet",
    rate_hz: WIDE_FM_HZ,
    render: Render::Always(wenet),
}];

fn wenet() -> Vec<Complex<f32>> {
    resample(&transmission(WenetMode::Uart, RATE), RATE, WIDE_FM_HZ)
}

/// The test picture, three bytes a pixel.
#[must_use]
pub fn scene() -> Vec<u8> {
    let (w, h) = (usize::from(WIDTH), usize::from(HEIGHT));
    (0..w * h)
        .flat_map(|index| {
            let (x, y) = ((index % w) as f32, (index / w) as f32);
            let sky = 255.0 - 1.5 * y;
            let sun: f32 = if (x - 70.0).hypot(y - 18.0) < 10.0 {
                255.0
            } else {
                0.0
            };
            [
                sun.max(60.0 + x) as u8,
                (sun * 0.9).max(120.0 + 0.5 * x) as u8,
                sky as u8,
            ]
        })
        .collect()
}

/// The SSDV packets of [`scene`].
#[must_use]
pub fn ssdv_packets() -> Vec<[u8; PACKET_LEN]> {
    let settings = EncoderSettings {
        callsign: CALLSIGN.to_owned(),
        image_id: IMAGE_ID,
        quality: 6,
        packet_type: PacketType::NoFec,
        mcu_mode: McuMode::Square,
        packet_len: PACKET_LEN,
    };
    encode(&settings, WIDTH, HEIGHT, &scene())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|packet| packet.try_into().ok())
        .collect()
}

/// A text message, a GPS fix, then the picture.
#[must_use]
pub fn packets() -> Vec<[u8; PACKET_LEN]> {
    let mut packets = vec![
        build::text(1, MESSAGE),
        build::gps(2_400, 345_600_000, LAT, LON, ALTITUDE_M, 5.5),
    ];
    packets.extend(ssdv_packets());
    packets
}

const fn deviation_hz(mode: WenetMode) -> f64 {
    match mode {
        WenetMode::Uart => 60_000.0,
        WenetMode::I2s => 48_000.0,
    }
}

/// 2-FSK at `rate` of every packet in `packets`, framed as `mode` sends them.
#[must_use]
pub fn modulate(mode: WenetMode, packets: &[[u8; PACKET_LEN]], rate: f64) -> Vec<Complex<f32>> {
    let code = code();
    let mut bits: Vec<bool> = (0..LEAD_BITS).map(|index| index % 2 == 1).collect();
    for packet in packets {
        bits.extend(frame_bits(mode, &code, packet));
    }
    bits.extend((0..TAIL_BITS).map(|index| index % 2 == 1));
    let samples = (bits.len() as f64 * rate / mode.baud()) as usize;
    let audio: Vec<f32> = (0..samples)
        .map(|n| {
            let bit = (n as f64 * mode.baud() / rate) as usize;
            if bits.get(bit).copied().unwrap_or(false) {
                1.0
            } else {
                -1.0
            }
        })
        .collect();
    fm_modulate(&audio, deviation_hz(mode), rate)
}

#[must_use]
pub fn transmission(mode: WenetMode, rate: f64) -> Vec<Complex<f32>> {
    modulate(mode, &packets(), rate)
}
