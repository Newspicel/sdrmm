use sdrmm_decoder::voice::keys_field;
use sdrmm_decoder::voice::with_vocoder;
use sdrmm_dsp::field_msb;
use sdrmm_wire::{FrameSync, LAND_MOBILE, SignalSignature, SyncPulse};
mod model;
#[cfg(any(test, feature = "synth"))]
pub mod synth;
pub use model::*;
use sdrmm_crypto::hex_bits;
use sdrmm_decoder::registry::Native;
use sdrmm_wire::FilterSpec;
use sdrmm_wire::{Control, KindSpec, SettingField};
use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_dsp::{
    Bptc128, Bptc196, CyclicCode, ParityCode, Permutation, TrellisCode, coding::fec::conv::Soft,
    crc_msb_bits, crc16_msb, pack_msb_into, pack_msb_slice, rs129_parity,
};
use sdrmm_modem::cpm::CpmDemod;
use sdrmm_wire::VoiceKey;
use sdrmm_wire::{
    CallGate, ChannelDescriptor, ChannelSettings, ClickProfile, DecoderEvent, DecoderFamily,
    DvChannelDefinition, DvFrame, DvFrameKind, DvSlotActivity, EventKind, Vendor,
};

use sdrmm_codec::voice::chip::ChipRate;
use sdrmm_codec::voice::{HALF_RATE_SOFT_BITS, MbeDecoder, half_soft_frame};
use sdrmm_decoder::voice::{Speaker, open_chip};
use sdrmm_modem::cpm::c4fm::{DibitWindow, c4fm_demod, c4fm_params};
pub(crate) mod privacy;
mod trunk;
use privacy::{CipherKey, Privacy, late_entry_fragment};
use sdrmm_decoder::{
    Channel, ChannelCtx, ChannelError, ChannelOutputs, NativeChannel, check_input_rate,
};
use trunk::{CAPACITY_PLUS, HYTERA_XPT, TIER_THREE};

const INPUT_RATE_HZ: f64 = 48_000.0;

const CHANNEL_TAPS: usize = 129;

pub(crate) const BAUD: f64 = 4_800.0;
pub(crate) const DEVIATION_HZ: f64 = 1_944.0;
pub(crate) const RRC_ALPHA: f64 = 0.2;
pub(crate) const BANDWIDTH_HZ: f64 = 12_500.0;

const BURST_BITS: usize = 264;
const BURST_SYMBOLS: usize = BURST_BITS / 2;
pub(crate) const SYNC_BITS: u32 = 48;
const HALF_PAYLOAD_BITS: usize = 108;
const TRAILING_SYMBOLS: usize = HALF_PAYLOAD_BITS / 2;

const SLOT_SYMBOLS: usize = 144;

const SUPERFRAME_STRIDE: usize = SLOT_SYMBOLS * 2;
const SPEAKER_HOLD_SYMBOLS: usize = SUPERFRAME_STRIDE * 3;

/// How long a slot's voice call may go without a voice burst before it has ended unheard: a
/// superframe, and two bursts of slack so one missed voice sync does not end it.
const CALL_LOSS_SYMBOLS: usize = SUPERFRAME_STRIDE * (VOICE_FOLLOWERS as usize + 3);

pub(crate) const SYNC_TOLERANCE: u32 = 4;

const DT_PI_HEADER: u8 = 0x0;
const DT_VOICE_LC_HEADER: u8 = 0x1;
const DT_TERMINATOR_WITH_LC: u8 = 0x2;
const DT_CSBK: u8 = 0x3;
const DT_MBC_HEADER: u8 = 0x4;
const DT_MBC_CONTINUATION: u8 = 0x5;
const DT_DATA_HEADER: u8 = 0x6;
const DT_RATE_HALF_DATA: u8 = 0x7;
const DT_RATE_THREE_QUARTER_DATA: u8 = 0x8;
const DT_RATE_ONE_DATA: u8 = 0xA;
const DT_UNIFIED_SINGLE_BLOCK_DATA: u8 = 0xB;

/// How much repair a block may have needed and still be handed on when its checksum cannot be
/// trusted. Every row and column of the BPTC block still has to check out either way, which is a
/// hundred parity bits of evidence. Insisting on an untouched block on top of that would drop
/// about one burst in thirty off the air, and a channel grant is a single burst.
const LCSS_FIRST: u32 = 0b01;
const LCSS_LAST: u32 = 0b10;
const LCSS_CONTINUATION: u32 = 0b11;

const SHORT_LC_BITS: usize = 68;
const CACH_BITS: usize = 24;

pub(crate) const RATE_ONE_CODED_BITS: usize = 196;
pub(crate) const RATE_ONE_DATA_BITS: usize = 192;
pub(crate) const RATE_ONE_PAD_AT: usize = 96;
pub(crate) const RATE_ONE_PAD_BITS: usize = RATE_ONE_CODED_BITS - RATE_ONE_DATA_BITS;

const MAX_UNVERIFIED_REPAIRS: u32 = 4;

/// How many control blocks in a row have to arrive whole yet fail the checksum before the site is
/// taken to be masking it. A restricted site folds a key the receiver does not hold into every
/// control block's sum, so the arithmetic never agrees however clean the burst was; one such block
/// is a bit error the parity happened to miss, but a run of them with nothing to repair is the
/// site, and discarding those hides the whole system behind a setting nobody knew to turn on.
const MASKED_SUM_RUN: u32 = 8;

pub(crate) const TRELLIS_DIBITS: usize = 98;
pub(crate) const TRELLIS_DATA_BITS: usize = 144;
pub(crate) const TRELLIS_STEPS: usize = TRELLIS_DIBITS / 2;

const VOICE_LC_HEADER_MASK: [u8; 3] = [0x96, 0x96, 0x96];
const TERMINATOR_LC_MASK: [u8; 3] = [0x99, 0x99, 0x99];
const PI_HEADER_MASK: u16 = 0x6969;
const CSBK_MASK: u16 = 0xA5A5;
const MBC_HEADER_MASK: u16 = 0xAAAA;
const DATA_HEADER_MASK: u16 = 0xCCCC;

const LC_BITS: usize = 72;
const VOCODER_FRAME_BITS: usize = HALF_RATE_SOFT_BITS;
const VOCODER_FRAMES_PER_BURST: usize = 3;
const VOICE_FOLLOWERS: u8 = 5;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "dmr".to_owned(),
    name: "DMR".to_owned(),
    summary: "DMR two-way radio voice".to_owned(),
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

struct Sync {
    bits: u64,
    voice: bool,
    slot: Option<u8>,
    cach: bool,
}

const SYNCS: [Sync; 8] = [
    Sync {
        bits: 0x755F_D7DF_75F7,
        voice: true,
        slot: None,
        cach: true,
    },
    Sync {
        bits: 0xDFF5_7D75_DF5D,
        voice: false,
        slot: None,
        cach: true,
    },
    Sync {
        bits: 0x7F7D_5DD5_7DFD,
        voice: true,
        slot: None,
        cach: false,
    },
    Sync {
        bits: 0xD5D7_F77F_D757,
        voice: false,
        slot: None,
        cach: false,
    },
    Sync {
        bits: 0x5D57_7F77_57FF,
        voice: true,
        slot: Some(1),
        cach: false,
    },
    Sync {
        bits: 0xF7FD_D5DD_FD55,
        voice: false,
        slot: Some(1),
        cach: false,
    },
    Sync {
        bits: 0x7DFF_D5F5_5D5F,
        voice: true,
        slot: Some(2),
        cach: false,
    },
    Sync {
        bits: 0xD755_7F5F_F7F5,
        voice: false,
        slot: Some(2),
        cach: false,
    },
];

pub(crate) const SYNC_PATTERNS: [u64; SYNCS.len()] = {
    let mut out = [0; SYNCS.len()];
    let mut i = 0;
    while i < SYNCS.len() {
        out[i] = SYNCS[i].bits;
        i += 1;
    }
    out
};

pub struct DmrChannel {
    demod: CpmDemod,
    symbols: Vec<f32>,
    decoder: Decoder,
}

pub(crate) fn dv_frame(kind: DvFrameKind) -> DvFrame {
    DvFrame::new("DMR", kind)
}

fn params(settings: &ChannelSettings) -> Result<DmrParams, ChannelError> {
    settings
        .params
        .parse()
        .map_err(ChannelError::InvalidSettings)
}

pub(crate) fn occupied_band() -> (f64, f64) {
    (-BANDWIDTH_HZ / 2.0, BANDWIDTH_HZ / 2.0)
}

pub(crate) fn channel_filter() -> FilterSpec {
    FilterSpec::lowpass(CHANNEL_TAPS, BANDWIDTH_HZ / 2.0)
}

impl NativeChannel for DmrChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }
    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        Self::with_params(ctx, params(&settings)?)
    }
}

impl Channel for DmrChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let params = params(&settings)?;
        self.decoder.keys = cipher_keys(&params.keys)?;
        self.decoder.params = params;
        Ok(())
    }

    fn retuned(&mut self) {
        self.demod.reset();
        self.decoder.reset();
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.symbols.clear();
        self.demod.process(iq, &mut self.symbols);
        out.symbols
            .c4fm(&self.demod, &self.symbols, BAUD, INPUT_RATE_HZ);
        for &symbol in &self.symbols {
            self.decoder.push(symbol, out);
        }
        for slot in &mut self.decoder.slots {
            slot.voice.drain(&mut Speaker(out));
        }
    }

    fn rejected(&self) -> u32 {
        self.decoder.rejected()
    }
}

