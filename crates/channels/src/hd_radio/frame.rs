//! Layer 2 of the P1 logical channel: the protocol control word, then one audio transport PDU
//! per program, each a Reed-Solomon protected header, program service data and HDC packets
//! (NRSC-5 1014s and 1017s).

use sdrmm_dsp::coding::fec::rs256::{GF256_PRIMITIVE, ReedSolomon};

use super::{id3, layout::P1_BITS};

/// Where the control word's 24 bits sit among a P1 frame's bits.
const PCI_START: usize = P1_BITS - 30_000;
const PCI_SPACING: usize = 1248;
const PCI_BITS: usize = 24;
const PCI_MAX_ERRORS: u32 = 4;
pub const PDU_BYTES: usize = (P1_BITS - PCI_BITS) / 8;
/// The header codeword: 88 data bytes and 8 parity bytes, sent last byte first.
const HEADER_BYTES: usize = 96;
const HEADER_PARITY: usize = 8;
const FIXED_HEADER: usize = 14;
pub const PROGRAMS: usize = 8;
const MAX_PACKETS: usize = 64;
/// Packet sequence numbers count modulo 64.
const SEQUENCES: u8 = 64;
pub const MAX_PACKET: usize = 1024;
const MAX_PSD: usize = 1024;
const HDLC_FLAG: u8 = 0x7e;
const HDLC_ESCAPE: u8 = 0x7d;
const FCS_GOOD: u16 = 0xf0b8;
/// An AAS frame: its protocol byte, then a port and a sequence number, both little endian.
const AAS_PROTOCOL: u8 = 0x21;
const AAS_HEADER: usize = 5;
/// Program service data arrives on port 0x5100 for the main program and 0x5201 to 0x5207
/// for the others, the program in the low three bits.
const PSD_MAIN_PORT: u16 = 0x5100;
const PSD_PORTS: u16 = 0x5200;

/// The control words a P1 frame may carry: audio, audio beside fixed data, fixed data alone,
/// and reserved ones.
const PCI_WORDS: [(u32, Content); 8] = [
    (0x38D8D3, Content::Audio),
    (0xCE3634, Content::Audio),
    (0xE3634C, Content::AudioFixed),
    (0x8D8D33, Content::AudioFixed),
    (0x3634CE, Content::Fixed),
    (0x8D338D, Content::Reserved),
    (0xD8D338, Content::Reserved),
    (0x634CE3, Content::Reserved),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Content {
    Audio,
    AudioFixed,
    Fixed,
    Reserved,
}

/// One audio program as its PDU headers describe it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioProgram {
    pub seen: bool,
    pub codec_mode: u8,
    pub program_type: u8,
    pub restricted: bool,
    pub blend: u8,
    pub latency: u8,
}

/// Splits a P1 frame into PDUs and their HDC packets and program service data.
pub struct Frame {
    bytes: Vec<u8>,
    rs: ReedSolomon,
    codeword: [u8; HEADER_BYTES],
    pub programs: [AudioProgram; PROGRAMS],
    halves: Vec<Vec<u8>>,
    packets: Vec<(usize, Vec<u8>)>,
    ready: usize,
    psd: Vec<Vec<u8>>,
    psd_open: [bool; PROGRAMS],
    unescaped: Vec<u8>,
    /// What each program's service data last said is playing.
    pub now_playing: [Option<String>; PROGRAMS],
    pub packets_ok: u32,
    pub packets_bad: u32,
    /// Packets of each program the last frame lost, to their CRC or to a PDU missing.
    pub lost: [u32; PROGRAMS],
    next_sequence: [Option<u8>; PROGRAMS],
    pub content: Option<Content>,
}

impl Frame {
    pub fn new() -> Self {
        Self {
            bytes: vec![0; PDU_BYTES],
            rs: ReedSolomon::new(GF256_PRIMITIVE, 1, HEADER_PARITY),
            codeword: [0; HEADER_BYTES],
            programs: [AudioProgram::default(); PROGRAMS],
            halves: (0..PROGRAMS)
                .map(|_| Vec::with_capacity(MAX_PACKET))
                .collect(),
            packets: (0..MAX_PACKETS)
                .map(|_| (0, Vec::with_capacity(MAX_PACKET)))
                .collect(),
            ready: 0,
            psd: (0..PROGRAMS).map(|_| Vec::with_capacity(MAX_PSD)).collect(),
            psd_open: [false; PROGRAMS],
            unescaped: Vec::with_capacity(MAX_PSD),
            now_playing: Default::default(),
            packets_ok: 0,
            packets_bad: 0,
            lost: [0; PROGRAMS],
            next_sequence: [None; PROGRAMS],
            content: None,
        }
    }

