use num_complex::Complex;
use sdrmm_dsp::crc16_x25;
use sdrmm_modem::cpm::CpmMod;
use sdrmm_wire::TetrapolBand;

use crate::tetrapol::{
    BAUD,
    coding::{BLOCK_BYTES, DataBlock, FRAME_BITS, encode_data, encode_voice, frame_bits},
    cpm_params,
};

pub const ALL_STATIONS: [u8; 2] = [0x7F, 0xFF];
const NO_STATION: [u8; 2] = [0x70, 0x00];
const UI: u8 = 0x03;
const TSAP: u8 = 0x05;
const SINGLE_INFO: usize = 3;
const FRAME_OVERHEAD: usize = 5;
const SUPERFRAME: usize = 200;
const HALF: usize = 100;
const BCH_BLOCKS: usize = 4;
const PCH: [usize; 2] = [98, 99];
const RCH_PERIOD: usize = 25;
const RCH_OFFSET: usize = 14;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Burst {
    Data { flag: u8, octets: [u8; BLOCK_BYTES] },
    Voice { seed: u32 },
}

#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub bn: u8,
    pub rsw: u8,
    pub bs: u8,
    pub country: u8,
    pub network: u8,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            bn: 28,
            rsw: 2,
            bs: 7,
            country: 3,
            network: 3,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GroupCall {
    pub group: u16,
    pub channel: u16,
    pub uplink_scrambling: u8,
    pub downlink_scrambling: u8,
}

impl Default for GroupCall {
    fn default() -> Self {
        Self {
            group: 393,
            channel: 2890,
            uplink_scrambling: 125,
            downlink_scrambling: 89,
        }
    }
}

fn split12(value: u16, high_flags: u8) -> [u8; 2] {
    [high_flags << 4 | (value >> 8) as u8 & 0x0F, value as u8]
}

#[must_use]
pub fn system_info(cell: &Cell, first_block: bool, superframe: u16) -> Vec<u8> {
    let state = u8::from(!first_block) << 4;
    let [cpt_high, cpt_low] = split12(superframe, 0);
    vec![
        0x90,
        state,
        0x40,
        cell.country,
        cell.network << 4 | 5,
        0,
        cell.bn,
        cell.bs & 0x3F,
        cell.rsw << 4 | 0x02,
        0x80,
        0x41,
        0x00,
        0x63,
        0xBB,
        0x00,
        cpt_high,
        cpt_low,
    ]
}

#[must_use]
pub fn group_activation(call: &GroupCall) -> Vec<u8> {
    let [mode_group, group] = split12(call.group, 0b0001);
    let [channel_high, channel] = split12(call.channel, 0);
    vec![
        0x55,
        mode_group,
        group,
        0xC8,
        channel_high,
        channel,
        call.uplink_scrambling,
        call.downlink_scrambling,
        0x4A,
    ]
}

#[must_use]
pub fn group_idle(cause: u8) -> Vec<u8> {
    vec![0x58, cause]
}

fn info_len(tsdu_len: usize) -> usize {
    if tsdu_len < SINGLE_INFO {
        return SINGLE_INFO;
    }
    [2, 4, 5, 6, 7, 8, 9]
        .into_iter()
        .map(|blocks| useful_blocks(blocks) * BLOCK_BYTES - FRAME_OVERHEAD)
        .find(|&len| len >= tsdu_len + 2)
        .unwrap_or(tsdu_len + 2)
}

fn useful_blocks(blocks: usize) -> usize {
    if blocks <= 2 { blocks } else { blocks - 1 }
}

#[must_use]
pub fn ui_frame(address: [u8; 2], tsdu: &[u8]) -> Vec<u8> {
    let len = info_len(tsdu.len());
    let mut frame = vec![address[0], address[1], UI];
    if len == SINGLE_INFO {
        frame.push(TSAP);
        frame.extend_from_slice(tsdu);
    } else {
        frame.push(0x30 | TSAP);
        frame.push(tsdu.len() as u8);
        frame.extend_from_slice(tsdu);
    }
    frame.resize(3 + len, 0);
    let fcs = crc16_x25(&frame);
    frame.extend_from_slice(&fcs.to_le_bytes());
    frame
}

#[must_use]
pub fn blocks(frame: &[u8]) -> Vec<Burst> {
    let chunks: Vec<[u8; BLOCK_BYTES]> = frame
        .chunks(BLOCK_BYTES)
        .map(|chunk| {
            let mut octets = [0; BLOCK_BYTES];
            octets[..chunk.len()].copy_from_slice(chunk);
            octets
        })
        .collect();
    let flags: Vec<u8> = match chunks.len() {
        1 => vec![0],
        2 => vec![1, 3],
        n => std::iter::once(1)
            .chain(std::iter::once(2))
            .chain(std::iter::repeat_n(3, n - 3))
            .chain([2, 1])
            .collect(),
    };
    let mut bursts: Vec<Burst> = chunks
        .iter()
        .zip(&flags)
        .map(|(&octets, &flag)| Burst::Data { flag, octets })
        .collect();
    if chunks.len() > 2 {
        let mut parity = [0u8; BLOCK_BYTES];
        for chunk in &chunks {
            for (sum, octet) in parity.iter_mut().zip(chunk) {
                *sum ^= octet;
            }
        }
        bursts.push(Burst::Data {
            flag: 1,
            octets: parity,
        });
    }
    bursts
}