impl DmrChannel {
    fn with_params(ctx: ChannelCtx, params: DmrParams) -> Result<Self, ChannelError> {
        let keys = cipher_keys(&params.keys)?;
        let port = params.vocoder.clone();
        let mut decoder = Decoder::new(params);
        decoder.keys = keys;
        for slot in &mut decoder.slots {
            slot.voice =
                MbeDecoder::half_rate().with_chip(open_chip(port.as_deref(), ChipRate::Half)?);
        }
        Ok(Self {
            demod: c4fm_demod(&c4fm_params(ctx.input_rate, BAUD, DEVIATION_HZ, RRC_ALPHA)),
            symbols: Vec::new(),
            decoder,
        })
    }
}

type CipherKeys = Vec<(u8, CipherKey)>;

fn cipher_keys(entries: &[VoiceKey]) -> Result<CipherKeys, ChannelError> {
    entries
        .iter()
        .map(|entry| {
            let key_id = u8::try_from(entry.key_id).map_err(|_| {
                ChannelError::InvalidSettings(format!(
                    "a DMR key ID is 0 to 255, got {}",
                    entry.key_id
                ))
            })?;
            CipherKey::parse(&entry.key)
                .map(|key| (key_id, key))
                .ok_or_else(|| {
                    ChannelError::InvalidSettings(format!(
                        "a DMR key is 10 (ARC4), 32 or 64 (AES) hex digits, got {:?}",
                        entry.key
                    ))
                })
        })
        .collect()
}

#[derive(Clone, Copy)]
enum Pending {
    None,
    Burst {
        voice: bool,
        slot: Option<u8>,
        cach: bool,
    },
}

struct Decoder {
    params: DmrParams,
    keys: CipherKeys,
    window: DibitWindow,
    pending: Pending,
    countdown: usize,
    followers: [u8; 2],
    follower_countdown: [usize; 2],
    follower_cach: [bool; 2],
    follower_slot: [Option<u8>; 2],
    bits: Vec<bool>,
    soft: Vec<i8>,
    coded: Vec<Soft>,
    bytes: Vec<u8>,
    slots: [SlotState; 2],
    short_lc: ShortLc,
    mbc_headers: [Option<MbcHeader>; 2],
    speaking: Option<usize>,
    speaking_hold: usize,
    calls: [Option<Call>; 2],
    tier_three: bool,
    masked_sums: u32,
    rejected: u32,
}

/// A voice call open on a slot, so one that stops without its terminator is counted lost.
#[derive(Clone, Copy)]
struct Call {
    voiced: bool,
    quiet: usize,
}

#[derive(Clone)]
struct MbcHeader {
    fid: u8,
    opcode: u8,
    frame: DvFrame,
}

struct SlotState {
    embedded: Vec<Soft>,
    embedded_errors: u32,
    encrypted: Option<bool>,
    privacy: Privacy,
    talker_alias: TalkerAlias,
    voice: MbeDecoder,
}

impl SlotState {
    fn new() -> Self {
        Self {
            embedded: Vec::with_capacity(Bptc128::CODED_BITS),
            embedded_errors: 0,
            encrypted: None,
            privacy: Privacy::new(),
            talker_alias: TalkerAlias::default(),
            voice: MbeDecoder::half_rate(),
        }
    }

    fn reset(&mut self) {
        self.embedded.clear();
        self.embedded_errors = 0;
        self.encrypted = None;
        self.privacy.reset();
        self.talker_alias.reset();
        self.voice.reset();
    }
}

impl Decoder {
    fn new(params: DmrParams) -> Self {
        Self {
            params,
            keys: Vec::new(),
            window: DibitWindow::new(SLOT_SYMBOLS),
            pending: Pending::None,
            countdown: 0,
            followers: [0; 2],
            follower_countdown: [0; 2],
            follower_cach: [false; 2],
            follower_slot: [None; 2],
            bits: Vec::with_capacity(BURST_BITS),
            soft: Vec::with_capacity(BURST_BITS),
            coded: Vec::with_capacity(BURST_BITS),
            bytes: Vec::with_capacity(BURST_BITS / 8),
            slots: std::array::from_fn(|_| SlotState::new()),
            short_lc: ShortLc::default(),
            mbc_headers: std::array::from_fn(|_| None),
            speaking: None,
            speaking_hold: 0,
            calls: [None; 2],
            tier_three: false,
            masked_sums: 0,
            rejected: 0,
        }
    }

    fn rejected(&self) -> u32 {
        self.rejected.saturating_add(self.short_lc.rejected())
    }

    fn reject(&mut self) {
        self.rejected = self.rejected.saturating_add(1);
    }

    fn reset(&mut self) {
        self.window.reset();
        self.pending = Pending::None;
        self.countdown = 0;
        self.followers = [0; 2];
        self.follower_countdown = [0; 2];
        self.follower_cach = [false; 2];
        self.follower_slot = [None; 2];
        self.slots.iter_mut().for_each(SlotState::reset);
        self.short_lc.reset();
        self.mbc_headers.fill(None);
        self.speaking = None;
        self.speaking_hold = 0;
        self.calls = [None; 2];
        self.masked_sums = 0;
        self.tier_three = false;
    }

    fn push(&mut self, symbol: f32, out: &mut ChannelOutputs) {
        self.window.push(symbol);
        self.age_calls();
        if self.speaking_hold > 0 {
            self.speaking_hold -= 1;
            if self.speaking_hold == 0 {
                self.speaking = None;
            }
        }
        for index in 0..2 {
            if self.follower_countdown[index] > 0 {
                self.follower_countdown[index] -= 1;
                if self.follower_countdown[index] == 0 {
                    self.voice_burst(index, out);
                }
            }
        }
        if self.countdown > 0 {
            self.countdown -= 1;
            if self.countdown > 0 {
                return;
            }
            match std::mem::replace(&mut self.pending, Pending::None) {
                Pending::None => {}
                Pending::Burst { voice, slot, cach } => self.burst(voice, slot, cach, out),
            }
            return;
        }
        self.hunt(out);
    }

    fn hunt(&mut self, out: &mut ChannelOutputs) {
        for sync in &SYNCS {
            if self.window.sync_distance(sync.bits, SYNC_BITS) <= SYNC_TOLERANCE {
                self.window.anchor(sync.bits, SYNC_BITS);
                let slot = match (sync.slot, sync.cach) {
                    (slot @ Some(_), _) => slot,
                    (None, true) => self.cach(out),
                    (None, false) => None,
                };
                self.pending = Pending::Burst {
                    voice: sync.voice,
                    slot,
                    cach: sync.cach,
                };
                self.countdown = TRAILING_SYMBOLS;
                if let Some(index) = slot_index(slot) {
                    self.drop_embedded(index);
                    self.followers[index] = 0;
                    self.follower_countdown[index] = 0;
                }
                return;
            }
        }
    }

    fn cach(&mut self, out: &mut ChannelOutputs) -> Option<u8> {
        const CACH_BACK: usize = HALF_PAYLOAD_BITS / 2 + SYNC_BITS as usize / 2;
        self.cach_at(CACH_BACK, out)
    }

    fn cach_at(&mut self, end_back: usize, out: &mut ChannelOutputs) -> Option<u8> {
        self.window
            .soft_bits(end_back, CACH_BITS / 2, &mut self.coded);
        let (slot, lcss, payload) = read_cach(&self.coded)?;
        if let Some(mut frame) = self.short_lc.push(lcss, payload) {
            frame.errors_corrected = 0;
            self.emit(&frame, out);
        }
        Some(slot)
    }

    fn burst(&mut self, voice: bool, slot: Option<u8>, cach: bool, out: &mut ChannelOutputs) {
        if voice {
            self.window.soft_bits(0, BURST_SYMBOLS, &mut self.coded);
            let index = slot_index(slot).unwrap_or(0);
            self.hear_voice(slot, cach);
            self.voice_payload(index, slot, 0, out);
            self.followers[index] = VOICE_FOLLOWERS;
            self.follower_cach[index] = cach;
            self.follower_slot[index] = slot;
            self.schedule_follower(index);
            return;
        }
        if let Some(index) = slot_index(slot) {
            self.followers[index] = 0;
            self.follower_countdown[index] = 0;
        }
        self.window.bits(0, BURST_SYMBOLS, &mut self.bits);
        self.window.soft_bits(0, BURST_SYMBOLS, &mut self.coded);
        let index = slot_index(slot).unwrap_or(0);
        let rejected = self.rejected;
        let Some(mut frame) = self.data_burst(index, slot) else {
            if self.rejected != rejected {
                self.close_counted_call(slot, cach);
            }
            return;
        };
        match frame.kind {
            DvFrameKind::Header => {
                self.open_call(slot, cach);
                self.track_encryption(index, &frame);
                self.slots[index].voice.reset();
                frame.cipher = self.slots[index].privacy.label();
            }
            DvFrameKind::Terminator => {
                frame.cipher = self.slots[index].privacy.label();
                self.end_call(index);
            }
            _ => {}
        }
        self.emit(&frame, out);
    }

