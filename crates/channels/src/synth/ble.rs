use num_complex::Complex;

use super::remote_id::{BlePhy, bluetooth};

const ADV_IND: u8 = 0x00;
const ADV_NONCONN_IND: u8 = 0x02;
const TX_ADD_RANDOM: u8 = 0x40;
const CHANNEL_38: u8 = 12;
const SCENE_GAP_S: f64 = 0.02;
const FLAGS: [u8; 3] = [0x02, 0x01, 0x06];

#[must_use]
pub fn advert(kind: u8, address: [u8; 6], data: &[u8]) -> Vec<u8> {
    let mut pdu = vec![kind | TX_ADD_RANDOM, (address.len() + data.len()) as u8];
    pdu.extend(address.iter().rev());
    pdu.extend_from_slice(data);
    pdu
}

#[must_use]
pub fn structure(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![(body.len() + 1) as u8, kind];
    out.extend_from_slice(body);
    out
}

#[must_use]
pub fn ibeacon(uuid: [u8; 16], major: u16, minor: u16, measured_dbm: i8) -> Vec<u8> {
    let mut body = vec![0x4C, 0x00, 0x02, 0x15];
    body.extend(uuid);
    body.extend(major.to_be_bytes());
    body.extend(minor.to_be_bytes());
    body.push(measured_dbm as u8);
    let mut data = FLAGS.to_vec();
    data.extend(structure(0xFF, &body));
    data
}

#[must_use]
pub fn eddystone_url(encoded: &[u8], tx_power_dbm: i8) -> Vec<u8> {
    let mut body = vec![0xAA, 0xFE, 0x10, tx_power_dbm as u8];
    body.extend_from_slice(encoded);
    let mut data = FLAGS.to_vec();
    data.extend(structure(0x03, &[0xAA, 0xFE]));
    data.extend(structure(0x16, &body));
    data
}

#[must_use]
pub fn named_sensor(name: &str, battery_percent: u8) -> Vec<u8> {
    let mut data = FLAGS.to_vec();
    data.extend(structure(0x09, name.as_bytes()));
    data.extend(structure(0x16, &[0x0F, 0x18, battery_percent]));
    data
}

#[must_use]
pub fn scene(rate: f64) -> Vec<Complex<f32>> {
    let adverts = [
        advert(
            ADV_NONCONN_IND,
            [0xD3, 0x10, 0x20, 0x30, 0x40, 0x50],
            &ibeacon(*b"SDR-mm beacon 01", 7, 300, -59),
        ),
        advert(
            ADV_NONCONN_IND,
            [0xE1, 0x22, 0x33, 0x44, 0x55, 0x66],
            &eddystone_url(&[3, b's', b'd', b'r', b'm', b'm', 0x07], -21),
        ),
        advert(
            ADV_IND,
            [0xC4, 0x9A, 0x8B, 0x7C, 0x6D, 0x5E],
            &named_sensor("Greenhouse", 87),
        ),
    ];
    let gap = vec![Complex::new(0.0, 0.0); (SCENE_GAP_S * rate) as usize];
    let mut iq = gap.clone();
    for pdu in &adverts {
        iq.extend(bluetooth(pdu, CHANNEL_38, BlePhy::Le1m, rate));
        iq.extend_from_slice(&gap);
    }
    iq
}
