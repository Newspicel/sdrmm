use super::data::{Link, crypt, data_mic};
use super::mic;
use crate::lora::crypto::Aes;

const UNCONFIRMED_UP: u8 = 0x40;
const CONFIRMED_UP: u8 = 0x80;
const JOIN_REQUEST: u8 = 0x00;

pub(crate) fn uplink(
    dev_addr: u32,
    f_cnt: u16,
    f_port: u8,
    payload: &[u8],
    nwk_s_key: &[u8; 16],
    app_s_key: &[u8; 16],
    confirmed: bool,
) -> Vec<u8> {
    let (Some(nwk_s_key), Some(app_s_key)) = (Aes::new(nwk_s_key), Aes::new(app_s_key)) else {
        return Vec::new();
    };
    let link = Link {
        uplink: true,
        dev_addr,
        f_cnt: u32::from(f_cnt),
    };
    let mut message = vec![if confirmed {
        CONFIRMED_UP
    } else {
        UNCONFIRMED_UP
    }];
    message.extend_from_slice(&dev_addr.to_le_bytes());
    message.push(0);
    message.extend_from_slice(&f_cnt.to_le_bytes());
    message.push(f_port);
    let mut body = payload.to_vec();
    crypt(
        if f_port == 0 { &nwk_s_key } else { &app_s_key },
        link,
        &mut body,
    );
    message.extend_from_slice(&body);
    let mic = data_mic(&nwk_s_key, link, &message);
    message.extend_from_slice(&mic);
    message
}

pub(crate) fn join_request(
    join_eui: u64,
    dev_eui: u64,
    dev_nonce: u16,
    app_key: &[u8; 16],
) -> Vec<u8> {
    let Some(app_key) = Aes::new(app_key) else {
        return Vec::new();
    };
    let mut message = vec![JOIN_REQUEST];
    message.extend_from_slice(&join_eui.to_le_bytes());
    message.extend_from_slice(&dev_eui.to_le_bytes());
    message.extend_from_slice(&dev_nonce.to_le_bytes());
    let mic = mic(&app_key, &[&message]);
    message.extend_from_slice(&mic);
    message
}
