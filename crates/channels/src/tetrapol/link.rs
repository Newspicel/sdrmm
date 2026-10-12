use std::fmt::Write as _;

use sdrmm_dsp::hdlc_fcs_ok;
use sdrmm_wire::{
    DecoderEvent, TetrapolAddress, TetrapolLogicalChannel, TetrapolMessage, TetrapolPayload,
};

use super::{
    assembly::{Assembler, MAX_FRAME_BYTES, Step},
    coding::{BLOCK_BYTES, Block, Decoded},
    message,
};
use crate::ChannelOutputs;

const SUPERFRAME: u64 = 200;
const HALF_SUPERFRAME: u64 = 100;
const BCH_BLOCKS: u64 = 4;
const PCH_FRAMES: [u64; 2] = [98, 99];
const RCH_PERIOD: u64 = 25;
const RCH_OFFSET: u64 = 14;
const FRAME_S: f32 = 0.02;
const VOICE_GAP_FRAMES: u64 = 50;
const TRAFFIC_FRAMES: u64 = 500;
const REPEAT_FRAMES: u64 = 500;
const RECENT: usize = 32;
const MAX_TSDU: usize = 512;
const NO_STATION: [u8; 2] = [0x70, 0x00];
const UI_MASK: u8 = 0xEF;
const UI: u8 = 0x03;
const UI_CIRCUIT: u8 = 0xA3;
const UI_VOICE_CHANNEL: u8 = 0xC3;
const SHORT_INFO: usize = 3;
const SEGMENTED_HEADER: usize = 3;
const PCH_ADDRESSES: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    Control,
    Broadcast,
    Paging,
    Random,
}

impl Route {
    fn index(self) -> usize {
        match self {
            Self::Control | Self::Random => 0,
            Self::Broadcast => 1,
            Self::Paging => 2,
        }
    }
}

enum Du<'a> {
    Whole(&'a [u8]),
    Segment {
        reference: u8,
        packet: u8,
        last: bool,
        data: &'a [u8],
    },
}

struct Segments {
    reference: Option<u8>,
    next: u8,
    len: usize,
    buffer: [u8; MAX_TSDU],
}

impl Segments {
    fn new() -> Self {
        Self {
            reference: None,
            next: 0,
            len: 0,
            buffer: [0; MAX_TSDU],
        }
    }

    fn push(&mut self, reference: u8, packet: u8, last: bool, data: &[u8]) -> (bool, bool) {
        let mut lost = false;
        if packet == 0 {
            lost = self.reference.is_some();
            self.reference = Some(reference);
            self.next = 0;
            self.len = 0;
        }
        if self.reference != Some(reference) || self.next != packet {
            let lost = lost || self.reference.is_some();
            self.reference = None;
            return (lost || packet != 0, false);
        }
        let Some(slot) = self.buffer.get_mut(self.len..self.len + data.len()) else {
            self.reference = None;
            return (true, false);
        };
        slot.copy_from_slice(data);
        self.len += data.len();
        self.next = self.next.wrapping_add(1);
        if last {
            self.reference = None;
        }
        (lost, last)
    }

    fn tsdu(&self) -> &[u8] {
        &self.buffer[..self.len]
    }

    fn reset(&mut self) {
        self.reference = None;
    }
}

struct Recent {
    entries: [Option<(u64, u64)>; RECENT],
    next: usize,
}

impl Recent {
    fn new() -> Self {
        Self {
            entries: [None; RECENT],
            next: 0,
        }
    }

    fn fresh(&mut self, key: u64, at: u64) -> bool {
        for (seen, when) in self.entries.iter_mut().flatten() {
            if *seen == key {
                let repeat = at.saturating_sub(*when) < REPEAT_FRAMES;
                *when = at;
                return !repeat;
            }
        }
        self.entries[self.next] = Some((key, at));
        self.next = (self.next + 1) % RECENT;
        true
    }

    fn reset(&mut self) {
        self.entries = [None; RECENT];
    }
}

fn fnv(parts: &[&[u8]]) -> u64 {
    parts
        .iter()
        .flat_map(|part| part.iter())
        .fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01B3)
        })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

fn du(info: &[u8]) -> Option<Du<'_>> {
    let &header = info.first()?;
    let segmented = header & 0x80 != 0;
    let last = header & 0x40 == 0;
    if !segmented {
        if info.len() <= SHORT_INFO {
            return Some(Du::Whole(&info[1..]));
        }
        let len = usize::from(*info.get(1)?);
        return info.get(2..2 + len).map(Du::Whole);
    }
    let reference = info.get(1)? & 0x7F;
    let packet = info.get(2)? & 0x3F;
    let data = if last {
        let len = usize::from(*info.get(SEGMENTED_HEADER)?);
        info.get(SEGMENTED_HEADER + 1..SEGMENTED_HEADER + 1 + len)?
    } else {
        info.get(SEGMENTED_HEADER..)?
    };
    Some(Du::Segment {
        reference,
        packet,
        last,
        data,
    })
}

