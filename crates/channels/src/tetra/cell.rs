use sdrmm_wire::{DvChannelDefinition, DvFrame, DvFrameKind};

use super::{
    bits::without_fill,
    burst::{
        AACH1_AT, AACH1_BITS, AACH2_AT, BLOCK1_AT, BLOCK2_AT, BURST_BITS, BurstKind, HALF_BITS,
        SB1_AT, SB1_BITS, SyncEvent, Synchronizer,
    },
    coding::{
        AACH_BITS, BSCH, BlockDecoder, FULL_SLOT, HALF_SLOT, MAX_TYPE5_BITS, SB1_SCRAMBLING, Soft,
        rm_decode, scrambling_code, scrambling_sequence,
    },
    mac::{
        Address, Downlink, FRAGMENT_HEADER_BITS, Length, PduType, SysInfo, parse_access_assign,
        parse_end, parse_resource, parse_sync, parse_sysinfo, pdu_type,
    },
    upper::{Stamp, Upper},
};

const TIMESLOTS: usize = 4;
const FRAMES: u8 = 18;
const MULTIFRAMES: u8 = 60;
const FRAGMENT_BITS: usize = 4_096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cell {
    mcc: u16,
    mnc: u16,
    colour: u8,
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
        if usize::from(self.timeslot) > TIMESLOTS {
            self.timeslot = 1;
            self.frame += 1;
        }
        if self.frame > FRAMES {
            self.frame = 1;
            self.multiframe += 1;
        }
        if self.multiframe > MULTIFRAMES {
            self.multiframe = 1;
        }
    }

    fn index(self) -> usize {
        usize::from(self.timeslot.clamp(1, 4) - 1)
    }
}

struct Fragment {
    bits: Box<[bool; FRAGMENT_BITS]>,
    len: usize,
    address: Option<Address>,
}

impl Fragment {
    fn new() -> Self {
        Self {
            bits: Box::new([false; FRAGMENT_BITS]),
            len: 0,
            address: None,
        }
    }

    fn start(&mut self, address: Address, bits: &[bool]) -> bool {
        self.address = Some(address);
        self.len = 0;
        self.append(bits)
    }

    fn append(&mut self, bits: &[bool]) -> bool {
        let Some(slot) = self.bits.get_mut(self.len..self.len + bits.len()) else {
            self.address = None;
            return false;
        };
        slot.copy_from_slice(bits);
        self.len += bits.len();
        true
    }
}

pub(crate) struct Receiver {
    sync: Synchronizer,
    decoder: BlockDecoder,
    bsch_scrambling: [bool; SB1_BITS],
    scrambling: [bool; MAX_TYPE5_BITS],
    cell: Option<Cell>,
    time: Option<Time>,
    sysinfo: Option<SysInfo>,
    reported: Option<(Cell, Option<SysInfo>)>,
    traffic: [Option<u8>; TIMESLOTS],
    fragments: [Fragment; TIMESLOTS],
    window: [Soft; BURST_BITS],
    type5: [Soft; MAX_TYPE5_BITS],
    block: [bool; FULL_SLOT.type1],
    upper: Upper,
    rejected: u32,
}

