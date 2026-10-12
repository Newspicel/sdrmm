use sdrmm_dsp::lfsr_digest16;
use sdrmm_wire::{IsmIntegrity, IsmMessage};

use crate::ism::{
    bits::{Bits, add_bytes},
    device::{Device, Found, Outcome, fsk, reading},
    slicer::Timing,
};

pub(super) const FIVE_IN_ONE: Device = fsk(Timing::new(124, 124, 25_000), five_in_one);

pub(super) const SIX_IN_ONE: Device =
    fsk(Timing::new(124, 124, 25_000), six_in_one).integrity(IsmIntegrity::Crc);

const FIVE_PREAMBLE: [u8; 5] = [0xaa, 0xaa, 0xaa, 0x2d, 0xd4];
const SIX_PREAMBLE: [u8; 4] = [0xaa, 0xaa, 0x2d, 0xd4];
const FIVE_BYTES: usize = 26;
const SIX_BYTES: usize = 18;

fn digits(nibbles: &[u8]) -> u32 {
    nibbles
        .iter()
        .fold(0, |value, &nibble| value * 10 + u32::from(nibble))
}

fn payload<const N: usize>(
    bits: &Bits,
    preamble: &[u8],
    min_bits: usize,
    slack: usize,
) -> Option<[u8; N]> {
    let len = bits.len(0);
    if bits.rows() != 1 || !(min_bits..=440).contains(&len) {
        return None;
    }
    let found = bits.search(0, 0, preamble, preamble.len() * 8);
    let start = found + preamble.len() * 8;
    if found == len || len - start + slack < N * 8 {
        return None;
    }
    let mut msg = [0u8; N];
    bits.extract(0, start, &mut msg, (len - start).min(N * 8));
    Some(msg)
}

fn five_in_one(bits: &mut Bits, found: &mut Found) -> Outcome {
    let Some(m) = payload::<FIVE_BYTES>(bits, &FIVE_PREAMBLE, 248, 7) else {
        return Outcome::Skip;
    };
    if (0..13).any(|col| m[col] ^ m[col + 13] != 0xff) {
        return Outcome::Mic;
    }
    let temperature = digits(&[m[21] & 0x0f, m[20] >> 4, m[20] & 0x0f]) as f32 * 0.1;
    let temperature = if m[25] & 0x0f != 0 {
        -temperature
    } else {
        temperature
    };
    let gust = (u32::from(m[17] & 0x0f) << 8) + u32::from(m[16]);
    let rain = digits(&[m[24] >> 4, m[24] & 0x0f, m[23] >> 4, m[23] & 0x0f]) as f32 * 0.1;
    let rain_gauge = (0x39..=0x3b).contains(&(m[15] & 0x7f));
    let model = if rain_gauge {
        "Bresser-ProRainGauge"
    } else {
        "Bresser-5in1"
    };
    Outcome::emit(
        found,
        IsmMessage {
            battery_ok: Some(m[25] & 0x80 == 0),
            temperature_c: (m[20] & 0x0f <= 9).then_some(temperature),
            humidity_pct: (!rain_gauge && m[22] & 0x0f <= 9)
                .then(|| (m[22] >> 4) * 10 + (m[22] & 0x0f)),
            wind_max_m_s: (!rain_gauge).then_some(gust as f32 * 0.1),
            wind_avg_m_s: (!rain_gauge)
                .then(|| digits(&[m[19] & 0x0f, m[18] >> 4, m[18] & 0x0f]) as f32 * 0.1),
            wind_dir_deg: (!rain_gauge).then(|| f32::from(m[17] >> 4) * 22.5),
            rain_mm: Some(if rain_gauge { rain * 2.5 } else { rain }),
            ..reading(model, m[14].to_string(), &m, FIVE_BYTES * 8)
        },
    )
}

fn six_in_one(bits: &mut Bits, found: &mut Found) -> Outcome {
    let Some(mut m) = payload::<SIX_BYTES>(bits, &SIX_PREAMBLE, 160, 0) else {
        return Outcome::Skip;
    };
    let digest = lfsr_digest16(0x8810, 0x5412, &m[2..17]);
    if digest != u16::from_be_bytes([m[0], m[1]]) || add_bytes(&m[2..18]) & 0xff != 0xff {
        return Outcome::Mic;
    }
    let raw = m;
    let id = u32::from_be_bytes([m[2], m[3], m[4], m[5]]);
    let kind = m[6] >> 4;
    let climate_only = kind == 2 || kind == 4;
    let temp_ok = m[12] <= 0x99 && m[13] & 0xf0 <= 0x90;
    let temp_raw = digits(&[m[12] >> 4, m[12] & 0x0f, m[13] >> 4]) as f32;
    let mut temperature = if (m[13] >> 3) & 1 == 1 {
        (temp_raw - 1000.0) * 0.1
    } else {
        temp_raw * 0.1
    };
    if temperature < -50.0 {
        temperature = -temp_raw * 0.1;
    }
    let humidity = (m[14] >> 4) * 10 + (m[14] & 0x0f);
    let uv_ok = m[16] & 0x0f == 0 && !m[15] <= 0x99 && !m[16] & 0xf0 <= 0x90 && !climate_only;
    let uv = digits(&[!m[15] >> 4, !m[15] & 0x0f, !m[16] >> 4]) as f32 * 0.1;
    for byte in &mut m[7..10] {
        *byte = !*byte;
    }
    let wind_ok = m[7] <= 0x99 && m[8] <= 0x99 && m[9] <= 0x99 && !climate_only;
    let gust = digits(&[m[7] >> 4, m[7] & 0x0f, m[8] >> 4]) as f32 * 0.1;
    let average = digits(&[m[9] >> 4, m[9] & 0x0f, m[8] & 0x0f]) as f32 * 0.1;
    let direction = digits(&[m[10] >> 4, m[10] & 0x0f, m[11] >> 4]) as f32;
    let rain_ok = m[16] & 1 == 1;
    let rain = digits(&[
        !m[12] >> 4,
        !m[12] & 0x0f,
        !m[13] >> 4,
        !m[13] & 0x0f,
        !m[14] >> 4,
        !m[14] & 0x0f,
    ]) as f32
        * 0.1;
    Outcome::emit(
        found,
        IsmMessage {
            channel: Some((m[6] & 0x07).to_string()),
            battery_ok: (!rain_ok).then_some((m[13] >> 1) & 1 == 1),
            temperature_c: (temp_ok && !rain_ok).then_some(temperature),
            humidity_pct: (temp_ok && !rain_ok && kind != 4).then_some(humidity),
            wind_max_m_s: wind_ok.then_some(gust),
            wind_avg_m_s: wind_ok.then_some(average),
            wind_dir_deg: wind_ok.then_some(direction),
            rain_mm: rain_ok.then_some(rain),
            uv_index: uv_ok.then_some(uv),
            ..reading("Bresser-6in1", id.to_string(), &raw, SIX_BYTES * 8)
        },
    )
}
