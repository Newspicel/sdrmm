use std::f64::consts::{FRAC_PI_2, TAU};

use num_complex::Complex;
use sdrmm_dsp::{Decimator, design_lowpass, fec::conv::encode_with};
use sdrmm_modem::pulse::{self, Norm};

use crate::gsm::{
    burst::{
        BLOCK_BURSTS, BURST_DATA, BURST_SYMBOLS, HALF_DATA, NORMAL_TRAINING_AT, SECOND_HALF_AT,
        SPS, SYNC_DATA, SYNC_SECOND_HALF_AT, SYNC_TRAINING_AT, TAIL_SYMBOLS, Training,
    },
    coding::{
        BLOCK_BYTES, BLOCK_CODED, BLOCK_INFO, FIRE_PARITY, G0, G1, SCH_PARITY, fire_parity,
        interleaved_position, sync_parity,
    },
    frames::{ControlSlot, SyncInfo, control_slot, sync_bits},
};

pub const INPUT_RATE_HZ: f64 = crate::gsm::burst::INPUT_RATE_HZ;

const OVERSAMPLE: usize = 4;
const GAUSSIAN_BT: f64 = 0.3;
const GAUSSIAN_SPAN: usize = 4;
const FILTER_TAPS: usize = 65;
const FILTER_CUTOFF: f64 = 0.06;
const SLOT_SYMBOLS: [usize; 8] = [157, 156, 156, 156, 157, 156, 156, 156];
const CODE_TAIL: usize = 4;
const STEALING: bool = true;

pub type Block = [u8; BLOCK_BYTES];

