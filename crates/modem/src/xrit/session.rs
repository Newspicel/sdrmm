use sdrmm_dsp::coding::fec::crc16_msb;

use super::header::{Headers, IMAGE, PRIMARY_LEN, Primary};
use super::rice::{RiceLine, RiceParams};
use crate::ccsds::{IDLE_APID, PACKET_HEADER, PacketHeader, sequence_gap};

/// The transport header ahead of a file: a file counter and the file length in bits.
pub const TRANSPORT_HEADER: usize = 10;
pub const CRC_LEN: usize = 2;
const CRC_POLY: u16 = 0x1021;
const CRC_INIT: u16 = 0xFFFF;
/// Files larger than this are given up rather than grown without bound.
pub const MAX_FILE: usize = 48 << 20;
/// NOAA's Rice image structure compression flag.
const RICE_COMPRESSED: u8 = 1;

/// A transport file finished on one virtual channel and APID.
#[derive(Debug)]
pub struct TransportFile<'a> {
    pub vcid: u8,
    pub apid: u16,
    pub counter: u16,
    /// Image lines lost to dropped packets and filled black.
    pub lines_missing: u32,
    pub bytes: &'a mut [u8],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionStats {
    pub files: u32,
    pub packets_failed: u32,
    pub files_lost: u32,
    pub lines_failed: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Idle,
    Collecting,
    Broken,
}

struct Slot {
    key: Option<(u8, u16)>,
    state: State,
    used: u64,
    sequence: u16,
    counter: u16,
    buf: Vec<u8>,
    primary: Option<Primary>,
    headers_done: bool,
    rice: Option<RiceLine>,
    lines: u32,
    lines_wanted: u32,
    lines_missing: u32,
}

impl Slot {
    fn new() -> Self {
        Self {
            key: None,
            state: State::Idle,
            used: 0,
            sequence: 0,
            counter: 0,
            buf: Vec::new(),
            primary: None,
            headers_done: false,
            rice: None,
            lines: 0,
            lines_wanted: 0,
            lines_missing: 0,
        }
    }

    fn begin(&mut self, sequence: u16, counter: u16) {
        self.state = State::Collecting;
        self.sequence = sequence;
        self.counter = counter;
        self.buf.clear();
        self.primary = None;
        self.headers_done = false;
        self.rice = None;
        self.lines = 0;
        self.lines_wanted = 0;
        self.lines_missing = 0;
    }

    fn line_len(&self) -> Option<usize> {
        self.rice.as_ref().map(|rice| rice.params().columns)
    }

    fn fill_lines(&mut self, count: u32) {
        let Some(len) = self.line_len() else {
            return;
        };
        let count = count.min(self.lines_wanted.saturating_sub(self.lines));
        self.buf.resize(self.buf.len() + len * count as usize, 0);
        self.lines += count;
        self.lines_missing += count;
    }

    fn can_fill(&self, count: u32) -> bool {
        self.rice.is_some() && self.lines + count <= self.lines_wanted
    }

    /// Takes the start of the file up to the end of its headers, verbatim.
    fn take_headers<'a>(&mut self, mut data: &'a [u8]) -> &'a [u8] {
        if self.primary.is_none() {
            let want = PRIMARY_LEN.saturating_sub(self.buf.len()).min(data.len());
            self.buf.extend_from_slice(&data[..want]);
            data = &data[want..];
            self.primary = Primary::parse(&self.buf);
        }
        if let Some(primary) = self.primary
            && !self.headers_done
        {
            let want = primary
                .header_len
                .saturating_sub(self.buf.len())
                .min(data.len());
            self.buf.extend_from_slice(&data[..want]);
            data = &data[want..];
            if self.buf.len() >= primary.header_len {
                self.headers_done = true;
                self.setup_rice();
            }
        }
        data
    }

    fn setup_rice(&mut self) {
        let Some(headers) = Headers::parse(&self.buf) else {
            return;
        };
        if headers.primary.file_type != IMAGE {
            return;
        }
        let (Some(image), Some(rice)) = (headers.image(), headers.rice()) else {
            return;
        };
        if image.compression != RICE_COMPRESSED {
            return;
        }
        self.lines_wanted = u32::from(image.lines);
        self.rice = RiceLine::new(RiceParams {
            flags: rice.flags,
            bits_per_pixel: image.bits_per_pixel,
            pixels_per_block: rice.pixels_per_block,
            columns: usize::from(image.columns),
        })
        .ok();
    }

    /// Appends one packet's worth of file; false when the line would not decode.
    fn append(&mut self, data: &[u8]) -> bool {
        let data = self.take_headers(data);
        if data.is_empty() {
            return true;
        }
        if !self.headers_done || self.primary.is_none() {
            return false;
        }
        let Some(rice) = self.rice.as_mut() else {
            self.buf.extend_from_slice(data);
            return true;
        };
        if self.lines >= self.lines_wanted {
            return true;
        }
        let before = self.buf.len();
        if rice.decode(data, &mut self.buf).is_err() {
            self.buf.truncate(before);
            return false;
        }
        self.lines += 1;
        true
    }
}

