use blip25_vocoder::{
    enhancement::{ClassicalConfig, EnhancementMode},
    fullrate,
    halfrate::{
        self,
        frame::{decode_code_vectors, decode_frame_soft},
    },
    vocoder::{FrameStatus, Rate, Vocoder},
};
use codec2::{Codec2, Codec2Mode};
use num_complex::Complex;
use sdrmm_dsp::FracResampler;
use sdrmm_wire::{
    ChannelStatus,
    present::{Field, FieldValue},
};

use super::{
    ambe::{AmbeDecoder, FRAME_SAMPLES as AMBE_FRAME_SAMPLES},
    chip::{ChipFrame, ChipHealth, ChipLink, ChipRate, MAX_CHIP_FRAME_BYTES},
};
use crate::{AUDIO_RATE, ChannelOutputs, clamp_full_scale};

const VOCODER_RATE_HZ: f64 = 8_000.0;
const OUTPUT_SAMPLES_PER_MBE_FRAME: usize = 960;

pub(crate) const HALF_RATE_SOFT_BITS: usize = halfrate::frame::SOFT_BITS;

pub(crate) type HalfRateFrame = halfrate::frame::Frame;

pub(crate) fn half_soft_frame(soft: &[i8; HALF_RATE_SOFT_BITS]) -> HalfRateFrame {
    decode_frame_soft(soft)
}

pub(crate) fn half_frame(code: [u32; 4]) -> HalfRateFrame {
    decode_code_vectors(code)
}

pub(crate) type FullRateFrame = fullrate::frame::Frame;

/// The 88 IMBE information bits, u0 to u7 most significant first, as the ciphers see them.
pub(crate) const FULL_RATE_INFO_BYTES: usize = 11;
const FULL_RATE_WIDTHS: [u32; 8] = [12, 12, 12, 12, 11, 11, 11, 7];

pub(crate) fn full_frame(dibits: &[u8; 72]) -> FullRateFrame {
    fullrate::frame::decode_frame(dibits)
}

pub(crate) fn pack_full(info: &[u16; 8]) -> [u8; FULL_RATE_INFO_BYTES] {
    let mut bits = 0u128;
    for (&vector, width) in info.iter().zip(FULL_RATE_WIDTHS) {
        bits = bits << width | u128::from(vector) & ((1 << width) - 1);
    }
    let bytes = (bits << 40).to_be_bytes();
    std::array::from_fn(|i| bytes[i])
}

pub(crate) fn unpack_full(bytes: &[u8; FULL_RATE_INFO_BYTES]) -> [u16; 8] {
    let mut wide = [0u8; 16];
    wide[..FULL_RATE_INFO_BYTES].copy_from_slice(bytes);
    let mut bits = u128::from_be_bytes(wide) >> 40;
    let mut info = [0u16; 8];
    for (vector, width) in info.iter_mut().zip(FULL_RATE_WIDTHS).rev() {
        *vector = (bits & ((1 << width) - 1)) as u16;
        bits >>= width;
    }
    info
}

const PRESENTATION_GAIN_DB: f32 = 9.0;
/// [`PRESENTATION_GAIN_DB`] as a factor, so a chip plays as loud as blip25.
const PRESENTATION_GAIN: f32 = 2.818_383;
/// IMBE frame repeat per TIA-102.BABA-A: a damaged first vector and a damaged frame.
const IMBE_REPEAT_FIRST_ERRORS: u8 = 2;
const IMBE_REPEAT_TOTAL_ERRORS: u16 = 10;

pub(crate) const AMBE_3600_INTERLEAVE: [[(u8, u8); 2]; 36] = [
    [(0, 23), (0, 5)],
    [(1, 10), (2, 3)],
    [(0, 22), (0, 4)],
    [(1, 9), (2, 2)],
    [(0, 21), (0, 3)],
    [(1, 8), (2, 1)],
    [(0, 20), (0, 2)],
    [(1, 7), (2, 0)],
    [(0, 19), (0, 1)],
    [(1, 6), (3, 13)],
    [(0, 18), (0, 0)],
    [(1, 5), (3, 12)],
    [(0, 17), (1, 22)],
    [(1, 4), (3, 11)],
    [(0, 16), (1, 21)],
    [(1, 3), (3, 10)],
    [(0, 15), (1, 20)],
    [(1, 2), (3, 9)],
    [(0, 14), (1, 19)],
    [(1, 1), (3, 8)],
    [(0, 13), (1, 18)],
    [(1, 0), (3, 7)],
    [(0, 12), (1, 17)],
    [(2, 10), (3, 6)],
    [(0, 11), (1, 16)],
    [(2, 9), (3, 5)],
    [(0, 10), (1, 15)],
    [(2, 8), (3, 4)],
    [(0, 9), (1, 14)],
    [(2, 7), (3, 3)],
    [(0, 8), (1, 13)],
    [(2, 6), (3, 2)],
    [(0, 7), (1, 12)],
    [(2, 5), (3, 1)],
    [(0, 6), (1, 11)],
    [(2, 4), (3, 0)],
];

