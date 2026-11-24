//! P25 Phase 2: two-slot TDMA voice channels, 6000 symbols a second of H-DQPSK.

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_codec::voice::chip::ChipRate;
use sdrmm_codec::voice::{HALF_RATE_SOFT_BITS, MbeDecoder, half_soft_frame};
use sdrmm_decoder::registry::Native;
use sdrmm_decoder::voice::{Speaker, open_chip, with_vocoder};
use sdrmm_decoder::{
    Channel, ChannelCtx, ChannelError, ChannelOutputs, NativeChannel, check_input_rate,
};
use sdrmm_modem::constellation::Constellation;
use sdrmm_wire::{
    CallGate, ChannelDescriptor, ChannelSettings, ClickProfile, Control, DecoderEvent,
    DecoderFamily, DvFrame, DvFrameKind, EventKind, FilterSpec, HzRange, KindSpec, LAND_MOBILE,
    Modulation, SettingField, SignalSignature, SignalTrial,
};

mod air;
mod demod;
mod frames;
mod mac;
mod model;
mod scrambler;
#[cfg(test)]
mod tests;

pub use model::*;

use super::trunk::BandPlan;
use air::{BODY_DIBITS, Burst, Framer, Packet};
use demod::{Demod, INPUT_RATE_HZ, SYMBOL_RATE, dibit, reliability, soft_bits};
use frames::{CallState, Heard};
use mac::{FACCH, LCCH, Layout, SACCH};
use scrambler::{Masks, solve_identity};

const BANDWIDTH_HZ: f64 = 12_500.0;
const OCCUPIED_HZ: f64 = 7_200.0;
const CHANNEL_TAPS: usize = 129;

/// The body dibits each voice frame of a burst starts at; a two-voice burst has the first two.
const VOICE_FRAMES: [usize; 4] = [1, 38, 86, 123];
const VOICE_FRAME_DIBITS: usize = HALF_RATE_SOFT_BITS / 2;
/// Scrambled signalling that keeps failing its CRC means the site changed under the decoder.
const MAX_SEED_MISSES: u32 = 8;
/// A logical channel keeps the speaker this many timeslots after its last voice.
const SPEAKER_HOLD_SLOTS: usize = 24;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "p25p2".to_owned(),
    name: "P25 Phase 2".to_owned(),
    summary: "P25 two-slot TDMA voice".to_owned(),
    family: DecoderFamily::DIGITAL_VOICE,
    verification: sdrmm_wire::Verification::OnAir,
    bandwidth_hz: BANDWIDTH_HZ,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: true,
    emits: vec![EventKind::DV],
    calls: Some(CallGate::Frames),
    click: ClickProfile::Vocoder,
    ..ChannelDescriptor::default()
});

fn params(settings: &ChannelSettings) -> Result<P25p2Params, ChannelError> {
    settings
        .params
        .parse()
        .map_err(ChannelError::InvalidSettings)
}

pub(crate) fn occupied_band() -> (f64, f64) {
    (-OCCUPIED_HZ / 2.0, OCCUPIED_HZ / 2.0)
}

pub(crate) fn channel_filter() -> FilterSpec {
    FilterSpec::lowpass(CHANNEL_TAPS, BANDWIDTH_HZ / 2.0)
}

pub struct P25p2Channel {
    demod: Demod,
    phasors: Vec<Complex<f32>>,
    constellation: Constellation,
    framer: Framer,
    receiver: Receiver,
}

impl NativeChannel for P25p2Channel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let params = params(&settings)?;
        let invalid = |error: demod::DemodError| ChannelError::InvalidSettings(error.to_string());
        let lane = || -> Result<Lane, ChannelError> {
            Ok(Lane {
                encrypted: None,
                voice: MbeDecoder::half_rate()
                    .with_chip(open_chip(params.vocoder.as_deref(), ChipRate::Half)?),
            })
        };
        Ok(Self {
            demod: Demod::new().map_err(invalid)?,
            phasors: Vec::new(),
            constellation: demod::constellation().map_err(invalid)?,
            framer: Framer::new(),
            receiver: Receiver {
                slots: params.slots,
                masks: None,
                seed_misses: 0,
                lanes: vec![lane()?, lane()?],
                speaking: None,
                speaking_hold: 0,
                bands: BandPlan::default(),
                rejected: 0,
                frames: Vec::new(),
                hard: [0; BODY_DIBITS],
                reliability: [0.0; BODY_DIBITS],
            },
        })
    }
}

