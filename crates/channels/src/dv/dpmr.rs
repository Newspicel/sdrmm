use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_dsp::ParityCode;
use sdrmm_modem::cpm::CpmDemod;
use sdrmm_wire::{
    CallGate, ChannelDescriptor, ChannelParams, ChannelSettings, DecoderEvent, DecoderFamily,
    DpmrParams, DvFrame, DvFrameKind, DvMode, EventKind,
};

use super::{
    INPUT_RATE_HZ, SymbolWindow, bits_to_u32, c4fm_demod, c4fm_params, invert_dibits, inverted,
    tap_c4fm,
    vocoder::{AMBE_3600_INTERLEAVE, MbeDecoder, half_rate_code_vectors},
};
use crate::{ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, check_input_rate};

pub(crate) const BAUD: f64 = 2_400.0;
pub(crate) const DEVIATION_HZ: f64 = 1_050.0;
pub(crate) const RRC_ALPHA: f64 = 0.2;
pub(crate) const BANDWIDTH_HZ: f64 = 6_250.0;

pub(crate) const FS1: u64 = 0x57FF_5F75_D577;
pub(crate) const FS4: u64 = 0xFD55_F5DF_7FDD;
const FS3: u64 = 0x7D_DFF5;
const FS2: u64 = 0x5F_F77D;
pub(crate) const LONG_SYNC_BITS: u32 = 48;
const SHORT_SYNC_BITS: u32 = 24;
pub(crate) const LONG_TOLERANCE: u32 = 4;
const SHORT_TOLERANCE: u32 = 2;

const CHANNEL_CODE_ZERO: u32 = 0x57_5F77;
const CHANNEL_CODE_ROWS: [u32; 6] = [
    0x57_7577, 0x57_DD75, 0x55_577D, 0x5F_555F, 0x77_5DD7, 0xD7_55F7,
];
const CHANNEL_CODE_TOLERANCE: u32 = 1;
const HAMMING_WORD_BITS: usize = 12;

const HI_BITS: usize = 72;
const HI_CODED_BITS: usize = 120;
const HI_BLOCKS: usize = 10;
const HI_SYMBOLS: usize = HI_CODED_BITS / 2;
const CC_SYMBOLS: usize = 12;
const HEADER_SYMBOLS: usize = HI_SYMBOLS * 2 + CC_SYMBOLS;
const SUPERFRAME_SYMBOLS: usize = 756;
const TCH_STARTS: [usize; 4] = [36, 228, 420, 612];
const TCH_SYMBOLS: usize = 144;
const AMBE_SYMBOLS: usize = 36;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "dpmr".to_owned(),
    name: "dPMR".to_owned(),
    summary: "dPMR two-way radio voice".to_owned(),
    family: DecoderFamily::DigitalVoice,
    bandwidth_hz: BANDWIDTH_HZ,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: true,
    emits: vec![EventKind::Dv],
    calls: Some(CallGate::Frames),
    ..ChannelDescriptor::default()
});

pub struct DpmrChannel {
    demod: CpmDemod,
    symbols: Vec<f32>,
    decoder: Decoder,
}