    /// The slot a burst's voice call is tracked on: one the burst names and the filter hears, or
    /// the one channel of a mobile's direct transmission. A base station burst whose CACH failed
    /// belongs to no call.
    fn call_slot(&self, slot: Option<u8>, cach: bool) -> Option<usize> {
        match slot {
            Some(slot) if self.params.slots.accepts(slot) => slot_index(Some(slot)),
            Some(_) => None,
            None => (!cach).then_some(0),
        }
    }

    fn open_call(&mut self, slot: Option<u8>, cach: bool) {
        let Some(index) = self.call_slot(slot, cach) else {
            return;
        };
        if self.calls[index].is_some_and(|call| call.voiced) {
            self.lose_call(index);
        }
        self.calls[index] = Some(Call {
            voiced: false,
            quiet: 0,
        });
    }

    fn hear_voice(&mut self, slot: Option<u8>, cach: bool) {
        if let Some(index) = self.call_slot(slot, cach) {
            self.calls[index] = Some(Call {
                voiced: true,
                quiet: 0,
            });
        }
    }

    /// A data burst that failed to decode on a slot in a call is its lost terminator, already
    /// counted as rejected.
    fn close_counted_call(&mut self, slot: Option<u8>, cach: bool) {
        if let Some(index) = self.call_slot(slot, cach)
            && self.calls[index].is_some()
        {
            self.end_call(index);
        }
    }

    fn age_calls(&mut self) {
        for index in 0..self.calls.len() {
            let Some(call) = &mut self.calls[index] else {
                continue;
            };
            call.quiet += 1;
            if call.quiet > CALL_LOSS_SYMBOLS {
                self.lose_call(index);
            }
        }
    }

    fn lose_call(&mut self, index: usize) {
        self.reject();
        self.end_call(index);
    }

    fn end_call(&mut self, index: usize) {
        self.calls[index] = None;
        self.slots[index].encrypted = None;
        self.slots[index].privacy.reset();
        self.slots[index].voice.reset();
        if self.speaking == Some(index) {
            self.speaking = None;
            self.speaking_hold = 0;
        }
    }

    fn schedule_follower(&mut self, index: usize) {
        if self.followers[index] == 0 {
            return;
        }
        self.followers[index] -= 1;
        self.follower_countdown[index] = SUPERFRAME_STRIDE;
    }

    fn voice_burst(&mut self, index: usize, out: &mut ChannelOutputs) {
        let slot = self.follower_slot[index];
        if self.follower_cach[index] {
            self.cach_at(BURST_SYMBOLS, out);
        }
        self.window.soft_bits(0, BURST_SYMBOLS, &mut self.coded);
        if embedded_signalling_type(&self.coded).is_some() {
            self.hear_voice(slot, self.follower_cach[index]);
        }
        if let Some(mut frame) = self.embedded_signalling(index, slot) {
            self.track_encryption(index, &frame);
            frame.cipher = self.slots[index].privacy.label();
            self.emit(&frame, out);
        }
        let burst = usize::from(VOICE_FOLLOWERS - self.followers[index]);
        self.voice_payload(index, slot, burst, out);
        self.schedule_follower(index);
    }

    fn take_speaker(&mut self, index: usize) -> bool {
        if self.speaking.is_some_and(|speaking| speaking != index) {
            return false;
        }
        self.speaking = Some(index);
        self.speaking_hold = SPEAKER_HOLD_SYMBOLS;
        true
    }

    fn track_encryption(&mut self, index: usize, frame: &DvFrame) {
        if let Some(encrypted) = frame.encrypted {
            let state = &mut self.slots[index];
            state.encrypted = Some(encrypted);
            state.privacy.link_control(encrypted, frame.manufacturer_id);
        }
    }

    fn voice_payload(
        &mut self,
        index: usize,
        slot: Option<u8>,
        burst: usize,
        out: &mut ChannelOutputs,
    ) {
        if slot.is_some_and(|slot| !self.params.slots.accepts(slot)) {
            return;
        }
        if !self.take_speaker(index) {
            return;
        }
        self.window
            .vocoder_soft_bits(0, BURST_SYMBOLS, &mut self.soft);
        let mut payload = [0i8; VOCODER_FRAME_BITS * VOCODER_FRAMES_PER_BURST];
        payload[..HALF_PAYLOAD_BITS].copy_from_slice(&self.soft[..HALF_PAYLOAD_BITS]);
        payload[HALF_PAYLOAD_BITS..].copy_from_slice(&self.soft[156..]);
        let mut coded = [0 as Soft; VOCODER_FRAME_BITS * VOCODER_FRAMES_PER_BURST];
        coded[..HALF_PAYLOAD_BITS].copy_from_slice(&self.coded[..HALF_PAYLOAD_BITS]);
        coded[HALF_PAYLOAD_BITS..].copy_from_slice(&self.coded[156..BURST_BITS]);
        let frames = payload.as_chunks::<VOCODER_FRAME_BITS>().0;
        let soft = coded.as_chunks::<VOCODER_FRAME_BITS>().0;
        for (position, (frame, soft)) in frames.iter().zip(soft).enumerate() {
            let state = &mut self.slots[index];
            let mut decoded = half_soft_frame(frame);
            let playable = match state.encrypted {
                Some(false) => true,
                Some(true) => state.privacy.decipher(
                    burst,
                    position,
                    &mut decoded,
                    late_entry_fragment(soft),
                    &self.keys,
                ),
                None => false,
            };
            if playable {
                state.voice.play_half(&decoded, &mut Speaker(out));
            } else {
                state.voice.mute(&mut Speaker(out));
            }
        }
    }

    fn emit(&self, frame: &DvFrame, out: &mut ChannelOutputs) {
        if frame
            .slot
            .is_none_or(|slot| self.params.slots.accepts(slot))
        {
            out.events.push(DecoderEvent::of(frame));
        }
    }

    fn data_burst(&mut self, index: usize, slot: Option<u8>) -> Option<DvFrame> {
        let Some((info, slot_errors)) = slot_type(&self.coded) else {
            self.reject();
            return None;
        };
        let colour = (info >> 4) as u16 & 0x0F;
        let data_type = info as u8 & 0x0F;

        if !matches!(data_type, DT_MBC_HEADER | DT_MBC_CONTINUATION) {
            self.mbc_headers[index] = None;
        }

        if data_type == DT_RATE_THREE_QUARTER_DATA || data_type == DT_RATE_ONE_DATA {
            let (mut frame, payload_errors) = if data_type == DT_RATE_THREE_QUARTER_DATA {
                let (decoded, errors) = decode_rate_three_quarter(&self.coded)?;
                (data_block(&decoded, "rate 3/4 data"), errors)
            } else {
                let mut coded = [false; RATE_ONE_CODED_BITS];
                for (slot, &bit) in coded
                    .iter_mut()
                    .zip(self.bits[0..98].iter().chain(&self.bits[166..264]))
                {
                    *slot = bit;
                }
                let decoded = decode_rate_one(&coded);
                (data_block(&decoded, "rate 1 data"), 0)
            };
            frame.slot = slot;
            frame.color_code = Some(colour);
            frame.errors_corrected = slot_errors + payload_errors;
            return Some(frame);
        }
        let mut coded = [0 as Soft; Bptc196::CODED_BITS];
        for (slot, &soft) in coded
            .iter_mut()
            .zip(self.coded[0..98].iter().chain(&self.coded[166..264]))
        {
            *slot = soft;
        }
        let Some((payload, payload_errors)) = Bptc196::decode_soft(&coded) else {
            self.reject();
            return None;
        };
        let errors = slot_errors + payload_errors;

        let mut frame = match data_type {
            DT_VOICE_LC_HEADER => self.link_control(index, &payload, VOICE_LC_HEADER_MASK)?,
            DT_TERMINATOR_WITH_LC => {
                let mut frame = self.link_control(index, &payload, TERMINATOR_LC_MASK)?;
                frame.kind = DvFrameKind::Terminator;
                frame
            }
            DT_CSBK => self.csbk(&payload, errors)?,
            DT_MBC_HEADER => self.mbc_header(index, &payload, errors)?,
            DT_MBC_CONTINUATION => self.mbc_continuation(index, &payload, errors)?,
            DT_DATA_HEADER => self.data_header(&payload, errors)?,
            DT_RATE_HALF_DATA => data_block(&payload, "rate 1/2 data"),
            DT_UNIFIED_SINGLE_BLOCK_DATA => {
                let mut frame = self.checked_block(&payload, DATA_HEADER_MASK, errors)?;
                frame.kind = DvFrameKind::Data;
                frame.opcode = Some("unified single block data".to_owned());
                frame.data = Some(hex_bits(&payload[..80]));
                frame
            }
            DT_PI_HEADER => {
                let mut frame = self.checked_block(&payload, PI_HEADER_MASK, errors)?;
                frame.kind = DvFrameKind::Header;
                frame.encrypted = Some(true);
                frame.algorithm_id = Some(field_msb(&payload, 5, 3) as u8);
                let fid = field_msb(&payload, 8, 8) as u8;
                set_dmr_vendor(&mut frame, fid);
                let algorithm = field_msb(&payload, 5, 3) as u8;
                let key_id = field_msb(&payload, 16, 8) as u8;
                let mi = field_msb(&payload, 24, 32);
                self.slots[index]
                    .privacy
                    .pi_header(algorithm, key_id, mi, &self.keys);
                frame.key_id = Some(u16::from(key_id));
                frame.message_indicator = Some(format!("{mi:08X}"));
                frame.destination = Some(field_msb(&payload, 56, 24));
                frame
            }
            _ => return None,
        };
        frame.slot = frame.slot.or(slot);
        frame.color_code = Some(colour);
        frame.errors_corrected = errors;
        Some(frame)
    }