struct PcmOutput {
    resampler: FracResampler,
    pcm_8k: Vec<Complex<f32>>,
    pcm_48k: Vec<Complex<f32>>,
}

impl PcmOutput {
    fn new() -> Self {
        Self {
            resampler: FracResampler::new(f64::from(AUDIO_RATE) / VOCODER_RATE_HZ),
            pcm_8k: Vec::with_capacity(320),
            pcm_48k: Vec::with_capacity(1_920),
        }
    }

    fn reset(&mut self) {
        self.resampler.reset();
        self.pcm_8k.clear();
        self.pcm_48k.clear();
    }

    fn append_i16(&mut self, pcm: &[i16], out: &mut ChannelOutputs) {
        self.pcm_8k.clear();
        self.pcm_8k.extend(
            pcm.iter()
                .map(|&sample| Complex::new(f32::from(sample) / 32_768.0, 0.0)),
        );
        self.append(out);
    }

    fn append_scaled(&mut self, pcm: &[i16], gain: f32, out: &mut ChannelOutputs) {
        self.pcm_8k.clear();
        self.pcm_8k.extend(
            pcm.iter()
                .map(|&sample| Complex::new(gain * f32::from(sample) / 32_768.0, 0.0)),
        );
        self.append(out);
    }

    fn append_f32(&mut self, pcm: &[f32], out: &mut ChannelOutputs) {
        self.pcm_8k.clear();
        self.pcm_8k.extend(
            pcm.iter()
                .map(|&sample| Complex::new(sample / 32_768.0, 0.0)),
        );
        self.append(out);
    }

    fn append(&mut self, out: &mut ChannelOutputs) {
        self.pcm_48k.clear();
        self.resampler.process(&self.pcm_8k, &mut self.pcm_48k);
        out.audio_pcm
            .extend(self.pcm_48k.iter().map(|sample| sample.re));
        clamp_full_scale(&mut out.audio_pcm);
        if !self.pcm_48k.is_empty() {
            out.audio_rate = AUDIO_RATE;
        }
    }

    fn silence(frames: usize, out: &mut ChannelOutputs) {
        out.audio_pcm.extend(std::iter::repeat_n(
            0.0,
            frames * OUTPUT_SAMPLES_PER_MBE_FRAME,
        ));
        out.audio_rate = AUDIO_RATE;
    }
}

/// A vocoder chip standing in for software synthesis, and what its readout last said.
struct ChipVoice {
    link: ChipLink,
    gain: f32,
    stalled: bool,
    shown: Option<(ChipHealth, bool)>,
}

impl ChipVoice {
    fn new(link: ChipLink, gain: f32) -> Self {
        Self {
            link,
            gain,
            stalled: false,
            shown: None,
        }
    }

    fn send(&mut self, frame: ChipFrame) -> bool {
        self.stalled = !self.link.send(frame);
        !self.stalled
    }

    /// Whether the chip will answer `bits`; when not, the caller plays them itself.
    fn offer(&mut self, bits: [u8; MAX_CHIP_FRAME_BYTES]) -> bool {
        let ready = self.link.health() == ChipHealth::Ready;
        let sent = self.send(ChipFrame::Voice {
            bits,
            played: !ready,
        });
        ready && sent
    }

    fn mute(&mut self, out: &mut ChannelOutputs) {
        if self.link.health() != ChipHealth::Ready || !self.send(ChipFrame::Silence) {
            PcmOutput::silence(1, out);
        }
    }

    fn reset(&mut self) {
        self.send(ChipFrame::Reset);
    }

