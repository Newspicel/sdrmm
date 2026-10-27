use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_dsp::{
    Viterbi5,
    fec::conv::{CONFIDENT, ERASURE},
};
use sdrmm_modem::cpm::CpmDemod;
use sdrmm_wire::{
    CallGate, ChannelDescriptor, ChannelParams, ChannelSettings, DecoderEvent, DecoderFamily,
    DvFrame, DvFrameKind, DvMode, EventKind, NxdnBandwidth, NxdnParams,
};

use super::{
    INPUT_RATE_HZ, SymbolWindow, bits_to_u32, c4fm_demod, c4fm_params, inverted, tap_c4fm,
    vocoder::{AMBE_3600_INTERLEAVE, MbeDecoder, half_rate_code_vectors},
};
use crate::{ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, check_input_rate};

pub(crate) const FSW: u64 = 0x000C_DF59;
pub(crate) const FSW_BITS: u32 = 20;
pub(crate) const FSWS: [u64; 2] = [FSW, inverted(FSW, FSW_BITS)];
pub(crate) const SYNC_TOLERANCE: u32 = 2;

const LICH_BITS: usize = 8;
const LICH_SYMBOLS: usize = 8;
const FSW_SYMBOLS: usize = FSW_BITS as usize / 2;

const FRAME_SYMBOLS: u64 = 192;
const HELD_RELEASE_SYMBOLS: u64 = FRAME_SYMBOLS * 3;
const POST_FSW_SYMBOLS: usize = FRAME_SYMBOLS as usize - FSW_SYMBOLS;
const SACCH_SYMBOLS: usize = 30;
const VOICE_START: usize = LICH_SYMBOLS + SACCH_SYMBOLS;
const SACCH_BITS: usize = SACCH_SYMBOLS * 2;
const FACCH_BITS: usize = 144;
const FACCH_INFO_BITS: usize = 80;
const FACCH_FIRST: usize = VOICE_START * 2;
const FACCH_SECOND: usize = FACCH_FIRST + FACCH_BITS;

const SACCH_PUNCTURES: [usize; 12] = [5, 11, 17, 23, 29, 35, 41, 47, 53, 59, 65, 71];
const FACCH_PUNCTURES: [usize; 48] = [
    1, 5, 9, 13, 17, 21, 25, 29, 33, 37, 41, 45, 49, 53, 57, 61, 65, 69, 73, 77, 81, 85, 89, 93,
    97, 101, 105, 109, 113, 117, 121, 125, 129, 133, 137, 141, 145, 149, 153, 157, 161, 165, 169,
    173, 177, 181, 185, 189,
];

const L3_VOICE_CALL: u32 = 0x01;
const L3_TX_RELEASE: u32 = 0x08;
const L3_PROPRIETARY: u32 = 0x3F;
const KENWOOD_ALIAS: u32 = 0x68_8204;
const ALIAS_SEGMENTS: usize = 4;
const ALIAS_CRC_BYTES: usize = 2;

pub(crate) const RRC_ALPHA: f64 = 0.2;

pub(crate) fn shape(bandwidth: NxdnBandwidth) -> (f64, f64, f64) {
    match bandwidth {
        NxdnBandwidth::Narrow => (2_400.0, 1_050.0, 6_250.0),
        NxdnBandwidth::Wide => (4_800.0, 1_944.0, 12_500.0),
    }
}

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "nxdn".to_owned(),
    name: "NXDN".to_owned(),
    summary: "NXDN two-way radio voice".to_owned(),
    family: DecoderFamily::DigitalVoice,
    bandwidth_hz: 6_250.0,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: true,
    emits: vec![EventKind::Dv],
    calls: Some(CallGate::Frames),
    ..ChannelDescriptor::default()
});

pub struct NxdnChannel {
    demod: CpmDemod,
    symbols: Vec<f32>,
    decoder: Decoder,
    bandwidth: NxdnBandwidth,
    input_rate: f64,
    baud: f64,
}

fn params(settings: &ChannelSettings) -> Result<&NxdnParams, ChannelError> {
    match &settings.params {
        ChannelParams::Nxdn(p) => Ok(p),
        other => Err(ChannelError::InvalidSettings(format!(
            "nxdn channel got {} params",
            other.type_id()
        ))),
    }
}

pub(crate) fn occupied_band(p: &NxdnParams) -> (f64, f64) {
    let (_, _, bandwidth) = shape(p.bandwidth);
    (-bandwidth / 2.0, bandwidth / 2.0)
}

pub(crate) fn channel_filter(p: &NxdnParams) -> ChannelFilter {
    let (_, _, bandwidth) = shape(p.bandwidth);
    super::channel_filter(bandwidth)
}

