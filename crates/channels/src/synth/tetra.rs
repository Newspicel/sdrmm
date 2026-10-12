use num_complex::Complex;
use sdrmm_dsp::crc16_msb_bits;
use sdrmm_modem::pulse::{self, Norm};

use crate::tetra::{
    burst::{
        AACH1_AT, AACH1_BITS, AACH2_AT, BLOCK1_AT, BLOCK2_AT, BURST_BITS, FREQUENCY_AT,
        FREQUENCY_BITS, HALF_BITS, HEAD_BITS, NORMAL_1, NORMAL_3, NORMAL_AT, SB1_AT, SYNC_AACH_AT,
        SYNC_AT, SYNC_TRAINING, TAIL_AT, frequency_correction,
    },
    coding::{
        AACH_BITS, BSCH, Block, CRC_BITS, CRC_POLY, FULL_SLOT, HALF_SLOT, MAX_TYPE5_BITS,
        SB1_SCRAMBLING, TAIL_BITS, conv_output, interleaved_position, punctured_position,
        rm_encode, scrambling_code, scrambling_sequence,
    },
    demod::{RRC_ALPHA, SPS},
};

pub const SAMPLE_RATE_HZ: f64 = 72_000.0;

const RRC_SPAN: usize = 8;
const AMPLITUDE: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub mcc: u16,
    pub mnc: u16,
    pub colour: u8,
    pub carrier: u16,
    pub band: u8,
    pub offset: u8,
    pub duplex: u8,
    pub reverse: bool,
    pub location_area: u16,
    pub air_encryption: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            mcc: 262,
            mnc: 1_010,
            colour: 17,
            carrier: 3_700,
            band: 3,
            offset: 0,
            duplex: 0,
            reverse: false,
            location_area: 4_242,
            air_encryption: false,
        }
    }
}

impl Cell {
    #[must_use]
    pub fn downlink_hz(&self) -> u64 {
        u64::from(self.band) * 100_000_000 + u64::from(self.carrier) * 25_000
    }
}

pub struct Bits(Vec<bool>);

impl Bits {
    #[must_use]
    pub fn new() -> Self {
        Self(Vec::new())
    }

    #[must_use]
    pub fn field(mut self, value: u64, len: usize) -> Self {
        self.0.extend((0..len).rev().map(|i| value >> i & 1 == 1));
        self
    }

