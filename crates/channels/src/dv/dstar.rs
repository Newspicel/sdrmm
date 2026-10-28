use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_dsp::{crc16_x25, hamming_distance};
use sdrmm_modem::{
    cpm::{CoherentCpmStream, CpmDemod, CpmParams, Mapping, TIMING_BW_BURST},
    pulse::{self, Norm},
};
use sdrmm_wire::{
    CallGate, ChannelDescriptor, ChannelParams, ChannelSettings, DecoderEvent, DecoderFamily,
    DstarParams, DvFrame, DvFrameKind, DvMode, EventKind,
};

use super::{INPUT_RATE_HZ, tap_symbols, vocoder::DstarVocoder};
use crate::{ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, check_input_rate};

pub(crate) const BAUD: f64 = 4_800.0;
pub(crate) const DEVIATION_HZ: f64 = 1_200.0;
pub(crate) const BT: f64 = 0.5;
pub(crate) const PULSE_SPAN: usize = 3;
pub(crate) const MATCHED_SPAN: usize = 3;
pub(crate) const BANDWIDTH_HZ: f64 = 6_250.0;

pub(crate) const SYNC: u32 = 0x00AA_B468;
pub(crate) const SYNC_BITS: u32 = 24;
pub(crate) const SYNC_TOLERANCE: u32 = 2;
const SYNC_MASK: u128 = (1 << SYNC_BITS) - 1;

const FRAME_BITS: usize = 96;
const DATA_BITS: usize = 24;
const VOICE_BITS: usize = FRAME_BITS - DATA_BITS;
const FRAMES_PER_SUPERFRAME: usize = 21;

const COHERENT_LOOP_BW: f64 = 0.01;
const COHERENT_TIMING_BW: f64 = 0.02;
const RELEASE_SYMBOLS: usize = FRAME_BITS;
const SYNC_SLACK: usize = 4;

const SCRAMBLER: [u8; 3] = [0x70, 0x4F, 0x93];

const TYPE_TEXT: u8 = 0x4;
const TYPE_HEADER: u8 = 0x5;

const HEADER_BYTES: usize = 41;
const CALLSIGN_LEN: usize = 8;
const PACKET_BYTES: usize = 6;
const SEGMENT_BYTES: usize = PACKET_BYTES - 1;
const MESSAGE_BLOCKS: usize = 4;
const MESSAGE_LEN: usize = MESSAGE_BLOCKS * SEGMENT_BYTES;
const ALL_BLOCKS: u8 = (1 << MESSAGE_BLOCKS) - 1;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "dstar".to_owned(),
    name: "D-STAR".to_owned(),
    summary: "D-STAR amateur digital voice".to_owned(),
    family: DecoderFamily::DigitalVoice,
    bandwidth_hz: BANDWIDTH_HZ,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: true,
    emits: vec![EventKind::Dv],
    calls: Some(CallGate::Frames),
    ..ChannelDescriptor::default()
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Discriminator,
    Coherent,
}

pub struct DstarChannel {
    demod: CpmDemod,
    coherent: CoherentCpmStream,
    slicer: Mapping,
    decoder: Decoder,
    coherent_decoder: Decoder,
    source: Source,
    unlocked: usize,
    soft: Vec<f32>,
    coherent_soft: Vec<f32>,
    shadow: ChannelOutputs,
}

pub(crate) fn cpm_params(sps: f64) -> CpmParams {
    CpmParams::from_deviation(
        Mapping::natural(2),
        DEVIATION_HZ,
        BAUD,
        pulse::gaussian_freq(sps, BT, PULSE_SPAN, Norm::Area),
        sps,
    )
}