impl ChannelRx for NxdnChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let p = *params(&settings)?;
        let (baud, deviation, _) = shape(p.bandwidth);
        Ok(Self {
            demod: c4fm_demod(&c4fm_params(ctx.input_rate, baud, deviation, RRC_ALPHA)),
            symbols: Vec::new(),
            decoder: Decoder::new(),
            bandwidth: p.bandwidth,
            input_rate: ctx.input_rate,
            baud,
        })
    }

    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let p = *params(&settings)?;
        if p.bandwidth != self.bandwidth {
            let (baud, deviation, _) = shape(p.bandwidth);
            self.demod = c4fm_demod(&c4fm_params(self.input_rate, baud, deviation, RRC_ALPHA));
            self.baud = baud;
            self.decoder.reset();
            self.bandwidth = p.bandwidth;
        }
        Ok(())
    }

    fn retuned(&mut self) {
        self.demod.reset();
        self.decoder.reset();
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.symbols.clear();
        self.demod.process(iq, &mut self.symbols);
        tap_c4fm(out, &self.demod, &self.symbols, self.baud, self.input_rate);
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
    last_kind: Option<DvFrameKind>,
    clock: u64,
    held: Option<Held>,
    sync_at: u64,
    vocoder: MbeDecoder,
    viterbi: Viterbi5,
    soft: Vec<i16>,
    sacch: SacchAssembler,
    alias: AliasAssembler,
    inverted: bool,
    in_run: bool,
    rejected: u32,
}

struct Held {
    at: u64,
    frame: DvFrame,
    voice: Vec<[bool; 72]>,
    confirmed: bool,
}