    fn checked_block(&mut self, payload: &[bool; 96], mask: u16, errors: u32) -> Option<DvFrame> {
        pack_msb_into(&payload[..80], &mut self.bytes);
        let expected = dmr_crc16(&self.bytes) ^ mask;
        let verified = expected == field_msb(payload, 80, 16) as u16;
        if !self.keep_unverified(verified, errors, MAX_UNVERIFIED_REPAIRS) {
            self.reject();
            return None;
        }
        let mut frame = dv_frame(DvFrameKind::Control);
        frame.crc_verified = Some(verified);
        Some(frame)
    }

    /// Decides whether a control block whose checksum did not come out is still worth reading, and
    /// remembers what the site has been doing so the next one can be judged on that history.
    fn keep_unverified(&mut self, verified: bool, errors: u32, repairs: u32) -> bool {
        if verified {
            self.masked_sums = 0;
            return true;
        }
        if errors == 0 {
            self.masked_sums = self.masked_sums.saturating_add(1);
        }
        if self.params.ignore_crc && errors <= repairs {
            return true;
        }
        errors == 0 && self.masked_sums >= MASKED_SUM_RUN
    }

    fn link_control(
        &mut self,
        index: usize,
        payload: &[bool; 96],
        mask: [u8; 3],
    ) -> Option<DvFrame> {
        pack_msb_into(payload, &mut self.bytes);
        let (lc, parity) = self.bytes.split_at(LC_BITS / 8);
        let mut received = [parity[0], parity[1], parity[2]];
        for (byte, m) in received.iter_mut().zip(mask) {
            *byte ^= m;
        }
        if rs129_parity(lc) != received {
            self.reject();
            return None;
        }
        let mut frame = self.decode_lc(index, payload);
        frame.crc_verified = Some(true);
        Some(frame)
    }

    fn csbk(&mut self, payload: &[bool; 96], errors: u32) -> Option<DvFrame> {
        let mut frame = self.checked_block(payload, CSBK_MASK, errors)?;
        let opcode = field_msb(payload, 2, 6) as u8;
        let fid = field_msb(payload, 8, 8) as u8;
        set_dmr_vendor(&mut frame, fid);
        frame.opcode = Some(csbk_opcode_name(fid, opcode, self.tier_three));
        if fid != 0 {
            frame.data = Some(hex_bits(&payload[16..80]));
        }
        if fid == 0 && matches!(opcode, 4 | 5 | 46 | 47 | 48..=56 | 61) {
            frame.destination = Some(field_msb(payload, 32, 24));
            frame.source = Some(field_msb(payload, 56, 24));
        } else if fid == 0 && opcode == 38 {
            frame.source = Some(field_msb(payload, 32, 24));
            frame.destination = Some(field_msb(payload, 56, 24));
        }
        match (fid, opcode) {
            (0, 49 | 50 | 52 | 56) => frame.group_call = Some(true),
            (0, 4 | 5 | 48 | 51 | 53 | 54 | 55) => frame.group_call = Some(false),
            _ => {}
        }
        decode_vendor_csbk(&mut frame, fid, opcode, payload);
        decode_tier_three_csbk(&mut frame, fid, opcode, payload, self.tier_three);
        self.tier_three |= frame.trunk_protocol.as_deref() == Some(TIER_THREE.id);
        Some(frame)
    }

    fn mbc_header(&mut self, index: usize, payload: &[bool; 96], errors: u32) -> Option<DvFrame> {
        let mut frame = self.checked_block(payload, MBC_HEADER_MASK, errors)?;
        let opcode = field_msb(payload, 2, 6) as u8;
        let fid = field_msb(payload, 8, 8) as u8;
        set_dmr_vendor(&mut frame, fid);
        frame.opcode = Some(format!(
            "{} MBC header",
            csbk_opcode_name(fid, opcode, self.tier_three)
        ));
        decode_tier_three_csbk(&mut frame, fid, opcode, payload, self.tier_three);
        if fid == 0 && opcode == 0b101000 {
            let announcement = field_msb(payload, 16, 5) as u8;
            if announcement == 0b00101 {
                frame.channel = Some(field_msb(payload, 68, 12) as u16);
                frame.opcode = Some("broadcast channel frequency MBC header".to_owned());
            }
        }
        self.mbc_headers[index] = Some(MbcHeader {
            fid,
            opcode,
            frame: frame.clone(),
        });
        Some(frame)
    }

    fn mbc_continuation(
        &mut self,
        index: usize,
        payload: &[bool; 96],
        errors: u32,
    ) -> Option<DvFrame> {
        let header = self.mbc_headers[index].take()?;
        let opcode = field_msb(payload, 2, 6) as u8;
        let verified = valid_mbc_crc(payload);
        if opcode != header.opcode || !payload[0] || !self.keep_unverified(verified, errors, 0) {
            return None;
        }
        let mut frame = header.frame;
        frame.crc_verified = Some(verified && frame.crc_verified == Some(true));
        let color_code = is_tier_three_grant(opcode).then(|| field_msb(payload, 12, 4) as u8);
        frame.channel_definition = decode_channel_definition(payload, color_code);
        if let Some(definition) = frame.channel_definition.clone() {
            frame.channel = Some(definition.channel);
            TIER_THREE.mark(&mut frame);
            frame.data = Some(format!(
                "TX {} Hz, RX {} Hz",
                definition.tx_hz, definition.rx_hz
            ));
        }
        frame.opcode = Some(format!(
            "{} absolute parameters",
            csbk_opcode_name(header.fid, opcode, self.tier_three)
        ));
        Some(frame)
    }

    fn data_header(&mut self, payload: &[bool; 96], errors: u32) -> Option<DvFrame> {
        let mut frame = self.checked_block(payload, DATA_HEADER_MASK, errors)?;
        let format = field_msb(payload, 4, 4) as u8;
        frame.kind = DvFrameKind::Data;
        frame.group_call = Some(payload[0]);
        frame.destination = Some(field_msb(payload, 16, 24));
        frame.source = Some(field_msb(payload, 40, 24));
        frame.opcode = Some(data_format_name(format).to_owned());
        frame.data = Some(format!(
            "SAP {:X}, {} block(s)",
            field_msb(payload, 8, 4),
            field_msb(payload, 65, 7)
        ));
        Some(frame)
    }

    fn decode_lc(&mut self, index: usize, lc: &[bool]) -> DvFrame {
        let flco = field_msb(lc, 2, 6) as u8;
        let fid = field_msb(lc, 8, 8) as u8;
        let mut frame = dv_frame(DvFrameKind::Header);
        set_dmr_vendor(&mut frame, fid);
        match (fid, flco) {
            (0 | 0x06, 0 | 3) | (0x10, 0) => {
                frame.group_call = Some(flco == 0);
                frame.destination = Some(field_msb(lc, 24, 24));
                frame.source = Some(field_msb(lc, 48, 24));
                frame.encrypted = Some(lc[17]);
                frame.emergency = Some(lc[16]);
                if fid != 0 {
                    frame.opcode = Some(format!(
                        "{} voice channel user",
                        frame.vendor.map_or("vendor", Vendor::label)
                    ));
                }
            }
            (0, 4..=7) => {
                frame.opcode = Some(talker_alias_opcode(flco).to_owned());
                frame.talker_alias = self.slots[index].talker_alias.update(flco, lc);
            }
            (0x68, 4..=7) => {
                frame.opcode = Some(format!("Hytera XPT {}", talker_alias_opcode(flco)));
                frame.talker_alias = self.slots[index].talker_alias.update(flco, lc);
            }
            (0x10, 0x14..=0x17) => {
                let alias_flco = flco - 0x10;
                frame.opcode = Some(format!("Motorola {}", talker_alias_opcode(alias_flco)));
                frame.talker_alias = self.slots[index].talker_alias.update(alias_flco, lc);
            }
            (0, 8) => {
                frame.opcode = Some("GPS Info".to_owned());
                decode_gps_info(&mut frame, lc);
            }
            (0x10, 4 | 7) => {
                frame.opcode = Some("Capacity Plus voice channel user".to_owned());
                CAPACITY_PLUS.mark(&mut frame);
                frame.group_call = Some(flco == 4);
                frame.destination = Some(field_msb(lc, 24, 24));
                frame.source = Some(field_msb(lc, 56, 16));
                frame.rest_channel = Some(field_msb(lc, 52, 4) as u16);
            }
            (0x68, 0 | 3 | 9) => {
                HYTERA_XPT.mark(&mut frame);
                frame.opcode = Some(
                    if flco == 9 {
                        "Hytera XPT call alert"
                    } else {
                        "Hytera XPT voice channel user"
                    }
                    .to_owned(),
                );
                frame.group_call = Some(lc[1]);
                frame.destination = Some(field_msb(lc, 32, 16));
                frame.source = Some(field_msb(lc, 56, 16));
                frame.channel = Some(field_msb(lc, 16, 4) as u16);
                if flco == 9 {
                    frame.data = Some(format!(
                        "free LCN {}, handshake {}",
                        field_msb(lc, 24, 4),
                        field_msb(lc, 28, 4)
                    ));
                }
            }
            _ => {
                let vendor = frame.vendor.map_or("vendor", Vendor::label);
                frame.opcode = Some(format!("{vendor} FLCO {flco}, unparsed"));
                frame.data = Some(hex_bits(&lc[16..72]));
            }
        }
        frame
    }