    fn drain(&mut self, output: &mut PcmOutput, out: &mut ChannelOutputs) {
        while let Some(speech) = self.link.receive() {
            output.append_scaled(&speech, self.gain, out);
        }
        self.report(out);
    }

    fn report(&mut self, out: &mut ChannelOutputs) {
        let health = self.link.health();
        if self.shown == Some((health, self.stalled)) {
            return;
        }
        self.shown = Some((health, self.stalled));
        let text = match (health, self.stalled) {
            (ChipHealth::Lost, _) => "blip25, chip lost".to_owned(),
            (_, true) => "blip25, chip stalled".to_owned(),
            (ChipHealth::Busy, false) => "blip25, chip busy".to_owned(),
            (ChipHealth::Ready, false) => self.link.name().to_owned(),
        };
        out.status = Some(ChannelStatus::readouts(vec![Field::new(
            "vocoder",
            "Vocoder",
            FieldValue::Text(text),
        )]));
    }
}

fn info_rate(rate: Rate) -> Rate {
    match rate {
        Rate::HalfRate3600x2450 | Rate::HalfRate2450x2450 => Rate::HalfRate2450x2450,
        Rate::FullRate7200x4400 | Rate::FullRate4400x4400 => Rate::FullRate4400x4400,
    }
}

fn chip_bits(packed: &[u8]) -> [u8; MAX_CHIP_FRAME_BYTES] {
    let mut bits = [0; MAX_CHIP_FRAME_BYTES];
    let len = packed.len().min(MAX_CHIP_FRAME_BYTES);
    bits[..len].copy_from_slice(&packed[..len]);
    bits
}

/// The information bits of one frame as a chip takes them, MSB first.
fn chip_info(rate: ChipRate, info: &[u16]) -> [u8; MAX_CHIP_FRAME_BYTES] {
    match (rate, info) {
        (ChipRate::Half, &[u0, u1, u2, u3, ..]) => {
            chip_bits(&halfrate::pack_natural(&[u0, u1, u2, u3]))
        }
        (ChipRate::Full, &[u0, u1, u2, u3, u4, u5, u6, u7, ..]) => {
            chip_bits(&pack_full(&[u0, u1, u2, u3, u4, u5, u6, u7]))
        }
        _ => [0; MAX_CHIP_FRAME_BYTES],
    }
}

pub(crate) struct MbeDecoder {
    vocoder: Vocoder,
    output: PcmOutput,
    chip: Option<ChipVoice>,
    erasure: [u8; MAX_CHIP_FRAME_BYTES],
}

impl MbeDecoder {
    pub(crate) fn half_rate() -> Self {
        Self::new(Rate::HalfRate3600x2450)
    }

    pub(crate) fn full_rate() -> Self {
        Self::new(Rate::FullRate7200x4400)
    }

    fn new(rate: Rate) -> Self {
        let mut vocoder = Vocoder::new(rate);
        vocoder.set_enhancement(EnhancementMode::Classical(ClassicalConfig {
            output_gain_db: PRESENTATION_GAIN_DB,
            ..ClassicalConfig::default()
        }));
        Self {
            vocoder,
            output: PcmOutput::new(),
            chip: None,
            erasure: chip_bits(&info_rate(rate).erasure_frame()),
        }
    }

    /// Hands synthesis to `link`, when there is one.
    pub(crate) fn with_chip(mut self, link: Option<ChipLink>) -> Self {
        self.chip = link.map(|link| ChipVoice::new(link, PRESENTATION_GAIN));
        self
    }

    pub(crate) fn reset(&mut self) {
        self.vocoder.reset();
        self.output.reset();
        if let Some(chip) = &mut self.chip {
            chip.reset();
        }
    }

    /// Plays whatever speech the chip has returned since; call once per processed block.
    pub(crate) fn drain(&mut self, out: &mut ChannelOutputs) {
        if let Some(chip) = &mut self.chip {
            chip.drain(&mut self.output, out);
        }
    }

    fn synthesize(
        &mut self,
        info: &[u16],
        status: FrameStatus,
        erased: bool,
        out: &mut ChannelOutputs,
    ) {
        if let Some(chip) = &mut self.chip {
            let bits = if erased {
                self.erasure
            } else {
                chip_info(chip.link.rate(), info)
            };
            if chip.offer(bits) {
                return;
            }
        }
        if let Ok(pcm) = self.vocoder.decode_info(info, status) {
            self.output.append_i16(&pcm, out);
        }
    }