impl Decoder {
    fn new() -> Self {
        Self {
            window: SymbolWindow::new(POST_FSW_SYMBOLS.max(FSW_SYMBOLS)),
            countdown: 0,
            hunting: true,
            bits: Vec::with_capacity(LICH_BITS),
            last_kind: None,
            clock: 0,
            held: None,
            sync_at: 0,
            vocoder: MbeDecoder::half_rate(),
            viterbi: Viterbi5::new(),
            soft: Vec::with_capacity(POST_FSW_SYMBOLS * 2),
            sacch: SacchAssembler::default(),
            alias: AliasAssembler::default(),
            inverted: false,
            in_run: false,
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
        self.last_kind = None;
        self.clock = 0;
        self.held = None;
        self.sync_at = 0;
        self.vocoder.reset();
        self.sacch.reset();
        self.alias = AliasAssembler::default();
        self.inverted = false;
        self.in_run = false;
    }

    fn push(&mut self, symbol: f32, out: &mut ChannelOutputs) {
        self.window.push(symbol);
        self.clock += 1;
        if self.countdown > 0 {
            self.countdown -= 1;
            if self.countdown == 0 {
                self.hunting = true;
                self.held = self.frame().map(|(frame, voice)| Held {
                    at: self.sync_at,
                    frame,
                    voice,
                    confirmed: self.in_run,
                });
            }
            return;
        }
        if self
            .held
            .as_ref()
            .is_some_and(|held| self.clock > held.at + HELD_RELEASE_SYMBOLS)
        {
            self.release(false, out);
            self.in_run = false;
        }
        let Some(fsw) = self.hunting_sync() else {
            return;
        };
        if let Some(held) = &self.held
            && self.clock < held.at + FRAME_SYMBOLS - 1
        {
            return;
        }
        match self
            .held
            .as_ref()
            .map(|held| on_cadence(self.clock - held.at))
        {
            Some(cadence) => {
                self.release(cadence, out);
                self.in_run = cadence;
            }
            None => self.in_run = false,
        }
        self.window.anchor(fsw, FSW_BITS);
        self.inverted = fsw != FSW;
        self.hunting = false;
        self.sync_at = self.clock;
        self.countdown = POST_FSW_SYMBOLS;
    }

    fn release(&mut self, trailing_sync_on_cadence: bool, out: &mut ChannelOutputs) {
        let Some(held) = self.held.take() else {
            return;
        };
        if !trailing_sync_on_cadence && !held.confirmed {
            self.rejected = self.rejected.saturating_add(1);
            return;
        }
        self.emit(held.frame, out);
        for frame in held.voice {
            self.vocoder.decode_half_code_vectors(
                half_rate_code_vectors(&frame, &AMBE_3600_INTERLEAVE),
                false,
                out,
            );
        }
    }

    fn hunting_sync(&self) -> Option<u64> {
        if !self.hunting {
            return None;
        }
        FSWS.into_iter()
            .find(|&fsw| self.window.sync_distance(fsw, FSW_BITS) <= SYNC_TOLERANCE)
    }

    fn emit(&mut self, frame: DvFrame, out: &mut ChannelOutputs) {
        if frame.kind == DvFrameKind::Voice
            && self.last_kind == Some(DvFrameKind::Voice)
            && frame.source.is_none()
            && frame.talker_alias.is_none()
        {
            return;
        }
        self.last_kind = Some(frame.kind);
        out.events.push(DecoderEvent::Dv(frame));
    }

    fn frame(&mut self) -> Option<(DvFrame, Vec<[bool; 72]>)> {
        self.window.soft_bits(0, POST_FSW_SYMBOLS, &mut self.soft);
        let mut register = 0xE4u16;
        for symbol in 0..POST_FSW_SYMBOLS {
            let pn = register & 1 != 0;
            let feedback = (register ^ (register >> 4)) & 1;
            register = register >> 1 | feedback << 8;
            if pn != self.inverted {
                self.soft[symbol * 2] = -self.soft[symbol * 2];
            }
        }
        self.bits.clear();
        self.bits.extend(self.soft.iter().map(|&bit| bit > 0));
        let information: Vec<bool> = (0..LICH_SYMBOLS).map(|i| self.bits[i * 2]).collect();
        if !lich_parity_holds(&information) {
            return None;
        }
        let rf_channel = bits_to_u32(&information, 0, 2);
        let functional = bits_to_u32(&information, 2, 2);
        let outbound = information[6];

        let kind = match functional {
            0 => DvFrameKind::Header,
            1 => DvFrameKind::Data,
            _ if rf_channel == 0 => DvFrameKind::Control,
            _ => DvFrameKind::Voice,
        };
        let mut frame = DvFrame::new(DvMode::Nxdn, kind);
        frame.opcode = Some(format!(
            "{} {}",
            channel_name(rf_channel),
            if outbound { "outbound" } else { "inbound" }
        ));
        let option = bits_to_u32(&information, 4, 2);
        let sacch_start = LICH_SYMBOLS * 2;
        if let Some(sacch) = decode_sacch(
            &self.soft[sacch_start..sacch_start + SACCH_BITS],
            &mut self.viterbi,
        ) {
            frame.color_code = Some(u16::from(sacch.ran));
            if let Some(layer3) = self.sacch.push(sacch) {
                self.apply_layer3(&mut frame, &layer3);
            }
        }
        let facch_ranges: &[(usize, usize)] = match option {
            0 => &[
                (FACCH_FIRST, FACCH_FIRST + FACCH_BITS),
                (FACCH_SECOND, FACCH_SECOND + FACCH_BITS),
            ],
            1 => &[(FACCH_FIRST, FACCH_FIRST + FACCH_BITS)],
            2 => &[(FACCH_SECOND, FACCH_SECOND + FACCH_BITS)],
            _ => &[],
        };
        for &(start, end) in facch_ranges {
            if let Some(layer3) = decode_facch(&self.soft[start..end], &mut self.viterbi) {
                self.apply_layer3(&mut frame, &layer3);
                break;
            }
        }
        let mut voice = Vec::new();
        if rf_channel != 0 && matches!(functional, 0 | 2) {
            let ranges: &[(usize, usize)] = match option {
                1 => &[(VOICE_START + 72, VOICE_START + 144)],
                2 => &[(VOICE_START, VOICE_START + 72)],
                3 => &[(VOICE_START, VOICE_START + 144)],
                _ => &[],
            };
            for &(start, end) in ranges {
                for frame_start in (start..end).step_by(36) {
                    let mut frame_bits = [false; 72];
                    frame_bits.copy_from_slice(&self.bits[frame_start * 2..frame_start * 2 + 72]);
                    voice.push(frame_bits);
                }
            }
        }
        Some((frame, voice))
    }

    fn apply_layer3(&mut self, frame: &mut DvFrame, layer3: &[bool]) {
        if let Some(segment) = alias_segment(layer3) {
            frame.talker_alias = self.alias.push(segment);
        } else {
            apply_call(frame, layer3);
        }
    }
}

fn lich_parity_holds(information: &[bool]) -> bool {
    information[..4]
        .iter()
        .fold(false, |parity, &bit| parity ^ bit)
        == information[7]
}

#[derive(Clone, Copy)]
struct Sacch {
    ran: u8,
    structure: u8,
    data: [bool; 18],
}

struct SacchAssembler {
    data: [bool; 72],
    received: u8,
}

impl Default for SacchAssembler {
    fn default() -> Self {
        Self {
            data: [false; 72],
            received: 0,
        }
    }
}

impl SacchAssembler {
    fn reset(&mut self) {
        self.received = 0;
        self.data.fill(false);
    }