    fn embedded_signalling(&mut self, index: usize, slot: Option<u8>) -> Option<DvFrame> {
        let Some((info, errors)) = embedded_signalling_type(&self.coded) else {
            self.drop_embedded(index);
            self.reject();
            return None;
        };
        let colour = (info >> 3) as u16 & 0x0F;
        let lcss = info & 0b11;

        match lcss {
            LCSS_FIRST => self.drop_embedded(index),
            LCSS_LAST | LCSS_CONTINUATION if !self.slots[index].embedded.is_empty() => {}
            _ => return None,
        }
        self.slots[index]
            .embedded
            .extend(self.coded[116..148].iter().copied());
        self.slots[index].embedded_errors += errors;

        if lcss != LCSS_LAST {
            if self.slots[index].embedded.len() >= Bptc128::CODED_BITS {
                self.drop_embedded(index);
                self.reject();
            }
            return None;
        }
        let assembled: Option<[Soft; Bptc128::CODED_BITS]> =
            self.slots[index].embedded.as_slice().try_into().ok();
        let fragment_errors = self.slots[index].embedded_errors;
        self.drop_embedded(index);
        let Some((data, bptc_errors)) = assembled.and_then(|coded| Bptc128::decode_soft(&coded))
        else {
            self.reject();
            return None;
        };

        let mut lc = [false; LC_BITS];
        let mut checksum = 0u32;
        let mut written = 0;
        for (i, &bit) in data.iter().enumerate() {
            if i >= 22 && i % 11 == 10 {
                checksum = checksum << 1 | u32::from(bit);
            } else {
                lc[written] = bit;
                written += 1;
            }
        }
        pack_msb_into(&lc, &mut self.bytes);
        let sum = self.bytes.iter().map(|&b| u32::from(b)).sum::<u32>() % 31;
        if sum != checksum {
            self.reject();
            return None;
        }
        let mut frame = self.decode_lc(index, &lc);
        frame.crc_verified = Some(true);
        frame.kind = DvFrameKind::Voice;
        frame.slot = slot;
        frame.color_code = Some(colour);
        frame.errors_corrected = fragment_errors + bptc_errors;
        Some(frame)
    }

    fn drop_embedded(&mut self, index: usize) {
        self.slots[index].embedded.clear();
        self.slots[index].embedded_errors = 0;
    }
}

fn slot_index(slot: Option<u8>) -> Option<usize> {
    slot.filter(|slot| (1..=2).contains(slot))
        .map(|slot| usize::from(slot - 1))
}

/// The slot type: Golay (20,8) split around the sync, decoded soft.
fn slot_type(coded: &[Soft]) -> Option<(u32, u32)> {
    CyclicCode::GOLAY_20_8.decode_soft(&split_word::<20>(coded, 98, 156)?)
}

/// The EMB: QR (16,7) split around the embedded signalling, decoded soft.
fn embedded_signalling_type(coded: &[Soft]) -> Option<(u32, u32)> {
    CyclicCode::QR_16_7.decode_soft(&split_word::<16>(coded, 108, 148)?)
}

/// A codeword sent in two halves, at `first` and `second` of a burst.
fn split_word<const N: usize>(coded: &[Soft], first: usize, second: usize) -> Option<[Soft; N]> {
    let half = N / 2;
    let mut word = [0; N];
    word[..half].copy_from_slice(coded.get(first..first + half)?);
    word[half..].copy_from_slice(coded.get(second..second + half)?);
    Some(word)
}

/// The slot and LCSS a CACH's Hamming (7,4) TACT names, and its short LC fragment.
fn read_cach(soft: &[Soft]) -> Option<(u8, u8, [Soft; 17])> {
    const TACT: [usize; 7] = [0, 4, 8, 12, 14, 18, 22];
    const PAYLOAD: [usize; 17] = [1, 2, 3, 5, 6, 7, 9, 10, 11, 13, 15, 16, 17, 19, 20, 21, 23];
    let soft: &[Soft; CACH_BITS] = soft.get(..CACH_BITS)?.try_into().ok()?;
    let mut tact = [false; 7];
    ParityCode::HAMMING_7_4.decode_soft(&TACT.map(|position| soft[position]), &mut tact)?;
    let slot = u8::from(tact[1]) + 1;
    let lcss = u8::from(tact[2]) << 1 | u8::from(tact[3]);
    Some((slot, lcss, PAYLOAD.map(|position| soft[position])))
}

#[derive(Default)]
struct ShortLc {
    payload: Vec<Soft>,
    rejected: u32,
}

impl ShortLc {
    fn reset(&mut self) {
        self.payload.clear();
    }

    fn rejected(&self) -> u32 {
        self.rejected
    }

    fn reject(&mut self) {
        self.rejected = self.rejected.saturating_add(1);
    }

    fn push(&mut self, lcss: u8, payload: [Soft; 17]) -> Option<DvFrame> {
        match u32::from(lcss) {
            LCSS_FIRST => self.payload.clear(),
            LCSS_CONTINUATION | LCSS_LAST if !self.payload.is_empty() => {}
            _ => return None,
        }
        self.payload.extend(payload);
        if u32::from(lcss) != LCSS_LAST {
            if self.payload.len() >= SHORT_LC_BITS {
                self.payload.clear();
                self.reject();
            }
            return None;
        }
        let assembled: Option<[Soft; SHORT_LC_BITS]> = self.payload.as_slice().try_into().ok();
        self.payload.clear();
        let frame = assembled.as_ref().and_then(short_lc_message);
        if frame.is_none() {
            self.reject();
        }
        frame
    }
}

/// Every row, the column parity row too, is a Hamming (17,12) codeword, so all four decode soft.
fn short_lc_message(payload: &[Soft; SHORT_LC_BITS]) -> Option<DvFrame> {
    let mut matrix = [[false; 17]; 4];
    for (row, cells) in matrix.iter_mut().enumerate() {
        let received: [Soft; 17] = std::array::from_fn(|column| payload[column * 4 + row]);
        ParityCode::HAMMING_17_12.decode_soft(&received, cells)?;
    }
    for column in 0..17 {
        if matrix
            .iter()
            .fold(false, |parity, row| parity ^ row[column])
        {
            return None;
        }
    }
    let mut message = [false; 36];
    for (target, bit) in message
        .iter_mut()
        .zip(matrix[..3].iter().flat_map(|row| &row[..12]))
    {
        *target = *bit;
    }
    if crc8_dmr(&message[..28]) != field_msb(&message, 28, 8) as u8 {
        return None;
    }
    Some(decode_short_lc(&message[..28]))
}

fn crc8_dmr(bits: &[bool]) -> u8 {
    crc_msb_bits(0x07, 8, 0, bits.iter().copied()) as u8
}

fn decode_short_lc(bits: &[bool]) -> DvFrame {
    let slco = field_msb(bits, 0, 4) as u8;
    let mut frame = dv_frame(DvFrameKind::Control);
    frame.vendor = Some(Vendor::Etsi);
    frame.manufacturer_id = Some(0);
    match slco {
        0 => frame.opcode = Some("Short LC null message".to_owned()),
        1 => {
            frame.opcode = Some("Short LC activity update".to_owned());
            for (slot, activity_at, hash_at) in [(1, 4, 12), (2, 8, 20)] {
                let activity = field_msb(bits, activity_at, 4) as u8;
                if activity != 0 {
                    frame.slot_activity.push(DvSlotActivity {
                        slot,
                        activity: activity_name(activity).to_owned(),
                        destination_hash: Some(field_msb(bits, hash_at, 8) as u8),
                        destination: None,
                        logical_channel: None,
                    });
                }
            }
        }
        2 | 3 => {
            frame.opcode = Some(if slco == 2 {
                "Tier III control-channel system parameters".to_owned()
            } else {
                "Tier III payload-channel system parameters".to_owned()
            });
            TIER_THREE.mark(&mut frame);
            frame.control_channel = Some(slco == 2);
            frame.system_id = Some(field_msb(bits, 6, 12) as u16);
            frame.data = Some(format!(
                "network model {}, common slot counter {}",
                field_msb(bits, 4, 2),
                field_msb(bits, 19, 9)
            ));
        }
        8 => {
            frame.opcode = Some("Hytera XPT Short LC".to_owned());
            HYTERA_XPT.mark(&mut frame);
            frame.vendor = Some(Vendor::Hytera);
            frame.manufacturer_id = Some(0x68);
            frame.channel = Some(field_msb(bits, 12, 4) as u16);
            frame.data = Some(format!(
                "priority LCN {}, priority hash {:02X}",
                field_msb(bits, 16, 4),
                field_msb(bits, 20, 8)
            ));
        }
        9 | 10 => {
            frame.opcode = Some(if slco == 9 {
                "Connect Plus traffic-channel Short LC".to_owned()
            } else {
                "Connect Plus control-channel Short LC".to_owned()
            });
            frame.vendor = Some(Vendor::Motorola);
            frame.manufacturer_id = Some(0x06);
            frame.network_id = Some(field_msb(bits, 8, 8));
            frame.site_id = Some(field_msb(bits, 16, 8) as u16);
        }
        15 => {
            frame.opcode = Some("Capacity Plus site Short LC".to_owned());
            frame.vendor = Some(Vendor::Motorola);
            frame.manufacturer_id = Some(0x10);
            frame.site_id = Some(field_msb(bits, 22, 3) as u16);
            frame.rest_channel = Some(field_msb(bits, 16, 4) as u16);
        }
        _ => frame.opcode = Some(format!("Short LC {slco:X}, unparsed")),
    }
    frame
}

