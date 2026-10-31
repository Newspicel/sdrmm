use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_dsp::{CyclicCode, ParityCode, crc16_msb, rs64_decode};
use sdrmm_modem::cpm::CpmDemod;
use sdrmm_wire::{
    CallGate, ChannelDescriptor, ChannelParams, ChannelSettings, DecoderEvent, DecoderFamily,
    DvFrame, DvFrameKind, DvMode, EventKind, P25Params, Vendor,
};

use super::{INPUT_RATE_HZ, SymbolWindow, c4fm_demod, c4fm_params, tap_c4fm, vocoder::MbeDecoder};
use crate::{ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, check_input_rate};

pub(crate) const BAUD: f64 = 4_800.0;
pub(crate) const DEVIATION_HZ: f64 = 1_944.0;
pub(crate) const RRC_ALPHA: f64 = 0.2;
pub(crate) const BANDWIDTH_HZ: f64 = 12_500.0;

pub(crate) const SYNC: u64 = 0x5575_F5FF_77FF;
pub(crate) const SYNC_BITS: u32 = 48;
pub(crate) const SYNC_TOLERANCE: u32 = 4;

const STATUS_START: usize = 70;
const STATUS_STRIDE: usize = 72;

const NID_BITS: usize = 64;
const NID_SYMBOLS: usize = (NID_BITS + 2) / 2;
const NID_ROUND_OUT_BIT: u64 = 1;
const MAX_FRAME_BITS: usize = 1_728;
const MAX_FRAME_SYMBOLS: usize = MAX_FRAME_BITS / 2;

const IMBE_OFFSETS: [usize; 9] = [0, 72, 164, 256, 348, 440, 532, 624, 712];
const IMBE_BITS: usize = 144;
const BODY_START: usize = SYNC_BITS as usize + NID_BITS;
const IMBE_SPANS: [(usize, usize); 9] = imbe_spans();

const DUID_HEADER: u8 = 0x0;
const DUID_TERMINATOR: u8 = 0x3;
const DUID_LDU1: u8 = 0x5;
const DUID_TSDU: u8 = 0x7;
const DUID_LDU2: u8 = 0xA;
const DUID_PDU: u8 = 0xC;
const DUID_TERMINATOR_LC: u8 = 0xF;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "p25".to_owned(),
    name: "P25 Phase 1".to_owned(),
    summary: "P25 public safety voice".to_owned(),
    family: DecoderFamily::DigitalVoice,
    bandwidth_hz: BANDWIDTH_HZ,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: true,
    emits: vec![EventKind::Dv],
    calls: Some(CallGate::Frames),
    ..ChannelDescriptor::default()
});

pub struct P25Channel {
    demod: CpmDemod,
    symbols: Vec<f32>,
    decoder: Decoder,
}

fn params(settings: &ChannelSettings) -> Result<&P25Params, ChannelError> {
    match &settings.params {
        ChannelParams::P25(p) => Ok(p),
        other => Err(ChannelError::InvalidSettings(format!(
            "p25 channel got {} params",
            other.type_id()
        ))),
    }
}

pub(crate) fn occupied_band() -> (f64, f64) {
    (-BANDWIDTH_HZ / 2.0, BANDWIDTH_HZ / 2.0)
}

pub(crate) fn channel_filter() -> ChannelFilter {
    super::channel_filter(BANDWIDTH_HZ)
}

impl ChannelRx for P25Channel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        params(&settings)?;
        Ok(Self {
            demod: c4fm_demod(&c4fm_params(ctx.input_rate, BAUD, DEVIATION_HZ, RRC_ALPHA)),
            symbols: Vec::new(),
            decoder: Decoder::new(),
        })
    }

    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        params(&settings)?;
        Ok(())
    }

    fn retuned(&mut self) {
        self.demod.reset();
        self.decoder.reset();
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.symbols.clear();
        self.demod.process(iq, &mut self.symbols);
        tap_c4fm(out, &self.demod, &self.symbols, BAUD, INPUT_RATE_HZ);
        for &symbol in &self.symbols {
            self.decoder.push(symbol, out);
        }
    }

    fn rejected(&self) -> u32 {
        self.decoder.rejected()
    }
}

struct Decoder {
    window: SymbolWindow,
    countdown: usize,
    hunting: bool,
    bits: Vec<bool>,
    last_duid: Option<u8>,
    pending_duid: Option<u8>,
    pending_nac: u16,
    pending_errors: u32,
    next_imbe: usize,
    encryption: Option<bool>,
    vocoder: MbeDecoder,
    logical: Vec<bool>,
    rejected: u32,
}