/// Collects the transport files of every virtual channel and APID from their space packets:
/// checks each packet's CRC, follows the sequence count, and expands NOAA's Rice-coded image
/// lines as they come. A lost line of a Rice image is filled black and counted; any other loss
/// gives the file up and counts it.
pub struct SessionAssembler {
    slots: Vec<Slot>,
    clock: u64,
    stats: SessionStats,
    finished: Option<usize>,
}

impl SessionAssembler {
    #[must_use]
    pub fn new(slots: usize) -> Self {
        Self {
            slots: (0..slots.max(1)).map(|_| Slot::new()).collect(),
            clock: 0,
            stats: SessionStats::default(),
            finished: None,
        }
    }

    #[must_use]
    pub fn stats(&self) -> SessionStats {
        self.stats
    }

    /// Forgets every file in progress, counting each as lost.
    pub fn reset(&mut self) {
        for slot in &mut self.slots {
            if slot.state == State::Collecting {
                self.stats.files_lost = self.stats.files_lost.saturating_add(1);
            }
            slot.state = State::Idle;
            slot.key = None;
        }
        self.finished = None;
    }

    fn slot_for(&mut self, key: (u8, u16), starting: bool) -> Option<usize> {
        if let Some(found) = self.slots.iter().position(|slot| slot.key == Some(key)) {
            return Some(found);
        }
        if !starting {
            return None;
        }
        let oldest = self
            .slots
            .iter()
            .enumerate()
            .min_by_key(|(_, slot)| (slot.key.is_some(), slot.used))
            .map(|(index, _)| index)?;
        if self.slots[oldest].state == State::Collecting {
            self.stats.files_lost = self.stats.files_lost.saturating_add(1);
        }
        self.slots[oldest].key = Some(key);
        self.slots[oldest].state = State::Idle;
        Some(oldest)
    }

    fn lose(&mut self, index: usize) {
        if self.slots[index].state == State::Collecting {
            self.stats.files_lost = self.stats.files_lost.saturating_add(1);
        }
        self.slots[index].state = State::Broken;
    }

    /// Takes one space packet from `vcid`; returns the file it completes.
    pub fn push(&mut self, vcid: u8, packet: &[u8]) -> Option<TransportFile<'_>> {
        self.finished = None;
        if packet.len() < PACKET_HEADER + CRC_LEN {
            return None;
        }
        let header = PacketHeader::parse(packet)?;
        if header.apid == IDLE_APID {
            return None;
        }
        let body = &packet[PACKET_HEADER..];
        let (user, check) = body.split_at(body.len() - CRC_LEN);
        if crc16_msb(CRC_POLY, CRC_INIT, user) != u16::from_be_bytes([check[0], check[1]]) {
            self.stats.packets_failed = self.stats.packets_failed.saturating_add(1);
            return None;
        }
        self.clock += 1;
        let index = self.slot_for((vcid, header.apid), header.segment.starts())?;
        self.slots[index].used = self.clock;
        if header.segment.starts() {
            self.start(index, header.sequence, user);
        } else {
            self.extend(index, header.sequence, user);
        }
        if header.segment.ends() && self.slots[index].state == State::Collecting {
            self.finish(index);
        }
        let index = self.finished?;
        let slot = &mut self.slots[index];
        Some(TransportFile {
            vcid,
            apid: header.apid,
            counter: slot.counter,
            lines_missing: slot.lines_missing,
            bytes: &mut slot.buf,
        })
    }

    fn start(&mut self, index: usize, sequence: u16, user: &[u8]) {
        if self.slots[index].state == State::Collecting {
            self.stats.files_lost = self.stats.files_lost.saturating_add(1);
        }
        let Some(transport) = user.get(..TRANSPORT_HEADER) else {
            self.slots[index].state = State::Broken;
            self.stats.files_lost = self.stats.files_lost.saturating_add(1);
            return;
        };
        let counter = u16::from_be_bytes([transport[0], transport[1]]);
        self.slots[index].begin(sequence, counter);
        self.take(index, &user[TRANSPORT_HEADER..]);
    }

    fn extend(&mut self, index: usize, sequence: u16, user: &[u8]) {
        let slot = &mut self.slots[index];
        if slot.state != State::Collecting {
            return;
        }
        let gap = sequence_gap(slot.sequence, sequence);
        slot.sequence = sequence;
        if gap > 0 {
            if slot.can_fill(gap) {
                slot.fill_lines(gap);
            } else {
                self.lose(index);
                return;
            }
        }
        self.take(index, user);
    }

    fn take(&mut self, index: usize, data: &[u8]) {
        if self.slots[index].buf.len() + data.len() > MAX_FILE {
            self.lose(index);
            return;
        }
        let slot = &mut self.slots[index];
        if slot.append(data) {
            return;
        }
        if slot.rice.is_some() {
            slot.fill_lines(1);
            self.stats.lines_failed = self.stats.lines_failed.saturating_add(1);
        } else {
            self.lose(index);
        }
    }

    fn finish(&mut self, index: usize) {
        let slot = &mut self.slots[index];
        if !slot.headers_done {
            self.lose(index);
            return;
        }
        let remaining = slot.lines_wanted.saturating_sub(slot.lines);
        slot.fill_lines(remaining);
        slot.state = State::Idle;
        self.stats.files = self.stats.files.saturating_add(1);
        self.finished = Some(index);
    }
}