    /// Plays a half-rate frame that has already been through forward error correction, for a
    /// caller that has to undo a cipher on the information bits first.
    pub(crate) fn play_half(&mut self, frame: &HalfRateFrame, out: &mut ChannelOutputs) {
        let lost = frame.errors[0] == u8::MAX;
        let status = if lost {
            FrameStatus::LOST
        } else {
            FrameStatus::new(u32::from(frame.error_total()), false)
        };
        self.synthesize(&frame.info, status, lost, out);
    }

    /// Plays a full-rate frame that has already been through forward error correction.
    pub(crate) fn play_full(&mut self, frame: &FullRateFrame, out: &mut ChannelOutputs) {
        let total = frame.error_total();
        let status = FrameStatus::new(u32::from(total), false);
        let repeat =
            frame.errors[0] >= IMBE_REPEAT_FIRST_ERRORS && total >= IMBE_REPEAT_TOTAL_ERRORS;
        self.synthesize(&frame.info, status, repeat, out);
    }

    pub(crate) fn decode_half_code_vectors(
        &mut self,
        code: [u32; 4],
        encrypted: bool,
        out: &mut ChannelOutputs,
    ) {
        if encrypted {
            self.mute(out);
            return;
        }
        self.play_half(&decode_code_vectors(code), out);
    }

    pub(crate) fn mute(&mut self, out: &mut ChannelOutputs) {
        match &mut self.chip {
            Some(chip) => chip.mute(out),
            None => PcmOutput::silence(1, out),
        }
    }

    pub(crate) fn decode_half_info(
        &mut self,
        bits: &[bool; 49],
        encrypted: bool,
        out: &mut ChannelOutputs,
    ) {
        if encrypted {
            self.mute(out);
            return;
        }
        let mut packed = [0u8; 7];
        for (i, &bit) in bits.iter().enumerate() {
            packed[i / 8] |= u8::from(bit) << (7 - i % 8);
        }
        let info = halfrate::unpack_natural(&packed);
        self.synthesize(&info, FrameStatus::CLEAN, false, out);
    }

    pub(crate) fn decode_full_dibits(
        &mut self,
        dibits: &[u8; 72],
        encrypted: bool,
        out: &mut ChannelOutputs,
    ) {
        if encrypted {
            self.mute(out);
            return;
        }
        self.play_full(&full_frame(dibits), out);
    }

    pub(crate) fn decode_full_code_vectors(
        &mut self,
        code: [u32; 8],
        encrypted: bool,
        out: &mut ChannelOutputs,
    ) {
        let dibits = fullrate::fec::interleave(&code);
        self.decode_full_dibits(&dibits, encrypted, out);
    }
}

/// The 72 D-STAR voice bits as bytes the way the air sends them, least significant bit first.
fn dstar_bytes(bits: &[bool; 72]) -> [u8; MAX_CHIP_FRAME_BYTES] {
    let mut bytes = [0u8; MAX_CHIP_FRAME_BYTES];
    for (byte, chunk) in bytes.iter_mut().zip(bits.as_chunks::<8>().0) {
        *byte = chunk
            .iter()
            .enumerate()
            .fold(0, |acc, (index, &bit)| acc | u8::from(bit) << index);
    }
    bytes
}

pub(crate) struct DstarVocoder {
    decoder: AmbeDecoder,
    output: PcmOutput,
    pcm: [f32; AMBE_FRAME_SAMPLES],
    chip: Option<ChipVoice>,
}

impl DstarVocoder {
    pub(crate) fn new() -> Self {
        Self {
            decoder: AmbeDecoder::new(),
            output: PcmOutput::new(),
            pcm: [0.0; AMBE_FRAME_SAMPLES],
            chip: None,
        }
    }

    /// Hands synthesis to `link`, when there is one.
    pub(crate) fn with_chip(mut self, link: Option<ChipLink>) -> Self {
        self.chip = link.map(|link| ChipVoice::new(link, 1.0));
        self
    }

    /// Moves the chip to whichever of two decoders now plays.
    pub(crate) fn swap_chip(&mut self, other: &mut Self) {
        std::mem::swap(&mut self.chip, &mut other.chip);
    }