    pub fn reset(&mut self) {
        self.programs = [AudioProgram::default(); PROGRAMS];
        for half in &mut self.halves {
            half.clear();
        }
        for psd in &mut self.psd {
            psd.clear();
        }
        self.psd_open = [false; PROGRAMS];
        self.now_playing = Default::default();
        self.next_sequence = [None; PROGRAMS];
        self.ready = 0;
        self.content = None;
    }

    /// The packets the last frame completed.
    pub fn packets(&self) -> impl Iterator<Item = (usize, &[u8])> {
        self.packets[..self.ready]
            .iter()
            .map(|(program, bytes)| (*program, bytes.as_slice()))
    }

    /// Reads one descrambled P1 frame; returns false when its control word is not one NRSC-5
    /// defines, which means the frame was not decoded at all.
    pub fn push(&mut self, bits: &[u8]) -> bool {
        self.ready = 0;
        self.lost = [0; PROGRAMS];
        let pci = self.split(bits);
        let Some(content) = classify(pci) else {
            self.content = None;
            return false;
        };
        self.content = Some(content);
        if matches!(content, Content::Audio | Content::AudioFixed) {
            let end = PDU_BYTES;
            self.read_pdus(end);
        }
        true
    }

    /// Separates the control word's bits from the PDU's bytes, undoing the bit order the
    /// frame was sent in, each byte least significant bit first.
    fn split(&mut self, bits: &[u8]) -> u32 {
        let mut header = 0u32;
        let mut taken = 0;
        let mut byte = 0u8;
        let mut filled = 0;
        let mut out = 0;
        for i in 0..P1_BITS {
            let base = i & !7;
            let bit = bits[base + 7 - (i & 7)];
            let is_pci =
                i >= PCI_START && (i - PCI_START).is_multiple_of(PCI_SPACING) && taken < PCI_BITS;
            if is_pci {
                header |= u32::from(bit) << (PCI_BITS - 1 - taken);
                taken += 1;
                continue;
            }
            byte |= bit << (7 - filled);
            filled += 1;
            if filled == 8 {
                self.bytes[out] = byte;
                out += 1;
                byte = 0;
                filled = 0;
            }
        }
        header
    }

    fn read_pdus(&mut self, end: usize) {
        let mut offset = 0;
        while offset + HEADER_BYTES < end {
            match self.read_pdu(offset, end) {
                Some(next) if next > offset => offset = next,
                _ => return,
            }
        }
    }

    fn correct_header(&mut self, offset: usize) -> bool {
        for (i, slot) in self.codeword.iter_mut().enumerate() {
            *slot = self.bytes[offset + HEADER_BYTES - 1 - i];
        }
        if self.rs.decode(&mut self.codeword).is_none() {
            return false;
        }
        for i in 0..HEADER_BYTES {
            self.bytes[offset + i] = self.codeword[HEADER_BYTES - 1 - i];
        }
        true
    }

    /// Reads the PDU at `offset`; returns where the next one starts.
    fn read_pdu(&mut self, start: usize, end: usize) -> Option<usize> {
        if !self.correct_header(start) {
            return None;
        }
        let header = Header::parse(&self.bytes[start + 8..start + FIXED_HEADER]);
        let mut offset = start + FIXED_HEADER;
        let location_bits = header.location_bits();
        let location_bytes = (location_bits * usize::from(header.packets) + 4) / 8;
        let psd_end = start + usize::from(header.psd_end);
        if psd_end + 1 < offset + location_bytes || psd_end >= end {
            return None;
        }
        let mut locations = [0usize; MAX_PACKETS];
        for j in 0..usize::from(header.packets) {
            let location = location(&self.bytes[offset..], location_bits, j);
            let previous = if j == 0 {
                usize::from(header.psd_end)
            } else {
                locations[j - 1]
            };
            if location <= previous || start + location >= end {
                return None;
            }
            locations[j] = location;
        }
        let last = start + locations[usize::from(header.packets).max(1) - 1] + 1;
        offset += location_bytes;
        if header.stream != 0 {
            return Some(last);
        }
        let mut program = 0usize;
        if header.extended {
            offset += self.read_extension(offset, psd_end + 1, &mut program, header);
        }
        self.follow_sequence(program, header);
        self.read_psd(program, offset, psd_end + 1);
        offset = psd_end + 1;
        for (j, &location) in locations[..usize::from(header.packets)].iter().enumerate() {
            let count = start + location - offset;
            let ok = crc8(&self.bytes[offset..=offset + count]) == 0;
            let first_half = j == 0 && header.first_partial;
            let last_half = j + 1 == usize::from(header.packets) && header.last_partial;
            self.take_packet(program, offset, count, ok, first_half, last_half);
            offset += count + 1;
        }
        Some(offset)
    }