fn tpdu(info: &[u8]) -> Option<&[u8]> {
    let &header = info.first()?;
    let whole = header & 0x40 == 0 && header & 0x20 != 0;
    if !whole {
        return None;
    }
    let len = usize::from(*info.get(2)?);
    info.get(3..3 + len)
}

#[derive(Clone, Copy)]
struct Origin {
    route: Route,
    start: u64,
    code: u8,
}

struct Voice {
    frames: u32,
    last: Option<u64>,
    scrambling: u8,
}

pub(crate) struct Link {
    frames: u64,
    superframe: Option<u64>,
    assemblers: [Assembler; 3],
    segments: Segments,
    recent: Recent,
    voice: Voice,
    traffic_at: Option<u64>,
    code: u8,
    rejected: u32,
}

impl Link {
    pub(crate) fn new() -> Self {
        Self {
            frames: 0,
            superframe: None,
            assemblers: [Assembler::new(), Assembler::new(), Assembler::new()],
            segments: Segments::new(),
            recent: Recent::new(),
            voice: Voice {
                frames: 0,
                last: None,
                scrambling: 0,
            },
            traffic_at: None,
            code: 0,
            rejected: 0,
        }
    }

    pub(crate) fn rejected(&self) -> u32 {
        self.rejected
    }

    pub(crate) fn reject(&mut self) {
        self.rejected = self.rejected.saturating_add(1);
    }

    pub(crate) fn reset(&mut self) {
        self.superframe = None;
        self.voice.frames = 0;
        self.voice.last = None;
        self.traffic_at = None;
        for assembler in &mut self.assemblers {
            assembler.clear();
        }
        self.segments.reset();
    }

    pub(crate) fn forget(&mut self) {
        self.reset();
        self.recent.reset();
    }

    pub(crate) fn frame(&mut self, decoded: Option<(Decoded, u8)>, out: &mut ChannelOutputs) {
        let at = self.frames;
        self.frames += 1;
        let route = self.route(at);
        if let Some((_, code)) = decoded {
            self.code = code;
        }
        match decoded {
            Some((
                Decoded {
                    block: Block::Voice { .. },
                    ..
                },
                code,
            )) => {
                self.voice.frames += 1;
                self.voice.last = Some(at);
                self.voice.scrambling = code;
                self.traffic_at = Some(at);
            }
            Some((
                Decoded {
                    block: Block::Data(block),
                    ..
                },
                _,
            )) if route != Route::Random => {
                let step = self.assemblers[route.index()].push(Some(&block), at);
                self.stepped(step, route, out);
            }
            None if route != Route::Random => {
                let step = self.assemblers[route.index()].push(None, at);
                self.stepped(step, route, out);
            }
            _ => {}
        }
        if self
            .voice
            .last
            .is_some_and(|last| at - last >= VOICE_GAP_FRAMES)
        {
            self.flush_voice(out);
        }
    }

    fn frame_number(&self, at: u64) -> Option<u64> {
        self.superframe.map(|offset| (at + offset) % SUPERFRAME)
    }

    fn traffic(&self, at: u64) -> bool {
        self.traffic_at
            .is_some_and(|last| at.abs_diff(last) < TRAFFIC_FRAMES)
    }

    fn route(&self, at: u64) -> Route {
        if self.traffic(at) {
            return Route::Control;
        }
        match self.frame_number(at).map(|number| number % HALF_SUPERFRAME) {
            Some(number) if number < BCH_BLOCKS => Route::Broadcast,
            Some(number) if PCH_FRAMES.contains(&number) => Route::Paging,
            Some(number) if number % RCH_PERIOD == RCH_OFFSET => Route::Random,
            _ => Route::Control,
        }
    }

    fn channel(&self, at: u64) -> TetrapolLogicalChannel {
        match (self.route(at), self.superframe) {
            _ if self.traffic(at) => TetrapolLogicalChannel::Traffic,
            (Route::Broadcast, _) => TetrapolLogicalChannel::Broadcast,
            (Route::Paging, _) => TetrapolLogicalChannel::Paging,
            (_, Some(_)) => TetrapolLogicalChannel::Signalling,
            (_, None) => TetrapolLogicalChannel::Unknown,
        }
    }

    fn stepped(&mut self, step: Step, route: Route, out: &mut ChannelOutputs) {
        if step.lost {
            self.reject();
        }
        let Some(completed) = step.completed else {
            return;
        };
        let mut frame = [0u8; MAX_FRAME_BYTES];
        let bytes = self.assemblers[route.index()].bytes(completed);
        let len = bytes.len();
        frame[..len].copy_from_slice(bytes);
        let origin = Origin {
            route,
            start: completed.start,
            code: self.code,
        };
        if route == Route::Paging {
            self.paging(&frame[..len], origin, out);
        } else {
            self.hdlc(&frame[..len], origin, out);
        }
    }