impl Decoder {
    fn new() -> Self {
        Self {
            window: SymbolWindow::new(MAX_FRAME_SYMBOLS),
            countdown: 0,
            hunting: true,
            bits: Vec::with_capacity(NID_BITS + 2),
            last_duid: None,
            pending_duid: None,
            pending_nac: 0,
            pending_errors: 0,
            next_imbe: 0,
            encryption: None,
            vocoder: MbeDecoder::full_rate(),
            logical: Vec::with_capacity(MAX_FRAME_BITS),
            rejected: 0,
        }
    }

    fn rejected(&self) -> u32 {
        self.rejected
    }

    fn reset(&mut self) {
        self.window.reset();
        self.countdown = 0;
        self.hunting = true;
        self.last_duid = None;
        self.pending_duid = None;
        self.pending_nac = 0;
        self.pending_errors = 0;
        self.next_imbe = 0;
        self.encryption = None;
        self.vocoder.reset();
    }

    fn push(&mut self, symbol: f32, out: &mut ChannelOutputs) {
        self.window.push(symbol);
        if self.countdown > 0 {
            self.countdown -= 1;
            if let Some(duid) = self.pending_duid.filter(|&duid| is_voice(duid)) {
                self.voice(duid, out);
            }
            if self.countdown == 0 {
                if let Some(duid) = self.pending_duid.take() {
                    for frame in self.metadata(duid) {
                        out.events.push(DecoderEvent::Dv(frame));
                    }
                    if matches!(duid, DUID_TERMINATOR | DUID_TERMINATOR_LC) {
                        self.encryption = None;
                    }
                    self.hunting = true;
                } else if let Some((duid, frame)) = self.nid() {
                    if let Some(frame) = frame {
                        out.events.push(DecoderEvent::Dv(frame));
                    }
                    let total_symbols = frame_bits(duid) / 2;
                    let consumed = SYNC_BITS as usize / 2 + NID_SYMBOLS;
                    self.pending_duid = Some(duid);
                    self.next_imbe = 0;
                    self.countdown = total_symbols.saturating_sub(consumed);
                    if self.countdown == 0 {
                        self.pending_duid = None;
                        self.hunting = true;
                    }
                } else {
                    self.hunting = true;
                }
            }
            return;
        }
        if self.hunting && self.window.sync_distance(SYNC, SYNC_BITS) <= SYNC_TOLERANCE {
            self.window.anchor(SYNC, SYNC_BITS);
            self.hunting = false;
            self.countdown = NID_SYMBOLS;
        }
    }

    fn reject(&mut self) {
        self.rejected = self.rejected.saturating_add(1);
    }

    fn nid(&mut self) -> Option<(u8, Option<DvFrame>)> {
        self.window.bits(0, NID_SYMBOLS, &mut self.bits);
        let mut word = 0u64;
        for (i, &bit) in self.bits.iter().enumerate() {
            let frame_bit = SYNC_BITS as usize + i;
            if is_status(frame_bit) {
                continue;
            }
            word = word << 1 | u64::from(bit);
        }
        let Some((info, _)) = CyclicCode::BCH_63_16.decode(word) else {
            self.reject();
            return None;
        };
        let errors =
            ((CyclicCode::BCH_63_16.encode(info) ^ word) & !NID_ROUND_OUT_BIT).count_ones();
        let nac = (info >> 4) as u16 & 0x0FFF;
        let duid = info as u8 & 0x0F;

        let kind = match duid {
            DUID_HEADER => DvFrameKind::Header,
            DUID_TERMINATOR | DUID_TERMINATOR_LC => DvFrameKind::Terminator,
            DUID_TSDU => DvFrameKind::Control,
            DUID_PDU => DvFrameKind::Data,
            _ => DvFrameKind::Voice,
        };
        if kind == DvFrameKind::Voice {
            self.last_duid = Some(duid);
            self.pending_nac = nac;
            self.pending_errors = errors;
            return Some((duid, None));
        }
        self.last_duid = Some(duid);
        self.pending_nac = nac;
        self.pending_errors = errors;

        let mut frame = DvFrame::new(DvMode::P25, kind);
        frame.color_code = Some(nac);
        frame.errors_corrected = errors;
        frame.opcode = Some(duid_name(duid).to_owned());
        Some((duid, Some(frame)))
    }

