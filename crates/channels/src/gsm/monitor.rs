use std::fmt::Write;

use sdrmm_wire::{
    GsmAssignment, GsmBand, GsmChannelType, GsmFrame, GsmLogicalChannel, GsmMessage, GsmParams,
    GsmSi,
};

use super::{
    burst::{BLOCK_BURSTS, BURST_DATA, HYPERFRAME, normal_data},
    coding::{BLOCK_BYTES, Decoder},
    dedicated::{self, Link},
    equalizer::Demodulated,
    frames::{BlockSlot, ControlSlot, SyncInfo, control_slot, sdcch8_slot},
    receiver::Sink,
    rr::{self, CellBook, Rr},
};

const FOLLOWERS: usize = 4;
const RECENT: usize = 16;
const REPEAT_WINDOW_FRAMES: u32 = 2 * 51;
const FOLLOW_IDLE_FRAMES: u32 = 4 * 51;
const MAX_FOLLOW_FAILURES: u8 = 3;
const PRESENT_SNR_DB: f32 = 2.0;
const SI_KINDS: usize = 15;

#[derive(Clone, Copy)]
struct Block {
    first: u32,
    filled: u8,
    snr_db: f32,
    present: bool,
    data: [[f32; BURST_DATA]; BLOCK_BURSTS],
}

impl Block {
    const fn new() -> Self {
        Self {
            first: 0,
            filled: 0,
            snr_db: 0.0,
            present: true,
            data: [[0.0; BURST_DATA]; BLOCK_BURSTS],
        }
    }

    fn add(&mut self, frame_number: u32, index: u8, burst: &Demodulated) -> bool {
        if index == 0 {
            *self = Self::new();
            self.first = frame_number;
        } else if self.filled != (1 << index) - 1
            || frame_number != (self.first + u32::from(index)) % HYPERFRAME
        {
            self.filled = 0;
            return false;
        }
        normal_data(&burst.soft, &mut self.data[usize::from(index)]);
        self.filled |= 1 << index;
        self.snr_db += burst.snr_db / BLOCK_BURSTS as f32;
        self.present &= burst.snr_db >= PRESENT_SNR_DB;
        usize::from(index) == BLOCK_BURSTS - 1
    }
}

#[derive(Clone, Copy)]
struct Follower {
    assignment_timeslot: u8,
    channel: GsmChannelType,
    subchannel: u8,
    tsc: u8,
    ciphered: bool,
    failures: u8,
    last_good: u32,
    dedicated: Block,
    associated: Block,
}

impl Follower {
    fn slot(&self, frame_number: u32, combined: bool) -> Option<BlockSlot> {
        let slot = match self.channel {
            GsmChannelType::Sdcch8 => sdcch8_slot(frame_number)?,
            _ => match control_slot(frame_number, combined) {
                ControlSlot::Block(slot) => slot,
                _ => return None,
            },
        };
        let dedicated = matches!(
            slot.channel,
            GsmLogicalChannel::Sdcch | GsmLogicalChannel::Sacch
        );
        (dedicated && slot.subchannel == Some(self.subchannel)).then_some(slot)
    }

    fn expired(&self, frame_number: u32) -> bool {
        (frame_number + HYPERFRAME - self.last_good) % HYPERFRAME > FOLLOW_IDLE_FRAMES
    }
}

struct Decoded {
    frame_number: u32,
    timeslot: u8,
    channel: GsmLogicalChannel,
    subchannel: Option<u8>,
    snr_db: f32,
    offset_hz: f32,
    octets: [u8; BLOCK_BYTES],
}

pub(crate) struct Monitor {
    params: GsmParams,
    band: Option<(GsmBand, u16)>,
    bsic: Option<u8>,
    book: CellBook,
    control: Block,
    followers: [Option<Follower>; FOLLOWERS],
    decoder: Decoder,
    system_information: [Option<[u8; BLOCK_BYTES]>; SI_KINDS],
    other: Vec<(u8, [u8; BLOCK_BYTES])>,
    recent: [Option<(u32, [u8; BLOCK_BYTES])>; RECENT],
    recent_at: usize,
    frames: Vec<GsmFrame>,
    rejected: u32,
}