    fn paging(&mut self, frame: &[u8], origin: Origin, out: &mut ChannelOutputs) {
        let terminals: Vec<TetrapolAddress> = frame
            .get(BLOCK_BYTES..BLOCK_BYTES + 2 * PCH_ADDRESSES)
            .into_iter()
            .flat_map(|addresses| addresses.as_chunks::<2>().0)
            .filter_map(|&[high, low]| message::address(high, low))
            .collect();
        if terminals.is_empty() || !self.recent.fresh(fnv(&[frame]), origin.start) {
            return;
        }
        let payload = TetrapolPayload::Paging { terminals };
        self.emit(origin, None, payload, frame, out);
    }

    fn hdlc(&mut self, frame: &[u8], origin: Origin, out: &mut ChannelOutputs) {
        if frame.starts_with(&NO_STATION) {
            return;
        }
        if !hdlc_fcs_ok(frame) {
            if self.superframe.is_some() || self.traffic(origin.start) {
                self.reject();
            }
            return;
        }
        let address = message::address(frame[0], frame[1]);
        let command = frame[2];
        let info = &frame[3..frame.len() - 2];
        if matches!(command & UI_MASK, UI_CIRCUIT | UI_VOICE_CHANNEL) {
            self.traffic_at = Some(origin.start);
        }
        if command & UI_MASK == UI {
            match du(info) {
                Some(Du::Whole(tsdu)) => {
                    self.tsdu(tsdu, address, origin, out);
                }
                Some(Du::Segment {
                    reference,
                    packet,
                    last,
                    data,
                }) => {
                    let (lost, done) = self.segments.push(reference, packet, last, data);
                    if lost {
                        self.reject();
                    }
                    if done {
                        let mut tsdu = [0u8; MAX_TSDU];
                        let whole = self.segments.tsdu();
                        tsdu[..whole.len()].copy_from_slice(whole);
                        let len = whole.len();
                        self.tsdu(&tsdu[..len], address, origin, out);
                    }
                }
                None => self.reject(),
            }
        } else if command & 1 == 0
            && let Some(tsdu) = tpdu(info)
        {
            self.tsdu(tsdu, address, origin, out);
        }
    }

    fn tsdu(
        &mut self,
        tsdu: &[u8],
        address: Option<TetrapolAddress>,
        origin: Origin,
        out: &mut ChannelOutputs,
    ) {
        if origin.route == Route::Broadcast || self.superframe.is_none() {
            self.align(tsdu, origin.start);
        }
        let Some(payload) = message::payload(tsdu) else {
            return;
        };
        let key_address = address.map_or([0; 2], |a| a.id.to_be_bytes());
        let stable = &tsdu[..message::stable_len(tsdu)];
        let mut head = [0u8; 2];
        let head_len = stable.len().min(head.len());
        head[..head_len].copy_from_slice(&stable[..head_len]);
        if message::first_bch_block(tsdu).is_some() {
            head[1] &= !message::BCH_BLOCK_BIT;
        }
        let key = fnv(&[&key_address, &head[..head_len], &stable[head_len..]]);
        if self.recent.fresh(key, origin.start) {
            self.emit(origin, address, payload, tsdu, out);
        }
    }

    fn align(&mut self, tsdu: &[u8], start: u64) {
        if let Some(first) = message::first_bch_block(tsdu) {
            let number = if first { 0 } else { HALF_SUPERFRAME };
            self.superframe = Some((number + SUPERFRAME - start % SUPERFRAME) % SUPERFRAME);
        }
    }

    fn emit(
        &self,
        origin: Origin,
        address: Option<TetrapolAddress>,
        payload: TetrapolPayload,
        data: &[u8],
        out: &mut ChannelOutputs,
    ) {
        out.events.push(DecoderEvent::Tetrapol(TetrapolMessage {
            channel: self.channel(origin.start),
            frame: self
                .frame_number(origin.start)
                .and_then(|number| u8::try_from(number).ok()),
            scrambling: origin.code,
            address,
            payload,
            data: hex(data),
            rejected: self.rejected,
        }));
    }

    pub(crate) fn flush_voice(&mut self, out: &mut ChannelOutputs) {
        if self.voice.frames == 0 {
            return;
        }
        let seconds = self.voice.frames as f32 * FRAME_S;
        self.voice.frames = 0;
        out.events.push(DecoderEvent::Tetrapol(TetrapolMessage {
            channel: TetrapolLogicalChannel::Traffic,
            frame: None,
            scrambling: self.voice.scrambling,
            address: None,
            payload: TetrapolPayload::Voice { seconds },
            data: String::new(),
            rejected: self.rejected,
        }));
    }
}