    fn read_extension(
        &mut self,
        from: usize,
        end: usize,
        program: &mut usize,
        header: Header,
    ) -> usize {
        let mut at = from;
        let mut info = AudioProgram {
            seen: true,
            codec_mode: header.codec_mode,
            blend: header.blend,
            latency: header.latency,
            ..AudioProgram::default()
        };
        while at < end {
            let byte = self.bytes[at];
            match (byte >> 4) & 7 {
                1 => {
                    *program = usize::from((byte >> 1) & 7);
                    if byte & 1 == 1 {
                        at += 2;
                    }
                }
                2 if at + 1 < end => {
                    info.restricted = (byte >> 3) & 1 == 1;
                    info.program_type = ((byte & 1) << 7) | (self.bytes[at + 1] & 0x7f);
                    at += 1;
                }
                3 => at += if byte & 8 != 0 { 4 } else { 3 },
                4 => at += if byte & 8 != 0 { 3 } else { 1 },
                _ => {}
            }
            at += 1;
            if byte & 0x80 == 0 {
                break;
            }
        }
        self.programs[*program] = info;
        at.min(end) - from
    }

    /// Counts the packets lost between this PDU and the program's last one, from the packet
    /// sequence numbers, so a PDU whose header failed still shows as lost audio.
    fn follow_sequence(&mut self, program: usize, header: Header) {
        let first = header.sequence.wrapping_sub(u8::from(header.first_partial)) % SEQUENCES;
        if let Some(expected) = self.next_sequence[program] {
            let missing = (first + SEQUENCES - expected) % SEQUENCES;
            if missing > 0 && missing < SEQUENCES / 2 {
                self.packets_bad = self.packets_bad.saturating_add(u32::from(missing));
                self.lost[program] = self.lost[program].saturating_add(u32::from(missing));
            }
        }
        let next = first
            .wrapping_add(header.packets)
            .wrapping_sub(u8::from(header.last_partial));
        self.next_sequence[program] = Some(next % SEQUENCES);
    }

    fn take_packet(
        &mut self,
        program: usize,
        offset: usize,
        count: usize,
        ok: bool,
        first_half: bool,
        last_half: bool,
    ) {
        let bytes = &self.bytes[offset..offset + count];
        let half = &mut self.halves[program];
        if first_half {
            if half.is_empty() {
                return;
            }
            if ok && half.len() + count <= MAX_PACKET {
                half.extend_from_slice(bytes);
                self.emit(program, ok);
            } else {
                self.count_lost(program);
            }
            self.halves[program].clear();
            return;
        }
        half.clear();
        if !ok {
            self.count_lost(program);
            return;
        }
        half.extend_from_slice(&bytes[..count.min(MAX_PACKET)]);
        if !last_half {
            self.emit(program, ok);
            self.halves[program].clear();
        }
    }

    fn count_lost(&mut self, program: usize) {
        self.packets_bad = self.packets_bad.saturating_add(1);
        self.lost[program] = self.lost[program].saturating_add(1);
    }

    fn emit(&mut self, program: usize, ok: bool) {
        if !ok {
            self.count_lost(program);
            return;
        }
        self.packets_ok = self.packets_ok.saturating_add(1);
        if self.ready == MAX_PACKETS {
            return;
        }
        let (slot_program, slot) = &mut self.packets[self.ready];
        *slot_program = program;
        slot.clear();
        slot.extend_from_slice(&self.halves[program]);
        self.ready += 1;
    }