    #[must_use]
    pub fn bits(mut self, bits: &[bool]) -> Self {
        self.0.extend_from_slice(bits);
        self
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn into_vec(self) -> Vec<bool> {
        self.0
    }
}

impl Default for Bits {
    fn default() -> Self {
        Self::new()
    }
}

pub fn group_setup(call_id: u16, group: u32, caller: u32, marker: u8, timeslot: u8) -> Vec<bool> {
    let cmce = Bits::new()
        .field(7, 5)
        .field(u64::from(call_id), 14)
        .field(0, 4 + 1 + 1)
        .field(0, 3)
        .field(0, 1)
        .field(1, 2)
        .field(1, 2)
        .field(0, 2 + 1 + 4)
        .field(1, 1)
        .field(0, 2)
        .field(1, 1)
        .field(1, 2)
        .field(u64::from(caller), 24)
        .field(0, 2);
    let allocation = Bits::new()
        .field(0, 2)
        .field(0b1000 >> (timeslot.clamp(1, 4) - 1), 4)
        .field(3, 2)
        .field(0, 2)
        .field(0, 12)
        .field(0, 1)
        .field(1, 2);
    resource(
        Address::Usage(group, marker),
        Some(&allocation.into_vec()),
        &sdu(2, cmce),
    )
}

pub fn tx_granted(call_id: u16, group: u32, talker: u32, marker: u8) -> Vec<bool> {
    let cmce = Bits::new()
        .field(11, 5)
        .field(u64::from(call_id), 14)
        .field(0, 2 + 1 + 1 + 1)
        .field(1, 1)
        .field(0, 1)
        .field(1, 1)
        .field(1, 2)
        .field(u64::from(talker), 24)
        .field(0, 1);
    resource(Address::Usage(group, marker), None, &sdu(2, cmce))
}

pub fn release(call_id: u16, group: u32) -> Vec<bool> {
    let cmce = Bits::new()
        .field(6, 5)
        .field(u64::from(call_id), 14)
        .field(13, 5)
        .field(0, 1);
    resource(Address::Ssi(group), None, &sdu(2, cmce))
}

pub fn location_update_accept(ssi: u32) -> Vec<bool> {
    let mm = Bits::new().field(5, 4).field(0, 3).field(0, 1);
    resource(Address::Ssi(ssi), None, &sdu(1, mm))
}

pub fn text_message(to: u32, from: u32, text: &str) -> Vec<bool> {
    resource(Address::Ssi(to), None, &sds_sdu(from, text_user(text)))
}

fn text_user(text: &str) -> Bits {
    let mut user = Bits::new()
        .field(0x82, 8)
        .field(0, 4)
        .field(0, 2 + 1 + 1)
        .field(0x2A, 8)
        .field(0, 1)
        .field(1, 7);
    for ch in text.chars() {
        user = user.field(u64::from(u8::try_from(u32::from(ch)).unwrap_or(b'?')), 8);
    }
    user
}

pub fn position(to: u32, from: u32, lat: f64, lon: f64) -> Vec<bool> {
    let lon = (lon / 180.0 * f64::from(1u32 << 24)).round() as i64 & 0x1FF_FFFF;
    let lat = (lat / 90.0 * f64::from(1u32 << 23)).round() as i64 & 0xFF_FFFF;
    let user = Bits::new()
        .field(0x0A, 8)
        .field(0, 2)
        .field(0, 2)
        .field(lon as u64, 25)
        .field(lat as u64, 24)
        .field(1, 3)
        .field(0, 7 + 4 + 1 + 8);
    resource(Address::Ssi(to), None, &sds_sdu(from, user))
}

fn sds_sdu(from: u32, user: Bits) -> Vec<bool> {
    let cmce = Bits::new()
        .field(15, 5)
        .field(1, 2)
        .field(u64::from(from), 24)
        .field(3, 2)
        .field(user.len() as u64, 11)
        .bits(&user.into_vec())
        .field(0, 1);
    sdu(2, cmce)
}

pub fn fragmented_text(to: u32, from: u32, text: &str) -> (Vec<bool>, Vec<bool>) {
    let body = sds_sdu(from, text_user(text));
    let header = resource_header(Address::Ssi(to), None, 0x3F, false);
    let first = (FULL_SLOT.type1 - header.len()).min(body.len());
    let start = Bits::new().bits(&header).bits(&body[..first]).into_vec();
    let rest = &body[first..];
    let end_header_bits = 3 + 1 + 1 + 6 + 1 + 1;
    let (padded, fill) = pad(rest, end_header_bits);
    let octets = (end_header_bits + padded.len()) / 8;
    let end = Bits::new()
        .field(0b011, 3)
        .field(u64::from(fill), 1)
        .field(0, 1)
        .field(octets as u64, 6)
        .field(0, 2)
        .bits(&padded)
        .into_vec();
    (start, end)
}

#[derive(Clone, Copy)]
enum Address {
    Ssi(u32),
    Usage(u32, u8),
}

fn sdu(discriminator: u64, body: Bits) -> Vec<bool> {
    Bits::new()
        .field(0b0010, 4)
        .field(discriminator, 3)
        .bits(&body.into_vec())
        .into_vec()
}

fn header_bits(address: Address, allocation: Option<&[bool]>) -> usize {
    let address_bits = match address {
        Address::Ssi(_) => 24,
        Address::Usage(..) => 30,
    };
    2 + 1 + 1 + 2 + 1 + 6 + 3 + address_bits + 3 + allocation.map_or(0, <[bool]>::len)
}

fn resource_header(
    address: Address,
    allocation: Option<&[bool]>,
    length: u8,
    fill: bool,
) -> Vec<bool> {
    let mut bits = Bits::new()
        .field(0, 2)
        .field(u64::from(fill), 1)
        .field(0, 1 + 2 + 1)
        .field(u64::from(length), 6);
    bits = match address {
        Address::Ssi(ssi) => bits.field(1, 3).field(u64::from(ssi), 24),
        Address::Usage(ssi, marker) => bits
            .field(6, 3)
            .field(u64::from(ssi), 24)
            .field(u64::from(marker), 6),
    };
    bits = bits.field(0, 2);
    match allocation {
        Some(element) => bits.field(1, 1).bits(element),
        None => bits.field(0, 1),
    }
    .into_vec()
}

fn pad(bits: &[bool], header: usize) -> (Vec<bool>, bool) {
    let total = header + bits.len();
    if total.is_multiple_of(8) {
        return (bits.to_vec(), false);
    }
    let fill = 8 - total % 8;
    let mut padded = bits.to_vec();
    padded.push(true);
    padded.extend(std::iter::repeat_n(false, fill - 1));
    (padded, true)
}

fn resource(address: Address, allocation: Option<&[bool]>, sdu: &[bool]) -> Vec<bool> {
    let header = header_bits(address, allocation);
    let (padded, fill) = pad(sdu, header);
    let octets = (header + padded.len()) / 8;
    let length = u8::try_from(octets).unwrap_or(0x3A).min(0x3A);
    Bits::new()
        .bits(&resource_header(address, allocation, length, fill))
        .bits(&padded)
        .into_vec()
}

fn null_pdu() -> Vec<bool> {
    Bits::new()
        .field(0, 2)
        .field(0, 1 + 1 + 2 + 1)
        .field(2, 6)
        .field(0, 3)
        .into_vec()
}

fn sync_pdu(cell: &Cell, time: Time) -> Vec<bool> {
    Bits::new()
        .field(0, 4)
        .field(u64::from(cell.colour), 6)
        .field(u64::from(time.timeslot - 1), 2)
        .field(u64::from(time.frame), 5)
        .field(u64::from(time.multiframe), 6)
        .field(0, 8)
        .field(u64::from(cell.mcc), 10)
        .field(u64::from(cell.mnc), 14)
        .field(0, 5)
        .into_vec()
}

fn sysinfo_pdu(cell: &Cell) -> Vec<bool> {
    let services = if cell.air_encryption { 0b10 } else { 0 };
    Bits::new()
        .field(0b10, 2)
        .field(0b00, 2)
        .field(u64::from(cell.carrier), 12)
        .field(u64::from(cell.band), 4)
        .field(u64::from(cell.offset), 2)
        .field(u64::from(cell.duplex), 3)
        .field(u64::from(cell.reverse), 1)
        .field(0, 2 + 3 + 4 + 4 + 4 + 1 + 16 + 2 + 20)
        .field(u64::from(cell.location_area), 14)
        .field(0xFFFF, 16)
        .field(services, 12)
        .into_vec()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Time {
    timeslot: u8,
    frame: u8,
    multiframe: u8,
}

impl Time {
    fn advance(&mut self) {
        self.timeslot += 1;
        if self.timeslot > 4 {
            self.timeslot = 1;
            self.frame += 1;
        }
        if self.frame > 18 {
            self.frame = 1;
            self.multiframe += 1;
        }
        if self.multiframe > 60 {
            self.multiframe = 1;
        }
    }
}

pub struct Downlink {
    cell: Cell,
    time: Time,
    scrambling: [bool; MAX_TYPE5_BITS],
    bits: Vec<bool>,
    noise: u32,
}

impl Downlink {
    #[must_use]
    pub fn new(cell: Cell) -> Self {
        let mut scrambling = [false; MAX_TYPE5_BITS];
        scrambling_sequence(
            scrambling_code(cell.mcc, cell.mnc, cell.colour),
            &mut scrambling,
        );
        Self {
            cell,
            time: Time {
                timeslot: 1,
                frame: 17,
                multiframe: 1,
            },
            scrambling,
            bits: Vec::new(),
            noise: 0x1234_5678,
        }
    }

    #[must_use]
    pub fn bits(&self) -> &[bool] {
        &self.bits
    }

    pub fn sync(&mut self) {
        let mut burst = frame_burst();
        for i in 0..FREQUENCY_BITS {
            burst[FREQUENCY_AT + i] = frequency_correction(i) == 1;
        }
        let mut sb1_scrambling = [false; MAX_TYPE5_BITS];
        scrambling_sequence(SB1_SCRAMBLING, &mut sb1_scrambling);
        let sb1 = encode(BSCH, &sync_pdu(&self.cell, self.time), &sb1_scrambling);
        burst[SB1_AT..SB1_AT + sb1.len()].copy_from_slice(&sb1);
        for (i, &bit) in SYNC_TRAINING.iter().enumerate() {
            burst[SYNC_AT + i] = bit == 1;
        }
        let aach = self.aach(0);
        burst[SYNC_AACH_AT..SYNC_AACH_AT + AACH_BITS].copy_from_slice(&aach);
        let sb2 = encode(HALF_SLOT, &sysinfo_pdu(&self.cell), &self.scrambling);
        burst[BLOCK2_AT..BLOCK2_AT + HALF_BITS].copy_from_slice(&sb2);
        self.push(&burst);
    }

    pub fn control(&mut self, pdus: &[Vec<bool>]) {
        let mut block: Vec<bool> = pdus.iter().flatten().copied().collect();
        if block.len() + 16 <= FULL_SLOT.type1 {
            block.extend(null_pdu());
        }
        block.resize(FULL_SLOT.type1, false);
        self.full_slot(0, &block);
    }

    pub fn idle(&mut self) {
        let mut block = null_pdu();
        block.resize(FULL_SLOT.type1, false);
        self.full_slot(1 << 12, &block);
    }

    pub fn traffic(&mut self, marker: u8) {
        let mut burst = frame_burst();
        for at in (BLOCK1_AT..BLOCK1_AT + HALF_BITS).chain(BLOCK2_AT..BLOCK2_AT + HALF_BITS) {
            burst[at] = self.random_bit();
        }
        self.normal(&mut burst, 1 << 12 | u16::from(marker) << 6);
        self.push(&burst);
    }

    fn full_slot(&mut self, access: u16, block: &[bool]) {
        let mut burst = frame_burst();
        let type5 = encode(FULL_SLOT, block, &self.scrambling);
        burst[BLOCK1_AT..BLOCK1_AT + HALF_BITS].copy_from_slice(&type5[..HALF_BITS]);
        burst[BLOCK2_AT..BLOCK2_AT + HALF_BITS].copy_from_slice(&type5[HALF_BITS..]);
        self.normal(&mut burst, access);
        self.push(&burst);
    }

    fn normal(&self, burst: &mut [bool; BURST_BITS], access: u16) {
        for (i, &bit) in NORMAL_1.iter().enumerate() {
            burst[NORMAL_AT + i] = bit == 1;
        }
        let aach = self.aach(access);
        burst[AACH1_AT..AACH1_AT + AACH1_BITS].copy_from_slice(&aach[..AACH1_BITS]);
        burst[AACH2_AT..AACH2_AT + AACH_BITS - AACH1_BITS].copy_from_slice(&aach[AACH1_BITS..]);
    }

    fn aach(&self, access: u16) -> [bool; AACH_BITS] {
        let word = rm_encode(access);
        std::array::from_fn(|i| (word >> (AACH_BITS - 1 - i) & 1 == 1) ^ self.scrambling[i])
    }

    fn push(&mut self, burst: &[bool; BURST_BITS]) {
        self.bits.extend_from_slice(burst);
        self.time.advance();
    }

    fn random_bit(&mut self) -> bool {
        self.noise ^= self.noise << 13;
        self.noise ^= self.noise >> 17;
        self.noise ^= self.noise << 5;
        self.noise & 1 == 1
    }

    #[must_use]
    pub fn modulate(&self) -> Vec<Complex<f32>> {
        modulate(&self.bits)
    }
}

fn frame_burst() -> [bool; BURST_BITS] {
    let mut burst = [false; BURST_BITS];
    for i in 0..HEAD_BITS {
        burst[i] = NORMAL_3[NORMAL_3.len() - HEAD_BITS + i] == 1;
    }
    for i in 0..BURST_BITS - TAIL_AT {
        burst[TAIL_AT + i] = NORMAL_3[i] == 1;
    }
    burst
}

fn encode(block: Block, type1: &[bool], scrambling: &[bool]) -> Vec<bool> {
    let mut type2 = type1[..block.type1].to_vec();
    let crc = !crc16_msb_bits(CRC_POLY, 0xFFFF, &type2);
    type2.extend((0..CRC_BITS).rev().map(|i| crc >> i & 1 == 1));
    type2.extend([false; TAIL_BITS]);
    let mut register = 0u8;
    let mother: Vec<bool> = type2
        .iter()
        .flat_map(|&bit| {
            register = (register << 1 | u8::from(bit)) & 0x1F;
            conv_output(register)
        })
        .collect();
    let mut type4 = vec![false; block.type5];
    for i in 0..block.type5 {
        type4[interleaved_position(block.type5, block.interleave, i)] =
            mother[punctured_position(i)];
    }
    type4
        .iter()
        .zip(scrambling)
        .map(|(&bit, &s)| bit ^ s)
        .collect()
}

#[must_use]
pub fn modulate(bits: &[bool]) -> Vec<Complex<f32>> {
    let taps = pulse::root_raised_cosine(SPS as f64, RRC_ALPHA, RRC_SPAN, Norm::Energy);
    let mut phase = 0.0f64;
    let symbols: Vec<Complex<f32>> = bits
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let step = match (pair[0], pair[1]) {
                (false, false) => 1.0,
                (false, true) => 3.0,
                (true, false) => -1.0,
                (true, true) => -3.0,
            };
            phase += step * std::f64::consts::FRAC_PI_4;
            Complex::from_polar(1.0, phase as f32)
        })
        .collect();
    let len = symbols.len() * SPS + taps.len();
    let mut out = vec![Complex::new(0.0, 0.0); len];
    for (k, &symbol) in symbols.iter().enumerate() {
        for (j, &tap) in taps.iter().enumerate() {
            out[k * SPS + j] += symbol * tap;
        }
    }
    let gain = AMPLITUDE * (SPS as f32).sqrt();
    out.iter_mut().for_each(|s| *s *= gain);
    out
}

