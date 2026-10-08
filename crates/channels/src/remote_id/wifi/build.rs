use super::{
    ASTM_OUI, BEACON, NAN_TYPE, PUBLIC_ACTION, REMOTE_ID_SERVICE, SERVICE_DESCRIPTOR, SERVICE_INFO,
    SSID, VENDOR, VENDOR_ACTION, WFA_OUI, french,
};
use crate::remote_id::odid::APP_CODE;

const BROADCAST: [u8; 6] = [0xFF; 6];
const NAN_CLUSTER: [u8; 6] = [0x51, 0x6F, 0x9A, 0x01, 0x00, 0x00];
const BEACON_INTERVAL_TU: u16 = 100;
const CAPABILITY: u16 = 0x0421;
const RATES: u8 = 1;
const SUPPORTED_RATES: [u8; 4] = [0x82, 0x84, 0x8B, 0x96];
const DATA: u16 = 0x0008;
const WMM: [u8; 7] = [0x00, 0x50, 0xF2, 0x02, 0x01, 0x01, 0x00];

fn header(
    control: u16,
    destination: [u8; 6],
    source: [u8; 6],
    bssid: [u8; 6],
    sequence: u16,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(64);
    out.extend_from_slice(&control.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&destination);
    out.extend_from_slice(&source);
    out.extend_from_slice(&bssid);
    out.extend_from_slice(&(sequence << 4).to_le_bytes());
    out
}

fn finish(mpdu: Vec<u8>) -> Vec<u8> {
    sdrmm_modem::wifi::append_fcs(mpdu)
}

fn beacon_with(source: [u8; 6], ssid: &str, vendor: &[u8]) -> Vec<u8> {
    let mut mpdu = header(BEACON, BROADCAST, source, source, 1);
    mpdu.extend_from_slice(&0u64.to_le_bytes());
    mpdu.extend_from_slice(&BEACON_INTERVAL_TU.to_le_bytes());
    mpdu.extend_from_slice(&CAPABILITY.to_le_bytes());
    mpdu.extend_from_slice(&[SSID, ssid.len() as u8]);
    mpdu.extend_from_slice(ssid.as_bytes());
    mpdu.extend_from_slice(&[RATES, SUPPORTED_RATES.len() as u8]);
    mpdu.extend_from_slice(&SUPPORTED_RATES);
    mpdu.extend_from_slice(&[VENDOR, vendor.len() as u8]);
    mpdu.extend_from_slice(vendor);
    finish(mpdu)
}

#[must_use]
pub fn ordinary_beacon(source: [u8; 6], ssid: &str) -> Vec<u8> {
    beacon_with(source, ssid, &WMM)
}

#[must_use]
pub fn data(source: [u8; 6], payload: &[u8]) -> Vec<u8> {
    let mut mpdu = header(DATA, BROADCAST, source, source, 3);
    mpdu.extend_from_slice(payload);
    finish(mpdu)
}

#[must_use]
pub fn beacon(source: [u8; 6], ssid: &str, counter: u8, pack: &[u8]) -> Vec<u8> {
    let mut vendor = ASTM_OUI.to_vec();
    vendor.extend_from_slice(&[APP_CODE, counter]);
    vendor.extend_from_slice(pack);
    beacon_with(source, ssid, &vendor)
}

#[must_use]
pub fn french_beacon(source: [u8; 6], ssid: &str, tlvs: &[u8]) -> Vec<u8> {
    let mut vendor = french::OUI.to_vec();
    vendor.push(french::TYPE);
    vendor.extend_from_slice(tlvs);
    beacon_with(source, ssid, &vendor)
}

#[must_use]
pub fn nan(source: [u8; 6], counter: u8, pack: &[u8]) -> Vec<u8> {
    let mut mpdu = header(super::ACTION, NAN_CLUSTER, source, NAN_CLUSTER, 2);
    mpdu.extend_from_slice(&[PUBLIC_ACTION, VENDOR_ACTION]);
    mpdu.extend_from_slice(&WFA_OUI);
    mpdu.push(NAN_TYPE);
    let info_len = 1 + pack.len();
    let attribute_len = REMOTE_ID_SERVICE.len() + 4 + info_len;
    mpdu.push(SERVICE_DESCRIPTOR);
    mpdu.extend_from_slice(&(attribute_len as u16).to_le_bytes());
    mpdu.extend_from_slice(&REMOTE_ID_SERVICE);
    mpdu.extend_from_slice(&[1, 0, SERVICE_INFO, info_len as u8, counter]);
    mpdu.extend_from_slice(pack);
    mpdu.extend_from_slice(&[0x0E, 4, 0, 1, 0x00, 0x02, counter]);
    finish(mpdu)
}