#[must_use]
pub fn stuffing() -> Burst {
    let mut frame = vec![NO_STATION[0], NO_STATION[1], UI, 0, 0, 0];
    let fcs = crc16_x25(&frame);
    frame.extend_from_slice(&fcs.to_le_bytes());
    blocks(&frame)[0]
}

fn random_access() -> Burst {
    let mut frame = vec![0xC0, 0x00, 0xC0, 0x00, 0xC0, 0x00];
    let fcs = crc16_x25(&frame);
    frame.extend_from_slice(&fcs.to_le_bytes());
    blocks(&frame)[0]
}

fn paging(group: u16) -> Vec<Burst> {
    let mut frame = vec![0x10, 0, 0, 0, 0, 0, 0, 0];
    frame.extend_from_slice(&split12(group, 0));
    for _ in 0..3 {
        frame.extend_from_slice(&NO_STATION);
    }
    blocks(&frame)
}

#[must_use]
pub fn control_superframe(cell: &Cell, calls: &[GroupCall], superframe: u16) -> Vec<Burst> {
    let mut slots: Vec<Option<Burst>> = vec![None; SUPERFRAME];
    for (start, first) in [(0, true), (HALF, false)] {
        let bch = blocks(&ui_frame(
            ALL_STATIONS,
            &system_info(cell, first, superframe),
        ));
        for (k, burst) in bch.into_iter().take(BCH_BLOCKS).enumerate() {
            slots[start + k] = Some(burst);
        }
        let group = calls.first().map_or(0, |call| call.group);
        for (slot, burst) in PCH.iter().zip(paging(group)) {
            slots[start + slot] = Some(burst);
        }
    }
    for slot in (RCH_OFFSET..SUPERFRAME).step_by(RCH_PERIOD) {
        slots[slot] = Some(random_access());
    }
    let mut messages = calls
        .iter()
        .map(|call| blocks(&ui_frame(ALL_STATIONS, &group_activation(call))))
        .cycle();
    let mut at = 0;
    while at < SUPERFRAME {
        let free = slots[at..].iter().take_while(|slot| slot.is_none()).count();
        if let Some(bursts) = messages
            .next()
            .filter(|bursts| free > 0 && bursts.len() <= free)
        {
            for (slot, &burst) in slots[at..].iter_mut().zip(&bursts) {
                *slot = Some(burst);
            }
            at += bursts.len();
        }
        if let Some(slot) = slots.get_mut(at).filter(|slot| slot.is_none()) {
            *slot = Some(stuffing());
        }
        at += 1;
    }
    slots
        .into_iter()
        .map(|slot| slot.unwrap_or_else(stuffing))
        .collect()
}

#[must_use]
pub fn traffic(idle_frames: usize, voice_frames: usize) -> Vec<Burst> {
    let idle = blocks(&ui_frame(ALL_STATIONS, &group_idle(0x12)))[0];
    std::iter::repeat_n(idle, idle_frames)
        .chain((0..voice_frames).map(|k| Burst::Voice { seed: k as u32 + 1 }))
        .chain(std::iter::repeat_n(idle, idle_frames))
        .collect()
}

fn pseudo_bits<const N: usize>(seed: u32) -> [u8; N] {
    let mut state = seed.wrapping_mul(2_654_435_761) | 1;
    std::array::from_fn(|_| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        (state & 1) as u8
    })
}

fn burst_bits(burst: &Burst, band: TetrapolBand, scrambling: u8) -> [u8; FRAME_BITS] {
    let e = match *burst {
        Burst::Data { flag, octets } => encode_data(
            &DataBlock {
                flag,
                octets,
                asb: [0; 2],
            },
            band,
        ),
        Burst::Voice { seed } => {
            encode_voice(&pseudo_bits(seed), &pseudo_bits(seed ^ 0xA5A5), band)
        }
    };
    frame_bits(&e, band, scrambling)
}

#[must_use]
pub fn transmission(
    bursts: &[Burst],
    band: TetrapolBand,
    scrambling: u8,
    rate: f64,
) -> Vec<Complex<f32>> {
    let mut previous = 0u8;
    let symbols: Vec<u8> = bursts
        .iter()
        .flat_map(|burst| burst_bits(burst, band, scrambling))
        .map(|bit| {
            let transition = bit ^ previous;
            previous = bit;
            transition ^ 1
        })
        .collect();
    let mut tx = CpmMod::new(cpm_params(rate / BAUD));
    let mut out = Vec::new();
    tx.modulate(&symbols, &mut out);
    tx.flush(&mut out);
    out
}

#[must_use]
pub fn control_channel(rate: f64, superframes: u16) -> Vec<Complex<f32>> {
    let calls = [
        GroupCall::default(),
        GroupCall {
            group: 171,
            channel: 2862,
            uplink_scrambling: 124,
            downlink_scrambling: 88,
        },
    ];
    let bursts: Vec<Burst> = (0..superframes)
        .flat_map(|count| control_superframe(&Cell::default(), &calls, 2118 + count))
        .collect();
    transmission(&bursts, TetrapolBand::Uhf, 70, rate)
}