pub const GROUP: u32 = 1_001;
pub const CALLER: u32 = 2_042_001;
pub const TALKER: u32 = 2_042_017;
pub const CALL_ID: u16 = 321;
pub const USAGE_MARKER: u8 = 9;
pub const TRAFFIC_SLOT: u8 = 2;
pub const TEXT: &str = "Einsatz Hbf";
pub const LONG_TEXT: &str = "Lage an der Bruecke unveraendert";
pub const POSITION: (f64, f64) = (52.520_8, 13.409_5);
const CALL_STARTS: usize = 1;

#[must_use]
pub fn sample(cell: Cell) -> Downlink {
    let mut downlink = Downlink::new(cell);
    let (start, end) = fragmented_text(GROUP, CALLER, LONG_TEXT);
    let control = [
        vec![location_update_accept(CALLER)],
        vec![group_setup(
            CALL_ID,
            GROUP,
            CALLER,
            USAGE_MARKER,
            TRAFFIC_SLOT,
        )],
        vec![tx_granted(CALL_ID, GROUP, TALKER, USAGE_MARKER)],
        vec![text_message(TALKER, CALLER, TEXT)],
        vec![position(GROUP, TALKER, POSITION.0, POSITION.1)],
        vec![start],
        vec![end],
        vec![release(CALL_ID, GROUP)],
    ];
    for _ in 0..4 {
        downlink.idle();
    }
    downlink.sync();
    for _ in 0..3 {
        downlink.idle();
    }
    for (index, pdus) in control.iter().enumerate() {
        downlink.control(pdus);
        if (CALL_STARTS..control.len() - 1).contains(&index) {
            downlink.traffic(USAGE_MARKER);
        } else {
            downlink.idle();
        }
        downlink.idle();
        downlink.idle();
    }
    for _ in 0..4 {
        downlink.idle();
    }
    downlink
}