pub const EMPTY_PAGING: Block = [
    0x15, 0x06, 0x21, 0x00, 0x01, 0xF0, 0x2B, 0x2B, 0x2B, 0x2B, 0x2B, 0x2B, 0x2B, 0x2B, 0x2B, 0x2B,
    0x2B, 0x2B, 0x2B, 0x2B, 0x2B, 0x2B, 0x2B,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scheduled {
    pub timeslot: u8,
    pub frame_number: u32,
    pub tsc: u8,
    pub data: Block,
}

#[derive(Clone, Debug)]
pub struct Downlink {
    pub bsic: u8,
    pub first_frame: u32,
    pub frames: u32,
    pub system_information: Vec<Block>,
    pub blocks: Vec<Scheduled>,
    pub seed: u32,
}

#[must_use]
pub fn block(hex: &str) -> Block {
    let mut out = [0x2B; BLOCK_BYTES];
    for (k, octet) in out.iter_mut().enumerate().take(hex.len() / 2) {
        *octet = u8::from_str_radix(&hex[2 * k..2 * k + 2], 16).unwrap_or(0x2B);
    }
    out
}

impl Downlink {
    #[must_use]
    pub fn modulate(&self) -> Vec<Complex<f32>> {
        gmsk(&self.bits())
    }

    #[must_use]
    pub fn bits(&self) -> Vec<bool> {
        let mut noise = Noise(self.seed | 1);
        let mut bits = Vec::with_capacity(self.frames as usize * 1_250);
        for frame in 0..self.frames {
            let frame_number = self.first_frame + frame;
            for (timeslot, &length) in SLOT_SYMBOLS.iter().enumerate() {
                let burst = self.burst(frame_number, timeslot as u8, &mut noise);
                bits.extend_from_slice(&burst);
                bits.extend(std::iter::repeat_n(false, length - BURST_SYMBOLS));
            }
        }
        bits
    }

    fn burst(&self, frame_number: u32, timeslot: u8, noise: &mut Noise) -> [bool; BURST_SYMBOLS] {
        if let Some(scheduled) = self.scheduled(frame_number, timeslot) {
            let index = (frame_number - scheduled.frame_number) as usize;
            return normal_burst(&interleave(&scheduled.data)[index], scheduled.tsc);
        }
        if timeslot != 0 {
            return self.dummy(noise);
        }
        match control_slot(frame_number, false) {
            ControlSlot::Frequency => [false; BURST_SYMBOLS],
            ControlSlot::Sync => sync_burst(SyncInfo {
                bsic: self.bsic,
                frame_number,
            }),
            ControlSlot::Block(slot) => {
                let data = match slot.channel {
                    sdrmm_wire::GsmLogicalChannel::Bcch => self.broadcast(frame_number),
                    _ => EMPTY_PAGING,
                };
                normal_burst(&interleave(&data)[usize::from(slot.index)], self.bsic & 7)
            }
            ControlSlot::Idle => self.dummy(noise),
        }
    }

    fn scheduled(&self, frame_number: u32, timeslot: u8) -> Option<&Scheduled> {
        self.blocks.iter().find(|block| {
            block.timeslot == timeslot
                && (block.frame_number..block.frame_number + BLOCK_BURSTS as u32)
                    .contains(&frame_number)
        })
    }

    fn broadcast(&self, frame_number: u32) -> Block {
        if self.system_information.is_empty() {
            return EMPTY_PAGING;
        }
        self.system_information[(frame_number / 51) as usize % self.system_information.len()]
    }

    fn dummy(&self, noise: &mut Noise) -> [bool; BURST_SYMBOLS] {
        let mut data = [false; BURST_DATA];
        for bit in &mut data {
            *bit = noise.bit();
        }
        normal_burst(&data, self.bsic & 7)
    }
}

struct Noise(u32);

impl Noise {
    fn bit(&mut self) -> bool {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 >> 31 == 1
    }
}

fn info_bits(data: &Block) -> Vec<bool> {
    (0..BLOCK_INFO)
        .map(|i| data[i / 8] >> (i % 8) & 1 == 1)
        .collect()
}

fn append_parity(bits: &mut Vec<bool>, parity: u64, width: usize) {
    bits.extend((0..width).rev().map(|k| parity >> k & 1 == 1));
    bits.extend(std::iter::repeat_n(false, CODE_TAIL));
}

fn interleave(data: &Block) -> [[bool; BURST_DATA]; BLOCK_BURSTS] {
    let mut bits = info_bits(data);
    let parity = fire_parity(&bits);
    append_parity(&mut bits, parity, FIRE_PARITY);
    let mut coded = Vec::with_capacity(BLOCK_CODED);
    encode_with(G0, G1, &bits, &mut coded);
    let mut bursts = [[false; BURST_DATA]; BLOCK_BURSTS];
    for (k, &bit) in coded.iter().enumerate() {
        let (burst, position) = interleaved_position(k);
        bursts[burst][position] = bit;
    }
    bursts
}

fn normal_burst(data: &[bool; BURST_DATA], tsc: u8) -> [bool; BURST_SYMBOLS] {
    let training = Training::normal(tsc);
    let mut burst = [false; BURST_SYMBOLS];
    burst[TAIL_SYMBOLS..TAIL_SYMBOLS + HALF_DATA].copy_from_slice(&data[..HALF_DATA]);
    burst[NORMAL_TRAINING_AT - 1] = STEALING;
    for (k, bit) in burst[NORMAL_TRAINING_AT..SECOND_HALF_AT - 1]
        .iter_mut()
        .enumerate()
    {
        *bit = training.bit(k);
    }
    burst[SECOND_HALF_AT - 1] = STEALING;
    burst[SECOND_HALF_AT..SECOND_HALF_AT + HALF_DATA].copy_from_slice(&data[HALF_DATA..]);
    burst
}

fn sync_burst(info: SyncInfo) -> [bool; BURST_SYMBOLS] {
    let mut bits = sync_bits(info).to_vec();
    let parity = sync_parity(&bits);
    append_parity(&mut bits, parity, SCH_PARITY);
    let mut coded = Vec::with_capacity(2 * bits.len());
    encode_with(G0, G1, &bits, &mut coded);
    let training = Training::sync();
    let mut burst = [false; BURST_SYMBOLS];
    burst[TAIL_SYMBOLS..TAIL_SYMBOLS + SYNC_DATA].copy_from_slice(&coded[..SYNC_DATA]);
    for (k, bit) in burst[SYNC_TRAINING_AT..SYNC_SECOND_HALF_AT]
        .iter_mut()
        .enumerate()
    {
        *bit = training.bit(k);
    }
    burst[SYNC_SECOND_HALF_AT..SYNC_SECOND_HALF_AT + SYNC_DATA]
        .copy_from_slice(&coded[SYNC_DATA..]);
    burst
}

#[must_use]
pub fn gmsk(bits: &[bool]) -> Vec<Complex<f32>> {
    let sps = SPS * OVERSAMPLE;
    let shape = pulse::gaussian_freq(sps as f64, GAUSSIAN_BT, GAUSSIAN_SPAN, Norm::Area);
    let mut frequency = vec![0.0f32; bits.len() * sps + shape.len()];
    let mut previous = true;
    for (index, &bit) in bits.iter().enumerate() {
        let level = if bit ^ previous { -1.0 } else { 1.0 };
        previous = bit;
        for (tap, &h) in shape.iter().enumerate() {
            frequency[index * sps + tap] += level * h;
        }
    }
    let mut phase = 0.0f64;
    let fine: Vec<Complex<f32>> = frequency
        .iter()
        .map(|&value| {
            phase = (phase + FRAC_PI_2 * f64::from(value)).rem_euclid(TAU);
            Complex::from_polar(1.0, phase as f32)
        })
        .collect();
    let mut decimator = Decimator::new(&design_lowpass(FILTER_TAPS, FILTER_CUTOFF), OVERSAMPLE);
    let mut out = Vec::with_capacity(fine.len() / OVERSAMPLE + 1);
    decimator.process(&fine, &mut out);
    out
}