    fn push(&mut self, sacch: Sacch) -> Option<[bool; 72]> {
        let quarter = 3usize.saturating_sub(usize::from(sacch.structure));
        if quarter == 0 {
            self.received = 0;
        }
        self.data[quarter * 18..quarter * 18 + 18].copy_from_slice(&sacch.data);
        self.received |= 1 << quarter;
        if self.received == 0x0f {
            self.received = 0;
            Some(self.data)
        } else {
            None
        }
    }
}

fn decode_sacch(channel: &[i16], viterbi: &mut Viterbi5) -> Option<Sacch> {
    if channel.len() != SACCH_BITS {
        return None;
    }
    let mut deinterleaved = [ERASURE; SACCH_BITS];
    for (i, value) in deinterleaved.iter_mut().enumerate() {
        *value = channel[(i % 12) * 5 + i / 12];
    }
    let mut coded = Vec::with_capacity(80);
    let mut read = 0;
    for i in 0..72 {
        if SACCH_PUNCTURES.contains(&i) {
            coded.push(ERASURE);
        } else {
            coded.push(deinterleaved[read]);
            read += 1;
        }
    }
    coded.extend([-CONFIDENT; 8]);
    let mut info = Vec::with_capacity(40);
    viterbi.decode(&coded, &mut info);
    if info.len() < 32 || crc_msb(0x27, 0x3f, &info[..26]) != bits_to_u32(&info, 26, 6) {
        return None;
    }
    let data = std::array::from_fn(|i| info[8 + i]);
    Some(Sacch {
        structure: bits_to_u32(&info, 0, 2) as u8,
        ran: bits_to_u32(&info, 2, 6) as u8,
        data,
    })
}

fn decode_facch(channel: &[i16], viterbi: &mut Viterbi5) -> Option<[bool; FACCH_INFO_BITS]> {
    if channel.len() != FACCH_BITS {
        return None;
    }
    let mut deinterleaved = [ERASURE; FACCH_BITS];
    for (i, value) in deinterleaved.iter_mut().enumerate() {
        *value = channel[(i % 16) * 9 + i / 16];
    }
    let mut coded = Vec::with_capacity(200);
    let mut read = 0;
    for i in 0..192 {
        if FACCH_PUNCTURES.contains(&i) {
            coded.push(ERASURE);
        } else {
            coded.push(deinterleaved[read]);
            read += 1;
        }
    }
    coded.extend([-CONFIDENT; 8]);
    let mut info = Vec::with_capacity(100);
    viterbi.decode(&coded, &mut info);
    if info.len() < 92 || crc_msb(0x080f, 0x0fff, &info[..80]) != bits_to_u32(&info, 80, 12) {
        return None;
    }
    Some(std::array::from_fn(|i| info[i]))
}

fn crc_msb(poly: u32, init: u32, bits: &[bool]) -> u32 {
    let width = 32 - poly.leading_zeros();
    let top = 1 << (width - 1);
    let mask = (1 << width) - 1;
    bits.iter().fold(init, |crc, &bit| {
        let feedback = bit ^ (crc & top != 0);
        ((crc << 1) ^ if feedback { poly } else { 0 }) & mask
    })
}

struct AliasSegment {
    index: usize,
    total: usize,
    bytes: [u8; 4],
}

#[derive(Default)]
struct AliasAssembler {
    bytes: [u8; ALIAS_SEGMENTS * 4],
    total: usize,
    received: u8,
}

impl AliasAssembler {
    fn push(&mut self, segment: AliasSegment) -> Option<String> {
        if segment.total != self.total {
            *self = Self {
                total: segment.total,
                ..Self::default()
            };
        }
        self.bytes[segment.index * 4..segment.index * 4 + 4].copy_from_slice(&segment.bytes);
        self.received |= 1 << segment.index;
        if self.received != (1 << self.total) - 1 {
            return None;
        }
        let text: String = self.bytes[..self.total * 4 - ALIAS_CRC_BYTES]
            .iter()
            .take_while(|&&byte| byte != 0)
            .map(|&byte| char::from(byte))
            .filter(|c| c.is_ascii_graphic() || *c == ' ')
            .collect();
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_owned())
    }
}

fn alias_segment(layer3: &[bool]) -> Option<AliasSegment> {
    if bits_to_u32(layer3, 2, 6) != L3_PROPRIETARY || bits_to_u32(layer3, 8, 24) != KENWOOD_ALIAS {
        return None;
    }
    let index = bits_to_u32(layer3, 32, 4) as usize;
    let total = bits_to_u32(layer3, 36, 4) as usize;
    if index == 0 || index > total || total > ALIAS_SEGMENTS {
        return None;
    }
    Some(AliasSegment {
        index: index - 1,
        total,
        bytes: std::array::from_fn(|i| bits_to_u32(layer3, 40 + i * 8, 8) as u8),
    })
}