impl Channel for P25p2Channel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        self.receiver.slots = params(&settings)?.slots;
        Ok(())
    }

    fn retuned(&mut self) {
        self.demod.reset();
        self.framer.reset();
        self.receiver.reset();
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.phasors.clear();
        self.demod.process(iq, &mut self.phasors);
        out.symbols.linear(
            &self.phasors,
            &self.constellation,
            SYMBOL_RATE,
            self.demod.frequency_offset_hz(),
        );
        for &phasor in &self.phasors {
            if self.framer.push(phasor) {
                self.receiver.take(self.framer.packet(), out);
            }
        }
        for lane in &mut self.receiver.lanes {
            lane.voice.drain(&mut Speaker(out));
        }
    }

    fn rejected(&self) -> u32 {
        self.receiver.rejected
    }
}

struct Lane {
    encrypted: Option<bool>,
    voice: MbeDecoder,
}

/// Everything after framing: descrambling, signalling, voice.
struct Receiver {
    slots: P25p2Slots,
    masks: Option<Masks>,
    seed_misses: u32,
    /// One per logical channel.
    lanes: Vec<Lane>,
    speaking: Option<u8>,
    speaking_hold: usize,
    bands: BandPlan,
    rejected: u32,
    frames: Vec<DvFrame>,
    hard: [u8; BODY_DIBITS],
    reliability: [f32; BODY_DIBITS],
}

impl Receiver {
    fn reset(&mut self) {
        self.masks = None;
        self.seed_misses = 0;
        for lane in &mut self.lanes {
            lane.encrypted = None;
            lane.voice.reset();
        }
        self.speaking = None;
        self.speaking_hold = 0;
        self.bands.reset();
    }

    fn reject(&mut self) {
        self.rejected = self.rejected.saturating_add(1);
    }

    fn take(&mut self, packet: &Packet, out: &mut ChannelOutputs) {
        self.speaking_hold = self.speaking_hold.saturating_sub(1);
        if self.speaking_hold == 0 {
            self.speaking = None;
        }
        match packet.burst() {
            burst @ (Burst::Voice4 | Burst::Voice2) => {
                self.voice(packet, burst.voice_frames(), out);
            }
            Burst::Sacch { scrambled } => self.signalling(packet, &SACCH, scrambled, out),
            Burst::Facch { scrambled } => self.signalling(packet, &FACCH, scrambled, out),
            Burst::Lcch { scrambled } => self.signalling(packet, &LCCH, scrambled, out),
            Burst::Datch | Burst::Reserved(_) => {}
            Burst::Unknown => self.reject(),
        }
    }

    fn take_speaker(&mut self, lane: u8) -> bool {
        if self.speaking.is_some_and(|speaking| speaking != lane) {
            return false;
        }
        self.speaking = Some(lane);
        self.speaking_hold = SPEAKER_HOLD_SLOTS;
        true
    }