    /// Hands each vocoder frame on the moment its last symbol lands, so a data unit leaves as
    /// nine 20 ms frames instead of one 180 ms burst that live audio downstream sheds.
    fn voice(&mut self, duid: u8, out: &mut ChannelOutputs) {
        let Some(&(first, ready)) = IMBE_SPANS.get(self.next_imbe) else {
            return;
        };
        if frame_bits(duid) / 2 - self.countdown != ready {
            return;
        }
        self.next_imbe += 1;
        self.window.bits(0, ready - first, &mut self.bits);
        let mut dibits = [0u8; IMBE_BITS / 2];
        let mut info = self
            .bits
            .iter()
            .enumerate()
            .filter(|&(i, _)| !is_status(first * 2 + i))
            .map(|(_, &bit)| u8::from(bit));
        for slot in &mut dibits {
            *slot = info.next().unwrap_or(0) << 1 | info.next().unwrap_or(0);
        }
        self.vocoder
            .decode_full_dibits(&dibits, self.encryption != Some(false), out);
    }

    fn logical_frame(&mut self, duid: u8) {
        let total_symbols = frame_bits(duid) / 2;
        self.window.bits(0, total_symbols, &mut self.bits);
        self.logical.clear();
        for (index, &bit) in self.bits.iter().enumerate() {
            if is_status(index) {
                continue;
            }
            self.logical.push(bit);
        }
    }

    fn metadata(&mut self, duid: u8) -> Vec<DvFrame> {
        self.logical_frame(duid);
        match duid {
            DUID_HEADER => self.header_data_unit().into_iter().collect(),
            DUID_LDU1 => self.ldu_link_control().into_iter().collect(),
            DUID_LDU2 => self.ldu_encryption_sync().into_iter().collect(),
            DUID_TSDU => self.trunking_blocks(),
            DUID_PDU => vec![self.packet_data()],
            _ => Vec::new(),
        }
    }

    fn signalling_symbols(&self) -> Option<([u8; 24], u32)> {
        let mut coded = Vec::with_capacity(240);
        for &voice_index in &[1usize, 2, 3, 4, 5, 6] {
            let start = BODY_START + (IMBE_OFFSETS[voice_index] + 72) * 2;
            coded.extend_from_slice(self.logical.get(start..start + 40)?);
        }
        let mut symbols = [0u8; 24];
        let mut errors = 0;
        for (symbol, word) in symbols
            .iter_mut()
            .zip(coded.as_slice().as_chunks::<10>().0.iter())
        {
            let mut word = *word;
            errors += ParityCode::HAMMING_10_6.decode(&mut word)?;
            *symbol = bits_value(&word[..6]) as u8;
        }
        Some((symbols, errors))
    }

    fn ldu_link_control(&mut self) -> Option<DvFrame> {
        let Some((symbols, hamming_errors)) = self.signalling_symbols() else {
            self.reject();
            return None;
        };
        let Some((data, rs_errors)) = rs64_decode(&symbols, 12) else {
            self.reject();
            return None;
        };
        let bits = symbols_to_bits(&data);
        let mut frame = decode_p25_link_control(&bits);
        frame.kind = DvFrameKind::Voice;
        self.finish_metadata(&mut frame, hamming_errors + rs_errors);
        if frame.encrypted.is_some() {
            self.encryption = frame.encrypted;
        }
        Some(frame)
    }

    fn ldu_encryption_sync(&mut self) -> Option<DvFrame> {
        let Some((symbols, hamming_errors)) = self.signalling_symbols() else {
            self.reject();
            return None;
        };
        let Some((data, rs_errors)) = rs64_decode(&symbols, 16) else {
            self.reject();
            return None;
        };
        let bits = symbols_to_bits(&data);
        let mut frame = DvFrame::new(DvMode::P25, DvFrameKind::Voice);
        frame.opcode = Some("encryption synchronization".to_owned());
        decode_encryption(&mut frame, &bits, 0);
        self.encryption = frame.encrypted;
        self.finish_metadata(&mut frame, hamming_errors + rs_errors);
        (frame.encrypted == Some(true)).then_some(frame)
    }