    pub(crate) fn reset(&mut self) {
        self.decoder.reset();
        self.output.reset();
        if let Some(chip) = &mut self.chip {
            chip.reset();
        }
    }

    /// Plays whatever speech the chip has returned since; call once per processed block.
    pub(crate) fn drain(&mut self, out: &mut ChannelOutputs) {
        if let Some(chip) = &mut self.chip {
            chip.drain(&mut self.output, out);
        }
    }

    pub(crate) fn decode(&mut self, bits: &[bool; 72], encrypted: bool, out: &mut ChannelOutputs) {
        if let Some(chip) = &mut self.chip {
            if encrypted {
                chip.mute(out);
                return;
            }
            if chip.offer(dstar_bytes(bits)) {
                return;
            }
        } else if encrypted {
            PcmOutput::silence(1, out);
            return;
        }
        self.decoder.decode(bits, &mut self.pcm);
        self.output.append_f32(&self.pcm, out);
    }
}

pub(crate) struct Codec2Decoder {
    codec_3200: Codec2,
    codec_1600: Codec2,
    codec_1300: Codec2,
    output: PcmOutput,
    pcm_3200: [i16; 160],
    pcm: [i16; 320],
}

impl Codec2Decoder {
    pub(crate) fn new() -> Self {
        Self {
            codec_3200: Codec2::new(Codec2Mode::MODE_3200),
            codec_1600: Codec2::new(Codec2Mode::MODE_1600),
            codec_1300: Codec2::new(Codec2Mode::MODE_1300),
            output: PcmOutput::new(),
            pcm_3200: [0; 160],
            pcm: [0; 320],
        }
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::new();
    }

    pub(crate) fn decode_3200(
        &mut self,
        payload: &[u8; 16],
        encrypted: bool,
        out: &mut ChannelOutputs,
    ) {
        if encrypted {
            PcmOutput::silence(2, out);
            return;
        }
        for frame in payload.as_chunks::<8>().0 {
            self.codec_3200.decode(&mut self.pcm_3200, frame);
            self.output.append_i16(&self.pcm_3200, out);
        }
    }

    pub(crate) fn decode_1600(
        &mut self,
        payload: &[u8; 16],
        encrypted: bool,
        out: &mut ChannelOutputs,
    ) {
        if encrypted {
            PcmOutput::silence(2, out);
            return;
        }
        let Some(frame) = payload.first_chunk::<8>() else {
            return;
        };
        self.codec_1600.decode(&mut self.pcm, frame);
        self.output.append_i16(&self.pcm, out);
    }

    pub(crate) fn decode_1300(&mut self, payload: &[u8; 7], out: &mut ChannelOutputs) {
        self.codec_1300.decode(&mut self.pcm, payload);
        self.output.append_i16(&self.pcm, out);
    }
}

pub(crate) fn half_rate_code_vectors(
    frame: &[bool; 72],
    interleave: &[[(u8, u8); 2]; 36],
) -> [u32; 4] {
    let mut code = [0u32; 4];
    for (dibit, places) in frame.as_chunks::<2>().0.iter().zip(interleave) {
        for (&bit, &(row, column)) in dibit.iter().zip(places) {
            code[usize::from(row)] |= u32::from(bit) << column;
        }
    }
    code
}

#[cfg(test)]
pub(crate) mod testutil {
    use blip25_vocoder::{
        halfrate::frame::encode_code_vectors,
        vocoder::{Rate, Vocoder},
    };

    use super::AMBE_3600_INTERLEAVE;

    fn tone(frame: usize) -> [i16; 160] {
        std::array::from_fn(|i| {
            let sample = frame * 160 + i;
            (6_000.0 * (std::f64::consts::TAU * 440.0 * sample as f64 / 8_000.0).sin()) as i16
        })
    }

    pub(crate) fn half_rate_frames(count: usize) -> Vec<[bool; 72]> {
        let mut encoder = Vocoder::new(Rate::HalfRate3600x2450);
        (0..count)
            .map(|frame| {
                let info: [u16; 4] = encoder
                    .encode_info(&tone(frame))
                    .expect("encode half-rate tone")
                    .try_into()
                    .expect("four half-rate information vectors");
                let code = encode_code_vectors(&info);
                let mut air = [false; 72];
                for (dibit, places) in air
                    .as_chunks_mut::<2>()
                    .0
                    .iter_mut()
                    .zip(AMBE_3600_INTERLEAVE)
                {
                    for (bit, (row, column)) in dibit.iter_mut().zip(places) {
                        *bit = code[usize::from(row)] >> column & 1 == 1;
                    }
                }
                air
            })
            .collect()
    }

