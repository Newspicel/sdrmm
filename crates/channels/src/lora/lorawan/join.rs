use sdrmm_wire::LorawanFrame;

use super::{Keys, frame, le_u16, le_u24, mic, msb_first_hex, split_mic};
use crate::lora::crypto::Aes;

const JOIN_REQUEST_LEN: usize = 23;
const JOIN_ACCEPT_LEN: usize = 17;
const JOIN_ACCEPT_WITH_CF_LIST_LEN: usize = 33;
const REJOIN_NET_LEN: usize = 19;
const REJOIN_JOIN_EUI_LEN: usize = 24;
const CF_LIST_FREQUENCIES: u8 = 0;
const BLOCK: usize = 16;

pub(super) fn request(payload: &[u8], keys: &Keys) -> Option<LorawanFrame> {
    if payload.len() != JOIN_REQUEST_LEN {
        return None;
    }
    let (message, received_mic) = split_mic(payload)?;
    let mut frame = frame(*message.first()?, received_mic);
    frame.join_eui = Some(msb_first_hex(message.get(1..9)?));
    frame.dev_eui = Some(msb_first_hex(message.get(9..17)?));
    frame.dev_nonce = Some(le_u16(message.get(17..19))?);
    frame.mic_ok = (!keys.app_keys.is_empty()).then(|| {
        keys.app_keys
            .iter()
            .any(|key| mic(key, &[message]) == received_mic)
    });
    Some(frame)
}

pub(super) fn accept(payload: &[u8], keys: &Keys) -> Option<LorawanFrame> {
    if payload.len() != JOIN_ACCEPT_LEN && payload.len() != JOIN_ACCEPT_WITH_CF_LIST_LEN {
        return None;
    }
    let (&mhdr, encrypted) = payload.split_first()?;
    if let Some(frame) = keys
        .app_keys
        .iter()
        .find_map(|key| open_accept(mhdr, encrypted, key))
    {
        return Some(frame);
    }
    let (_, encrypted_mic) = split_mic(encrypted)?;
    let mut frame = frame(mhdr, encrypted_mic);
    frame.mic_ok = (!keys.app_keys.is_empty()).then_some(false);
    Some(frame)
}

fn open_accept(mhdr: u8, encrypted: &[u8], key: &Aes) -> Option<LorawanFrame> {
    let mut plain = encrypted.to_vec();
    for block in plain.as_chunks_mut::<BLOCK>().0 {
        key.encrypt(block);
    }
    let (fields, received_mic) = split_mic(&plain)?;
    if mic(key, &[&[mhdr], fields]) != received_mic {
        return None;
    }
    let mut frame = frame(mhdr, received_mic);
    frame.mic_ok = Some(true);
    frame.join_nonce = Some(le_u24(fields.get(0..3))?);
    frame.net_id = Some(msb_first_hex(fields.get(3..6)?));
    frame.dev_addr = Some(msb_first_hex(fields.get(6..10)?));
    let settings = *fields.get(10)?;
    frame.rx1_dr_offset = Some((settings >> 4) & 0x07);
    frame.rx2_data_rate = Some(settings & 0x0f);
    frame.rx_delay_s = Some((fields.get(11)? & 0x0f).max(1));
    frame.cf_list_hz = fields.get(12..).map(cf_list).unwrap_or_default();
    Some(frame)
}

fn cf_list(list: &[u8]) -> Vec<f64> {
    match list.split_last() {
        Some((&CF_LIST_FREQUENCIES, frequencies)) => frequencies
            .as_chunks::<3>()
            .0
            .iter()
            .map(|[low, middle, high]| u32::from_le_bytes([*low, *middle, *high, 0]))
            .filter(|raw| *raw != 0)
            .map(|raw| f64::from(raw) * 100.0)
            .collect(),
        _ => Vec::new(),
    }
}

pub(super) fn rejoin(payload: &[u8]) -> Option<LorawanFrame> {
    let (message, received_mic) = split_mic(payload)?;
    let mut frame = frame(*message.first()?, received_mic);
    match (*message.get(1)?, payload.len()) {
        (0 | 2, REJOIN_NET_LEN) => {
            frame.net_id = Some(msb_first_hex(message.get(2..5)?));
            frame.dev_eui = Some(msb_first_hex(message.get(5..13)?));
            frame.dev_nonce = Some(le_u16(message.get(13..15))?);
        }
        (1, REJOIN_JOIN_EUI_LEN) => {
            frame.join_eui = Some(msb_first_hex(message.get(2..10)?));
            frame.dev_eui = Some(msb_first_hex(message.get(10..18)?));
            frame.dev_nonce = Some(le_u16(message.get(18..20))?);
        }
        _ => return None,
    }
    Some(frame)
}