    fn header_data_unit(&mut self) -> Option<DvFrame> {
        let body = self.logical.get(BODY_START..BODY_START + 36 * 18)?;
        let mut symbols = [0u8; 36];
        let mut golay_errors = 0;
        for (symbol, word) in symbols.iter_mut().zip(body.as_chunks::<18>().0.iter()) {
            let received = word
                .iter()
                .fold(0u64, |value, &bit| value << 1 | u64::from(bit));
            let (data, errors) = CyclicCode::GOLAY_18_6.decode(received)?;
            *symbol = data as u8;
            golay_errors += errors;
        }
        let (data, rs_errors) = rs64_decode(&symbols, 20)?;
        let bits = symbols_to_bits(&data);
        let mut frame = DvFrame::new(DvMode::P25, DvFrameKind::Header);
        frame.opcode = Some("header data unit".to_owned());
        decode_p25_vendor(&mut frame, bits_value(&bits[72..80]) as u8);
        frame.message_indicator = Some(hex_bits(&bits[..72]));
        frame.algorithm_id = Some(bits_value(&bits[80..88]) as u8);
        frame.key_id = Some(bits_value(&bits[88..104]) as u16);
        frame.encrypted = frame.algorithm_id.map(|algorithm| algorithm != 0x80);
        frame.destination = Some(bits_value(&bits[104..120]));
        frame.group_call = Some(true);
        self.encryption = frame.encrypted;
        self.finish_metadata(&mut frame, golay_errors + rs_errors);
        Some(frame)
    }

    fn trunking_blocks(&mut self) -> Vec<DvFrame> {
        let mut frames = Vec::new();
        for block in 0..3 {
            let start = BODY_START + block * 196;
            let Some(coded) = self.logical.get(start..start + 196) else {
                break;
            };
            let Some(payload) = decode_p25_trellis(coded) else {
                self.rejected = self.rejected.saturating_add(1);
                continue;
            };
            let bytes: Vec<u8> = payload[..80]
                .as_chunks::<8>()
                .0
                .iter()
                .map(|byte| bits_value(byte) as u8)
                .collect();
            let found = bits_value(&payload[80..96]) as u16;
            if crc16_msb(0x1021, 0, &bytes) ^ 0xFFFF != found {
                self.rejected = self.rejected.saturating_add(1);
                continue;
            }
            let mut frame = decode_tsbk(&payload);
            frame.color_code = Some(self.pending_nac);
            frame.errors_corrected = self.pending_errors;
            frames.push(frame);
            if payload[0] {
                break;
            }
        }
        frames
    }

    fn packet_data(&self) -> DvFrame {
        let mut frame = DvFrame::new(DvMode::P25, DvFrameKind::Data);
        frame.opcode = Some("packet data unit".to_owned());
        frame.data = Some(hex_bits(
            &self.logical[BODY_START.min(self.logical.len())..],
        ));
        frame.color_code = Some(self.pending_nac);
        frame.errors_corrected = self.pending_errors;
        frame
    }

    fn finish_metadata(&self, frame: &mut DvFrame, errors: u32) {
        frame.color_code = Some(self.pending_nac);
        frame.errors_corrected = self.pending_errors + errors;
    }
}

fn bits_value(bits: &[bool]) -> u32 {
    bits.iter()
        .fold(0u32, |value, &bit| value << 1 | u32::from(bit))
}

fn symbols_to_bits(symbols: &[u8]) -> Vec<bool> {
    let mut bits = Vec::with_capacity(symbols.len() * 6);
    for &symbol in symbols {
        bits.extend((0..6).rev().map(|bit| symbol >> bit & 1 != 0));
    }
    bits
}

fn hex_bits(bits: &[bool]) -> String {
    bits.chunks(4)
        .map(|nibble| {
            char::from_digit(
                nibble
                    .iter()
                    .fold(0u32, |value, &bit| value << 1 | u32::from(bit)),
                16,
            )
            .unwrap_or('0')
        })
        .collect()
}

fn decode_p25_vendor(frame: &mut DvFrame, mfid: u8) {
    frame.manufacturer_id = Some(mfid);
    frame.vendor = Some(match mfid {
        0 => Vendor::Standard,
        0x90 => Vendor::Motorola,
        0xA4 => Vendor::Harris,
        _ => Vendor::Unknown,
    });
}

fn decode_p25_link_control(bits: &[bool]) -> DvFrame {
    let lcf = bits_value(&bits[..8]) as u8;
    let mfid = bits_value(&bits[8..16]) as u8;
    let mut frame = DvFrame::new(DvMode::P25, DvFrameKind::Voice);
    decode_p25_vendor(&mut frame, mfid);
    if mfid != 0 {
        frame.opcode = Some(format!(
            "{} link control {lcf:02X}, unparsed",
            frame.vendor.map_or("vendor", Vendor::label)
        ));
        frame.data = Some(hex_bits(&bits[16..72]));
        return frame;
    }
    match lcf {
        0 => {
            frame.opcode = Some("group voice channel user".to_owned());
            frame.group_call = Some(true);
            frame.emergency = Some(bits[16]);
            frame.encrypted = Some(bits[17]);
            frame.destination = Some(bits_value(&bits[32..48]));
            frame.source = Some(bits_value(&bits[48..72]));
        }
        3 => {
            frame.opcode = Some("unit-to-unit voice channel user".to_owned());
            frame.group_call = Some(false);
            frame.emergency = Some(bits[16]);
            frame.encrypted = Some(bits[17]);
            frame.destination = Some(bits_value(&bits[24..48]));
            frame.source = Some(bits_value(&bits[48..72]));
        }
        _ => frame.opcode = Some(format!("link control {lcf:02X}")),
    }
    frame
}

