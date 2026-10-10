use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_modem::pulse::{self, Norm};
use sdrmm_wire::DectBand;

use crate::dect::{
    bfield::{B_BYTES, BField},
    burst::{
        BIT_RATE_HZ, DEVIATION_HZ, FRAME_SAMPLES, INPUT_RATE_HZ, PP_SYNC, RFP_SYNC,
        SLOTS_PER_FRAME, SPS,
    },
    g726::G726,
    mac::append_r_crc,
    wideband,
};

const S_FIELD_BITS: usize = 32;
const A_FIELD_BITS: usize = 64;
const B_FIELD_BITS: usize = 324;
const GAUSSIAN_BT: f64 = 0.5;
const GAUSSIAN_SPAN: usize = 3;
const VOICE_PER_FRAME: usize = 80;
const MULTIFRAME: usize = 16;
const QT_FRAME: usize = 8;
const DUPLEX_SLOTS: u8 = 12;

pub const SLOT_SAMPLES: u64 = FRAME_SAMPLES / SLOTS_PER_FRAME;

#[derive(Clone, Copy, Debug)]
pub struct Station {
    pub rfpi: u64,
    pub slot: u8,
    pub carrier: u8,
    pub slot_pair: u8,
    pub rf_carriers: u16,
    pub transceivers: u8,
    pub pscn: u8,
    pub capabilities: u64,
}

impl Default for Station {
    fn default() -> Self {
        Self {
            rfpi: 0x0001_2345_6780,
            slot: 2,
            carrier: 3,
            slot_pair: 2,
            rf_carriers: 0x3FF,
            transceivers: 0,
            pscn: 5,
            capabilities: 0,
        }
    }
}

#[must_use]
pub fn header(ta: u8, ba: u8) -> u8 {
    ((ta & 0x07) << 5) | ((ba & 0x07) << 1)
}

#[must_use]
pub fn a_field(header: u8, tail: u64) -> u64 {
    append_r_crc((u64::from(header) << 56) | ((tail & ((1u64 << 40) - 1)) << 16))
}

#[must_use]
pub fn nt(rfpi: u64) -> u64 {
    a_field(header(3, 7), rfpi)
}

#[must_use]
pub fn qt_static(station: &Station) -> u64 {
    a_field(header(4, 7), static_tail(station))
}

fn static_tail(station: &Station) -> u64 {
    (u64::from(station.slot_pair & 0x0F) << 32)
        | (u64::from(station.transceivers & 0x03) << 27)
        | (u64::from(station.rf_carriers & 0x3FF) << 16)
        | (u64::from(station.carrier & 0x3F) << 8)
        | u64::from(station.pscn & 0x3F)
}

#[must_use]
pub fn qt_capabilities(capabilities: u64) -> u64 {
    qt_message(0x3, capabilities)
}

#[must_use]
pub fn qt_message(qh: u8, body: u64) -> u64 {
    a_field(
        header(4, 7),
        (u64::from(qh & 0x0F) << 36) | (body & ((1u64 << 36) - 1)),
    )
}

#[must_use]
pub fn capability_bits(offsets: &[usize]) -> u64 {
    offsets
        .iter()
        .fold(0u64, |acc, &offset| acc | (1u64 << (47 - offset)))
}

#[must_use]
pub fn mt_encryption(command: u8, phase: u8, fmid: u16, pmid: u32) -> u64 {
    let tail = (5u64 << 36)
        | (u64::from(command & 0x03) << 34)
        | (u64::from(phase & 0x03) << 32)
        | (u64::from(fmid & 0x0FFF) << 20)
        | u64::from(pmid & 0x000F_FFFF);
    a_field(header(6, 7), tail)
}

fn packet_bits(from_rfp: bool, a_field: u64, b_field: Option<&BField>) -> Vec<bool> {
    let sync = if from_rfp { RFP_SYNC } else { PP_SYNC };
    let mut bits = Vec::with_capacity(S_FIELD_BITS + A_FIELD_BITS + B_FIELD_BITS);
    for index in 0..S_FIELD_BITS {
        bits.push((sync >> (S_FIELD_BITS - 1 - index)) & 1 == 1);
    }
    for index in 0..A_FIELD_BITS {
        bits.push((a_field >> (A_FIELD_BITS - 1 - index)) & 1 == 1);
    }
    match b_field {
        Some(field) => bits.extend((0..B_FIELD_BITS).map(|index| field.bit(index))),
        None => {
            let mut state = 0x5A5Au32;
            for _ in 0..B_FIELD_BITS {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                bits.push(state >> 31 == 1);
            }
        }
    }
    bits
}

