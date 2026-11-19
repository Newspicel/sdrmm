use super::link::{
    MpduLayout, NO_PACKET_START, PACKET_HEADER, PacketHeader, VCDU_BYTES, VcduHeader,
};

/// The largest space packet: a 16-bit length field over the data field.
pub const MAX_PACKET: usize = PACKET_HEADER + (1 << 16);

/// Cuts the space packets of one virtual channel out of its `M_PDU`s.
///
/// Packets span VCDUs; a gap in the frame counter abandons the one in progress and the next
/// first-header pointer picks up again. Abandoned and oversized packets are counted.
pub struct Depacketizer {
    layout: MpduLayout,
    vcid: Option<u8>,
    zone: [u8; VCDU_BYTES],
    zone_len: usize,
    at: usize,
    boundary: Option<usize>,
    packet: Vec<u8>,
    capacity: usize,
    need: usize,
    assembling: bool,
    counter: Option<u32>,
    dropped: u32,
}

impl Depacketizer {
    #[must_use]
    pub fn new(layout: MpduLayout, capacity: usize) -> Self {
        let capacity = capacity.clamp(PACKET_HEADER + 1, MAX_PACKET);
        Self {
            layout,
            vcid: None,
            zone: [0; VCDU_BYTES],
            zone_len: 0,
            at: 0,
            boundary: None,
            packet: Vec::with_capacity(capacity),
            capacity,
            need: PACKET_HEADER,
            assembling: false,
            counter: None,
            dropped: 0,
        }
    }

    /// The same depacketizer passing over units of every other virtual channel.
    #[must_use]
    pub const fn with_vcid(mut self, vcid: u8) -> Self {
        self.vcid = Some(vcid);
        self
    }

    pub fn reset(&mut self) {
        self.abandon();
        self.zone_len = 0;
        self.at = 0;
        self.boundary = None;
        self.counter = None;
    }

    /// Packets abandoned part-built or longer than the capacity.
    #[must_use]
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    fn abandon(&mut self) {
        if self.assembling && !self.packet.is_empty() {
            self.dropped = self.dropped.saturating_add(1);
        }
        self.assembling = false;
    }

    /// Takes the next VCDU. One too short to hold a whole unit is passed over; the counter gap
    /// it leaves counts the packet it cut.
    pub fn load(&mut self, vcdu: &[u8]) {
        self.zone_len = 0;
        self.at = 0;
        self.boundary = None;
        let zone_at = self.layout.zone_at();
        let (Some(header), Some(first_header), Some(zone)) = (
            VcduHeader::parse(vcdu),
            self.layout.first_header(vcdu),
            vcdu.get(zone_at..VCDU_BYTES),
        ) else {
            return;
        };
        if self.vcid.is_some_and(|vcid| vcid != header.vcid) {
            return;
        }
        let continuous = self
            .counter
            .is_some_and(|previous| header.follows(previous));
        self.counter = Some(header.counter);
        if !continuous {
            self.abandon();
        }
        let zone_len = zone.len();
        self.zone[..zone_len].copy_from_slice(zone);
        self.zone_len = zone_len;
        let first = usize::from(first_header);
        let pointer = (first_header != NO_PACKET_START && first < zone_len).then_some(first);
        match (self.assembling, pointer) {
            (true, pointer) => self.boundary = pointer,
            (false, Some(first)) => self.at = first,
            (false, None) => self.at = zone_len,
        }
    }

    fn start_packet(&mut self) {
        self.packet.clear();
        self.need = PACKET_HEADER;
        self.assembling = true;
    }

    fn skip_oversized(&mut self) {
        self.dropped = self.dropped.saturating_add(1);
        self.assembling = false;
        self.packet.clear();
        self.at = self.boundary.unwrap_or(self.zone_len);
    }

    pub fn next_packet(&mut self) -> Option<&[u8]> {
        loop {
            if self.at >= self.zone_len {
                return None;
            }
            if let Some(boundary) = self.boundary
                && self.at >= boundary
            {
                self.boundary = None;
                self.abandon();
            }
            if !self.assembling {
                self.start_packet();
            }
            let limit = self.boundary.unwrap_or(self.zone_len).min(self.zone_len);
            let take = (self.need - self.packet.len()).min(limit - self.at);
            self.packet
                .extend_from_slice(&self.zone[self.at..self.at + take]);
            self.at += take;
            if self.need == PACKET_HEADER
                && let Some(header) = PacketHeader::parse(&self.packet)
            {
                self.need = PACKET_HEADER + header.length;
                if self.need > self.capacity {
                    self.skip_oversized();
                    continue;
                }
            }
            if self.need > PACKET_HEADER && self.packet.len() == self.need {
                self.assembling = false;
                break;
            }
        }
        Some(&self.packet)
    }
}