fn decode_encryption(frame: &mut DvFrame, bits: &[bool], offset: usize) {
    frame.message_indicator = Some(hex_bits(&bits[offset..offset + 72]));
    frame.algorithm_id = Some(bits_value(&bits[offset + 72..offset + 80]) as u8);
    frame.key_id = Some(bits_value(&bits[offset + 80..offset + 96]) as u16);
    frame.encrypted = frame.algorithm_id.map(|algorithm| algorithm != 0x80);
}

fn decode_p25_trellis(coded: &[bool]) -> Option<[bool; 96]> {
    const OUTPUT: [[[u8; 2]; 4]; 4] = [
        [[0, 2], [3, 0], [0, 1], [3, 3]],
        [[3, 2], [0, 0], [3, 1], [0, 3]],
        [[2, 1], [1, 3], [2, 2], [1, 0]],
        [[1, 1], [2, 3], [1, 2], [2, 0]],
    ];
    let deinterleaved = p25_data_deinterleave(coded)?;
    let dibits: Vec<u8> = deinterleaved
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from(pair[0]) << 1 | u8::from(pair[1]))
        .collect();
    if dibits.len() != 98 {
        return None;
    }
    let mut metric = [u16::MAX; 4];
    metric[0] = 0;
    let mut history = [[(0u8, 0u8); 4]; 49];
    for step in 0..49 {
        let observed = [dibits[step * 2], dibits[step * 2 + 1]];
        let mut next = [u16::MAX; 4];
        for (state, &cost) in metric.iter().enumerate() {
            if cost == u16::MAX {
                continue;
            }
            for input in 0..4 {
                let expected = OUTPUT[state][input];
                let distance =
                    u16::from(expected[0] != observed[0]) + u16::from(expected[1] != observed[1]);
                if cost + distance < next[input] {
                    next[input] = cost + distance;
                    history[step][input] = (state as u8, input as u8);
                }
            }
        }
        metric = next;
    }
    let mut state = 0usize;
    let mut input = [0u8; 49];
    for step in (0..49).rev() {
        let (previous, value) = history[step][state];
        input[step] = value;
        state = usize::from(previous);
    }
    let mut payload = [false; 96];
    for (pair, &dibit) in payload.as_chunks_mut::<2>().0.iter_mut().zip(&input[..48]) {
        pair[0] = dibit & 2 != 0;
        pair[1] = dibit & 1 != 0;
    }
    Some(payload)
}

fn p25_data_deinterleave(coded: &[bool]) -> Option<[bool; 196]> {
    let coded: &[bool; 196] = coded.try_into().ok()?;
    let mut output = [false; 196];
    let mut target = 0;
    for row in 0..12 {
        for base in [0, 52, 100, 148] {
            output[target..target + 4].copy_from_slice(&coded[base + row * 4..base + row * 4 + 4]);
            target += 4;
        }
    }
    output[target..].copy_from_slice(&coded[48..52]);
    Some(output)
}