fn activity_name(activity: u8) -> &'static str {
    match activity {
        2 => "group CSBK",
        3 => "individual CSBK",
        8 => "group voice",
        9 => "individual voice",
        10 => "individual data",
        11 => "group data",
        12 => "emergency group voice",
        13 => "emergency individual voice",
        _ => "reserved activity",
    }
}

fn csbk_opcode_name(fid: u8, opcode: u8, tier_three: bool) -> String {
    if fid == 0 && opcode == 56 {
        return if tier_three {
            "talkgroup data channel grant, multiple items".to_owned()
        } else {
            "BS outbound activation".to_owned()
        };
    }
    let name = match (fid, opcode) {
        (0, 3) => "feature not supported",
        (0, 4) => "unit-to-unit voice service request",
        (0, 5) => "unit-to-unit voice service answer response",
        (0, 7) => "channel timing",
        (0, 25) => "ALOHA",
        (0, 26) => "unified data transport outbound header",
        (0, 27) => "unified data transport inbound header",
        (0, 28) => "AHOY",
        (0, 30) => "activation",
        (0, 31) => "random access service request",
        (0, 32) => "acknowledge response, outbound control channel",
        (0, 33) => "acknowledge response, inbound control channel",
        (0, 34) => "acknowledge response, outbound traffic channel",
        (0, 35) => "acknowledge response, inbound traffic channel",
        (0, 36) => "unified data transport for DGNA, outbound header",
        (0, 37) => "unified data transport for DGNA, inbound header",
        (0, 38) => "negative acknowledge response",
        (0, 40) => "broadcast",
        (0, 42) => "maintenance",
        (0, 46) => "clear",
        (0, 47) => "protect",
        (0, 48) => "private voice channel grant",
        (0, 49) => "talkgroup voice channel grant",
        (0, 50) => "broadcast talkgroup voice channel grant",
        (0, 51) => "private data channel grant, single item",
        (0, 52) => "talkgroup data channel grant, single item",
        (0, 53) => "duplex private voice channel grant",
        (0, 54) => "duplex private data channel grant",
        (0, 55) => "private data channel grant, multiple items",
        (0, 57) => "move TSCC",
        (0, 61) => "preamble",
        (0x10, 25) => "Capacity Max ALOHA",
        (0x10, 33) => "Capacity Max voice channel update, open mode",
        (0x10, 34) => "Capacity Max voice channel update, advantage mode",
        (0x10, 0x3A) => "Capacity Plus system CSBK",
        (0x10, 0x3B) => "Capacity Plus adjacent sites",
        (0x10, 0x3E) => "Capacity Plus channel status",
        (0x06, 0x01) => "Connect Plus adjacent sites",
        (0x06, 0x03) => "Connect Plus voice channel grant",
        (0x06, 0x06) => "Connect Plus data channel grant",
        (0x06, 0x0C) => "Connect Plus slot termination",
        (0x06, _) => "Connect Plus CSBK, unparsed",
        (0x68, 0x0A) => "Hytera XPT site status",
        (0x68, 0x0B) => "Hytera XPT adjacent sites",
        (0x08 | 0x68, _) => "Hytera CSBK, unparsed",
        _ => return format!("{} CSBK {opcode:02X}, unparsed", dmr_vendor(fid).label()),
    };
    name.to_owned()
}

fn dmr_vendor(fid: u8) -> Vendor {
    match fid {
        0 => Vendor::Etsi,
        0x06 | 0x10 => Vendor::Motorola,
        0x04 => Vendor::FlydeMicro,
        0x05 => Vendor::ProdEl,
        0x08 | 0x68 | 0x88 => Vendor::Hytera,
        0x0D | 0x13 | 0x1C | 0x20 => Vendor::Emc,
        0x33 => Vendor::JvcKenwood,
        0x3C => Vendor::RadioActivity,
        0x58 => Vendor::Tait,
        _ => Vendor::Unknown,
    }
}

fn set_dmr_vendor(frame: &mut DvFrame, fid: u8) {
    frame.vendor = Some(dmr_vendor(fid));
    frame.manufacturer_id = Some(fid);
}

fn decode_vendor_csbk(frame: &mut DvFrame, fid: u8, opcode: u8, payload: &[bool]) {
    if fid == 0x10 && matches!(opcode, 0x3A | 0x3B | 0x3E) {
        CAPACITY_PLUS.mark(frame);
    } else if fid == 0x68 && matches!(opcode, 0x0A | 0x0B) {
        HYTERA_XPT.mark(frame);
    } else if fid == 0x10 && matches!(opcode, 25 | 33 | 34) {
        TIER_THREE.mark(frame);
    }
    match (fid, opcode) {
        (0x10, 25) => {
            set_system_identity(frame, payload, 40);
            frame.destination = Some(field_msb(payload, 56, 24));
            frame.data = Some(format!(
                "version {}, mask {}, service {}, wait {}, registration {}, backoff {}",
                field_msb(payload, 19, 3),
                field_msb(payload, 24, 5),
                field_msb(payload, 29, 2),
                field_msb(payload, 31, 4),
                u8::from(payload[35]),
                field_msb(payload, 36, 4)
            ));
        }
        (0x10, 33 | 34) => decode_capacity_max_update(frame, opcode, payload),
        (0x10, 0x3A | 0x3E) => {
            frame.rest_channel = Some(field_msb(payload, 20, 4) as u16);
            if opcode == 0x3E {
                decode_capacity_plus_status(frame, payload);
            }
        }
        (0x10, 0x3B) => {
            let sites = (0..6)
                .filter_map(|index| {
                    let site = field_msb(payload, 32 + index * 8, 4);
                    (site != 0).then(|| {
                        format!("site {site} rest {}", field_msb(payload, 36 + index * 8, 4))
                    })
                })
                .collect::<Vec<_>>();
            frame.data = Some(if sites.is_empty() {
                "no adjacent sites".to_owned()
            } else {
                sites.join(", ")
            });
        }
        (0x06, 0x01) => {
            let sites = (0..5)
                .filter_map(|index| {
                    let site = field_msb(payload, 16 + index * 8, 8) & 0x3F;
                    (site != 0).then_some(site.to_string())
                })
                .collect::<Vec<_>>();
            frame.data = Some(format!("adjacent sites {}", sites.join(", ")));
        }
        (0x06, 0x03) => {
            let option = field_msb(payload, 72, 8) as u8;
            frame.source = Some(field_msb(payload, 16, 24));
            frame.destination = Some(field_msb(payload, 40, 24));
            frame.channel = Some(field_msb(payload, 64, 4) as u16);
            frame.group_call = match option {
                2 => Some(true),
                3 => Some(false),
                _ => None,
            };
            frame.data = Some(format!(
                "granted TS {}, option {option:02X}",
                u8::from(payload[68]) + 1
            ));
        }
        (0x06, 0x06) => {
            frame.destination = Some(field_msb(payload, 16, 24));
            frame.channel = Some(field_msb(payload, 40, 4) as u16);
            frame.data = Some(format!("granted TS {}", u8::from(payload[44]) + 1));
        }
        (0x68, 0x0A) => {
            frame.channel = Some(field_msb(payload, 16, 4) as u16);
            frame.data = Some(format!(
                "sequence {}, slot states {:03X}",
                field_msb(payload, 0, 2),
                field_msb(payload, 20, 12)
            ));
        }
        _ => {}
    }
}

/// The repeater says which of its own two timeslots are carrying a call and who the call is for.
/// The address is one field for both slots, so it is only attributed while a single slot is busy.
fn decode_capacity_plus_status(frame: &mut DvFrame, payload: &[bool]) {
    let busy: Vec<u8> = [(1, 26), (2, 27)]
        .into_iter()
        .filter(|(_, at)| payload[*at])
        .map(|(slot, _)| slot)
        .collect();
    let destination = match field_msb(payload, 28, 12) {
        0 => None,
        address if busy.len() == 1 => Some(address),
        _ => None,
    };
    for slot in &busy {
        frame.slot_activity.push(DvSlotActivity {
            slot: *slot,
            activity: "group voice".to_owned(),
            destination_hash: None,
            destination,
            logical_channel: None,
        });
    }
    frame.data = Some(match (busy.as_slice(), destination) {
        ([], _) => "no call".to_owned(),
        ([slot], Some(destination)) => format!("TS{slot} carrying TG {destination}"),
        (slots, _) => slots
            .iter()
            .map(|slot| format!("TS{slot} busy"))
            .collect::<Vec<_>>()
            .join(", "),
    });
}

