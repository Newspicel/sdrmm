use sdrmm_wire::{WmbusKey, WmbusRecord, WmbusSecurity};

use super::{coding::crc, records};
use crate::crypto::{self, Aes};

const LINK_ADDRESS: usize = 2;
const LINK_CI: usize = 10;
const ELL_SHORT: u8 = 0x8C;
const ELL_LONG: u8 = 0x8D;
const TPL_LONG: u8 = 0x72;
const TPL_SHORT: u8 = 0x7A;
const TPL_NONE: u8 = 0x78;
const ELL_PLAIN: u32 = 0;
const ELL_AES_CTR: u32 = 1;
const PLAIN: u8 = 0;
const AES_CBC_IV: u8 = 5;
const VERIFIED: [u8; 2] = [0x2F, 0x2F];
const BLOCK: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Address {
    manufacturer: [u8; 2],
    id: [u8; 4],
    pub(super) version: u8,
    pub(super) device_type: u8,
}

impl Address {
    fn read(bytes: &[u8]) -> Option<Self> {
        let bytes: &[u8; 8] = bytes.get(..8)?.try_into().ok()?;
        Some(Self {
            manufacturer: [bytes[0], bytes[1]],
            id: [bytes[2], bytes[3], bytes[4], bytes[5]],
            version: bytes[6],
            device_type: bytes[7],
        })
    }

    fn long_header(bytes: &[u8]) -> Option<Self> {
        let bytes: &[u8; 8] = bytes.get(..8)?.try_into().ok()?;
        Some(Self {
            id: [bytes[0], bytes[1], bytes[2], bytes[3]],
            manufacturer: [bytes[4], bytes[5]],
            version: bytes[6],
            device_type: bytes[7],
        })
    }

    fn wire(&self) -> [u8; 8] {
        let [m0, m1] = self.manufacturer;
        let [a, b, c, d] = self.id;
        [m0, m1, a, b, c, d, self.version, self.device_type]
    }

    pub(super) fn id(&self) -> String {
        let [a, b, c, d] = self.id;
        format!("{d:02X}{c:02X}{b:02X}{a:02X}")
    }

    pub(super) fn manufacturer(&self) -> String {
        let code = u16::from_le_bytes(self.manufacturer);
        [10, 5, 0]
            .iter()
            .map(|shift| char::from(b'@' + ((code >> shift) & 0x1F) as u8))
            .collect()
    }

    pub(super) fn station(&self) -> String {
        format!("{} {}", self.manufacturer(), self.id())
    }
}

pub(super) struct Keys(Vec<(String, Aes)>);

impl Keys {
    pub(super) fn new(keys: &[WmbusKey]) -> Self {
        Self(
            keys.iter()
                .filter_map(|entry| {
                    let key = crypto::hex(&entry.key)?;
                    Some((entry.meter.to_ascii_uppercase(), Aes::new(&key)?))
                })
                .collect(),
        )
    }

    fn find(&self, address: &Address) -> Option<&Aes> {
        let id = address.id();
        self.0
            .iter()
            .find_map(|(meter, aes)| (*meter == id).then_some(aes))
    }
}

pub(super) struct Reading {
    pub(super) link: Address,
    pub(super) meter: Address,
    pub(super) ci: Option<u8>,
    pub(super) access: Option<u8>,
    pub(super) status: Option<u8>,
    pub(super) security: WmbusSecurity,
    pub(super) records: Vec<WmbusRecord>,
}

pub(super) fn read(payload: &[u8], keys: &Keys) -> Option<Reading> {
    let link = Address::read(payload.get(LINK_ADDRESS..)?)?;
    let mut reading = Reading {
        link,
        meter: link,
        ci: None,
        access: None,
        status: None,
        security: WmbusSecurity::None,
        records: Vec::new(),
    };
    let mut body = payload.get(LINK_CI..)?.to_vec();
    while let Some(&ci) = body.first() {
        reading.ci = Some(ci);
        let next = match ci {
            ELL_SHORT => ell_short(&body, &mut reading),
            ELL_LONG => ell_long(&body, &mut reading, keys),
            TPL_SHORT | TPL_LONG => tpl(&body, &mut reading, keys),
            TPL_NONE => {
                reading.records = records::parse(&body[1..]);
                None
            }
            _ => None,
        };
        match next {
            Some(rest) => body = rest,
            None => break,
        }
    }
    Some(reading)
}

fn ell_short(body: &[u8], reading: &mut Reading) -> Option<Vec<u8>> {
    reading.access = Some(*body.get(2)?);
    Some(body.get(3..)?.to_vec())
}

fn ell_long(body: &[u8], reading: &mut Reading, keys: &Keys) -> Option<Vec<u8>> {
    let control = *body.get(1)?;
    reading.access = Some(*body.get(2)?);
    let session: [u8; 4] = body.get(3..7)?.try_into().ok()?;
    let mut sealed = body.get(7..)?.to_vec();
    match u32::from_le_bytes(session) >> 29 {
        ELL_PLAIN => {}
        ELL_AES_CTR => {
            let Some(aes) = keys.find(&reading.link) else {
                reading.security = WmbusSecurity::NoKey;
                return None;
            };
            let mut iv = [0u8; BLOCK];
            iv[..8].copy_from_slice(&reading.link.wire());
            iv[8] = control;
            iv[9..13].copy_from_slice(&session);
            crypto::ctr(aes, &iv, &mut sealed);
            let check = u16::from_le_bytes([*sealed.first()?, *sealed.get(1)?]);
            if crc(sealed.get(2..)?) != check {
                reading.security = WmbusSecurity::WrongKey;
                return None;
            }
            reading.security = WmbusSecurity::Decrypted;
        }
        _ => {
            reading.security = WmbusSecurity::Unsupported;
            return None;
        }
    }
    Some(sealed.get(2..)?.to_vec())
}