fn modulate_bits(bits: &[bool], sps: usize, rate: f64) -> Vec<Complex<f32>> {
    let shape = pulse::gaussian_freq(sps as f64, GAUSSIAN_BT, GAUSSIAN_SPAN, Norm::Area);
    let mut freq = vec![0.0f32; bits.len() * sps + shape.len()];
    for (index, &bit) in bits.iter().enumerate() {
        let level = if bit { 1.0 } else { -1.0 };
        for (tap, &h) in shape.iter().enumerate() {
            freq[index * sps + tap] += level * h;
        }
    }
    let gain = TAU * DEVIATION_HZ * sps as f64 / rate;
    let mut phase = 0.0f64;
    freq.iter()
        .map(|&value| {
            phase += gain * f64::from(value);
            Complex::from_polar(1.0, phase as f32)
        })
        .collect()
}

#[must_use]
pub fn modulate(from_rfp: bool, a_field: u64) -> Vec<Complex<f32>> {
    modulate_bits(&packet_bits(from_rfp, a_field, None), SPS, INPUT_RATE_HZ)
}

fn place(canvas: &mut [Complex<f32>], at: usize, burst: &[Complex<f32>]) {
    let end = (at + burst.len()).min(canvas.len());
    if at >= end {
        return;
    }
    canvas[at..end].copy_from_slice(&burst[..end - at]);
}

#[must_use]
pub fn dummy_bearer(station: &Station, frames: usize) -> Vec<Complex<f32>> {
    let mut canvas = vec![Complex::default(); frames * FRAME_SAMPLES as usize];
    let lead = station.slot as u64 * SLOT_SAMPLES;
    for frame in 0..frames {
        let a_field = match frame % 3 {
            0 => nt(station.rfpi),
            1 => qt_static(station),
            _ => qt_capabilities(station.capabilities),
        };
        let at = (frame as u64 * FRAME_SAMPLES + lead) as usize;
        place(&mut canvas, at, &modulate(true, a_field));
    }
    canvas
}

#[must_use]
pub fn with_burst(
    station: &Station,
    frames: usize,
    at_frame: usize,
    from_rfp: bool,
    a_field: u64,
) -> Vec<Complex<f32>> {
    let mut canvas = dummy_bearer(station, frames);
    let lead = station.slot as u64 * SLOT_SAMPLES;
    let at = (at_frame as u64 * FRAME_SAMPLES + lead) as usize;
    place(&mut canvas, at, &modulate(from_rfp, a_field));
    canvas
}

pub struct Air {
    rate: f64,
    sps: usize,
    center_hz: Option<f64>,
    band: DectBand,
    canvas: Vec<Complex<f32>>,
}

impl Air {
    #[must_use]
    pub fn carrier(frames: usize) -> Self {
        Self {
            rate: INPUT_RATE_HZ,
            sps: SPS,
            center_hz: None,
            band: DectBand::Eu,
            canvas: vec![Complex::default(); frames * FRAME_SAMPLES as usize],
        }
    }

    #[must_use]
    pub fn band(band: DectBand, frames: usize) -> Self {
        let rate = wideband::input_rate(band);
        Self {
            rate,
            sps: (rate / BIT_RATE_HZ).round() as usize,
            center_hz: Some(band.center_hz()),
            band,
            canvas: vec![Complex::default(); frames * (rate / 100.0) as usize],
        }
    }

    #[must_use]
    pub fn rate(&self) -> f64 {
        self.rate
    }

    pub fn signalling(
        &mut self,
        carrier: u8,
        frame: usize,
        slot: u8,
        from_rfp: bool,
        a_field: u64,
    ) {
        self.burst(carrier, frame, slot, from_rfp, a_field, None);
    }

