use super::bits::Reader;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Sync {
    pub(crate) colour: u8,
    pub(crate) timeslot: u8,
    pub(crate) frame: u8,
    pub(crate) multiframe: u8,
    pub(crate) mcc: u16,
    pub(crate) mnc: u16,
}

pub(crate) fn parse_sync(bits: &[bool]) -> Option<Sync> {
    let mut r = Reader::new(bits);
    r.skip(4)?;
    let colour = r.take_u8(6)?;
    let timeslot = r.take_u8(2)? + 1;
    let frame = r.take_u8(5)?;
    let multiframe = r.take_u8(6)?;
    r.skip(8)?;
    let mcc = r.take_u16(10)?;
    let mnc = r.take_u16(14)?;
    ((1..=18).contains(&frame) && (1..=60).contains(&multiframe)).then_some(Sync {
        colour,
        timeslot,
        frame,
        multiframe,
        mcc,
        mnc,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Carrier {
    pub(crate) number: u16,
    pub(crate) band: u8,
    pub(crate) offset: u8,
    pub(crate) duplex: u8,
    pub(crate) reverse: bool,
}

const CARRIER_OFFSET_HZ: [i64; 4] = [0, 6_250, -6_250, 12_500];

const DUPLEX_KHZ: [[i32; 16]; 8] = [
    [
        -1, 1_600, 10_000, 10_000, 10_000, 10_000, 10_000, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    ],
    [
        -1, 4_500, -1, 36_000, 7_000, -1, -1, -1, 45_000, 45_000, -1, -1, -1, -1, -1, -1,
    ],
    [0; 16],
    [
        -1, -1, -1, 8_000, 8_000, -1, -1, -1, 18_000, 18_000, -1, -1, -1, -1, -1, -1,
    ],
    [
        -1, -1, -1, 18_000, 5_000, -1, 30_000, 30_000, -1, 39_000, -1, -1, -1, -1, -1, -1,
    ],
    [
        -1, -1, -1, -1, 9_500, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    ],
    [-1; 16],
    [-1; 16],
];

impl Carrier {
    pub(crate) fn downlink_hz(self) -> u64 {
        let hz = i64::from(self.band) * 100_000_000
            + i64::from(self.number) * 25_000
            + CARRIER_OFFSET_HZ[usize::from(self.offset & 3)];
        u64::try_from(hz).unwrap_or(0)
    }

    pub(crate) fn uplink_hz(self) -> Option<u64> {
        let spacing = DUPLEX_KHZ[usize::from(self.duplex & 7)][usize::from(self.band & 15)];
        if spacing < 0 {
            return None;
        }
        let spacing = u64::from(spacing.unsigned_abs()) * 1_000;
        let downlink = self.downlink_hz();
        Some(if self.reverse {
            downlink + spacing
        } else {
            downlink.saturating_sub(spacing)
        })
    }
}

pub(crate) const SERVICE_AIR_ENCRYPTION: u16 = 1 << 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SysInfo {
    pub(crate) carrier: Carrier,
    pub(crate) location_area: u16,
    pub(crate) services: u16,
}

impl SysInfo {
    pub(crate) fn air_encryption(&self) -> bool {
        self.services & SERVICE_AIR_ENCRYPTION != 0
    }
}

pub(crate) fn parse_sysinfo(bits: &[bool]) -> Option<SysInfo> {
    let mut r = Reader::new(bits);
    if r.take(2)? != 0b10 || r.take(2)? != 0b00 {
        return None;
    }
    let number = r.take_u16(12)?;
    let band = r.take_u8(4)?;
    let offset = r.take_u8(2)?;
    let duplex = r.take_u8(3)?;
    let reverse = r.flag()?;
    r.skip(2 + 3 + 4 + 4 + 4 + 1 + 16 + 2 + 20)?;
    let location_area = r.take_u16(14)?;
    r.skip(16)?;
    let services = r.take_u16(12)?;
    Some(SysInfo {
        carrier: Carrier {
            number,
            band,
            offset,
            duplex,
            reverse,
        },
        location_area,
        services,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Downlink {
    Control,
    Unallocated,
    Traffic(u8),
}

pub(crate) fn parse_access_assign(info: u16, frame: u8) -> Downlink {
    let header = info >> 12;
    let field1 = (info >> 6 & 0x3F) as u8;
    if frame == 18 || header == 0 {
        return Downlink::Control;
    }
    match field1 {
        0 => Downlink::Unallocated,
        1..=3 => Downlink::Control,
        marker => Downlink::Traffic(marker),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Address {
    Null,
    Ssi(u32),
    EventLabel(u16),
    Ussi(u32),
    Smi(u32),
    SsiEvent(u32),
    SsiUsage(u32, u8),
    SmiEvent(u32),
}

impl Address {
    pub(crate) fn ssi(self) -> Option<u32> {
        match self {
            Self::Ssi(ssi)
            | Self::Ussi(ssi)
            | Self::Smi(ssi)
            | Self::SsiEvent(ssi)
            | Self::SsiUsage(ssi, _)
            | Self::SmiEvent(ssi) => Some(ssi),
            Self::Null | Self::EventLabel(_) => None,
        }
    }

    pub(crate) fn usage_marker(self) -> Option<u8> {
        match self {
            Self::SsiUsage(_, marker) => Some(marker),
            _ => None,
        }
    }
}

fn parse_address(r: &mut Reader<'_>) -> Option<Address> {
    Some(match r.take(3)? {
        0 => Address::Null,
        1 => Address::Ssi(r.take(24)?),
        2 => Address::EventLabel(r.take_u16(10)?),
        3 => Address::Ussi(r.take(24)?),
        4 => Address::Smi(r.take(24)?),
        5 => {
            let ssi = r.take(24)?;
            r.skip(10)?;
            Address::SsiEvent(ssi)
        }
        6 => Address::SsiUsage(r.take(24)?, r.take_u8(6)?),
        _ => {
            let ssi = r.take(24)?;
            r.skip(10)?;
            Address::SmiEvent(ssi)
        }
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Length {
    Bits(usize),
    SecondHalfStolen,
    StartsFragment,
}

fn parse_length(indicator: u8) -> Option<Length> {
    match indicator {
        1..=0x3A => Some(Length::Bits(usize::from(indicator) * 8)),
        0x3E => Some(Length::SecondHalfStolen),
        0x3F => Some(Length::StartsFragment),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Allocation {
    pub(crate) timeslots: u8,
    pub(crate) carrier: u16,
}

fn parse_allocation(r: &mut Reader<'_>) -> Option<Allocation> {
    r.skip(2)?;
    let timeslots = r.take_u8(4)?;
    let direction = r.take(2)?;
    r.skip(2)?;
    let carrier = r.take_u16(12)?;
    if r.flag()? {
        r.skip(10)?;
    }
    if r.take(2)? == 0 {
        r.skip(2)?;
    }
    if direction == 0 {
        skip_augmented_allocation(r)?;
    }
    Some(Allocation { timeslots, carrier })
}

fn skip_augmented_allocation(r: &mut Reader<'_>) -> Option<()> {
    r.skip(2 + 3 + 3 + 3 + 3 + 3 + 4 + 5)?;
    if r.take(2)? == 1 {
        r.skip(11)?;
    }
    r.skip(4)?;
    for _ in 0..2 {
        if r.flag()? {
            r.skip(16)?;
        }
    }
    r.skip(1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Resource {
    pub(crate) encrypted: bool,
    pub(crate) length: Length,
    pub(crate) fill: bool,
    pub(crate) address: Address,
    pub(crate) allocation: Option<Allocation>,
    pub(crate) header_bits: usize,
}

pub(crate) fn parse_resource(bits: &[bool]) -> Option<Resource> {
    let mut r = Reader::new(bits);
    if r.take(2)? != 0b00 {
        return None;
    }
    let fill = r.flag()?;
    r.skip(1)?;
    let encrypted = r.take(2)? != 0;
    r.skip(1)?;
    let length = parse_length(r.take_u8(6)?)?;
    let address = parse_address(&mut r)?;
    if address == Address::Null {
        return Some(Resource {
            encrypted,
            length,
            fill,
            address,
            allocation: None,
            header_bits: r.position(),
        });
    }
    if r.flag()? {
        r.skip(4)?;
    }
    if r.flag()? {
        r.skip(8)?;
    }
    let allocated = r.flag()?;
    let allocation = if allocated && !encrypted {
        Some(parse_allocation(&mut r)?)
    } else {
        None
    };
    Some(Resource {
        encrypted,
        length,
        fill,
        address,
        allocation,
        header_bits: r.position(),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct End {
    pub(crate) length: usize,
    pub(crate) fill: bool,
    pub(crate) header_bits: usize,
}

pub(crate) fn parse_end(bits: &[bool]) -> Option<End> {
    let mut r = Reader::new(bits);
    if r.take(3)? != 0b011 {
        return None;
    }
    let fill = r.flag()?;
    r.skip(1)?;
    let Length::Bits(length) = parse_length(r.take_u8(6)?)? else {
        return None;
    };
    if r.flag()? {
        r.skip(8)?;
    }
    if r.flag()? {
        parse_allocation(&mut r)?;
    }
    Some(End {
        length,
        fill,
        header_bits: r.position(),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PduType {
    Resource,
    Fragment,
    End,
    Broadcast,
    Supplementary,
}

pub(crate) fn pdu_type(bits: &[bool]) -> Option<PduType> {
    let mut r = Reader::new(bits);
    Some(match r.take(2)? {
        0 => PduType::Resource,
        1 if r.flag()? => PduType::End,
        1 => PduType::Fragment,
        2 => PduType::Broadcast,
        _ => PduType::Supplementary,
    })
}

pub(crate) const FRAGMENT_HEADER_BITS: usize = 4;