fn params(settings: &ChannelSettings) -> Result<&DpmrParams, ChannelError> {
    match &settings.params {
        ChannelParams::Dpmr(p) => Ok(p),
        other => Err(ChannelError::InvalidSettings(format!(
            "dpmr channel got {} params",
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

impl ChannelRx for DpmrChannel {
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
    pending: Option<Pending>,
    bits: Vec<bool>,
    in_call: bool,
    voice_call: bool,
    inverted: bool,
    vocoder: MbeDecoder,
    rejected: u32,
}

#[derive(Clone, Copy)]
enum Pending {
    Header { fs4: bool },
    Superframe,
}

impl Decoder {
    fn new() -> Self {
        Self {
            window: SymbolWindow::new(SUPERFRAME_SYMBOLS),
            countdown: 0,
            pending: None,
            bits: Vec::with_capacity(SUPERFRAME_SYMBOLS * 2),
            in_call: false,
            voice_call: false,
            inverted: false,
            vocoder: MbeDecoder::half_rate(),
            rejected: 0,
        }
    }

    fn rejected(&self) -> u32 {
        self.rejected
    }

    fn reset(&mut self) {
        self.window.reset();
        self.countdown = 0;
        self.pending = None;
        self.in_call = false;
        self.voice_call = false;
        self.inverted = false;
        self.vocoder.reset();
    }

    fn push(&mut self, symbol: f32, out: &mut ChannelOutputs) {
        self.window.push(symbol);
        if self.countdown > 0 {
            self.countdown -= 1;
            if self.countdown == 0 {
                match self.pending.take() {
                    Some(Pending::Header { fs4 }) => match self.header(fs4) {
                        Some((frame, voice_call)) => {
                            self.in_call = true;
                            self.voice_call = voice_call;
                            out.events.push(DecoderEvent::Dv(frame));
                        }
                        None => self.rejected = self.rejected.saturating_add(1),
                    },
                    Some(Pending::Superframe) => self.superframe(out),
                    None => {}
                }
            }
            return;
        }
        if self.pending.is_some() {
            return;
        }
        for (sync, fs4) in [(FS1, false), (FS4, true)] {
            if self.window.sync_distance(sync, LONG_SYNC_BITS) <= LONG_TOLERANCE {
                self.window.anchor(sync, LONG_SYNC_BITS);
                self.pending = Some(Pending::Header { fs4 });
                self.countdown = HEADER_SYMBOLS;
                return;
            }
        }
        if !self.in_call {
            return;
        }
        let fs2 = self.observed(FS2);
        if self.window.sync_distance(fs2, SHORT_SYNC_BITS) <= SHORT_TOLERANCE {
            self.window.anchor(fs2, SHORT_SYNC_BITS);
            self.pending = Some(Pending::Superframe);
            self.countdown = SUPERFRAME_SYMBOLS;
            return;
        }
        let fs3 = self.observed(FS3);
        if self.window.sync_distance(fs3, SHORT_SYNC_BITS) <= SHORT_TOLERANCE {
            self.window.anchor(fs3, SHORT_SYNC_BITS);
            self.in_call = false;
            self.voice_call = false;
            out.events.push(DecoderEvent::Dv(DvFrame::new(
                DvMode::Dpmr,
                DvFrameKind::Terminator,
            )));
        }
    }

    fn observed(&self, sync: u64) -> u64 {
        if self.inverted {
            inverted(sync, SHORT_SYNC_BITS)
        } else {
            sync
        }
    }

    fn header(&mut self, fs4: bool) -> Option<(DvFrame, bool)> {
        self.window.bits(0, HEADER_SYMBOLS, &mut self.bits);
        for inverted in [false, true] {
            if inverted {
                invert_dibits(&mut self.bits);
            }
            let hi = header_info(&self.bits[..HI_CODED_BITS])
                .or_else(|| header_info(&self.bits[HI_CODED_BITS + CC_SYMBOLS * 2..]));
            if let Some(hi) = hi {
                self.inverted = inverted;
                return Some(self.header_frame(&hi, fs4 != inverted));
            }
        }
        None
    }

    fn header_frame(&self, hi: &[bool], packet: bool) -> (DvFrame, bool) {
        let mut frame = DvFrame::new(
            DvMode::Dpmr,
            if packet {
                DvFrameKind::Data
            } else {
                DvFrameKind::Header
            },
        );
        frame.color_code =
            channel_code_number(bits_to_u32(&self.bits, HI_CODED_BITS, CC_SYMBOLS * 2));
        frame.destination = Some(bits_to_u32(hi, 4, 24));
        frame.source = Some(bits_to_u32(hi, 28, 24));
        let mode = bits_to_u32(hi, 52, 3);
        (frame, matches!(mode, 0 | 1 | 5))
    }

    fn superframe(&mut self, out: &mut ChannelOutputs) {
        if !self.voice_call {
            return;
        }
        self.window.bits(0, SUPERFRAME_SYMBOLS, &mut self.bits);
        if self.inverted {
            invert_dibits(&mut self.bits);
        }
        for start in TCH_STARTS {
            for frame_start in (start..start + TCH_SYMBOLS).step_by(AMBE_SYMBOLS) {
                let mut frame = [false; 72];
                frame.copy_from_slice(&self.bits[frame_start * 2..frame_start * 2 + 72]);
                self.vocoder.decode_half_code_vectors(
                    half_rate_code_vectors(&frame, &AMBE_3600_INTERLEAVE),
                    false,
                    out,
                );
            }
        }
    }
}

fn channel_code(number: u8) -> u32 {
    CHANNEL_CODE_ROWS
        .iter()
        .enumerate()
        .filter(|(bit, _)| number >> bit & 1 == 1)
        .fold(CHANNEL_CODE_ZERO, |code, (_, &row)| {
            code ^ row ^ CHANNEL_CODE_ZERO
        })
}

fn channel_code_number(received: u32) -> Option<u16> {
    (0..64u8)
        .find(|&number| (channel_code(number) ^ received).count_ones() <= CHANNEL_CODE_TOLERANCE)
        .map(u16::from)
}

fn scramble(bits: &mut [bool]) {
    let mut register = 0x1FFu16;
    for bit in bits {
        *bit ^= register & 1 == 1;
        let feedback = (register ^ register >> 4) & 1;
        register = register >> 1 | feedback << 8;
    }
}

fn header_info(coded: &[bool]) -> Option<Vec<bool>> {
    let mut descrambled = [false; HI_CODED_BITS];
    descrambled.copy_from_slice(coded);
    scramble(&mut descrambled);
    let mut blocks = [false; HI_CODED_BITS];
    for r in 0..HAMMING_WORD_BITS {
        for c in 0..HI_BLOCKS {
            blocks[c * HAMMING_WORD_BITS + r] = descrambled[r * HI_BLOCKS + c];
        }
    }
    let mut bytes = [0u8; HI_BLOCKS];
    let mut info = Vec::with_capacity(HI_BITS);
    for (block, byte) in bytes.iter_mut().enumerate() {
        let word = &mut blocks[block * HAMMING_WORD_BITS..(block + 1) * HAMMING_WORD_BITS];
        ParityCode::HAMMING_12_8.decode(word)?;
        for (bit, &value) in word[..8].iter().enumerate() {
            *byte = *byte << 1 | u8::from(value);
            if block * 8 + bit < HI_BITS {
                info.push(value);
            }
        }
    }
    (crc8(&bytes[..9]) == bytes[9]).then_some(info)
}

fn crc8(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &byte in data {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                crc << 1 ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        dv::{
            testutil::{assert_tone_audio, decode, decode_with_audio},
            vocoder::testutil::half_rate_frames,
        },
        synth::dv::dpmr as tx,
        testutil::settings,
    };

    fn channel() -> DpmrChannel {
        DpmrChannel::new(
            ChannelCtx {
                input_rate: INPUT_RATE_HZ,
            },
            settings(ChannelParams::Dpmr(DpmrParams::default())),
        )
        .expect("dpmr channel")
    }

    #[test]
    fn a_header_that_cannot_be_read_is_reported_as_rejected() {
        let iq = tx::transmission_with_unreadable_header(&tx::Call::default(), INPUT_RATE_HZ);
        let mut chan = channel();

        let frames = decode(&mut chan, &iq);

        assert!(
            frames.iter().all(|frame| frame.kind != DvFrameKind::Header),
            "an unreadable header was reported: {frames:?}"
        );
        assert!(
            chan.rejected() > 0,
            "an unreadable header was dropped in silence"
        );
    }

    #[test]
    fn decodes_a_header_and_its_end_frame() {
        let call = tx::Call::default();
        let iq = tx::transmission(&call, INPUT_RATE_HZ);
        let frames = decode(&mut channel(), &iq);

        let header = frames.first().expect("a decoded frame");
        assert_eq!(header.mode, DvMode::Dpmr);
        assert_eq!(header.kind, DvFrameKind::Header);
        assert_eq!(header.color_code, Some(u16::from(call.channel_code)));
        assert_eq!(header.destination, Some(call.called));
        assert_eq!(header.source, Some(call.own));
        assert!(
            frames.iter().any(|f| f.kind == DvFrameKind::Terminator),
            "no end frame: {frames:?}"
        );
    }

    #[test]
    fn decodes_a_voice_only_call() {
        let call = tx::Call {
            mode: 0,
            called: 0x00_00FF,
            ..tx::Call::default()
        };
        let iq = tx::transmission(&call, INPUT_RATE_HZ);
        let frames = decode(&mut channel(), &iq);
        let header = frames.first().expect("a decoded frame");
        assert_eq!(header.group_call, None);
        assert_eq!(header.destination, Some(call.called));
    }

    #[test]
    fn decodes_inverted_polarity() {
        let call = tx::Call::default();
        let iq: Vec<Complex<f32>> = tx::transmission(&call, INPUT_RATE_HZ)
            .iter()
            .map(Complex::conj)
            .collect();
        let frames = decode(&mut channel(), &iq);
        let header = frames.first().expect("a decoded frame");
        assert_eq!(header.kind, DvFrameKind::Header);
        assert_eq!(header.source, Some(call.own));
        assert!(
            frames.iter().any(|f| f.kind == DvFrameKind::Terminator),
            "{frames:?}"
        );
    }

    const SPEC_CHANNEL_CODES: [u32; 63] = [
        0x57_7577, 0x57_DD75, 0x57_F775, 0x55_577D, 0x55_7D7D, 0x55_D57F, 0x55_FF7F, 0x5F_555F,
        0x5F_7F5F, 0x5F_D75D, 0x5F_FD5D, 0x5D_5D55, 0x5D_7755, 0x5D_DF57, 0x5D_F557, 0x77_5DD7,
        0x77_77D7, 0x77_DFD5, 0x77_F5D5, 0x75_55DD, 0x75_7FDD, 0x75_D7DF, 0x75_FDDF, 0x7F_57FF,
        0x7F_7DFF, 0x7F_D5FD, 0x7F_FFFD, 0x7D_5FF5, 0x7D_75F5, 0x7D_DDF7, 0x7D_F7F7, 0xD7_55F7,
        0xD7_7FF7, 0xD7_D7F5, 0xD7_FDF5, 0xD5_5DFD, 0xD5_77FD, 0xD5_DFFF, 0xD5_F5FF, 0xDF_5FDF,
        0xDF_75DF, 0xDF_DDDD, 0xDF_F7DD, 0xDD_57D5, 0xDD_7DD5, 0xDD_D5D7, 0xDD_FFD7, 0xF7_5757,
        0xF7_7D57, 0xF7_D555, 0xF7_FF55, 0xF5_5F5D, 0xF5_755D, 0xF5_DD5F, 0xF5_F75F, 0xFF_5D7F,
        0xFF_777F, 0xFF_DF7D, 0xFF_F57D, 0xFD_5575, 0xFD_7F75, 0xFD_D777, 0xFD_FD77,
    ];

    #[test]
    fn the_frame_syncs_match_the_etsi_ts_102_490_sequences() {
        assert_eq!(FS1, 0x57FF_5F75_D577);
        assert_eq!(FS2, 0x5F_F77D);
        assert_eq!(FS3, 0x7D_DFF5);
        assert_eq!(FS4, 0xFD55_F5DF_7FDD);
    }

    #[test]
    fn the_channel_codes_match_the_etsi_ts_102_490_table() {
        for (index, &code) in SPEC_CHANNEL_CODES.iter().enumerate() {
            let number = index as u8 + 1;
            assert_eq!(channel_code(number), code, "channel code {number}");
            assert_eq!(channel_code_number(code), Some(u16::from(number)));
            assert_eq!(channel_code_number(code ^ 0x80), Some(u16::from(number)));
        }
        for number in 0..64u8 {
            let code = channel_code(number);
            for pair in 0..12 {
                assert!(
                    matches!(code >> (pair * 2) & 0b11, 0b01 | 0b11),
                    "channel code {number} is not di-bit coded"
                );
            }
        }
    }

    #[test]
    fn the_header_checksum_matches_the_published_crc_8_check_value() {
        assert_eq!(crc8(b"123456789"), 0xF4);
    }

    #[test]
    fn the_scrambler_matches_the_etsi_ts_102_490_polynomial() {
        const SPEC_STAGES: usize = 9;
        const SPEC_TAP: usize = 5;
        const PERIOD: usize = (1 << SPEC_STAGES) - 1;
        let mut expected = [true; PERIOD];
        for step in SPEC_STAGES..PERIOD {
            expected[step] = expected[step - SPEC_STAGES] ^ expected[step - SPEC_TAP];
        }
        let mut sequence = [false; PERIOD];
        scramble(&mut sequence);
        assert_eq!(sequence, expected);
        assert_eq!(expected.iter().filter(|bit| **bit).count(), PERIOD / 2 + 1);
    }

    #[test]
    fn a_header_built_to_the_etsi_ts_102_490_coding_rules_decodes() {
        const SPEC_GENERATOR: [[u8; 4]; 8] = [
            [1, 1, 1, 0],
            [0, 1, 1, 1],
            [1, 0, 1, 0],
            [0, 1, 0, 1],
            [1, 0, 1, 1],
            [1, 1, 0, 0],
            [0, 1, 1, 0],
            [0, 0, 1, 1],
        ];
        const SPEC_HEADER: [u8; HI_BITS / 8] =
            [0x5A, 0x3C, 0x96, 0xE1, 0x0F, 0xF0, 0x12, 0x34, 0xAB];
        let information: [bool; HI_BITS] =
            std::array::from_fn(|i| SPEC_HEADER[i / 8] >> (7 - i % 8) & 1 == 1);
        const SPEC_CRC_TAPS: [u8; 3] = [2, 1, 0];
        let mut remainder = 0u8;
        for &bit in &information {
            let feedback = bit != (remainder & 0x80 != 0);
            remainder <<= 1;
            if feedback {
                remainder = SPEC_CRC_TAPS
                    .iter()
                    .fold(remainder, |value, &tap| value ^ 1 << tap);
            }
        }
        let mut bytes = [0u8; HI_BLOCKS];
        bytes[..9].copy_from_slice(&SPEC_HEADER);
        bytes[9] = remainder;

        let mut blocks = [false; HI_CODED_BITS];
        for (block, &byte) in bytes.iter().enumerate() {
            let data: [bool; 8] = std::array::from_fn(|bit| byte >> (7 - bit) & 1 == 1);
            let word = &mut blocks[block * HAMMING_WORD_BITS..(block + 1) * HAMMING_WORD_BITS];
            word[..8].copy_from_slice(&data);
            for check in 0..4 {
                word[8 + check] = data
                    .iter()
                    .zip(SPEC_GENERATOR)
                    .fold(false, |parity, (&bit, row)| {
                        parity ^ (bit && row[check] == 1)
                    });
            }
        }

        let mut coded = [false; HI_CODED_BITS];
        for row in 0..HAMMING_WORD_BITS {
            for column in 0..HI_BLOCKS {
                coded[row * HI_BLOCKS + column] = blocks[column * HAMMING_WORD_BITS + row];
            }
        }
        scramble(&mut coded);

        assert_eq!(header_info(&coded).as_deref(), Some(&information[..]));
        let mut single_error = coded;
        single_error[37] = !single_error[37];
        assert_eq!(
            header_info(&single_error).as_deref(),
            Some(&information[..])
        );
        let mut wrong_checksum = coded;
        wrong_checksum[9] = !wrong_checksum[9];
        wrong_checksum[19] = !wrong_checksum[19];
        wrong_checksum[29] = !wrong_checksum[29];
        assert_eq!(header_info(&wrong_checksum), None);
    }

    #[test]
    fn decodes_superframe_voice_to_audio() {
        let voice = half_rate_frames(32);
        let iq = tx::transmission_with_voice(&tx::Call::default(), &voice, INPUT_RATE_HZ);
        let (_, audio) = decode_with_audio(&mut channel(), &iq);
        assert_tone_audio(&audio, 32);
    }

    #[test]
    fn noise_decodes_to_nothing() {
        let noise = crate::testutil::complex_noise(69, 0.5, 400_000);
        assert!(decode(&mut channel(), &noise).is_empty());
    }
}