    /// Feeds program service data through its HDLC framing; each whole frame whose FCS holds
    /// and whose port is a PSD port updates what that program is playing.
    fn read_psd(&mut self, program: usize, from: usize, to: usize) {
        for at in from..to.min(self.bytes.len()) {
            let byte = self.bytes[at];
            if byte == HDLC_FLAG {
                if self.psd_open[program] {
                    self.finish_psd(program);
                }
                self.psd[program].clear();
                self.psd_open[program] = true;
            } else if self.psd_open[program] {
                if self.psd[program].len() == MAX_PSD {
                    self.psd_open[program] = false;
                    continue;
                }
                self.psd[program].push(byte);
            }
        }
    }

    fn finish_psd(&mut self, program: usize) {
        self.unescaped.clear();
        let mut escaped = false;
        for &byte in &self.psd[program] {
            if escaped {
                self.unescaped.push(byte | 0x20);
                escaped = false;
            } else if byte == HDLC_ESCAPE {
                escaped = true;
            } else {
                self.unescaped.push(byte);
            }
        }
        let frame = self.unescaped.as_slice();
        if frame.len() < AAS_HEADER + 2 || fcs16(frame) != FCS_GOOD || frame[0] != AAS_PROTOCOL {
            return;
        }
        let port = u16::from_le_bytes([frame[1], frame[2]]);
        let is_psd = port == PSD_MAIN_PORT || (PSD_PORTS..PSD_PORTS + 8).contains(&port);
        let body = &frame[AAS_HEADER..frame.len() - 2];
        if let Some(now) = is_psd
            .then(|| id3::parse(body))
            .flatten()
            .and_then(|tags| tags.now_playing())
        {
            let slot = &mut self.now_playing[usize::from(port & 7)];
            if slot.as_deref() != Some(now.as_str()) {
                *slot = Some(now);
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Header {
    codec_mode: u8,
    stream: u8,
    blend: u8,
    latency: u8,
    first_partial: bool,
    last_partial: bool,
    sequence: u8,
    packets: u8,
    extended: bool,
    psd_end: u8,
}

impl Header {
    fn parse(b: &[u8]) -> Self {
        Self {
            codec_mode: b[0] & 0xf,
            stream: (b[0] >> 4) & 3,
            blend: (b[1] >> 1) & 3,
            latency: (b[2] >> 6) | ((b[3] & 1) << 2),
            first_partial: (b[3] >> 1) & 1 == 1,
            last_partial: (b[3] >> 2) & 1 == 1,
            sequence: (b[3] >> 3) | ((b[4] & 1) << 5),
            packets: (b[4] >> 1) & 0x3f,
            extended: b[4] >> 7 == 1,
            psd_end: b[5],
        }
    }

    const fn location_bits(self) -> usize {
        match self.codec_mode {
            1..=3 if self.stream == 0 => 12,
            10 | 13 => 12,
            _ => 16,
        }
    }
}

fn location(bytes: &[u8], bits: usize, index: usize) -> usize {
    if bits == 16 {
        usize::from(bytes[2 * index + 1]) << 8 | usize::from(bytes[2 * index])
    } else if index.is_multiple_of(2) {
        usize::from(bytes[index / 2 * 3 + 1] & 0xf) << 8 | usize::from(bytes[index / 2 * 3])
    } else {
        usize::from(bytes[index / 2 * 3 + 2]) << 4 | usize::from(bytes[index / 2 * 3 + 1] >> 4)
    }
}

fn classify(pci: u32) -> Option<Content> {
    PCI_WORDS
        .iter()
        .find(|(word, _)| (pci ^ word).count_ones() <= PCI_MAX_ERRORS)
        .map(|&(_, content)| content)
}

/// CRC-8 over x^8 + x^5 + x^4 + 1 from all ones, zero over a packet and its check byte.
fn crc8(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0xffu8, |crc, &byte| {
        let mut value = crc ^ byte;
        for _ in 0..8 {
            value = if value & 0x80 != 0 {
                value << 1 ^ 0x31
            } else {
                value << 1
            };
        }
        value
    })
}

/// The HDLC frame check sequence (RFC 1662).
fn fcs16(bytes: &[u8]) -> u16 {
    bytes.iter().fold(0xffffu16, |fcs, &byte| {
        let mut value = fcs ^ u16::from(byte);
        for _ in 0..8 {
            value = if value & 1 != 0 {
                value >> 1 ^ 0x8408
            } else {
                value >> 1
            };
        }
        value
    })
}

#[cfg(test)]
mod tests;