fn params(settings: &ChannelSettings) -> Result<&DstarParams, ChannelError> {
    match &settings.params {
        ChannelParams::Dstar(p) => Ok(p),
        other => Err(ChannelError::InvalidSettings(format!(
            "dstar channel got {} params",
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

impl ChannelRx for DstarChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        params(&settings)?;
        let sps = ctx.input_rate / BAUD;
        let cpm = cpm_params(sps);
        let coherent = CoherentCpmStream::new(&cpm, COHERENT_LOOP_BW, COHERENT_TIMING_BW)
            .map_err(|e| ChannelError::InvalidSettings(e.to_string()))?;
        Ok(Self {
            demod: CpmDemod::new(
                &cpm,
                &pulse::gaussian(sps, BT, MATCHED_SPAN, Norm::Area),
                TIMING_BW_BURST,
            ),
            coherent,
            slicer: cpm.mapping().clone(),
            decoder: Decoder::new(),
            coherent_decoder: Decoder::new(),
            source: Source::Discriminator,
            unlocked: 0,
            soft: Vec::new(),
            coherent_soft: Vec::new(),
            shadow: ChannelOutputs::default(),
        })
    }

    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        params(&settings)?;
        Ok(())
    }

    fn retuned(&mut self) {
        self.demod.reset();
        self.coherent.reset();
        self.decoder.reset();
        self.coherent_decoder.reset();
        self.source = Source::Discriminator;
        self.unlocked = 0;
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.soft.clear();
        self.demod.process(iq, &mut self.soft);
        self.coherent_soft.clear();
        self.coherent.process(iq, &mut self.coherent_soft);
        self.follow_lock();
        self.shadow.reset();
        let (live, shadow) = match self.source {
            Source::Discriminator => (&mut *out, &mut self.shadow),
            Source::Coherent => (&mut self.shadow, &mut *out),
        };
        feed(&mut self.decoder, &self.soft, &self.slicer, live);
        feed(
            &mut self.coherent_decoder,
            &self.coherent_soft,
            &self.slicer,
            shadow,
        );
        self.tap(out);
    }

    fn rejected(&self) -> u32 {
        match self.source {
            Source::Discriminator => self.decoder.rejected(),
            Source::Coherent => self.coherent_decoder.rejected(),
        }
    }
}

fn feed(decoder: &mut Decoder, soft: &[f32], slicer: &Mapping, out: &mut ChannelOutputs) {
    for &symbol in soft {
        decoder.push(slicer.slice(symbol) == 1, out);
    }
}

impl DstarChannel {
    fn follow_lock(&mut self) {
        for &locked in self.coherent.locked() {
            self.unlocked = if locked { 0 } else { self.unlocked + 1 };
        }
        let next = match self.source {
            Source::Discriminator if self.coherent.is_locked() => Source::Coherent,
            Source::Coherent if self.unlocked >= RELEASE_SYMBOLS => Source::Discriminator,
            current => current,
        };
        if next == self.source {
            return;
        }
        self.source = next;
        match next {
            Source::Coherent => self.coherent_decoder.adopt(&self.decoder),
            Source::Discriminator => self.decoder.adopt(&self.coherent_decoder),
        }
    }

    fn tap(&self, out: &mut ChannelOutputs) {
        match self.source {
            Source::Discriminator => tap_symbols(
                out,
                &self.demod,
                &self.soft,
                &self.slicer,
                BAUD,
                INPUT_RATE_HZ,
            ),
            Source::Coherent => out.symbols.levels(
                &self.coherent_soft,
                self.coherent.locked(),
                &self.slicer,
                BAUD,
                self.coherent.frequency_error_cycles_per_sample() * INPUT_RATE_HZ,
            ),
        }
    }
}

struct Decoder {
    register: u128,
    bit: usize,
    frame: usize,
    synced: bool,
    expecting: usize,
    data: u32,
    voice_bits: [bool; VOICE_BITS],
    packet: Vec<u8>,
    header: Vec<u8>,
    heard: Vec<u8>,
    text: [u8; MESSAGE_LEN],
    blocks: u8,
    reported: Option<String>,
    announced: Option<String>,
    vocoder: DstarVocoder,
    rejected: u32,
}

impl Decoder {
    fn new() -> Self {
        Self {
            register: 0,
            bit: 0,
            frame: 0,
            synced: false,
            expecting: 0,
            data: 0,
            voice_bits: [false; VOICE_BITS],
            packet: Vec::with_capacity(PACKET_BYTES),
            header: Vec::with_capacity(HEADER_BYTES),
            heard: Vec::with_capacity(HEADER_BYTES),
            text: [b' '; MESSAGE_LEN],
            blocks: 0,
            reported: None,
            announced: None,
            vocoder: DstarVocoder::new(),
            rejected: 0,
        }
    }

    fn rejected(&self) -> u32 {
        self.rejected
    }

    fn adopt(&mut self, other: &Self) {
        self.reported.clone_from(&other.reported);
        self.announced.clone_from(&other.announced);
        self.heard.clone_from(&other.heard);
        self.text = other.text;
        self.blocks = other.blocks;
    }

    fn reset(&mut self) {
        self.register = 0;
        self.synced = false;
        self.expecting = 0;
        self.bit = 0;
        self.frame = 0;
        self.packet.clear();
        self.header.clear();
        self.heard.clear();
        self.text = [b' '; MESSAGE_LEN];
        self.blocks = 0;
        self.reported = None;
        self.announced = None;
        self.vocoder.reset();
    }

    fn push(&mut self, bit: bool, out: &mut ChannelOutputs) {
        self.register = self.register << 1 | u128::from(bit);
        if !self.synced {
            let expected = self.expecting > 0;
            let tolerance = if expected { SYNC_TOLERANCE } else { 0 };
            self.expecting = self.expecting.saturating_sub(1);
            if hamming_distance((self.register & SYNC_MASK) as u64, u64::from(SYNC)) <= tolerance {
                self.synced = true;
                self.bit = 0;
                self.frame = 1;
                self.packet.clear();
                if expected {
                    self.sync_frame_voice(out);
                }
            }
            return;
        }
        self.bit += 1;
        if self.bit <= VOICE_BITS {
            self.voice_bits[self.bit - 1] = bit;
        }
        if self.bit > VOICE_BITS {
            self.data = self.data << 1 | u32::from(bit);
        }
        if self.bit < FRAME_BITS {
            return;
        }
        self.bit = 0;
        self.vocoder.decode(&self.voice_bits, false, out);
        let frame = self.frame;
        self.frame += 1;
        if self.frame >= FRAMES_PER_SUPERFRAME {
            self.synced = false;
            self.expecting = FRAME_BITS + SYNC_SLACK;
        }
        if !frame.is_multiple_of(FRAMES_PER_SUPERFRAME) {
            self.slow_data(frame, out);
        }
    }

    fn sync_frame_voice(&mut self, out: &mut ChannelOutputs) {
        let voice = self.register >> SYNC_BITS;
        for (index, slot) in self.voice_bits.iter_mut().enumerate() {
            *slot = voice >> (VOICE_BITS - 1 - index) & 1 == 1;
        }
        self.vocoder.decode(&self.voice_bits, false, out);
    }

    fn slow_data(&mut self, frame: usize, out: &mut ChannelOutputs) {
        for (i, mask) in SCRAMBLER.into_iter().enumerate() {
            self.packet
                .push(((self.data >> (16 - i * 8)) as u8).reverse_bits() ^ mask);
        }
        if frame.is_multiple_of(2) && self.packet.len() >= PACKET_BYTES {
            let packet: Vec<u8> = self.packet.drain(..PACKET_BYTES).collect();
            self.packet.clear();
            self.packet_complete(&packet, out);
        } else if self.packet.len() > PACKET_BYTES {
            self.packet.clear();
        }
    }

    fn packet_complete(&mut self, packet: &[u8], out: &mut ChannelOutputs) {
        let kind = packet[0] >> 4;
        let length = usize::from(packet[0] & 0x0F).min(packet.len() - 1);
        match kind {
            TYPE_HEADER => {
                if self.header.len() + length > HEADER_BYTES {
                    self.header.clear();
                }
                self.header.extend_from_slice(&packet[1..=length]);
                if self.header.len() == HEADER_BYTES
                    && let Some(frame) = self.header_frame()
                {
                    out.events.push(DecoderEvent::Dv(frame));
                }
            }
            TYPE_TEXT => {
                let block = usize::from(packet[0] & 0x03);
                let slot = block * SEGMENT_BYTES;
                self.text[slot..slot + SEGMENT_BYTES].copy_from_slice(&packet[1..PACKET_BYTES]);
                self.blocks |= 1 << block;
                if let Some(frame) = self.message_frame() {
                    out.events.push(DecoderEvent::Dv(frame));
                }
            }
            _ => {}
        }
    }

    fn call(&self, offset: usize, len: usize) -> Option<String> {
        let field = self.heard.get(offset..offset + len)?;
        let text = String::from_utf8_lossy(field).trim().to_owned();
        (!text.is_empty()).then_some(text)
    }

    fn call_frame(&self, kind: DvFrameKind) -> DvFrame {
        let mut frame = DvFrame::new(DvMode::Dstar, kind);
        frame.destination_call = self.call(19, CALLSIGN_LEN);
        frame.source_call = self.call(27, CALLSIGN_LEN);
        frame.via = self.call(3, CALLSIGN_LEN);
        frame.group_call = Some(frame.destination_call.as_deref() == Some("CQCQCQ"));
        frame
    }

    fn message_frame(&mut self) -> Option<DvFrame> {
        if self.blocks != ALL_BLOCKS || self.heard.is_empty() {
            return None;
        }
        let text = String::from_utf8_lossy(&self.text).trim().to_owned();
        if text.is_empty() || self.announced.as_deref() == Some(text.as_str()) {
            return None;
        }
        self.announced = Some(text.clone());
        let mut frame = self.call_frame(DvFrameKind::Data);
        frame.text = Some(text);
        Some(frame)
    }

    fn header_frame(&mut self) -> Option<DvFrame> {
        let header = std::mem::take(&mut self.header);
        let (body, crc) = header.split_at(HEADER_BYTES - 2);
        if crc16_x25(body) != u16::from_le_bytes([crc[0], crc[1]]) {
            self.rejected = self.rejected.saturating_add(1);
            return None;
        }
        self.heard = header;
        let source = self.call(27, CALLSIGN_LEN);
        if self.reported == source && source.is_some() {
            return None;
        }
        self.reported.clone_from(&source);
        Some(self.call_frame(DvFrameKind::Header))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        dv::testutil::{decode, decode_with_audio},
        synth::dv::dstar as tx,
        testutil::settings,
    };

    fn channel() -> DstarChannel {
        DstarChannel::new(
            ChannelCtx {
                input_rate: INPUT_RATE_HZ,
            },
            settings(ChannelParams::Dstar(DstarParams::default())),
        )
        .expect("dstar channel")
    }

    #[test]
    fn decodes_the_callsigns_from_the_slow_data_channel() {
        let call = tx::Call::default();
        let iq = tx::transmission(&call, INPUT_RATE_HZ);
        let frames = decode(&mut channel(), &iq);

        let header = frames.first().expect("a decoded frame");
        assert_eq!(header.mode, DvMode::Dstar);
        assert_eq!(header.kind, DvFrameKind::Header);
        assert_eq!(header.source_call.as_deref(), Some(call.mycall.as_str()));
        assert_eq!(
            header.destination_call.as_deref(),
            Some(call.urcall.as_str())
        );
        assert_eq!(header.via.as_deref(), Some(call.repeater.as_str()));
        assert_eq!(header.group_call, Some(true));
    }

    #[test]
    fn a_repeated_header_is_reported_once() {
        let iq = tx::transmission(&tx::Call::default(), INPUT_RATE_HZ);
        let frames = decode(&mut channel(), &iq);
        assert_eq!(
            frames
                .iter()
                .filter(|frame| frame.kind == DvFrameKind::Header)
                .count(),
            1,
            "{frames:?}"
        );
    }

    #[test]
    fn decodes_ambe_voice_to_audio() {
        let superframes = 4;
        let iq = tx::repeated_transmission(&tx::Call::default(), superframes, INPUT_RATE_HZ);
        let (_, audio) = decode_with_audio(&mut channel(), &iq);
        let acquired_without_voice = 1;
        let expected = superframes * FRAMES_PER_SUPERFRAME - acquired_without_voice;
        assert_eq!(
            audio.len(),
            expected * 960,
            "the frame carrying the superframe sync lost its voice"
        );
        assert!(audio.iter().all(|sample| sample.is_finite()));
        assert!(audio.iter().all(|sample| sample.abs() <= 1.0));
    }

    #[test]
    fn a_header_that_fails_its_checksum_is_reported_as_rejected() {
        let iq = tx::transmission_with_broken_header(&tx::Call::default(), INPUT_RATE_HZ);
        let mut chan = channel();

        let frames = decode(&mut chan, &iq);

        assert!(
            frames.is_empty(),
            "a broken header was reported: {frames:?}"
        );
        assert!(
            chan.rejected() > 0,
            "a broken header was dropped in silence"
        );
    }

    #[test]
    fn the_frame_sync_is_the_published_preamble_and_two_maximal_length_sequences() {
        const SPEC_TEN_BIT_SYNC: &str = "1010101010";
        const SPEC_MAXIMAL_LENGTH_SEQUENCE: &str = "1101000";
        let on_air = format!(
            "{SPEC_TEN_BIT_SYNC}{SPEC_MAXIMAL_LENGTH_SEQUENCE}{SPEC_MAXIMAL_LENGTH_SEQUENCE}"
        );
        assert_eq!(on_air.len(), SYNC_BITS as usize);
        let expected = on_air
            .bytes()
            .fold(0u32, |word, bit| word << 1 | u32::from(bit == b'1'));
        assert_eq!(SYNC, expected);
    }

    #[test]
    fn the_slow_data_scrambler_is_the_published_seven_stage_sequence() {
        const SPEC_STAGES: usize = 7;
        const SPEC_TAP: usize = 4;
        let mut history = [true; SPEC_STAGES];
        let mut on_air = [0u8; SCRAMBLER.len()];
        for index in 0..SCRAMBLER.len() * 8 {
            let bit = history[SPEC_STAGES - 1] ^ history[SPEC_TAP - 1];
            history.rotate_right(1);
            history[0] = bit;
            on_air[index / 8] |= u8::from(bit) << (7 - index % 8);
        }
        let descrambled: [u8; SCRAMBLER.len()] = std::array::from_fn(|i| on_air[i].reverse_bits());
        assert_eq!(descrambled, SCRAMBLER);
    }

    #[test]
    fn slow_data_built_to_the_jarl_mini_header_rules_reports_the_call_and_its_message() {
        const SPEC_MESSAGE: &str = "hello from SDRmm";
        let mut header = vec![0u8, 0, 0];
        for field in ["DB0ABC B", "DB0ABC B", "CQCQCQ  ", "DL1ABC  "] {
            header.extend_from_slice(field.as_bytes());
        }
        header.extend_from_slice(b"SDR ");
        header.extend_from_slice(&crc16_x25(&header).to_le_bytes());
        assert_eq!(header.len(), HEADER_BYTES);

        let mut decoder = Decoder::new();
        let mut out = ChannelOutputs::default();
        for segment in header.chunks(SEGMENT_BYTES) {
            let mut packet = [0x66u8; PACKET_BYTES];
            packet[0] = TYPE_HEADER << 4 | segment.len() as u8;
            packet[1..=segment.len()].copy_from_slice(segment);
            decoder.packet_complete(&packet, &mut out);
        }
        let padded = format!("{SPEC_MESSAGE:<MESSAGE_LEN$}");
        for (block, segment) in padded.as_bytes().chunks(SEGMENT_BYTES).enumerate() {
            let mut packet = [0x66u8; PACKET_BYTES];
            packet[0] = TYPE_TEXT << 4 | block as u8;
            packet[1..PACKET_BYTES].copy_from_slice(segment);
            decoder.packet_complete(&packet, &mut out);
        }

        let frames: Vec<&DvFrame> = out
            .events
            .iter()
            .filter_map(|event| match event {
                DecoderEvent::Dv(frame) => Some(frame),
                _ => None,
            })
            .collect();
        let header_frame = frames
            .iter()
            .find(|frame| frame.kind == DvFrameKind::Header)
            .expect("a header frame");
        assert_eq!(header_frame.source_call.as_deref(), Some("DL1ABC"));
        assert_eq!(header_frame.destination_call.as_deref(), Some("CQCQCQ"));
        assert_eq!(header_frame.via.as_deref(), Some("DB0ABC B"));

        let message = frames
            .iter()
            .find(|frame| frame.text.is_some())
            .expect("the slow data message never reached a frame");
        assert_eq!(message.text.as_deref(), Some(SPEC_MESSAGE));
        assert_eq!(message.source_call.as_deref(), Some("DL1ABC"));

        let before = out.events.len();
        for (block, segment) in padded.as_bytes().chunks(SEGMENT_BYTES).enumerate() {
            let mut packet = [0x66u8; PACKET_BYTES];
            packet[0] = TYPE_TEXT << 4 | block as u8;
            packet[1..PACKET_BYTES].copy_from_slice(segment);
            decoder.packet_complete(&packet, &mut out);
        }
        assert_eq!(out.events.len(), before, "the message repeated");
    }

    #[test]
    fn the_header_checksum_matches_the_published_crc_ccitt_check_value() {
        assert_eq!(crc16_x25(b"123456789"), 0x906E);
    }

    #[test]
    fn noise_plays_no_voice() {
        let noise = crate::testutil::complex_noise(7, 0.5, 4_800_000);
        let mut channel = channel();
        let mut out = ChannelOutputs::default();
        for chunk in noise.chunks(4_096) {
            channel.process(chunk, &mut out);
        }
        assert!(out.audio_pcm.is_empty(), "{} samples", out.audio_pcm.len());
    }

    #[test]
    fn noise_decodes_to_nothing() {
        let noise = crate::testutil::complex_noise(81, 0.5, 400_000);
        assert!(decode(&mut channel(), &noise).is_empty());
    }

    fn weak(snr_db: f32, offset_hz: f64, seed: u64) -> Vec<Complex<f32>> {
        let mut iq = vec![Complex::new(0.0, 0.0); 9_600];
        iq.extend(tx::repeated_transmission(
            &tx::Call::default(),
            6,
            INPUT_RATE_HZ,
        ));
        let step = std::f64::consts::TAU * offset_hz / INPUT_RATE_HZ;
        for (k, s) in iq.iter_mut().enumerate() {
            let theta = step * k as f64;
            *s *= Complex::new(theta.cos() as f32, theta.sin() as f32);
        }
        let signal_power = 1.0;
        let sigma = (signal_power / 10f32.powf(snr_db / 10.0) / 2.0).sqrt();
        crate::testutil::add_awgn(&mut iq, sigma, seed);
        let mut filtered = Vec::new();
        channel_filter().process(&iq, &mut filtered);
        filtered
    }

    fn discriminator_headers(iq: &[Complex<f32>]) -> usize {
        let sps = INPUT_RATE_HZ / BAUD;
        let cpm = cpm_params(sps);
        let mut demod = CpmDemod::new(
            &cpm,
            &pulse::gaussian(sps, BT, MATCHED_SPAN, Norm::Area),
            TIMING_BW_BURST,
        );
        let mut decoder = Decoder::new();
        let mut out = ChannelOutputs::default();
        let mut soft = Vec::new();
        demod.process(iq, &mut soft);
        feed(&mut decoder, &soft, cpm.mapping(), &mut out);
        out.events.len()
    }

    #[test]
    fn coherent_detection_decodes_headers_the_discriminator_loses() {
        let (mut coherent, mut discriminator) = (0, 0);
        for seed in 0..4 {
            let iq = weak(-2.0, 700.0, 0x5eed + seed);
            coherent += usize::from(!decode(&mut channel(), &iq).is_empty());
            discriminator += usize::from(discriminator_headers(&iq) > 0);
        }
        assert_eq!(coherent, 4, "coherent decoded {coherent} of 4");
        assert_eq!(
            discriminator, 0,
            "discriminator decoded {discriminator} of 4"
        );
    }
}