    fn voice(&mut self, packet: &Packet, count: usize, out: &mut ChannelOutputs) {
        let lane = packet.channel();
        if !self.slots.accepts(lane + 1) || !self.take_speaker(lane) {
            return;
        }
        let body = packet.body();
        for &start in &VOICE_FRAMES[..count] {
            let mut soft = [0i8; HALF_RATE_SOFT_BITS];
            for (offset, pair) in soft.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                *pair = soft_bits(body[start + offset]);
            }
            let state = &mut self.lanes[usize::from(lane)];
            let Some(masks) = &self.masks else {
                state.voice.mute(&mut Speaker(out));
                continue;
            };
            let mask = &masks.body(packet.slot)[start..start + VOICE_FRAME_DIBITS];
            for (pair, &scramble) in soft.as_chunks_mut::<2>().0.iter_mut().zip(mask) {
                if scramble & 2 != 0 {
                    pair[0] = -pair[0];
                }
                if scramble & 1 != 0 {
                    pair[1] = -pair[1];
                }
            }
            if state.encrypted == Some(false) {
                state
                    .voice
                    .play_half(&half_soft_frame(&soft), &mut Speaker(out));
            } else {
                state.voice.mute(&mut Speaker(out));
            }
        }
    }

    fn signalling(
        &mut self,
        packet: &Packet,
        layout: &Layout,
        scrambled: bool,
        out: &mut ChannelOutputs,
    ) {
        for ((hard, confidence), &phasor) in self
            .hard
            .iter_mut()
            .zip(&mut self.reliability)
            .zip(packet.body())
        {
            *hard = dibit(phasor);
            *confidence = reliability(phasor);
        }
        let lane = packet.channel();
        if scrambled {
            if self.masks.is_none() {
                self.learn_identity(packet, layout, out);
            }
            let Some(masks) = &self.masks else {
                self.reject();
                return;
            };
            for (hard, &scramble) in self.hard.iter_mut().zip(masks.body(packet.slot)) {
                *hard ^= scramble;
            }
        }
        let Some(pdu) = mac::decode(&self.hard, &self.reliability, layout) else {
            self.reject();
            if scrambled {
                self.seed_misses += 1;
                if self.seed_misses >= MAX_SEED_MISSES {
                    self.masks = None;
                    self.seed_misses = 0;
                }
            }
            return;
        };
        if scrambled {
            self.seed_misses = 0;
        }
        let heard = Heard {
            slot: lane + 1,
            identity: self.masks.as_ref().map(Masks::identity),
            corrected: pdu.corrected,
        };
        let state = frames::pdu_frames(&pdu, heard, &mut self.bands, &mut self.frames);
        if let Some(state) = state
            && let Some(lane) = self.lanes.get_mut(usize::from(lane))
        {
            lane.encrypted = match state {
                CallState::Encrypted(encrypted) => Some(encrypted),
                CallState::Ended => None,
            };
        }
        self.emit(out);
    }

    /// Works the site's scrambling seed out of a burst and keeps it when the burst then
    /// decodes with it.
    fn learn_identity(&mut self, packet: &Packet, layout: &Layout, out: &mut ChannelOutputs) {
        let Some(identity) = solve_identity(&self.hard, packet.slot, layout) else {
            return;
        };
        let masks = Masks::new(identity);
        let mut descrambled = self.hard;
        for (hard, &scramble) in descrambled.iter_mut().zip(masks.body(packet.slot)) {
            *hard ^= scramble;
        }
        if mac::decode(&descrambled, &self.reliability, layout).is_none() {
            return;
        }
        self.masks = Some(masks);
        self.seed_misses = 0;
        let heard = Heard {
            slot: packet.channel() + 1,
            identity: Some(identity),
            corrected: 0,
        };
        let mut frame = heard.frame(DvFrameKind::Control, "scrambling identity");
        frame.slot = None;
        self.frames.push(frame);
        self.emit(out);
    }

    fn emit(&mut self, out: &mut ChannelOutputs) {
        for frame in self.frames.drain(..) {
            if frame.slot.is_none_or(|slot| self.slots.accepts(slot)) {
                out.events.push(DecoderEvent::of(&frame));
            }
        }
    }
}

fn p25p2_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "P25 Phase 2",
            &[Modulation::Psk4],
            HzRange::new(6_000.0, BANDWIDTH_HZ),
            "6 ksym/s H-DQPSK in a 12.5 kHz land mobile channel",
        )
        .symbol_rate(HzRange::about(SYMBOL_RATE, 0.03))
        .on(LAND_MOBILE),
    ]
}

fn p25p2_trial() -> SignalTrial {
    SignalTrial::proven_by(EventKind::DV)
}

pub(crate) fn p25p2_decoder() -> Native<P25p2Params> {
    Native::new::<P25p2Channel>(|_| occupied_band())
        .filter(|_| Ok(channel_filter()))
        .signatures(p25p2_signatures)
        .trial(p25p2_trial)
        .form(p25p2_form)
        .kinds(p25p2_kinds)
}

pub(crate) fn p25p2_form() -> Vec<SettingField> {
    with_vocoder(vec![
        SettingField::new("slots", "Slot", Control::choice::<P25p2Slots>()).title("Slot"),
    ])
}

fn p25p2_kinds() -> Vec<KindSpec> {
    vec![]
}