impl Receiver {
    pub(crate) fn new() -> Self {
        let mut bsch_scrambling = [false; SB1_BITS];
        scrambling_sequence(SB1_SCRAMBLING, &mut bsch_scrambling);
        Self {
            sync: Synchronizer::new(),
            decoder: BlockDecoder::new(),
            bsch_scrambling,
            scrambling: [false; MAX_TYPE5_BITS],
            cell: None,
            time: None,
            sysinfo: None,
            reported: None,
            traffic: [None; TIMESLOTS],
            fragments: std::array::from_fn(|_| Fragment::new()),
            window: [0; BURST_BITS],
            type5: [0; MAX_TYPE5_BITS],
            block: [false; FULL_SLOT.type1],
            upper: Upper::new(),
            rejected: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.sync.reset();
        self.cell = None;
        self.time = None;
        self.sysinfo = None;
        self.reported = None;
        self.drop_state();
        self.upper.reset();
    }

    pub(crate) fn rejected(&self) -> u32 {
        self.rejected
    }

    pub(crate) fn push(&mut self, first: Soft, second: Soft, out: &mut Vec<DvFrame>) {
        match self.sync.push(first, second) {
            None => {}
            Some(SyncEvent::Candidate) => self.acquire(),
            Some(SyncEvent::Burst(kind)) => {
                self.advance();
                self.burst(kind, out);
            }
            Some(SyncEvent::Missed) => {
                self.advance();
                self.rejected = self.rejected.saturating_add(1);
            }
            Some(SyncEvent::Lost) => {
                self.time = None;
                self.drop_state();
            }
        }
    }

    fn drop_state(&mut self) {
        self.traffic = [None; TIMESLOTS];
        for fragment in &mut self.fragments {
            fragment.address = None;
        }
    }

    fn advance(&mut self) {
        if let Some(time) = &mut self.time {
            time.advance();
        }
    }

    fn acquire(&mut self) {
        let inverted = self.sync.candidate_inverted();
        let mut sb1 = [0; SB1_BITS];
        self.sync.candidate_bits(SB1_AT, &mut sb1, inverted);
        let found = self
            .decoder
            .decode(BSCH, &sb1, &self.bsch_scrambling)
            .and_then(parse_sync);
        if found.is_some() {
            self.sync.confirm(inverted);
        }
    }

    fn burst(&mut self, kind: BurstKind, out: &mut Vec<DvFrame>) {
        self.window = *self.sync.window();
        match kind {
            BurstKind::Sync => self.sync_burst(out),
            BurstKind::Normal => self.normal_burst(out),
            BurstKind::Split => self.split_burst(out),
        }
    }

    fn sync_burst(&mut self, out: &mut Vec<DvFrame>) {
        let sb1 = &self.window[SB1_AT..SB1_AT + SB1_BITS];
        match self
            .decoder
            .decode(BSCH, sb1, &self.bsch_scrambling)
            .and_then(parse_sync)
        {
            Some(sync) => {
                self.time = Some(Time {
                    timeslot: sync.timeslot,
                    frame: sync.frame,
                    multiframe: sync.multiframe,
                });
                self.set_cell(Cell {
                    mcc: sync.mcc,
                    mnc: sync.mnc,
                    colour: sync.colour,
                });
                self.report(out);
            }
            None => self.reject(),
        }
        self.half(BLOCK2_AT, true, out);
    }

    fn normal_burst(&mut self, out: &mut Vec<DvFrame>) {
        let downlink = self.aach(out);
        if matches!(downlink, Some(Downlink::Traffic(_))) {
            return;
        }
        self.type5[..HALF_BITS].copy_from_slice(&self.window[BLOCK1_AT..BLOCK1_AT + HALF_BITS]);
        self.type5[HALF_BITS..].copy_from_slice(&self.window[BLOCK2_AT..BLOCK2_AT + HALF_BITS]);
        let counted = downlink != Some(Downlink::Unallocated);
        self.decode(FULL_SLOT.type5, counted, out);
    }

    fn split_burst(&mut self, out: &mut Vec<DvFrame>) {
        let downlink = self.aach(out);
        let traffic = matches!(downlink, Some(Downlink::Traffic(_)));
        let counted = downlink != Some(Downlink::Unallocated);
        self.half(BLOCK1_AT, counted, out);
        self.half(BLOCK2_AT, counted && !traffic, out);
    }

    fn half(&mut self, at: usize, counted: bool, out: &mut Vec<DvFrame>) {
        if self.cell.is_none() {
            return;
        }
        self.type5[..HALF_BITS].copy_from_slice(&self.window[at..at + HALF_BITS]);
        self.decode(HALF_BITS, counted, out);
    }

    fn decode(&mut self, len: usize, counted: bool, out: &mut Vec<DvFrame>) {
        if self.cell.is_none() || self.time.is_none() {
            return;
        }
        let block = if len == FULL_SLOT.type5 {
            FULL_SLOT
        } else {
            HALF_SLOT
        };
        let Some(bits) = self
            .decoder
            .decode(block, &self.type5[..len], &self.scrambling)
        else {
            if counted {
                self.reject();
            }
            return;
        };
        self.block[..bits.len()].copy_from_slice(bits);
        self.mac(block.type1, out);
    }

    fn reject(&mut self) {
        self.rejected = self.rejected.saturating_add(1);
    }

    fn set_cell(&mut self, cell: Cell) {
        if self.cell == Some(cell) {
            return;
        }
        self.cell = Some(cell);
        self.sysinfo = None;
        scrambling_sequence(
            scrambling_code(cell.mcc, cell.mnc, cell.colour),
            &mut self.scrambling,
        );
        self.drop_state();
    }

    fn aach(&mut self, out: &mut Vec<DvFrame>) -> Option<Downlink> {
        let downlink = self.access_assign(AACH1_AT, AACH1_BITS, AACH2_AT)?;
        let time = self.time?;
        if time.frame == FRAMES {
            return Some(downlink);
        }
        let slot = time.index();
        let marker = match downlink {
            Downlink::Traffic(marker) => Some(marker),
            _ => None,
        };
        if self.traffic[slot] != marker {
            self.traffic[slot] = marker;
            if let (Some(marker), Some(stamp)) = (marker, self.stamp()) {
                self.upper.traffic(stamp, marker, out);
            }
        }
        Some(downlink)
    }

    fn access_assign(&self, at: usize, first: usize, second_at: usize) -> Option<Downlink> {
        self.cell?;
        let time = self.time?;
        let positions = (at..at + first).chain(second_at..second_at + AACH_BITS - first);
        let word = positions.enumerate().fold(0u32, |word, (i, k)| {
            word << 1 | u32::from((self.window[k] > 0) ^ self.scrambling[i])
        });
        let (info, _) = rm_decode(word)?;
        Some(parse_access_assign(info, time.frame))
    }

    fn stamp(&self) -> Option<Stamp> {
        let cell = self.cell?;
        let time = self.time?;
        Some(Stamp {
            timeslot: time.timeslot,
            colour: cell.colour,
            mcc: cell.mcc,
            mnc: cell.mnc,
            location_area: self.sysinfo.map(|info| info.location_area),
        })
    }

    fn report(&mut self, out: &mut Vec<DvFrame>) {
        let Some(cell) = self.cell else {
            return;
        };
        if self.reported == Some((cell, self.sysinfo)) {
            return;
        }
        self.reported = Some((cell, self.sysinfo));
        let Some(stamp) = self.stamp() else {
            return;
        };
        let mut frame = stamp.frame(DvFrameKind::Control);
        frame.opcode = Some(
            if self.sysinfo.is_some() {
                "SYSINFO"
            } else {
                "SYNC"
            }
            .to_owned(),
        );
        frame.control_channel = Some(true);
        if let Some(info) = self.sysinfo {
            frame.encrypted = Some(info.air_encryption());
            frame.channel = Some(info.carrier.number);
            frame.channel_definition = Some(DvChannelDefinition {
                channel: info.carrier.number,
                tx_hz: info.carrier.downlink_hz(),
                rx_hz: info.carrier.uplink_hz().unwrap_or(0),
                color_code: Some(cell.colour),
            });
        }
        out.push(frame);
    }

    fn mac(&mut self, len: usize, out: &mut Vec<DvFrame>) {
        let mut at = 0;
        while at < len {
            let rest = &self.block[at..len];
            if without_fill(rest).is_empty() {
                return;
            }
            let used = match pdu_type(rest) {
                Some(PduType::Resource) => self.resource(at, len, out),
                Some(PduType::Fragment) => {
                    self.fragment(at, len);
                    None
                }
                Some(PduType::End) => self.end(at, len, out),
                Some(PduType::Broadcast) if at == 0 => {
                    self.broadcast(len, out);
                    None
                }
                _ => None,
            };
            match used {
                Some(used) if used > 0 => at += used,
                _ => return,
            }
        }
    }

    fn resource(&mut self, at: usize, len: usize, out: &mut Vec<DvFrame>) -> Option<usize> {
        let rest = &self.block[at..len];
        let resource = parse_resource(rest)?;
        if resource.address == Address::Null {
            return None;
        }
        let end = match resource.length {
            Length::Bits(bits) => bits.min(rest.len()),
            Length::SecondHalfStolen | Length::StartsFragment => rest.len(),
        };
        let body = rest.get(resource.header_bits..end)?;
        let body = if resource.fill {
            without_fill(body)
        } else {
            body
        };
        if resource.encrypted {
            return Some(end);
        }
        let slot = self.time?.index();
        if resource.length == Length::StartsFragment {
            if !self.fragments[slot].start(resource.address, body) {
                self.reject();
            }
            return None;
        }
        let stamp = self.stamp()?;
        self.upper.sdu(stamp, resource.address, body, out);
        Some(end)
    }

    fn fragment(&mut self, at: usize, len: usize) {
        let Some(slot) = self.time.map(Time::index) else {
            return;
        };
        let rest = &self.block[at..len];
        let Some(body) = rest.get(FRAGMENT_HEADER_BITS..) else {
            return;
        };
        let body = if rest[FRAGMENT_HEADER_BITS - 1] {
            without_fill(body)
        } else {
            body
        };
        let fragment = &mut self.fragments[slot];
        if fragment.address.is_some() && !fragment.append(body) {
            self.reject();
        }
    }

    fn end(&mut self, at: usize, len: usize, out: &mut Vec<DvFrame>) -> Option<usize> {
        let rest = &self.block[at..len];
        let end = parse_end(rest)?;
        let stop = end.length.min(rest.len());
        let body = rest.get(end.header_bits..stop)?;
        let body = if end.fill { without_fill(body) } else { body };
        let slot = self.time?.index();
        let stamp = self.stamp()?;
        let fragment = &mut self.fragments[slot];
        let address = fragment.address.take()?;
        if !fragment.append(body) {
            self.rejected = self.rejected.saturating_add(1);
            return Some(stop);
        }
        self.upper
            .sdu(stamp, address, &fragment.bits[..fragment.len], out);
        Some(stop)
    }

    fn broadcast(&mut self, len: usize, out: &mut Vec<DvFrame>) {
        if let Some(info) = parse_sysinfo(&self.block[..len]) {
            self.sysinfo = Some(info);
            self.report(out);
        }
    }
}