fn decode_tsbk(payload: &[bool; 96]) -> DvFrame {
    let opcode = bits_value(&payload[2..8]) as u8;
    let mfid = bits_value(&payload[8..16]) as u8;
    let mut frame = DvFrame::new(DvMode::P25, DvFrameKind::Control);
    decode_p25_vendor(&mut frame, mfid);
    if mfid != 0 {
        frame.opcode = Some(format!(
            "{} TSBK {opcode:02X}, unparsed",
            frame.vendor.map_or("vendor", Vendor::label)
        ));
        frame.data = Some(hex_bits(&payload[16..80]));
        return frame;
    }
    frame.opcode = Some(tsbk_name(opcode).to_owned());
    match opcode {
        0 => {
            frame.group_call = Some(true);
            frame.emergency = Some(payload[16]);
            frame.encrypted = Some(payload[17]);
            frame.channel = Some(bits_value(&payload[24..40]) as u16);
            frame.destination = Some(bits_value(&payload[40..56]));
            frame.source = Some(bits_value(&payload[56..80]));
        }
        2 => {
            frame.group_call = Some(true);
            frame.channel = Some(bits_value(&payload[16..32]) as u16);
            frame.destination = Some(bits_value(&payload[32..48]));
            frame.data = Some(format!(
                "second channel {:04X}, TG {}",
                bits_value(&payload[48..64]),
                bits_value(&payload[64..80])
            ));
        }
        3 => {
            frame.group_call = Some(true);
            frame.emergency = Some(payload[16]);
            frame.encrypted = Some(payload[17]);
            frame.channel = Some(bits_value(&payload[32..48]) as u16);
            frame.destination = Some(bits_value(&payload[64..80]));
            frame.data = Some(format!(
                "receive channel {:04X}",
                bits_value(&payload[48..64])
            ));
        }
        4 => {
            frame.group_call = Some(false);
            frame.emergency = Some(payload[16]);
            frame.encrypted = Some(payload[17]);
            frame.channel = Some(bits_value(&payload[24..40]) as u16);
            frame.destination = Some(bits_value(&payload[40..64]));
            frame.data = Some(hex_bits(&payload[64..80]));
        }
        0x3A | 0x3C => {
            if opcode == 0x3A {
                frame.system_id = Some(bits_value(&payload[28..40]) as u16);
            }
            frame.site_id = Some(bits_value(&payload[48..56]) as u16);
            frame.channel = Some(bits_value(&payload[56..72]) as u16);
        }
        0x3B => {
            frame.network_id = Some(bits_value(&payload[24..44]));
            frame.system_id = Some(bits_value(&payload[44..56]) as u16);
            frame.channel = Some(bits_value(&payload[56..72]) as u16);
        }
        _ => frame.data = Some(hex_bits(&payload[16..80])),
    }
    frame
}

fn tsbk_name(opcode: u8) -> &'static str {
    match opcode {
        0 => "group voice channel grant",
        2 => "group voice channel grant update",
        3 => "group voice channel grant update explicit",
        4 => "unit-to-unit voice channel grant",
        0x3A => "RFSS status broadcast",
        0x3B => "network status broadcast",
        0x3C => "adjacent site status broadcast",
        _ => "trunking signalling block",
    }
}

fn frame_bits(duid: u8) -> usize {
    match duid {
        DUID_HEADER => 792,
        DUID_TERMINATOR => 144,
        DUID_TSDU => 720,
        DUID_TERMINATOR_LC => 432,
        _ => MAX_FRAME_BITS,
    }
}

const fn is_status(frame_bit: usize) -> bool {
    frame_bit >= STATUS_START && (frame_bit - STATUS_START) % STATUS_STRIDE < 2
}

const fn on_air_bit(logical: usize) -> usize {
    logical + (STATUS_STRIDE - STATUS_START) * (logical / STATUS_START)
}

/// The first symbol of each vocoder frame in a voice data unit, and the symbol count at which
/// its last bit has arrived.
const fn imbe_spans() -> [(usize, usize); 9] {
    let mut spans = [(0, 0); 9];
    let mut i = 0;
    while i < spans.len() {
        let start = BODY_START + IMBE_OFFSETS[i] * 2;
        spans[i] = (
            on_air_bit(start) / 2,
            on_air_bit(start + IMBE_BITS - 1) / 2 + 1,
        );
        i += 1;
    }
    spans
}

fn is_voice(duid: u8) -> bool {
    matches!(duid, DUID_LDU1 | DUID_LDU2)
}