    pub(crate) fn natural_half_rate_frames(count: usize) -> Vec<[bool; 49]> {
        let mut encoder = Vocoder::new(Rate::HalfRate2450x2450);
        (0..count)
            .map(|frame| {
                let bytes = encoder
                    .encode_pcm(&tone(frame))
                    .expect("encode natural half-rate tone");
                std::array::from_fn(|bit| bytes[bit / 8] >> (7 - bit % 8) & 1 == 1)
            })
            .collect()
    }

    pub(crate) fn full_rate_frames(count: usize) -> Vec<[bool; 144]> {
        let mut encoder = Vocoder::new(Rate::FullRate7200x4400);
        (0..count)
            .map(|frame| {
                let bytes = encoder
                    .encode_pcm(&tone(frame))
                    .expect("encode full-rate tone");
                std::array::from_fn(|bit| bytes[bit / 8] >> (7 - bit % 8) & 1 == 1)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A harmonic voice with two formants and a slow swell, 8 kHz.
    fn voiced(frame: usize) -> [i16; 160] {
        std::array::from_fn(|i| {
            let t = (frame * 160 + i) as f64 / 8_000.0;
            let swell = 0.6 + 0.4 * (std::f64::consts::TAU * 3.0 * t).sin();
            let sample: f64 = (1..30)
                .map(|harmonic| 120.0 * f64::from(harmonic))
                .filter(|&f| f < 3_600.0)
                .enumerate()
                .map(|(index, f)| {
                    let formants = 1.0 / (1.0 + ((f - 500.0) / 300.0).powi(2))
                        + 0.5 / (1.0 + ((f - 1_500.0) / 400.0).powi(2));
                    formants * (std::f64::consts::TAU * f * t).sin() / (index + 1) as f64
                })
                .sum();
            (4_000.0 * swell * sample) as i16
        })
    }

    fn decoded_level(rate: Rate) -> f32 {
        let mut encoder = Vocoder::new(rate);
        let mut decoder = MbeDecoder::new(rate);
        let mut out = ChannelOutputs::default();
        for frame in 0..200 {
            let info = encoder.encode_info(&voiced(frame)).expect("encode voice");
            decoder.synthesize(&info, FrameStatus::CLEAN, false, &mut out);
        }
        crate::testutil::rms(&out.audio_pcm[out.audio_pcm.len() / 4..])
    }

    #[test]
    fn half_and_full_rate_play_one_voice_equally_loud() {
        let full = decoded_level(Rate::FullRate7200x4400);
        let half = decoded_level(Rate::HalfRate3600x2450);
        let difference_db = 20.0 * (half / full).log10();
        assert!(
            difference_db.abs() < 1.0,
            "half rate {difference_db:.1} dB from full rate"
        );
    }

    #[test]
    fn full_rate_information_packs_most_significant_first() {
        let info = [0xABC, 0x123, 0xFFF, 0x000, 0x7FF, 0x001, 0x400, 0x7F];
        let bytes = pack_full(&info);
        assert_eq!(bytes[..3], [0xAB, 0xC1, 0x23]);
        assert_eq!(bytes[10], 0x7F);
        assert_eq!(unpack_full(&bytes), info);
    }

    fn words(seed: u32) -> impl FnMut() -> u32 {
        let mut state = seed | 1;
        move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        }
    }

    fn assert_presentable(out: &ChannelOutputs) {
        assert!(
            out.audio_pcm.iter().all(|sample| sample.is_finite()),
            "vocoder produced a non-finite sample"
        );
        assert!(
            out.audio_pcm.iter().all(|sample| sample.abs() <= 1.0),
            "vocoder produced a sample outside full scale"
        );
    }

    #[test]
    fn the_dstar_vocoder_survives_arbitrary_air_bits() {
        let mut next = words(0x5EED_BEEF);
        let mut vocoder = DstarVocoder::new();
        let mut out = ChannelOutputs::default();
        for _ in 0..4_000 {
            let frame: [bool; 72] = std::array::from_fn(|_| next() & 1 == 1);
            out.reset();
            vocoder.decode(&frame, false, &mut out);
            assert_presentable(&out);
        }
    }

    #[test]
    fn the_dstar_vocoder_still_decodes_speech_after_arbitrary_air_bits() {
        let mut next = words(0x0BAD_F00D);
        let mut vocoder = DstarVocoder::new();
        let mut out = ChannelOutputs::default();
        for _ in 0..500 {
            let frame: [bool; 72] = std::array::from_fn(|_| next() & 1 == 1);
            out.reset();
            vocoder.decode(&frame, false, &mut out);
        }
        vocoder.reset();
        out.reset();
        vocoder.decode(&[false; 72], false, &mut out);
        assert_eq!(out.audio_rate, AUDIO_RATE);
        assert_presentable(&out);
    }

    #[test]
    fn the_half_rate_decoder_survives_arbitrary_frames() {
        let mut next = words(0x1234_5678);
        let mut decoder = MbeDecoder::half_rate();
        let mut out = ChannelOutputs::default();
        for _ in 0..2_000 {
            out.reset();
            decoder.decode_half_code_vectors(std::array::from_fn(|_| next()), false, &mut out);
            decoder.play_half(
                &half_soft_frame(&std::array::from_fn(|_| next() as i8)),
                &mut out,
            );
            decoder.decode_half_info(&std::array::from_fn(|_| next() & 1 == 1), false, &mut out);
            assert_presentable(&out);
        }
    }

    #[test]
    fn the_full_rate_decoder_survives_arbitrary_frames() {
        let mut next = words(0x9ABC_DEF0);
        let mut decoder = MbeDecoder::full_rate();
        let mut out = ChannelOutputs::default();
        for _ in 0..2_000 {
            out.reset();
            decoder.decode_full_code_vectors(std::array::from_fn(|_| next()), false, &mut out);
            decoder.decode_full_dibits(
                &std::array::from_fn(|_| (next() & 3) as u8),
                false,
                &mut out,
            );
            assert_presentable(&out);
        }
    }

    use crate::dv::chip::{CHIP_FRAME_SAMPLES, ChipEnd, chip_link};

    fn chipped(rate: Rate) -> (MbeDecoder, ChipEnd) {
        let (link, end) = chip_link(chip_rate_of(rate), "AMBE3000R");
        (MbeDecoder::new(rate).with_chip(Some(link)), end)
    }

    fn chip_rate_of(rate: Rate) -> ChipRate {
        match rate {
            Rate::HalfRate3600x2450 | Rate::HalfRate2450x2450 => ChipRate::Half,
            Rate::FullRate7200x4400 | Rate::FullRate4400x4400 => ChipRate::Full,
        }
    }

    fn sent_voice(end: &mut ChipEnd) -> ([u8; MAX_CHIP_FRAME_BYTES], bool) {
        match end.next_frame() {
            Some(ChipFrame::Voice { bits, played }) => (bits, played),
            other => panic!("expected a voice frame, got {other:?}"),
        }
    }

    #[test]
    fn a_ready_chip_gets_the_natural_half_rate_bits_and_plays_their_speech() {
        let (mut decoder, mut end) = chipped(Rate::HalfRate3600x2450);
        let mut encoder = Vocoder::new(Rate::HalfRate2450x2450);
        let natural = encoder.encode_pcm(&voiced(0)).expect("encode voice");
        let mut out = ChannelOutputs::default();
        decoder.decode_half_info(
            &std::array::from_fn(|bit| natural[bit / 8] >> (7 - bit % 8) & 1 == 1),
            false,
            &mut out,
        );
        assert!(
            out.audio_pcm.is_empty(),
            "software played a frame the chip took"
        );
        let (bits, played) = sent_voice(&mut end);
        assert_eq!(bits[..7], natural[..]);
        assert!(!played);
        assert!(end.deliver([8_000; CHIP_FRAME_SAMPLES]));
        decoder.drain(&mut out);
        assert_eq!(out.audio_pcm.len(), OUTPUT_SAMPLES_PER_MBE_FRAME);
        assert_eq!(out.audio_rate, AUDIO_RATE);
        assert!(out.status.is_some(), "the vocoder readout names the chip");
    }

    #[test]
    fn full_rate_information_reaches_the_chip_in_its_no_fec_layout() {
        let (mut decoder, mut end) = chipped(Rate::FullRate7200x4400);
        let mut fec = Vocoder::new(Rate::FullRate7200x4400);
        let mut no_fec = Vocoder::new(Rate::FullRate4400x4400);
        let frame = fec.encode_pcm(&voiced(3)).expect("encode voice");
        let expected = no_fec.encode_pcm(&voiced(3)).expect("encode voice");
        let dibits: [u8; 72] = std::array::from_fn(|i| frame[i / 4] >> (6 - 2 * (i % 4)) & 0b11);
        decoder.decode_full_dibits(&dibits, false, &mut ChannelOutputs::default());
        assert_eq!(sent_voice(&mut end).0[..], expected[..]);
    }

    #[test]
    fn a_lost_half_rate_frame_reaches_the_chip_as_an_erasure() {
        let (mut decoder, mut end) = chipped(Rate::HalfRate3600x2450);
        let mut lost = half_frame([0; 4]);
        lost.errors[0] = u8::MAX;
        decoder.play_half(&lost, &mut ChannelOutputs::default());
        assert_eq!(
            sent_voice(&mut end).0[..7],
            Rate::HalfRate2450x2450.erasure_frame()[..]
        );
    }

    #[test]
    fn a_busy_chip_leaves_the_frame_to_blip25_and_still_hears_it() {
        let (mut decoder, mut end) = chipped(Rate::HalfRate3600x2450);
        end.set_health(ChipHealth::Busy);
        let mut out = ChannelOutputs::default();
        decoder.decode_half_code_vectors([0x123, 0x456, 0x78, 0x9A], false, &mut out);
        assert_eq!(out.audio_pcm.len(), OUTPUT_SAMPLES_PER_MBE_FRAME);
        assert!(
            sent_voice(&mut end).1,
            "the chip was not told the frame already played"
        );
    }

    #[test]
    fn a_lost_chip_falls_back_to_blip25_and_says_so() {
        let (mut decoder, end) = chipped(Rate::FullRate7200x4400);
        drop(end);
        let mut out = ChannelOutputs::default();
        decoder.decode_full_code_vectors([0; 8], false, &mut out);
        decoder.drain(&mut out);
        assert_eq!(out.audio_pcm.len(), OUTPUT_SAMPLES_PER_MBE_FRAME);
        let status = out.status.expect("a readout for the lost chip");
        assert!(
            matches!(&status.readouts[0].value, FieldValue::Text(text) if text.contains("lost"))
        );
    }

    #[test]
    fn muting_through_a_ready_chip_keeps_speech_in_order() {
        let (mut decoder, mut end) = chipped(Rate::HalfRate3600x2450);
        let mut out = ChannelOutputs::default();
        decoder.decode_half_code_vectors([0; 4], true, &mut out);
        assert!(out.audio_pcm.is_empty());
        assert_eq!(end.next_frame(), Some(ChipFrame::Silence));
    }

    #[test]
    fn dstar_bytes_carry_the_first_air_bit_least_significant() {
        let mut bits = [false; 72];
        bits[0] = true;
        bits[9] = true;
        bits[71] = true;
        let bytes = dstar_bytes(&bits);
        assert_eq!(bytes[..9], [0x01, 0x02, 0, 0, 0, 0, 0, 0, 0x80]);
    }

    #[test]
    fn the_dstar_chip_follows_the_decoder_that_plays() {
        let (link, mut end) = chip_link(ChipRate::Dstar, "AMBE3000R");
        let mut live = DstarVocoder::new().with_chip(Some(link));
        let mut shadow = DstarVocoder::new();
        live.swap_chip(&mut shadow);
        let mut out = ChannelOutputs::default();
        live.decode(&[false; 72], false, &mut out);
        assert_eq!(out.audio_pcm.len(), OUTPUT_SAMPLES_PER_MBE_FRAME);
        assert_eq!(end.next_frame(), None);
        out.reset();
        shadow.decode(&[true; 72], false, &mut out);
        assert!(out.audio_pcm.is_empty());
        assert_eq!(sent_voice(&mut end).0[..9], [0xFF; 9]);
    }
}