    pub(crate) fn burst(
        &mut self,
        carrier: u8,
        frame: usize,
        slot: u8,
        from_rfp: bool,
        a_field: u64,
        b_field: Option<&BField>,
    ) {
        let frame_len = self.rate / 100.0;
        let at = (frame as f64 * frame_len + f64::from(slot) * frame_len / SLOTS_PER_FRAME as f64)
            as usize;
        let mut burst = modulate_bits(
            &packet_bits(from_rfp, a_field, b_field),
            self.sps,
            self.rate,
        );
        let offset = match (self.center_hz, self.band.carrier_hz(carrier)) {
            (Some(center), Some(carrier_hz)) => carrier_hz - center,
            _ => 0.0,
        };
        let turns_per_sample = offset / self.rate;
        for (index, sample) in burst.iter_mut().enumerate() {
            let turns = (turns_per_sample * (at + index) as f64).fract();
            let (sin, cos) = (TAU * turns).sin_cos();
            *sample *= Complex::new(cos as f32, sin as f32);
        }
        let end = (at + burst.len()).min(self.canvas.len());
        if at >= end {
            return;
        }
        for (slot, value) in self.canvas[at..end].iter_mut().zip(&burst) {
            *slot += *value;
        }
    }

    pub fn dummy_bearer(&mut self, station: &Station, frames: usize) {
        for frame in 0..frames {
            let a_field = match frame % 3 {
                0 => nt(station.rfpi),
                1 => qt_static(station),
                _ => qt_capabilities(station.capabilities),
            };
            self.burst(station.carrier, frame, station.slot, true, a_field, None);
        }
    }

    pub fn qt_cycle(&mut self, station: &Station, messages: &[u64], frames: usize) {
        for frame in 0..frames {
            let a_field = if frame % 2 == 0 {
                nt(station.rfpi)
            } else {
                messages[(frame / 2) % messages.len()]
            };
            self.burst(station.carrier, frame, station.slot, true, a_field, None);
        }
    }

    #[must_use]
    pub fn into_iq(self) -> Vec<Complex<f32>> {
        self.canvas
    }
}

pub struct Call<'a> {
    pub station: Station,
    pub pmid: u32,
    pub first_frame: usize,
    pub base: &'a [i16],
    pub handset: &'a [i16],
    pub grant_at: Option<usize>,
}

impl Call<'_> {
    pub fn transmit(&self, air: &mut Air, frames: usize) {
        let mut base = G726::default();
        let mut handset = G726::default();
        let slot = self.station.slot;
        for frame in 0..frames {
            let number = (self.first_frame + frame) % MULTIFRAME;
            let rfp_tail = match (self.grant_at, number) {
                (Some(grant), _) if grant == frame => mt_encryption(0, 2, 0x0ABC, self.pmid),
                (_, QT_FRAME) => a_field(header(4, 0), static_tail(&self.station)),
                _ => a_field(header(3, 0), self.station.rfpi),
            };
            let rfp_voice = voice_field(&mut base, self.base, frame, number);
            air.burst(
                self.station.carrier,
                frame,
                slot,
                true,
                rfp_tail,
                Some(&rfp_voice),
            );
            let pp_tail = a_field(header(3, 0), self.station.rfpi);
            let pp_voice = voice_field(&mut handset, self.handset, frame, number);
            air.burst(
                self.station.carrier,
                frame,
                slot + DUPLEX_SLOTS,
                false,
                pp_tail,
                Some(&pp_voice),
            );
        }
    }
}

fn voice_field(codec: &mut G726, pcm: &[i16], frame: usize, number: usize) -> BField {
    let mut data = [0u8; B_BYTES];
    for (pair, byte) in data.iter_mut().enumerate() {
        let at = frame * VOICE_PER_FRAME + pair * 2;
        let sample = |index: usize| pcm.get(index).copied().unwrap_or(0);
        *byte = (codec.encode(sample(at)) << 4) | codec.encode(sample(at + 1));
    }
    BField::scrambled(&data, number as u8)
}

#[must_use]
pub fn tone(freq_hz: f64, amplitude: f64, len: usize) -> Vec<i16> {
    (0..len)
        .map(|n| (amplitude * 32_767.0 * (TAU * freq_hz * n as f64 / 8_000.0).sin()) as i16)
        .collect()
}
