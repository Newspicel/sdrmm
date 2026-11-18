use crate::ble::BleBeacon;
use crate::ble::BleFlags;
use crate::ble::BleManufacturer;
use crate::ble::BleService;
use sdrmm_crypto::hex_string as hex;

use super::{beacon, names};
use sdrmm_modem::ble::ad::{
    APPEARANCE, FLAGS, MANUFACTURER, NAME_COMPLETE, NAME_SHORT, SERVICE_DATA_16, SERVICE_DATA_32,
    SERVICE_DATA_128, TX_POWER, URI, UUID16_ALL, UUID16_SOME, UUID32_ALL, UUID32_SOME, UUID128_ALL,
    UUID128_SOME, structures,
};

const BASE_UUID_TAIL: [u8; 12] = [
    0xFB, 0x34, 0x9B, 0x5F, 0x80, 0x00, 0x00, 0x80, 0x00, 0x10, 0x00, 0x00,
];

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
    pub truncated: u32,
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
                    self.beacon = self
                        .beacon
                        .take()
                        .or_else(|| beacon::maker(company_id, data));
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
        let mut rest = body;
        while let Some((uuid, next)) = rest.split_at_checked(width) {
            self.services.push(service(uuid, None));
            rest = next;
        }
        if !rest.is_empty() {
            self.truncated = self.truncated.saturating_add(1);
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