fn tpl(body: &[u8], reading: &mut Reading, keys: &Keys) -> Option<Vec<u8>> {
    let mut at = 1;
    if body[0] == TPL_LONG {
        reading.meter = Address::long_header(body.get(1..)?)?;
        at += 8;
    }
    let access = *body.get(at)?;
    reading.access = Some(access);
    reading.status = Some(*body.get(at + 1)?);
    let config = u16::from_le_bytes([*body.get(at + 2)?, *body.get(at + 3)?]);
    let mut data = body.get(at + 4..)?.to_vec();
    match (config >> 8) as u8 & 0x1F {
        PLAIN => {}
        AES_CBC_IV => {
            let blocks = usize::from(config >> 4 & 0x0F);
            decrypt_cbc(reading, keys, access, blocks, &mut data)?;
        }
        _ => {
            reading.security = WmbusSecurity::Unsupported;
            return None;
        }
    }
    reading.records = records::parse(&data);
    None
}

fn decrypt_cbc(
    reading: &mut Reading,
    keys: &Keys,
    access: u8,
    blocks: usize,
    data: &mut [u8],
) -> Option<()> {
    let Some(aes) = keys.find(&reading.meter) else {
        reading.security = WmbusSecurity::NoKey;
        return None;
    };
    let mut iv = [access; BLOCK];
    iv[..8].copy_from_slice(&reading.meter.wire());
    let len = (blocks * BLOCK).min(data.len() / BLOCK * BLOCK);
    crypto::cbc_decrypt(aes, &iv, &mut data[..len]);
    if data.get(..2) != Some(&VERIFIED[..]) {
        reading.security = WmbusSecurity::WrongKey;
        return None;
    }
    reading.security = WmbusSecurity::Decrypted;
    Some(())
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::WmbusQuantity;

    use super::*;

    fn bytes(text: &str) -> Vec<u8> {
        crypto::hex(text).unwrap()
    }

    fn keys(meter: &str, key: &str) -> Keys {
        Keys::new(&[WmbusKey {
            meter: meter.to_owned(),
            key: key.to_owned(),
        }])
    }

    fn volume(reading: &Reading) -> Option<f64> {
        reading
            .records
            .iter()
            .find(|record| record.quantity == WmbusQuantity::Volume)
            .and_then(|record| record.value)
    }

    const KAMSTRUP: &str =
        "2A442D2C998734761B168D2091D37CAC21E1D68CDAFFCD3DC452BD802913FF7B1706CA9E355D6C2701CC24";
    const KAMSTRUP_KEY: &str = "28F64A24988064A079AA2C807D6102AE";
    const APATOR: &str = "6e4401068888888805077a85006085bc2630713819512eb4cd87fba554fb43f67cf9654a68ee8e194088160df752e716238292e8af1ac20986202ee561d743602466915e42f1105d9c6782a54504e4f099e65a7656b930c73a30775122d2fdf074b5035cfaa7e0050bf32faae03a77";

    #[test]
    fn a_kamstrup_ell_frame_opens_with_its_key() {
        let reading = read(&bytes(KAMSTRUP), &keys("76348799", KAMSTRUP_KEY)).unwrap();
        assert_eq!(reading.link.station(), "KAM 76348799");
        assert_eq!(reading.security, WmbusSecurity::Decrypted);
        assert_eq!(reading.ci, Some(0x78));
        assert_eq!(volume(&reading), Some(6.408));
        let locked = read(&bytes(KAMSTRUP), &Keys::new(&[])).unwrap();
        assert_eq!(locked.security, WmbusSecurity::NoKey);
        assert_eq!(locked.ci, Some(0x8D));
        let wrong = read(&bytes(KAMSTRUP), &keys("76348799", &"0".repeat(32))).unwrap();
        assert_eq!(wrong.security, WmbusSecurity::WrongKey);
    }

    #[test]
    fn an_apator_mode_5_frame_opens_with_its_key() {
        let reading = read(&bytes(APATOR), &keys("88888888", &"0".repeat(32))).unwrap();
        assert_eq!(reading.meter.station(), "APA 88888888");
        assert_eq!(reading.security, WmbusSecurity::Decrypted);
        assert_eq!(reading.access, Some(0x85));
        let locked = read(&bytes(APATOR), &keys("88888889", &"0".repeat(32))).unwrap();
        assert_eq!(locked.security, WmbusSecurity::NoKey);
    }

    #[test]
    fn a_long_header_names_the_meter_behind_the_radio() {
        let payload = bytes(
            "4644b42571550210050e7287545505b42501079a003025403e848957876e48759da51bd3f945751967d301a2254d6a2851fd29931b624681f21e8106633cc25a6e3e8a06812405",
        );
        let reading = read(&payload, &Keys::new(&[])).unwrap();
        assert_eq!(reading.link.station(), "IMT 10025571");
        assert_eq!(reading.meter.station(), "IMT 05555487");
        assert_eq!(reading.meter.device_type, 0x07);
        assert_eq!(reading.access, Some(0x9A));
        assert_eq!(reading.security, WmbusSecurity::NoKey);
    }

    #[test]
    fn plain_records_follow_a_short_header() {
        let payload = bytes("1844AE4C4455223368077A55000000041389E20100023B0000");
        let reading = read(&payload, &Keys::new(&[])).unwrap();
        assert_eq!(reading.security, WmbusSecurity::None);
        assert_eq!(volume(&reading), Some(123.529));
        assert_eq!(reading.link.device_type, 0x07);
    }
}
