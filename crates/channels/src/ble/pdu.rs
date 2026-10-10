use sdrmm_wire::{BleAddress, BleAddressKind, BleAdi, BleAuxPointer, BlePdu, BlePhy};

pub(crate) const ADDRESS_BYTES: usize = 6;
const PRIMARY_CHANNELS: std::ops::RangeInclusive<u8> = 37..=39;
const TX_ADD: u8 = 0x40;
const RX_ADD: u8 = 0x80;
const HAS_ADV_A: u8 = 0x01;
const HAS_TARGET_A: u8 = 0x02;
const HAS_CTE_INFO: u8 = 0x04;
const HAS_ADI: u8 = 0x08;
const HAS_AUX_PTR: u8 = 0x10;
const HAS_SYNC_INFO: u8 = 0x20;
const HAS_TX_POWER: u8 = 0x40;
const SYNC_INFO_BYTES: usize = 18;
const SMALL_OFFSET_US: u32 = 30;
const LARGE_OFFSET_US: u32 = 300;

pub(crate) type Address = [u8; ADDRESS_BYTES];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Party {
    pub bytes: Address,
    pub random: bool,
}

impl Party {
    fn read(bytes: &[u8], random: bool) -> Option<Self> {
        Some(Self {
            bytes: bytes.get(..ADDRESS_BYTES)?.try_into().ok()?,
            random,
        })
    }

    #[must_use]
    pub(crate) fn text(&self) -> String {
        address_text(&self.bytes)
    }

    #[must_use]
    pub(crate) fn kind(&self) -> BleAddressKind {
        if !self.random {
            return BleAddressKind::Public;
        }
        match self.bytes[ADDRESS_BYTES - 1] >> 6 {
            0b11 => BleAddressKind::RandomStatic,
            0b01 => BleAddressKind::ResolvablePrivate,
            0b00 => BleAddressKind::NonResolvablePrivate,
            _ => BleAddressKind::Reserved,
        }
    }

    #[must_use]
    pub(crate) fn wire(&self) -> BleAddress {
        BleAddress {
            address: self.text(),
            kind: self.kind(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Extended {
    pub adi: Option<BleAdi>,
    pub aux: Option<BleAuxPointer>,
    pub tx_power_dbm: Option<i8>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AdvPdu<'a> {
    pub kind: BlePdu,
    pub sender: Option<Party>,
    pub target: Option<Party>,
    pub data: &'a [u8],
    pub extended: Option<Extended>,
}

impl AdvPdu<'_> {
    #[must_use]
    pub(crate) fn is_extended(&self) -> bool {
        self.extended.is_some()
    }
}

#[must_use]
pub(crate) fn parse(pdu: &[u8], channel: Option<u8>) -> Option<AdvPdu<'_>> {
    let [header, length, payload @ ..] = pdu else {
        return None;
    };
    let payload = payload.get(..usize::from(*length))?;
    let tx = *header & TX_ADD != 0;
    let rx = *header & RX_ADD != 0;
    let legacy = |kind, data_from: usize| {
        Some(AdvPdu {
            kind,
            sender: Some(Party::read(payload, tx)?),
            target: None,
            data: payload.get(data_from..)?,
            extended: None,
        })
    };
    match header & 0x0F {
        0 => legacy(BlePdu::AdvInd, ADDRESS_BYTES),
        2 => legacy(BlePdu::AdvNonconnInd, ADDRESS_BYTES),
        4 => legacy(BlePdu::ScanRsp, ADDRESS_BYTES),
        6 => legacy(BlePdu::AdvScanInd, ADDRESS_BYTES),
        1 => directed(BlePdu::AdvDirectInd, payload, tx, rx),
        3 => directed(BlePdu::ScanReq, payload, tx, rx),
        5 => directed(BlePdu::ConnectInd, payload, tx, rx),
        7 => {
            let primary = channel.is_some_and(|index| PRIMARY_CHANNELS.contains(&index));
            let kind = if primary {
                BlePdu::AdvExtInd
            } else {
                BlePdu::AuxAdvInd
            };
            extended(kind, payload, tx, rx)
        }
        8 => extended(BlePdu::AuxConnectRsp, payload, tx, rx),
        _ => None,
    }
}

fn directed(kind: BlePdu, payload: &[u8], tx: bool, rx: bool) -> Option<AdvPdu<'_>> {
    Some(AdvPdu {
        kind,
        sender: Some(Party::read(payload, tx)?),
        target: Some(Party::read(payload.get(ADDRESS_BYTES..)?, rx)?),
        data: &[],
        extended: None,
    })
}

fn extended(kind: BlePdu, payload: &[u8], tx: bool, rx: bool) -> Option<AdvPdu<'_>> {
    let (&first, rest) = payload.split_first()?;
    let (header, data) = rest.split_at_checked(usize::from(first & 0x3F))?;
    let mut pdu = AdvPdu {
        kind,
        sender: None,
        target: None,
        data,
        extended: Some(Extended::default()),
    };
    let Some((&flags, mut fields)) = header.split_first() else {
        return Some(pdu);
    };
    let mut take = |present: u8, bytes: usize| -> Option<Option<&[u8]>> {
        if flags & present == 0 {
            return Some(None);
        }
        let (field, rest) = fields.split_at_checked(bytes)?;
        fields = rest;
        Some(Some(field))
    };
    pdu.sender = take(HAS_ADV_A, ADDRESS_BYTES)?.and_then(|bytes| Party::read(bytes, tx));
    pdu.target = take(HAS_TARGET_A, ADDRESS_BYTES)?.and_then(|bytes| Party::read(bytes, rx));
    take(HAS_CTE_INFO, 1)?;
    let adi = take(HAS_ADI, 2)?.map(adi);
    let aux = take(HAS_AUX_PTR, 3)?.and_then(aux_pointer);
    take(HAS_SYNC_INFO, SYNC_INFO_BYTES)?;
    let tx_power_dbm = take(HAS_TX_POWER, 1)?.map(|bytes| bytes[0] as i8);
    pdu.extended = Some(Extended {
        adi,
        aux,
        tx_power_dbm,
    });
    Some(pdu)
}

fn adi(bytes: &[u8]) -> BleAdi {
    let value = u16::from_le_bytes([bytes[0], bytes[1]]);
    BleAdi {
        set: (value >> 12) as u8,
        data_id: value & 0x0FFF,
    }
}

fn aux_pointer(bytes: &[u8]) -> Option<BleAuxPointer> {
    let [first, low, high] = *bytes else {
        return None;
    };
    let rest = u16::from_le_bytes([low, high]);
    let unit = if first & 0x80 == 0 {
        SMALL_OFFSET_US
    } else {
        LARGE_OFFSET_US
    };
    let phy = match rest >> 13 {
        0 => BlePhy::Le1m,
        1 => BlePhy::Le2m,
        2 => BlePhy::LeCoded,
        _ => return None,
    };
    Some(BleAuxPointer {
        channel: first & 0x3F,
        phy,
        offset_us: u32::from(rest & 0x1FFF) * unit,
    })
}

#[must_use]
pub(crate) fn address_text(address: &Address) -> String {
    let mut text = String::with_capacity(17);
    for (index, byte) in address.iter().rev().enumerate() {
        if index > 0 {
            text.push(':');
        }
        text.push_str(&format!("{byte:02X}"));
    }
    text
}