/// Advantage mode packs its two shortened talkgroups straight after the channel number rather
/// than starting them on the next byte.
fn advantage_mode_talkgroups(payload: &[bool]) -> [u32; 2] {
    [field_msb(payload, 28, 10), field_msb(payload, 38, 10)]
}

fn decode_capacity_max_update(frame: &mut DvFrame, opcode: u8, payload: &[bool]) {
    let channel = field_msb(payload, 16, 12) as u16;
    frame.channel = Some(channel);
    let talkgroups = if opcode == 33 {
        [field_msb(payload, 32, 24), field_msb(payload, 56, 24)]
    } else {
        advantage_mode_talkgroups(payload)
    };
    for (index, destination) in talkgroups.into_iter().enumerate() {
        if destination == 0 {
            continue;
        }
        frame.slot_activity.push(DvSlotActivity {
            slot: index as u8 + 1,
            activity: "group voice".to_owned(),
            destination_hash: None,
            destination: Some(destination),
            logical_channel: Some(channel),
        });
    }
    frame.data = Some(format!(
        "channel {channel}, TS1 {}, TS2 {}",
        talkgroups[0], talkgroups[1]
    ));
}

fn is_tier_three_grant(opcode: u8) -> bool {
    matches!(opcode, 48..=56)
}

fn is_tier_three_voice_grant(opcode: u8) -> bool {
    matches!(opcode, 48 | 49 | 50 | 53)
}

fn tier_three_opcode(opcode: u8) -> bool {
    is_tier_three_grant(opcode) || matches!(opcode, 25 | 28 | 31 | 32 | 33 | 40 | 42 | 57)
}

fn decode_tier_three_csbk(
    frame: &mut DvFrame,
    fid: u8,
    opcode: u8,
    payload: &[bool],
    tier_three: bool,
) {
    if fid != 0 || (opcode == 56 && !tier_three) {
        return;
    }
    if tier_three_opcode(opcode) {
        TIER_THREE.mark(frame);
    }
    match opcode {
        _ if is_tier_three_grant(opcode) => {
            frame.channel = Some(field_msb(payload, 16, 12) as u16);
            frame.slot = Some(if payload[28] { 2 } else { 1 });
            frame.late_entry = Some(payload[29]);
            frame.emergency = Some(payload[30]);
            if !is_tier_three_voice_grant(opcode) {
                frame.data = Some("data call".to_owned());
            }
        }
        25 => {
            set_system_identity(frame, payload, 40);
            frame.destination = Some(field_msb(payload, 56, 24));
            frame.data = Some(format!(
                "version {}, mask {}, service {}, wait {}, registration {}, backoff {}",
                field_msb(payload, 19, 3),
                field_msb(payload, 24, 5),
                field_msb(payload, 29, 2),
                field_msb(payload, 31, 4),
                u8::from(payload[35]),
                field_msb(payload, 36, 4)
            ));
        }
        28 => decode_ahoy(frame, payload),
        31 => {
            frame.source = Some(field_msb(payload, 56, 24));
            frame.data = Some(format!(
                "service {}, target {}",
                field_msb(payload, 21, 6),
                field_msb(payload, 32, 24)
            ));
        }
        32..=35 => decode_acknowledge(frame, payload),
        40 => decode_announcement(frame, payload),
        57 => {
            frame.channel = Some(field_msb(payload, 44, 12) as u16);
            frame.destination = Some(field_msb(payload, 56, 24));
            frame.data = Some(format!(
                "move to TSCC on channel {}, mask {}, registration {}, backoff {}",
                field_msb(payload, 44, 12),
                field_msb(payload, 25, 5),
                u8::from(payload[35]),
                field_msb(payload, 36, 4)
            ));
        }
        _ => {}
    }
}

fn set_system_identity(frame: &mut DvFrame, payload: &[bool], at: usize) {
    let model = field_msb(payload, at, 2);
    let (net_bits, site_bits) = match model {
        0 => (9, 3),
        1 => (7, 5),
        2 => (4, 8),
        _ => (2, 10),
    };
    frame.system_id = Some(field_msb(payload, at, 16) as u16);
    frame.network_id = Some(field_msb(payload, at + 2, net_bits));
    frame.site_id = Some(field_msb(payload, at + 2 + net_bits, site_bits) as u16);
}

fn service_kind_name(service: u8) -> &'static str {
    match service {
        0 => "individual voice call",
        1 => "talkgroup voice call",
        2 => "individual packet call",
        3 => "talkgroup packet call",
        4 => "individual short data call",
        5 => "talkgroup short data call",
        6 => "short data polling",
        7 => "status transport",
        8 => "call diversion",
        9 => "call answer",
        10 => "duplex radio-to-radio voice call",
        11 => "duplex radio-to-radio packet call",
        13 => "supplementary service",
        14 => "registration or radio check",
        15 => "cancel call",
        _ => "reserved service",
    }
}

fn decode_ahoy(frame: &mut DvFrame, payload: &[bool]) {
    let service = field_msb(payload, 28, 4) as u8;
    frame.group_call = Some(payload[25]);
    frame.destination = Some(field_msb(payload, 32, 24));
    frame.source = Some(field_msb(payload, 56, 24));
    frame.encrypted = Some(payload[17]);
    frame.opcode = Some(format!("AHOY, {}", service_kind_name(service)));
    frame.data = Some(format!(
        "service {service}, options {:02X}, blocks {}",
        field_msb(payload, 16, 7),
        field_msb(payload, 26, 2)
    ));
}

fn decode_acknowledge(frame: &mut DvFrame, payload: &[bool]) {
    let response = field_msb(payload, 16, 7) as u8;
    let reason = field_msb(payload, 23, 8) as u8;
    let name = match response {
        0 => "acknowledged",
        1 => "queued",
        2 => "registration accepted",
        3 => "refused",
        _ => "response",
    };
    frame.group_call = Some(payload[16]);
    frame.destination = Some(field_msb(payload, 32, 24));
    frame.source = Some(field_msb(payload, 56, 24));
    frame.opcode = Some(format!("acknowledge response, {name}"));
    frame.data = Some(format!("response {response}, reason {reason:02X}"));
}

fn announcement_name(announcement: u8) -> &'static str {
    match announcement {
        0 => "announce or withdraw TSCC",
        1 => "call timer parameters",
        2 => "vote now advice",
        3 => "local time",
        4 => "mass registration",
        5 => "channel frequency",
        6 => "adjacent site",
        7 => "site information",
        30 | 31 => "vendor specific announcement",
        _ => "reserved announcement",
    }
}

fn decode_announcement(frame: &mut DvFrame, payload: &[bool]) {
    let announcement = field_msb(payload, 16, 5) as u8;
    frame.opcode = Some(format!("broadcast, {}", announcement_name(announcement)));
    match announcement {
        0 => {
            frame.channel = Some(field_msb(payload, 56, 12) as u16);
            set_system_identity(frame, payload, 40);
            frame.data = Some(format!(
                "TSCC on channel {} colour {}, second channel {} colour {}",
                field_msb(payload, 56, 12),
                field_msb(payload, 25, 4),
                field_msb(payload, 68, 12),
                field_msb(payload, 29, 4)
            ));
        }
        2 => {
            set_system_identity(frame, payload, 21);
            frame.channel = Some(field_msb(payload, 68, 12) as u16);
            frame.data = Some(format!(
                "vote for channel {}, priority {}/{}",
                field_msb(payload, 68, 12),
                field_msb(payload, 58, 3),
                field_msb(payload, 61, 3)
            ));
        }
        4 => {
            set_system_identity(frame, payload, 40);
            frame.destination = Some(field_msb(payload, 56, 24));
            frame.data = Some(format!("mask {}", field_msb(payload, 21, 14)));
        }
        6 => {
            set_system_identity(frame, payload, 21);
            frame.channel = Some(field_msb(payload, 68, 12) as u16);
            frame.data = Some(format!(
                "neighbour TSCC on channel {}, network connection {}, priority {}/{}",
                field_msb(payload, 68, 12),
                u8::from(payload[57]),
                field_msb(payload, 58, 3),
                field_msb(payload, 61, 3)
            ));
        }
        7 => {
            set_system_identity(frame, payload, 40);
            frame.data = Some(format!(
                "site parameters {:04X}",
                field_msb(payload, 21, 14)
            ));
        }
        _ => set_system_identity(frame, payload, 40),
    }
}

fn valid_mbc_crc(payload: &[bool; 96]) -> bool {
    let mut bytes = [0u8; 10];
    pack_msb_slice(&payload[..80], &mut bytes);
    dmr_crc16(&bytes) == field_msb(payload, 80, 16) as u16
}

fn decode_channel_definition(
    payload: &[bool; 96],
    color_code: Option<u8>,
) -> Option<DvChannelDefinition> {
    if field_msb(payload, 16, 4) != 0 || payload[20] || payload[21] {
        return None;
    }
    let channel = field_msb(payload, 22, 12) as u16;
    let tx_mhz = u64::from(field_msb(payload, 34, 10));
    let tx_fraction = u64::from(field_msb(payload, 44, 13));
    let rx_mhz = u64::from(field_msb(payload, 57, 10));
    let rx_fraction = u64::from(field_msb(payload, 67, 13));
    Some(DvChannelDefinition {
        channel,
        tx_hz: tx_mhz * 1_000_000 + tx_fraction * 125,
        rx_hz: rx_mhz * 1_000_000 + rx_fraction * 125,
        color_code,
    })
}