impl Monitor {
    pub(crate) fn new(params: GsmParams, frequency_hz: f64) -> Self {
        Self {
            params,
            band: GsmBand::at(frequency_hz),
            bsic: None,
            book: CellBook::default(),
            control: Block::new(),
            followers: [None; FOLLOWERS],
            decoder: Decoder::new(),
            system_information: [None; SI_KINDS],
            other: Vec::with_capacity(8),
            recent: [None; RECENT],
            recent_at: 0,
            frames: Vec::with_capacity(16),
            rejected: 0,
        }
    }

    pub(crate) fn set_params(&mut self, params: GsmParams) {
        if !params.follow {
            self.followers = [None; FOLLOWERS];
        }
        self.params = params;
    }

    pub(crate) fn retune(&mut self, frequency_hz: f64) {
        self.band = GsmBand::at(frequency_hz);
        self.forget();
    }

    pub(crate) fn forget(&mut self) {
        self.bsic = None;
        self.book.clear();
        self.followers = [None; FOLLOWERS];
        self.system_information = [None; SI_KINDS];
        self.other.clear();
        self.recent = [None; RECENT];
        self.control = Block::new();
    }

    pub(crate) fn rejected(&self) -> u32 {
        self.rejected
    }

    pub(crate) fn drain(&mut self) -> std::vec::Drain<'_, GsmFrame> {
        self.frames.drain(..)
    }

    fn arfcn(&self) -> Option<u16> {
        self.band.map(|(_, arfcn)| arfcn)
    }

    fn control_burst(&mut self, frame_number: u32, burst: &Demodulated, offset_hz: f32) {
        let ControlSlot::Block(slot) = control_slot(frame_number, self.book.combined()) else {
            return;
        };
        if matches!(
            slot.channel,
            GsmLogicalChannel::Bcch | GsmLogicalChannel::Ccch
        ) {
            if self.control.add(frame_number, slot.index, burst) {
                let block = self.control;
                self.control_block(&block, slot, offset_hz);
            }
            return;
        }
        self.follower_burst(0, frame_number, burst, offset_hz);
    }

    fn control_block(&mut self, block: &Block, slot: BlockSlot, offset_hz: f32) {
        let Some(octets) = self.decoder.block(&block.data) else {
            self.rejected += 1;
            return;
        };
        let Some((message_type, body)) = rr::unit_data(&octets) else {
            return;
        };
        let Some(parsed) = rr::parse(message_type, body) else {
            return;
        };
        let decoded = Decoded {
            frame_number: block.first,
            timeslot: 0,
            channel: slot.channel,
            subchannel: None,
            snr_db: block.snr_db,
            offset_hz,
            octets,
        };
        self.handle(parsed, &decoded);
    }

    fn handle(&mut self, parsed: Rr, decoded: &Decoded) {
        let message = match parsed {
            Rr::SystemInformation(si) => {
                if !self.system_information_changed(si, &decoded.octets) {
                    return;
                }
                self.book.apply(si, &decoded.octets[3..]);
                GsmMessage::SystemInformation {
                    si,
                    cell: self.book.cell().clone(),
                }
            }
            Rr::Paging(identities) => {
                if identities.is_empty() || !self.params.paging || self.repeated(decoded) {
                    return;
                }
                GsmMessage::Paging { identities }
            }
            Rr::Assignment(assignments) => {
                for assignment in &assignments {
                    self.follow(assignment, decoded.frame_number);
                }
                if !self.params.paging || self.repeated(decoded) {
                    return;
                }
                GsmMessage::ImmediateAssignment { assignments }
            }
            Rr::Reject(requests) => {
                if !self.params.paging || self.repeated(decoded) {
                    return;
                }
                GsmMessage::AssignmentReject { requests }
            }
            Rr::Other(message_type) => {
                if !self.other_changed(message_type, &decoded.octets) {
                    return;
                }
                GsmMessage::Other { message_type }
            }
        };
        self.emit(decoded, message);
    }

    fn system_information_changed(&mut self, si: GsmSi, octets: &[u8; BLOCK_BYTES]) -> bool {
        let slot = &mut self.system_information[si as usize];
        if slot.as_ref() == Some(octets) {
            return false;
        }
        *slot = Some(*octets);
        true
    }

    fn other_changed(&mut self, message_type: u8, octets: &[u8; BLOCK_BYTES]) -> bool {
        match self
            .other
            .iter_mut()
            .find(|(kind, _)| *kind == message_type)
        {
            Some((_, seen)) if seen == octets => false,
            Some((_, seen)) => {
                *seen = *octets;
                true
            }
            None => {
                self.other.push((message_type, *octets));
                true
            }
        }
    }

    fn repeated(&mut self, decoded: &Decoded) -> bool {
        let fresh = |(seen_at, _): &(u32, [u8; BLOCK_BYTES])| {
            (decoded.frame_number + HYPERFRAME - seen_at) % HYPERFRAME <= REPEAT_WINDOW_FRAMES
        };
        if self
            .recent
            .iter()
            .flatten()
            .any(|entry| fresh(entry) && entry.1 == decoded.octets)
        {
            return true;
        }
        self.recent[self.recent_at] = Some((decoded.frame_number, decoded.octets));
        self.recent_at = (self.recent_at + 1) % RECENT;
        false
    }

    fn follow(&mut self, assignment: &GsmAssignment, frame_number: u32) {
        if !self.params.follow || assignment.hopping.is_some() {
            return;
        }
        if assignment.arfcn.is_some() && self.arfcn().is_some() && assignment.arfcn != self.arfcn()
        {
            return;
        }
        let combined = self.book.combined();
        let fits = match assignment.channel {
            GsmChannelType::Sdcch8 => assignment.timeslot != 0 || !combined,
            GsmChannelType::Sdcch4 => assignment.timeslot == 0 && combined,
            _ => false,
        };
        let Some(subchannel) = assignment.subchannel.filter(|_| fits) else {
            return;
        };
        let follower = Follower {
            assignment_timeslot: assignment.timeslot,
            channel: assignment.channel,
            subchannel,
            tsc: assignment.tsc,
            ciphered: false,
            failures: 0,
            last_good: frame_number,
            dedicated: Block::new(),
            associated: Block::new(),
        };
        let place = self
            .followers
            .iter()
            .position(|slot| {
                slot.is_some_and(|f| {
                    f.assignment_timeslot == follower.assignment_timeslot
                        && f.subchannel == follower.subchannel
                })
            })
            .or_else(|| self.followers.iter().position(Option::is_none));
        if let Some(place) = place {
            self.followers[place] = Some(follower);
        }
    }

    fn follower_burst(
        &mut self,
        timeslot: u8,
        frame_number: u32,
        burst: &Demodulated,
        offset_hz: f32,
    ) {
        let combined = self.book.combined();
        for place in 0..FOLLOWERS {
            let Some(mut follower) = self.followers[place] else {
                continue;
            };
            if follower.assignment_timeslot != timeslot {
                continue;
            }
            if follower.expired(frame_number) {
                self.followers[place] = None;
                continue;
            }
            let Some(slot) = follower.slot(frame_number, combined) else {
                continue;
            };
            let sacch = slot.channel == GsmLogicalChannel::Sacch;
            let block = if sacch {
                &mut follower.associated
            } else {
                &mut follower.dedicated
            };
            let complete = block.add(frame_number, slot.index, burst);
            let block = *block;
            self.followers[place] = Some(follower);
            if complete {
                self.follower_block(place, &block, slot, timeslot, offset_hz);
            }
        }
    }

    fn follower_block(
        &mut self,
        place: usize,
        block: &Block,
        slot: BlockSlot,
        timeslot: u8,
        offset_hz: f32,
    ) {
        let Some(mut follower) = self.followers[place] else {
            return;
        };
        if !block.present {
            return;
        }
        let Some(octets) = self.decoder.block(&block.data) else {
            if follower.ciphered {
                self.followers[place] = None;
                return;
            }
            self.rejected += 1;
            follower.failures += 1;
            self.followers[place] = (follower.failures < MAX_FOLLOW_FAILURES).then_some(follower);
            return;
        };
        follower.failures = 0;
        follower.last_good = block.first;
        let decoded = Decoded {
            frame_number: block.first,
            timeslot,
            channel: slot.channel,
            subchannel: slot.subchannel,
            snr_db: block.snr_db,
            offset_hz,
            octets,
        };
        let sacch = slot.channel == GsmLogicalChannel::Sacch;
        let release = match dedicated::link(&octets, sacch) {
            Some(link) => self.dedicated(&link, &decoded, &mut follower),
            None => false,
        };
        self.followers[place] = (!release).then_some(follower);
    }

    fn dedicated(&mut self, link: &Link<'_>, decoded: &Decoded, follower: &mut Follower) -> bool {
        if let Some(&[0x06, message_type, ..]) = link.info.get(..2)
            && let Some(si) = rr::si_type(message_type)
        {
            if self.system_information_changed(si, &decoded.octets) {
                self.book.apply(si, &link.info[2..]);
                let cell = self.book.cell().clone();
                self.emit(decoded, GsmMessage::SystemInformation { si, cell });
            }
            return false;
        }
        let Some(description) = dedicated::describe(link) else {
            return false;
        };
        if description.cipher.is_some_and(|algorithm| algorithm > 0) {
            follower.ciphered = true;
        }
        let release = match (description.protocol, link.info.get(1)) {
            (Some(protocol), Some(&octet)) => {
                dedicated::releases(protocol, dedicated::message_type(protocol, octet))
            }
            _ => description.frame == sdrmm_wire::GsmLapdmFrame::Disc,
        };
        self.emit(decoded, GsmMessage::Dedicated(description));
        release
    }

    fn emit(&mut self, decoded: &Decoded, message: GsmMessage) {
        let Some(bsic) = self.bsic else {
            return;
        };
        let mut raw = String::with_capacity(2 * BLOCK_BYTES);
        for octet in decoded.octets {
            let _ = write!(raw, "{octet:02x}");
        }
        self.frames.push(GsmFrame {
            arfcn: self.arfcn(),
            band: self.band.map(|(band, _)| band),
            bsic,
            frame_number: decoded.frame_number,
            timeslot: decoded.timeslot,
            channel: decoded.channel,
            subchannel: decoded.subchannel,
            cgi: self.book.cell().cgi(),
            message,
            snr_db: (decoded.snr_db * 10.0).round() / 10.0,
            frequency_error_hz: decoded.offset_hz.round(),
            raw,
        });
    }
}

impl Sink for Monitor {
    fn training(&self, frame_number: u32, timeslot: u8) -> Option<u8> {
        let combined = self.book.combined();
        self.followers
            .iter()
            .flatten()
            .find(|f| f.assignment_timeslot == timeslot && f.slot(frame_number, combined).is_some())
            .map(|f| f.tsc)
    }

    fn locked(&mut self, info: SyncInfo) {
        if self.bsic.is_some_and(|bsic| bsic != info.bsic) {
            self.forget();
        }
        self.bsic = Some(info.bsic);
    }

    fn lost(&mut self) {
        self.followers = [None; FOLLOWERS];
        self.control = Block::new();
    }

    fn sync_missed(&mut self) {
        self.rejected += 1;
    }

    fn burst(&mut self, frame_number: u32, timeslot: u8, burst: &Demodulated, offset_hz: f32) {
        if timeslot == 0 {
            self.control_burst(frame_number, burst, offset_hz);
        } else {
            self.follower_burst(timeslot, frame_number, burst, offset_hz);
        }
    }
}