fn duid_name(duid: u8) -> &'static str {
    match duid {
        DUID_HEADER => "header",
        DUID_TERMINATOR => "terminator",
        DUID_LDU1 => "voice (LDU1)",
        DUID_TSDU => "trunking block",
        DUID_LDU2 => "voice (LDU2)",
        DUID_PDU => "packet data",
        DUID_TERMINATOR_LC => "terminator with link control",
        _ => "reserved",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        dv::{
            testutil::{assert_tone_audio, decode, decode_with_audio},
            vocoder::testutil::full_rate_frames,
        },
        synth::dv::p25 as tx,
        testutil::settings,
    };

    fn channel() -> P25Channel {
        P25Channel::new(
            ChannelCtx {
                input_rate: INPUT_RATE_HZ,
            },
            settings(ChannelParams::P25(P25Params::default())),
        )
        .expect("p25 channel")
    }

    #[test]
    fn decodes_the_network_identifier_of_a_transmission() {
        let iq = tx::transmission(0x293, INPUT_RATE_HZ);
        let frames = decode(&mut channel(), &iq);

        let header = frames.first().expect("a decoded frame");
        assert_eq!(header.mode, DvMode::P25);
        assert_eq!(header.kind, DvFrameKind::Header);
        assert_eq!(header.color_code, Some(0x293));
        assert_eq!(header.errors_corrected, 0);
        let hdu = frames
            .iter()
            .find(|frame| frame.opcode.as_deref() == Some("header data unit"))
            .expect("decoded HDU");
        assert_eq!(hdu.destination, Some(0x1201));
        assert_eq!(hdu.algorithm_id, Some(0x80));
        assert_eq!(hdu.key_id, Some(0x1234));
        let link_control = frames
            .iter()
            .find(|frame| frame.opcode.as_deref() == Some("group voice channel user"))
            .expect("LDU1 link control");
        assert_eq!(link_control.destination, Some(0x1201));
        assert_eq!(link_control.source, Some(0xABCDEF));
        assert!(
            frames.iter().any(|f| f.kind == DvFrameKind::Terminator),
            "no terminator: {frames:?}"
        );
        assert_eq!(
            frames
                .iter()
                .filter(|f| f.kind == DvFrameKind::Voice)
                .count(),
            1,
            "{frames:?}"
        );
    }

    #[test]
    fn decodes_a_trunking_block() {
        let iq = tx::trunking(0x4D2, INPUT_RATE_HZ);
        let frames = decode(&mut channel(), &iq);
        let control = frames
            .iter()
            .find(|frame| frame.opcode.as_deref() == Some("group voice channel grant"))
            .expect("decoded TSBK");
        assert_eq!(control.color_code, Some(0x4D2));
        assert_eq!(control.channel, Some(0x1234));
        assert_eq!(control.destination, Some(0x1201));
        assert_eq!(control.source, Some(0xABCDEF));
    }

    #[test]
    fn decodes_imbe_voice_to_audio() {
        let encoded = full_rate_frames(18);
        let voice: [[[bool; 144]; 9]; 2] =
            std::array::from_fn(|ldu| std::array::from_fn(|frame| encoded[ldu * 9 + frame]));
        let iq = tx::transmission_with_voice(0x293, &voice, INPUT_RATE_HZ);
        let (_, audio) = decode_with_audio(&mut channel(), &iq);
        assert_tone_audio(&audio, 18);
    }

    #[test]
    fn voice_leaves_the_decoder_one_vocoder_frame_at_a_time() {
        const BLOCK: usize = 480;
        let encoded = full_rate_frames(18);
        let voice: [[[bool; 144]; 9]; 2] =
            std::array::from_fn(|ldu| std::array::from_fn(|frame| encoded[ldu * 9 + frame]));
        let iq = tx::transmission_with_voice(0x293, &voice, INPUT_RATE_HZ);
        let mut channel = channel();
        let mut out = ChannelOutputs::default();
        let mut largest = 0;
        let mut total = 0;
        for block in iq.chunks(BLOCK) {
            out.reset();
            channel.process(block, &mut out);
            largest = largest.max(out.audio_pcm.len());
            total += out.audio_pcm.len();
        }
        assert!((total as isize - 18 * 960).abs() <= 1, "{total} samples");
        assert!(largest <= 960 + 1, "{largest} samples left in one block");
    }

    #[test]
    fn a_late_entry_stays_muted_until_the_call_says_it_is_clear() {
        const HDU_TAIL_SYMBOL: usize = 200 + 380;
        let encoded = full_rate_frames(18);
        let voice: [[[bool; 144]; 9]; 2] =
            std::array::from_fn(|ldu| std::array::from_fn(|frame| encoded[ldu * 9 + frame]));
        let iq = tx::transmission_with_voice(0x293, &voice, INPUT_RATE_HZ);
        let samples_per_symbol = (INPUT_RATE_HZ / BAUD) as usize;
        let (frames, audio) =
            decode_with_audio(&mut channel(), &iq[HDU_TAIL_SYMBOL * samples_per_symbol..]);
        assert!(
            frames.iter().all(|f| f.kind != DvFrameKind::Header),
            "{frames:?}"
        );
        assert_eq!(audio.len(), 18 * 960);
        let (first, second) = audio.split_at(9 * 960);
        assert!(first.iter().all(|&sample| sample == 0.0));
        assert!(crate::testutil::rms(second) > 0.005);
    }

    #[test]
    fn encrypted_calls_report_identifiers_and_are_muted() {
        let encoded = full_rate_frames(18);
        let voice: [[[bool; 144]; 9]; 2] =
            std::array::from_fn(|ldu| std::array::from_fn(|frame| encoded[ldu * 9 + frame]));
        let iq = tx::encrypted_transmission(0x293, &voice, INPUT_RATE_HZ);
        let (frames, audio) = decode_with_audio(&mut channel(), &iq);
        let hdu = frames
            .iter()
            .find(|frame| frame.opcode.as_deref() == Some("header data unit"))
            .expect("HDU encryption metadata");
        assert_eq!(hdu.encrypted, Some(true));
        assert_eq!(hdu.algorithm_id, Some(0x84));
        assert_eq!(hdu.key_id, Some(0x1234));
        assert!(!audio.is_empty());
        assert!(audio.iter().all(|&sample| sample == 0.0));
    }

    fn spec_symbol(dibit: u8) -> f32 {
        const SPEC_SYMBOL_FOR_DIBIT: [f32; 4] = [1.0, 3.0, -1.0, -3.0];
        SPEC_SYMBOL_FOR_DIBIT[usize::from(dibit)]
    }

    #[test]
    fn the_frame_sync_expands_the_tia_102_baaa_a_sync_word_sequence() {
        const SPEC_SYNC: u32 = 0x04_CF5F;
        const SPEC_SYNC_BITS: u32 = 24;
        let expanded = (0..SPEC_SYNC_BITS).rev().fold(0u64, |word, shift| {
            let dibit = if SPEC_SYNC >> shift & 1 == 1 {
                0b11
            } else {
                0b01
            };
            word << 2 | dibit
        });
        assert_eq!(SPEC_SYNC_BITS * 2, SYNC_BITS);
        assert_eq!(expanded, SYNC);
    }

    #[test]
    fn the_status_symbols_follow_every_seventy_information_bits() {
        assert_eq!(STATUS_START + 2, STATUS_STRIDE);
        for (duid, expected) in [
            (DUID_HEADER, 11),
            (DUID_LDU1, 24),
            (DUID_LDU2, 24),
            (DUID_TERMINATOR, 2),
            (DUID_TERMINATOR_LC, 6),
        ] {
            let inserted = (0..frame_bits(duid)).filter(|&bit| is_status(bit)).count();
            assert_eq!(inserted, expected * 2, "data unit {duid:#X}");
        }
    }

    #[test]
    fn a_network_identifier_from_the_tia_102_baaa_a_generator_matrix_decodes() {
        const SPEC_PARITY: [u64; 16] = [
            0o6331141367235452,
            0o5265521614723276,
            0o4603711461164164,
            0o2301744630472072,
            0o7271623073000466,
            0o5605650752635660,
            0o2702724365316730,
            0o1341352172547354,
            0o0560565075263566,
            0o6141333751704220,
            0o3060555764742110,
            0o1430266772361044,
            0o0614133375170422,
            0o6037114611641642,
            0o5326507063515373,
            0o4662302756473127,
        ];
        const SPEC_STATUS_IDLE: u8 = 0b11;
        for (nac, duid, kind) in [
            (0x293u32, DUID_HEADER, DvFrameKind::Header),
            (0x293, DUID_TERMINATOR, DvFrameKind::Terminator),
            (0xFFF, DUID_TSDU, DvFrameKind::Control),
            (0x001, DUID_PDU, DvFrameKind::Data),
        ] {
            let info = nac << 4 | u32::from(duid);
            let word = (0..16)
                .filter(|bit| info >> bit & 1 == 1)
                .fold(u64::from(info) << 48, |word, bit| {
                    word ^ SPEC_PARITY[15 - bit]
                });

            let mut decoder = Decoder::new();
            let mut taken = 0;
            for symbol in 0..NID_SYMBOLS {
                let dibit = if is_status(SYNC_BITS as usize + symbol * 2) {
                    SPEC_STATUS_IDLE
                } else {
                    taken += 2;
                    (word >> (NID_BITS - taken)) as u8 & 0b11
                };
                decoder.window.push(spec_symbol(dibit));
            }

            let (decoded, frame) = decoder.nid().expect("conformant network identifier");
            assert_eq!(decoded, duid, "NAC {nac:#05X}");
            let frame = frame.expect("network identifier metadata");
            assert_eq!(frame.kind, kind);
            assert_eq!(frame.color_code, Some(nac as u16));
            assert_eq!(frame.errors_corrected, 0, "NAC {nac:#05X}");
        }
    }

    #[test]
    fn noise_decodes_to_nothing() {
        let noise = crate::testutil::complex_noise(33, 0.5, 400_000);
        assert!(decode(&mut channel(), &noise).is_empty());
    }
}