fn data_format_name(format: u8) -> &'static str {
    match format {
        0 => "unified data transport header",
        1 => "response header",
        2 => "unconfirmed data header",
        3 => "confirmed data header",
        13 => "defined data header",
        14 => "raw data header",
        15 => "proprietary data header",
        _ => "data header",
    }
}

fn data_block(payload: &[bool], name: &str) -> DvFrame {
    let mut frame = dv_frame(DvFrameKind::Data);
    frame.opcode = Some(name.to_owned());
    frame.data = Some(hex_bits(payload));
    frame
}

/// The rate 3/4 trellis: the next state is the tribit just sent.
pub(crate) const TRELLIS: TrellisCode<8, 8, 2> = TrellisCode::input_state(trellis_outputs());

/// Air dibits to trellis order: four columns read two dibits at a time.
pub(crate) static TRELLIS_ORDER: LazyLock<Permutation> =
    LazyLock::new(|| Permutation::columns(&[26, 24, 24, 24], 2));

const TRELLIS_NEXT: [[u8; 8]; 8] = [
    [0, 8, 4, 12, 2, 10, 6, 14],
    [4, 12, 2, 10, 6, 14, 0, 8],
    [1, 9, 5, 13, 3, 11, 7, 15],
    [5, 13, 3, 11, 7, 15, 1, 9],
    [3, 11, 7, 15, 1, 9, 5, 13],
    [7, 15, 1, 9, 5, 13, 3, 11],
    [2, 10, 6, 14, 0, 8, 4, 12],
    [6, 14, 0, 8, 4, 12, 2, 10],
];

const TRELLIS_MAP: [[u8; 2]; 16] = [
    [0, 2],
    [2, 2],
    [1, 3],
    [3, 3],
    [3, 2],
    [1, 2],
    [2, 3],
    [0, 3],
    [3, 1],
    [1, 1],
    [2, 0],
    [0, 0],
    [0, 1],
    [2, 1],
    [1, 0],
    [3, 0],
];

const fn trellis_outputs() -> [[[u8; 2]; 8]; 8] {
    let mut outputs = [[[0u8; 2]; 8]; 8];
    let mut state = 0;
    while state < 8 {
        let mut input = 0;
        while input < 8 {
            outputs[state][input] = TRELLIS_MAP[TRELLIS_NEXT[state][input] as usize];
            input += 1;
        }
        state += 1;
    }
    outputs
}

/// The 144 data bits of a rate 3/4 burst and the decisions the soft trellis overturned.
fn decode_rate_three_quarter(burst: &[Soft]) -> Option<([bool; TRELLIS_DATA_BITS], u32)> {
    let mut air = [[0 as Soft; 2]; TRELLIS_DIBITS];
    let pairs = burst
        .get(..98)?
        .as_chunks::<2>()
        .0
        .iter()
        .chain(burst.get(166..BURST_BITS)?.as_chunks::<2>().0);
    for (dibit, pair) in air.iter_mut().zip(pairs) {
        *dibit = *pair;
    }
    let mut encoded = [[0 as Soft; 2]; TRELLIS_DIBITS];
    TRELLIS_ORDER.gather(&air, &mut encoded);
    let steps: &[[[Soft; 2]; 2]; TRELLIS_STEPS] = encoded.as_chunks::<2>().0.try_into().ok()?;
    let mut tribits = [0u8; TRELLIS_STEPS];
    let errors = TRELLIS.decode_soft(steps, 0, &mut tribits);
    let mut out = [false; TRELLIS_DATA_BITS];
    for (chunk, &tribit) in out.as_chunks_mut::<3>().0.iter_mut().zip(&tribits[..48]) {
        chunk[0] = tribit & 4 != 0;
        chunk[1] = tribit & 2 != 0;
        chunk[2] = tribit & 1 != 0;
    }
    Some((out, u32::from(errors)))
}

fn decode_rate_one(coded: &[bool; RATE_ONE_CODED_BITS]) -> [bool; RATE_ONE_DATA_BITS] {
    let mut out = [false; RATE_ONE_DATA_BITS];
    out[..RATE_ONE_PAD_AT].copy_from_slice(&coded[..RATE_ONE_PAD_AT]);
    out[RATE_ONE_PAD_AT..].copy_from_slice(&coded[RATE_ONE_PAD_AT + RATE_ONE_PAD_BITS..]);
    out
}

fn talker_alias_opcode(flco: u8) -> &'static str {
    match flco {
        4 => "talker alias header",
        5 => "talker alias block 1",
        6 => "talker alias block 2",
        _ => "talker alias block 3",
    }
}

#[derive(Default)]
struct TalkerAlias {
    format: u8,
    length: usize,
    bits: Vec<bool>,
    next_block: u8,
}

impl TalkerAlias {
    fn reset(&mut self) {
        self.bits.clear();
        self.length = 0;
        self.next_block = 0;
    }

    fn update(&mut self, flco: u8, lc: &[bool]) -> Option<String> {
        if flco == 4 {
            self.format = field_msb(lc, 16, 2) as u8;
            self.length = field_msb(lc, 18, 5) as usize;
            self.bits.clear();
            let start = if self.format == 0 { 23 } else { 24 };
            self.bits.extend_from_slice(&lc[start..72]);
            self.next_block = 5;
        } else if flco == self.next_block {
            self.bits.extend_from_slice(&lc[16..72]);
            self.next_block += 1;
        } else {
            return None;
        }
        self.decode()
    }

    fn decode(&self) -> Option<String> {
        let needed = match self.format {
            0 => self.length * 7,
            1 | 2 => self.length * 8,
            3 => self.length * 16,
            _ => return None,
        };
        if self.bits.len() < needed {
            return None;
        }
        let text = match self.format {
            0 => self.bits[..needed]
                .as_chunks::<7>()
                .0
                .iter()
                .map(|bits| field_msb(bits, 0, 7) as u8 as char)
                .collect(),
            1 => self.bits[..needed]
                .as_chunks::<8>()
                .0
                .iter()
                .map(|bits| field_msb(bits, 0, 8) as u8 as char)
                .collect(),
            2 => String::from_utf8(
                self.bits[..needed]
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|bits| field_msb(bits, 0, 8) as u8)
                    .collect(),
            )
            .ok()?,
            3 => String::from_utf16(
                &self.bits[..needed]
                    .as_chunks::<16>()
                    .0
                    .iter()
                    .map(|bits| field_msb(bits, 0, 16) as u16)
                    .collect::<Vec<_>>(),
            )
            .ok()?,
            _ => return None,
        };
        Some(text.trim_end_matches('\0').to_owned())
    }
}

fn decode_gps_info(frame: &mut DvFrame, lc: &[bool]) {
    let error = field_msb(lc, 20, 3);
    let lon = signed_bits(field_msb(lc, 23, 25), 25);
    let lat = signed_bits(field_msb(lc, 48, 24), 24);
    frame.lon = Some(f64::from(lon) * 360.0 / 2f64.powi(25));
    frame.lat = Some(f64::from(lat) * 180.0 / 2f64.powi(24));
    frame.position_error_m = match error {
        0 => Some(2),
        1 => Some(20),
        2 => Some(200),
        3 => Some(2_000),
        4 => Some(20_000),
        5 => Some(200_000),
        _ => None,
    };
}

fn signed_bits(value: u32, width: u32) -> i32 {
    let shift = 32 - width;
    ((value << shift) as i32) >> shift
}

fn dmr_crc16(data: &[u8]) -> u16 {
    !crc16_msb(0x1021, 0, data)
}

/// A DMR transmission keys up one 30 ms slot of every two.
const SLOT_BURST_MS: sdrmm_wire::HzRange = sdrmm_wire::HzRange::new(25.0, 35.0);

fn dmr_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::synced(
            "DMR",
            BANDWIDTH_HZ,
            FrameSync {
                symbol_rate_hz: BAUD,
                deviation_hz: DEVIATION_HZ,
                pulse: SyncPulse::C4fm { alpha: RRC_ALPHA },
                bits: SYNC_BITS,
                tolerance: SYNC_TOLERANCE,
                patterns: SYNC_PATTERNS.to_vec(),
                min_hits: 1,
            },
        )
        .on(LAND_MOBILE)
        .burst_hint(SLOT_BURST_MS),
    ]
}

pub(crate) fn dmr_decoder() -> Native<DmrParams> {
    let decoder = Native::new::<DmrChannel>(|_| occupied_band())
        .filter(|_| Ok(channel_filter()))
        .signatures(dmr_signatures)
        .form(dmr_form)
        .kinds(dmr_kinds)
        .trunking(trunk::trunking());
    cfg_select! {
        any(test, feature = "synth") => decoder.synth(synth::SIGNALS),
        _ => decoder,
    }
}

pub(crate) fn dmr_form() -> Vec<SettingField> {
    with_vocoder(vec![
        SettingField::new("slots", "Slot", Control::choice::<DmrSlots>()).title("Slot"),
        SettingField::new("ignore_crc", "Skip CRC", Control::Toggle).title("Ignore data CRC"),
        keys_field(
            "ARC4 or AES key per key ID. Basic Privacy needs no key.",
            u16::from(u8::MAX),
            "10, 32 or 64 hex digits",
        ),
    ])
}

fn dmr_kinds() -> Vec<KindSpec> {
    vec![]
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;