fn apply_call(frame: &mut DvFrame, layer3: &[bool]) {
    let message_type = bits_to_u32(layer3, 2, 6);
    frame.kind = match message_type {
        L3_VOICE_CALL => frame.kind,
        L3_TX_RELEASE => DvFrameKind::Terminator,
        _ => return,
    };
    frame.group_call = Some(!layer3[16]);
    frame.source = Some(bits_to_u32(layer3, 24, 16));
    frame.destination = Some(bits_to_u32(layer3, 40, 16));
    frame.opcode = Some(
        match message_type {
            L3_VOICE_CALL => "voice call",
            L3_TX_RELEASE => "transmit release",
            _ => unreachable!(),
        }
        .to_owned(),
    );
}

fn on_cadence(delta: u64) -> bool {
    let rem = delta % FRAME_SYMBOLS;
    delta > 0 && rem.min(FRAME_SYMBOLS - rem) <= 1
}

fn channel_name(rf_channel: u32) -> &'static str {
    match rf_channel {
        0 => "control channel",
        1 => "traffic channel",
        2 => "direct channel",
        _ => "traffic channel (composite control)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        dv::{
            testutil::{assert_tone_audio, decode, decode_with_audio},
            vocoder::testutil::half_rate_frames,
        },
        synth::dv::nxdn as tx,
        testutil::settings,
    };

    fn channel(bandwidth: NxdnBandwidth) -> NxdnChannel {
        NxdnChannel::new(
            ChannelCtx {
                input_rate: INPUT_RATE_HZ,
            },
            settings(ChannelParams::Nxdn(NxdnParams { bandwidth })),
        )
        .expect("nxdn channel")
    }

    #[test]
    fn decodes_the_link_information_channel_of_a_traffic_channel() {
        let iq = tx::transmission(&tx::Shape::default(), 1, true, INPUT_RATE_HZ);
        let frames = decode(&mut channel(NxdnBandwidth::Narrow), &iq);
        let first = frames.first().expect("a decoded frame");
        assert_eq!(first.mode, DvMode::Nxdn);
        assert_eq!(first.kind, DvFrameKind::Header);
        assert_eq!(
            first.opcode.as_deref(),
            Some("traffic channel outbound"),
            "{frames:?}"
        );
    }

    #[test]
    fn decodes_the_twelve_and_a_half_kilohertz_variant() {
        let shape = tx::Shape {
            baud: 4_800.0,
            deviation_hz: 1_944.0,
        };
        let iq = tx::transmission(&shape, 0, false, INPUT_RATE_HZ);
        let frames = decode(&mut channel(NxdnBandwidth::Wide), &iq);
        let control = frames
            .iter()
            .find(|f| f.kind == DvFrameKind::Control)
            .expect("control channel frame");
        assert_eq!(control.opcode.as_deref(), Some("control channel inbound"));
    }

    #[test]
    fn decodes_ehr_voice_to_audio() {
        let encoded = half_rate_frames(20);
        let voice: [[[bool; 72]; 4]; 5] =
            std::array::from_fn(|frame| std::array::from_fn(|slot| encoded[frame * 4 + slot]));
        let iq = tx::transmission_with_voice(&tx::Shape::default(), 1, true, &voice, INPUT_RATE_HZ);
        let (_, audio) = decode_with_audio(&mut channel(NxdnBandwidth::Narrow), &iq);
        assert_tone_audio(&audio, 16);
    }

    #[test]
    fn the_last_voice_frame_of_an_over_still_reaches_the_speaker() {
        let encoded = half_rate_frames(20);
        let voice: [[[bool; 72]; 4]; 5] =
            std::array::from_fn(|frame| std::array::from_fn(|slot| encoded[frame * 4 + slot]));
        let mut iq =
            tx::transmission_with_voice(&tx::Shape::default(), 1, true, &voice, INPUT_RATE_HZ);
        iq.extend(crate::testutil::complex_noise(0x4E58, 0.01, 48_000));

        let (_, audio) = decode_with_audio(&mut channel(NxdnBandwidth::Narrow), &iq);

        assert_tone_audio(&audio, 20);
    }

    #[test]
    fn a_sync_lost_mid_transmission_does_not_delete_the_frame_before_it() {
        const PREAMBLE_SYMBOLS: usize = 200;
        const JITTER_SYMBOLS: usize = 4;
        let encoded = half_rate_frames(20);
        let voice: [[[bool; 72]; 4]; 5] =
            std::array::from_fn(|frame| std::array::from_fn(|slot| encoded[frame * 4 + slot]));
        let clean =
            tx::transmission_with_voice(&tx::Shape::default(), 1, true, &voice, INPUT_RATE_HZ);
        let samples_per_symbol = (INPUT_RATE_HZ / 2_400.0) as usize;
        let last_sync = (PREAMBLE_SYMBOLS + 4 * FRAME_SYMBOLS as usize) * samples_per_symbol;
        let mut jittered = clean[..last_sync].to_vec();
        jittered.extend(crate::testutil::complex_noise(
            0x9A17,
            0.01,
            samples_per_symbol * JITTER_SYMBOLS,
        ));
        jittered.extend_from_slice(&clean[last_sync..]);
        jittered.extend(crate::testutil::complex_noise(0x4E58, 0.01, 48_000));

        let (_, audio) = decode_with_audio(&mut channel(NxdnBandwidth::Narrow), &jittered);

        assert_tone_audio(&audio, 16);
    }

    #[test]
    fn facch_reports_the_call_addresses_and_ran() {
        let iq =
            tx::addressed_transmission(&tx::Shape::default(), 17, 12_345, 234, true, INPUT_RATE_HZ);
        let frames = decode(&mut channel(NxdnBandwidth::Narrow), &iq);
        let header = frames
            .iter()
            .find(|frame| frame.kind == DvFrameKind::Header && frame.source.is_some())
            .expect("FACCH voice-call header");
        assert_eq!(header.color_code, Some(17));
        assert_eq!(header.source, Some(12_345));
        assert_eq!(header.destination, Some(234));
        assert_eq!(header.group_call, Some(true));
        assert_eq!(header.opcode.as_deref(), Some("voice call"));

        let release = frames
            .iter()
            .find(|frame| frame.kind == DvFrameKind::Terminator)
            .expect("FACCH transmit release");
        assert_eq!(
            (release.source, release.destination),
            (Some(12_345), Some(234))
        );
        assert_eq!(release.opcode.as_deref(), Some("transmit release"));
    }

    #[test]
    fn sacch_superframe_supports_late_entry_addressing() {
        let iq =
            tx::addressed_transmission(&tx::Shape::default(), 9, 65_000, 42, false, INPUT_RATE_HZ);
        let frames = decode(&mut channel(NxdnBandwidth::Narrow), &iq);
        let late_entry = frames
            .iter()
            .find(|frame| frame.kind == DvFrameKind::Voice && frame.source == Some(65_000))
            .expect("four assembled SACCH quarters");
        assert_eq!(late_entry.color_code, Some(9));
        assert_eq!(late_entry.destination, Some(42));
        assert_eq!(late_entry.group_call, Some(false));
    }

    #[test]
    fn decodes_the_committed_addressed_iq_fixture() {
        const FIXTURE: &[u8] = include_bytes!("../../../../fixtures/nxdn_addressed_48k.sigmf-data");
        let iq: Vec<Complex<f32>> = FIXTURE
            .as_chunks::<8>()
            .0
            .iter()
            .map(|sample| {
                Complex::new(
                    f32::from_le_bytes(sample[..4].try_into().expect("I bytes")),
                    f32::from_le_bytes(sample[4..].try_into().expect("Q bytes")),
                )
            })
            .collect();
        let frames = decode(&mut channel(NxdnBandwidth::Narrow), &iq);
        let addressed = frames
            .iter()
            .find(|frame| frame.source == Some(12_345))
            .expect("fixture FACCH/SACCH addressing");
        assert_eq!(addressed.color_code, Some(17));
        assert_eq!(addressed.destination, Some(234));
        assert_eq!(addressed.group_call, Some(true));
    }

    #[test]
    fn lich_parity_covers_only_the_channel_type_bits() {
        let octet_bits =
            |octet: u8| -> Vec<bool> { (0..8).rev().map(|i| octet >> i & 1 == 1).collect() };
        for octet in [0x02, 0x41, 0xA0, 0xAC, 0xAE, 0x6C] {
            assert!(lich_parity_holds(&octet_bits(octet)), "{octet:#04x}");
            assert!(!lich_parity_holds(&octet_bits(octet ^ 1)), "{octet:#04x}");
        }
    }

    fn spec_symbol(dibit: usize) -> f32 {
        const SPEC_SYMBOL_FOR_DIBIT: [f32; 4] = [1.0, 3.0, -1.0, -3.0];
        SPEC_SYMBOL_FOR_DIBIT[dibit]
    }

    fn message_bits(value: u32, width: usize, out: &mut Vec<bool>) {
        out.extend((0..width).rev().map(|shift| value >> shift & 1 == 1));
    }

    fn convolutional(message: &[bool]) -> Vec<bool> {
        const SPEC_G1: [usize; 3] = [0, 3, 4];
        const SPEC_G2: [usize; 4] = [0, 1, 2, 4];
        let tapped = |step: usize, taps: &[usize]| {
            taps.iter().fold(false, |acc, &delay| {
                acc ^ (step >= delay && message[step - delay])
            })
        };
        (0..message.len())
            .flat_map(|step| [tapped(step, &SPEC_G1), tapped(step, &SPEC_G2)])
            .collect()
    }

    fn erased(first_row: &[u8], second_row: &[u8], coded_bits: usize) -> Vec<usize> {
        let period = first_row.len() * 2;
        (0..coded_bits)
            .filter(|index| {
                let slot = index % period;
                let row = if slot.is_multiple_of(2) {
                    first_row
                } else {
                    second_row
                };
                row[slot / 2] == 0
            })
            .collect()
    }

    fn punctured(coded: &[bool], first_row: &[u8], second_row: &[u8]) -> Vec<bool> {
        let period = first_row.len() * 2;
        coded
            .iter()
            .enumerate()
            .filter(|(index, _)| {
                let slot = index % period;
                let row = if slot.is_multiple_of(2) {
                    first_row
                } else {
                    second_row
                };
                row[slot / 2] == 1
            })
            .map(|(_, &bit)| bit)
            .collect()
    }

    fn interleaved(payload: &[bool], depth: usize) -> Vec<i16> {
        let columns = payload.len() / depth;
        (0..payload.len())
            .map(|slot| payload[(slot % depth) * columns + slot / depth])
            .map(|bit| if bit { CONFIDENT } else { -CONFIDENT })
            .collect()
    }

    #[test]
    fn the_frame_sync_word_matches_the_nxdn_ts_1_a_symbol_table() {
        const SPEC_SYMBOLS: [f32; FSW_SYMBOLS] =
            [-3.0, 1.0, -3.0, 3.0, -3.0, -3.0, 3.0, 3.0, -1.0, 3.0];
        let symbols: [f32; FSW_SYMBOLS] = std::array::from_fn(|i| {
            spec_symbol((FSW >> (FSW_BITS as usize - 2 - i * 2)) as usize & 0b11)
        });
        assert_eq!(symbols, SPEC_SYMBOLS);
    }

    #[test]
    fn the_check_polynomials_match_their_published_check_values() {
        let bits: Vec<bool> = b"123456789"
            .iter()
            .flat_map(|byte| (0..8).rev().map(move |shift| byte >> shift & 1 == 1))
            .collect();
        assert_eq!(crc_msb(0x27, 0x3F, &bits), 0x0D);
        assert_eq!(crc_msb(0x80F, 0, &bits).reverse_bits() >> 20, 0xDAF);
    }

    #[test]
    fn the_lich_conformance_vectors_of_nxdn_ts_2_b_name_their_channels() {
        const SPEC_LICH: [(u16, &str); 23] = [
            (0x5D77, "control channel outbound"),
            (0x5F7D, "control channel inbound"),
            (0x577F, "control channel inbound"),
            (0xD5DF, "direct channel inbound"),
            (0x75DF, "traffic channel inbound"),
            (0xD5D7, "direct channel outbound"),
            (0x75D7, "traffic channel outbound"),
            (0xDD7D, "direct channel inbound"),
            (0x7D7D, "traffic channel inbound"),
            (0xDD75, "direct channel outbound"),
            (0x7D75, "traffic channel outbound"),
            (0xD57F, "direct channel inbound"),
            (0x757F, "traffic channel inbound"),
            (0xD577, "direct channel outbound"),
            (0x7577, "traffic channel outbound"),
            (0xDF7F, "direct channel inbound"),
            (0x7F7F, "traffic channel inbound"),
            (0xDF77, "direct channel outbound"),
            (0x7F77, "traffic channel outbound"),
            (0xDFDF, "direct channel inbound"),
            (0x7FDF, "traffic channel inbound"),
            (0xDFD7, "direct channel outbound"),
            (0x7FD7, "traffic channel outbound"),
        ];
        for (scrambled, expected) in SPEC_LICH {
            let mut decoder = Decoder::new();
            for position in 0..POST_FSW_SYMBOLS {
                let dibit = if position < LICH_SYMBOLS {
                    (scrambled >> (14 - position * 2)) as usize & 0b11
                } else {
                    0
                };
                decoder.window.push(spec_symbol(dibit));
            }
            let (frame, _) = decoder.frame().expect("conformant LICH");
            assert_eq!(frame.opcode.as_deref(), Some(expected), "{scrambled:#06X}");
        }
    }

    #[test]
    fn a_sacch_built_to_the_nxdn_ts_1_a_coding_rules_decodes() {
        const SPEC_FIRST_ROW: [u8; 6] = [1, 1, 1, 1, 1, 1];
        const SPEC_SECOND_ROW: [u8; 6] = [1, 1, 0, 1, 1, 0];
        const SPEC_DEPTH: usize = 5;
        const STRUCTURE: u32 = 0b01;
        const RAN: u32 = 0b10_1010;
        let data: [bool; 18] = std::array::from_fn(|i| i.is_multiple_of(3));

        let mut message = Vec::new();
        message_bits(STRUCTURE, 2, &mut message);
        message_bits(RAN, 6, &mut message);
        message.extend(data);
        let check = crc_msb(0x27, 0x3F, &message);
        message_bits(check, 6, &mut message);
        message.extend([false; 4]);

        assert_eq!(
            erased(&SPEC_FIRST_ROW, &SPEC_SECOND_ROW, 72),
            SACCH_PUNCTURES
        );
        let coded = punctured(&convolutional(&message), &SPEC_FIRST_ROW, &SPEC_SECOND_ROW);
        assert_eq!(coded.len(), SACCH_BITS);
        let channel = interleaved(&coded, SPEC_DEPTH);

        let sacch = decode_sacch(&channel, &mut Viterbi5::new()).expect("conformant SACCH");
        assert_eq!(u32::from(sacch.structure), STRUCTURE);
        assert_eq!(u32::from(sacch.ran), RAN);
        assert_eq!(sacch.data, data);

        let mut wrong_check = message.clone();
        wrong_check[26] = !wrong_check[26];
        let broken = interleaved(
            &punctured(
                &convolutional(&wrong_check),
                &SPEC_FIRST_ROW,
                &SPEC_SECOND_ROW,
            ),
            SPEC_DEPTH,
        );
        assert!(decode_sacch(&broken, &mut Viterbi5::new()).is_none());
    }

    #[test]
    fn a_facch1_built_to_the_nxdn_ts_1_a_coding_rules_decodes() {
        const SPEC_FIRST_ROW: [u8; 2] = [1, 1];
        const SPEC_SECOND_ROW: [u8; 2] = [0, 1];
        const SPEC_DEPTH: usize = 9;
        let layer3: [bool; FACCH_INFO_BITS] = std::array::from_fn(|i| i % 5 < 2);

        let mut message = layer3.to_vec();
        let check = crc_msb(0x080F, 0x0FFF, &message);
        message_bits(check, 12, &mut message);
        message.extend([false; 4]);

        assert_eq!(
            erased(&SPEC_FIRST_ROW, &SPEC_SECOND_ROW, 192),
            FACCH_PUNCTURES
        );
        let coded = punctured(&convolutional(&message), &SPEC_FIRST_ROW, &SPEC_SECOND_ROW);
        assert_eq!(coded.len(), FACCH_BITS);
        let channel = interleaved(&coded, SPEC_DEPTH);

        assert_eq!(
            decode_facch(&channel, &mut Viterbi5::new()),
            Some(layer3),
            "conformant FACCH1"
        );
    }

    #[test]
    fn decodes_inbound_frames() {
        let iq = tx::transmission(&tx::Shape::default(), 1, false, INPUT_RATE_HZ);
        let frames = decode(&mut channel(NxdnBandwidth::Narrow), &iq);
        assert_eq!(
            frames.first().and_then(|f| f.opcode.as_deref()),
            Some("traffic channel inbound"),
            "{frames:?}"
        );
    }

    #[test]
    fn decodes_inverted_polarity() {
        let iq: Vec<Complex<f32>> =
            tx::addressed_transmission(&tx::Shape::default(), 1, 901, 0, true, INPUT_RATE_HZ)
                .iter()
                .map(Complex::conj)
                .collect();
        let frames = decode(&mut channel(NxdnBandwidth::Narrow), &iq);
        let call = frames
            .iter()
            .find(|frame| frame.source == Some(901))
            .expect("inverted call addressing");
        assert_eq!(call.color_code, Some(1));
        assert_eq!(call.destination, Some(0));
    }

    #[test]
    fn assembles_the_kenwood_talker_alias() {
        let iq = tx::aliased_transmission(&tx::Shape::default(), 1, "REPEATER", INPUT_RATE_HZ);
        let frames = decode(&mut channel(NxdnBandwidth::Narrow), &iq);
        assert!(
            frames
                .iter()
                .any(|frame| frame.talker_alias.as_deref() == Some("REPEATER")),
            "{frames:?}"
        );
    }

    #[test]
    fn noise_decodes_to_nothing() {
        let noise = crate::testutil::complex_noise(57, 0.5, 400_000);
        assert!(decode(&mut channel(NxdnBandwidth::Narrow), &noise).is_empty());
    }
}
