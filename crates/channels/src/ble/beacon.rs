use sdrmm_wire::BleBeacon;

use super::ad::hex;

const APPLE: u16 = 0x004C;
const IBEACON: [u8; 2] = [0x02, 0x15];
const FIND_MY: [u8; 2] = [0x12, 0x19];
const FIND_MY_MAINTAINED: u8 = 0x04;
const ALTBEACON: [u8; 2] = [0xBE, 0xAC];
const EDDYSTONE: u16 = 0xFEAA;
const EXPOSURE_NOTIFICATION: u16 = 0xFD6F;
const FAST_PAIR: u16 = 0xFE2C;
const TLM_NO_BATTERY: u16 = 0;
const TLM_NO_TEMPERATURE: u16 = 0x8000;
const DECISECONDS: f64 = 10.0;
const URL_SCHEMES: [&str; 4] = ["http://www.", "https://www.", "http://", "https://"];
const URL_EXPANSIONS: [&str; 14] = [
    ".com/", ".org/", ".edu/", ".net/", ".info/", ".biz/", ".gov/", ".com", ".org", ".edu", ".net",
    ".info", ".biz", ".gov",
];

pub(super) fn maker(company: u16, data: &[u8]) -> Option<BleBeacon> {
    match (company, data) {
        (APPLE, [a, b, uuid @ .., m1, m2, n1, n2, power])
            if [*a, *b] == IBEACON && uuid.len() == 16 =>
        {
            Some(BleBeacon::Ibeacon {
                uuid: uuid_text(uuid),
                major: u16::from_be_bytes([*m1, *m2]),
                minor: u16::from_be_bytes([*n1, *n2]),
                measured_dbm: *power as i8,
            })
        }
        (APPLE, [a, b, status, ..]) if [*a, *b] == FIND_MY => Some(BleBeacon::FindMy {
            maintained: status & FIND_MY_MAINTAINED != 0,
        }),
        (_, [a, b, id @ .., power, _]) if [*a, *b] == ALTBEACON && id.len() == 20 => {
            Some(BleBeacon::AltBeacon {
                id: hex(id),
                measured_dbm: *power as i8,
            })
        }
        _ => None,
    }
}

pub(super) fn service(uuid: u16, data: &[u8]) -> Option<BleBeacon> {
    match uuid {
        EDDYSTONE => eddystone(data),
        EXPOSURE_NOTIFICATION => Some(BleBeacon::ExposureNotification {
            identifier: hex(data.get(..16)?),
        }),
        FAST_PAIR if data.len() == 3 => Some(BleBeacon::FastPair { model: hex(data) }),
        _ => None,
    }
}

fn eddystone(data: &[u8]) -> Option<BleBeacon> {
    let (&frame, rest) = data.split_first()?;
    match (frame, rest) {
        (0x00, [power, id @ ..]) if id.len() >= 16 => Some(BleBeacon::EddystoneUid {
            namespace: hex(&id[..10]),
            instance: hex(&id[10..16]),
            tx_power_dbm: *power as i8,
        }),
        (0x10, [power, scheme, encoded @ ..]) => Some(BleBeacon::EddystoneUrl {
            url: url(*scheme, encoded)?,
            tx_power_dbm: *power as i8,
        }),
        (0x20, [0, v1, v2, t1, t2, a1, a2, a3, a4, s1, s2, s3, s4, ..]) => {
            let battery = u16::from_be_bytes([*v1, *v2]);
            let temperature = u16::from_be_bytes([*t1, *t2]);
            Some(BleBeacon::EddystoneTlm {
                battery_mv: (battery != TLM_NO_BATTERY).then_some(battery),
                temperature_c: (temperature != TLM_NO_TEMPERATURE)
                    .then(|| f32::from(temperature as i16) / 256.0),
                adverts: u32::from_be_bytes([*a1, *a2, *a3, *a4]),
                uptime_s: f64::from(u32::from_be_bytes([*s1, *s2, *s3, *s4])) / DECISECONDS,
            })
        }
        (0x30, [power, eid @ ..]) if eid.len() >= 8 => Some(BleBeacon::EddystoneEid {
            eid: hex(&eid[..8]),
            tx_power_dbm: *power as i8,
        }),
        _ => None,
    }
}

fn url(scheme: u8, encoded: &[u8]) -> Option<String> {
    let mut url = (*URL_SCHEMES.get(usize::from(scheme))?).to_owned();
    for &byte in encoded {
        match URL_EXPANSIONS.get(usize::from(byte)) {
            Some(expansion) => url.push_str(expansion),
            None if byte.is_ascii_graphic() => url.push(char::from(byte)),
            None => return None,
        }
    }
    Some(url)
}

fn uuid_text(uuid: &[u8]) -> String {
    let text = hex(uuid).to_uppercase();
    format!(
        "{}-{}-{}-{}-{}",
        &text[..8],
        &text[8..12],
        &text[12..16],
        &text[16..20],
        &text[20..]
    )
}
