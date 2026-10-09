use sdrmm_wire::{BleBeacon, BleFlags, BleManufacturer, BleService};

use super::{beacon, names};

const FLAGS: u8 = 0x01;
const UUID16_SOME: u8 = 0x02;
const UUID16_ALL: u8 = 0x03;
const UUID32_SOME: u8 = 0x04;
const UUID32_ALL: u8 = 0x05;
const UUID128_SOME: u8 = 0x06;
const UUID128_ALL: u8 = 0x07;
const NAME_SHORT: u8 = 0x08;
const NAME_COMPLETE: u8 = 0x09;
const TX_POWER: u8 = 0x0A;
pub(crate) const SERVICE_DATA_16: u8 = 0x16;
const APPEARANCE: u8 = 0x19;
const SERVICE_DATA_32: u8 = 0x20;
const SERVICE_DATA_128: u8 = 0x21;
const URI: u8 = 0x24;
const MANUFACTURER: u8 = 0xFF;
const BASE_UUID_TAIL: [u8; 12] = [
    0xFB, 0x34, 0x9B, 0x5F, 0x80, 0x00, 0x00, 0x80, 0x00, 0x10, 0x00, 0x00,
];

pub(crate) fn structures(data: &[u8]) -> impl Iterator<Item = (u8, &[u8])> {
    let mut rest = data;
    std::iter::from_fn(move || {
        let (&length, tail) = rest.split_first()?;
        let (structure, next) = tail.split_at_checked(usize::from(length))?;
        let (&kind, body) = structure.split_first()?;
        rest = next;
        Some((kind, body))
    })
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Fields {
    pub name: Option<String>,
    pub flags: Option<BleFlags>,
    pub tx_power_dbm: Option<i8>,
    pub appearance: Option<u16>,
    pub services: Vec<BleService>,
    pub manufacturer: Vec<BleManufacturer>,
    pub uri: Option<String>,
    pub beacon: Option<BleBeacon>,
}

impl Fields {
    pub(crate) fn read(data: &[u8]) -> Self {
        let mut fields = Self::default();
        for (kind, body) in structures(data) {
            fields.take(kind, body);
        }
        fields
    }

    fn take(&mut self, kind: u8, body: &[u8]) {
        match kind {
            FLAGS => {
                self.flags = body.first().map(|&bits| BleFlags {
                    limited: bits & 0x01 != 0,
                    general: bits & 0x02 != 0,
                    le_only: bits & 0x04 != 0,
                });
            }
            UUID16_SOME | UUID16_ALL => self.list(body, 2),
            UUID32_SOME | UUID32_ALL => self.list(body, 4),
            UUID128_SOME | UUID128_ALL => self.list(body, 16),
            NAME_SHORT if self.name.is_none() => self.name = Some(text(body)),
            NAME_COMPLETE => self.name = Some(text(body)),
            TX_POWER => self.tx_power_dbm = body.first().map(|&power| power as i8),
            APPEARANCE => {
                if let [low, high] = *body {
                    self.appearance = Some(u16::from_le_bytes([low, high]));
                }
            }
            SERVICE_DATA_16 => self.service_data(body, 2),
            SERVICE_DATA_32 => self.service_data(body, 4),
            SERVICE_DATA_128 => self.service_data(body, 16),
            URI => self.uri = uri(body),
            MANUFACTURER => {
                if let [low, high, data @ ..] = body {
                    let company_id = u16::from_le_bytes([*low, *high]);
                    self.beacon = self.beacon.take().or_else(|| beacon::maker(company_id, data));
                    self.manufacturer.push(BleManufacturer {
                        company_id,
                        company: names::company(company_id).map(str::to_owned),
                        data: hex(data),
                    });
                }
            }
            _ => {}
        }
    }

    fn list(&mut self, body: &[u8], width: usize) {
        for uuid in body.chunks_exact(width) {
            self.services.push(service(uuid, None));
        }
    }

    fn service_data(&mut self, body: &[u8], width: usize) {
        let Some((uuid, data)) = body.split_at_checked(width) else {
            return;
        };
        if let Some(short) = short_uuid(uuid) {
            self.beacon = self.beacon.take().or_else(|| beacon::service(short, data));
        }
        self.services.push(service(uuid, Some(data)));
    }
}

fn short_uuid(uuid: &[u8]) -> Option<u16> {
    match uuid {
        [low, high] | [low, high, 0, 0] => Some(u16::from_le_bytes([*low, *high])),
        [tail @ .., low, high, 0, 0] if tail == BASE_UUID_TAIL => {
            Some(u16::from_le_bytes([*low, *high]))
        }
        _ => None,
    }
}

fn service(uuid: &[u8], data: Option<&[u8]>) -> BleService {
    let short = short_uuid(uuid);
    BleService {
        uuid: short.map_or_else(|| uuid_text(uuid), |short| format!("{short:04X}")),
        name: short.and_then(names::service).map(str::to_owned),
        data: data.map(hex),
    }
}

fn uuid_text(uuid: &[u8]) -> String {
    let mut text = String::with_capacity(36);
    for (index, byte) in uuid.iter().rev().enumerate() {
        if uuid.len() == 16 && matches!(index, 4 | 6 | 8 | 10) {
            text.push('-');
        }
        text.push_str(&format!("{byte:02X}"));
    }
    text
}

fn uri(body: &[u8]) -> Option<String> {
    let (&scheme, rest) = body.split_first()?;
    let prefix = match scheme {
        0x16 => "http:",
        0x17 => "https:",
        _ => "",
    };
    Some(format!("{prefix}{}", text(rest)))
}

#[must_use]
pub(crate) fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

#[must_use]
pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}
