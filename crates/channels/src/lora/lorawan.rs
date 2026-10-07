mod data;
mod join;
mod mac;
#[cfg(any(test, feature = "synth"))]
mod synth;
#[cfg(test)]
mod tests;

use sdrmm_wire::{LoraKey, LorawanFrame, LorawanMessageType};

use super::crypto::{self, Aes};
use crate::datalink::hex;

#[cfg(any(test, feature = "synth"))]
pub(crate) use synth::{join_request, uplink};

const MIC_LEN: usize = 4;
const MHDR_RFU_AND_MAJOR: u8 = 0x1f;
const MAJOR_MASK: u8 = 0x03;

struct Session {
    dev_addr: u32,
    nwk_s_key: Aes,
    app_s_key: Aes,
}

#[derive(Default)]
pub(crate) struct Keys {
    sessions: Vec<Session>,
    app_keys: Vec<Aes>,
}

impl Keys {
    pub(crate) fn new(keys: &[LoraKey]) -> Result<Self, String> {
        let mut parsed = Self::default();
        for key in keys {
            match key {
                LoraKey::LorawanSession {
                    dev_addr,
                    nwk_s_key,
                    app_s_key,
                } => parsed.sessions.push(Session {
                    dev_addr: parse_dev_addr(dev_addr)?,
                    nwk_s_key: parse_key(nwk_s_key, "NwkSKey")?,
                    app_s_key: parse_key(app_s_key, "AppSKey")?,
                }),
                LoraKey::LorawanAppKey { app_key } => {
                    parsed.app_keys.push(parse_key(app_key, "AppKey")?);
                }
                LoraKey::MeshtasticChannel { .. } | LoraKey::MeshcoreChannel { .. } => {}
            }
        }
        Ok(parsed)
    }
}

fn parse_key(text: &str, label: &str) -> Result<Aes, String> {
    crypto::hex(text.trim())
        .filter(|bytes| bytes.len() == 16)
        .and_then(|bytes| Aes::new(&bytes))
        .ok_or_else(|| format!("LoRaWAN {label} needs 32 hex digits"))
}

fn parse_dev_addr(text: &str) -> Result<u32, String> {
    crypto::hex(text.trim())
        .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
        .map(u32::from_be_bytes)
        .ok_or_else(|| "LoRaWAN DevAddr needs 8 hex digits".to_owned())
}

pub(crate) fn decode(payload: &[u8], keys: &Keys) -> Option<LorawanFrame> {
    let mhdr = *payload.first()?;
    let message_type = message_type(mhdr);
    if message_type != LorawanMessageType::Proprietary && mhdr & MHDR_RFU_AND_MAJOR != 0 {
        return None;
    }
    match message_type {
        LorawanMessageType::JoinRequest => join::request(payload, keys),
        LorawanMessageType::JoinAccept => join::accept(payload, keys),
        LorawanMessageType::RejoinRequest => join::rejoin(payload),
        LorawanMessageType::Proprietary => proprietary(payload),
        LorawanMessageType::UnconfirmedUp
        | LorawanMessageType::UnconfirmedDown
        | LorawanMessageType::ConfirmedUp
        | LorawanMessageType::ConfirmedDown => data::decode(payload, message_type, keys),
    }
}

fn message_type(mhdr: u8) -> LorawanMessageType {
    match mhdr >> 5 {
        0 => LorawanMessageType::JoinRequest,
        1 => LorawanMessageType::JoinAccept,
        2 => LorawanMessageType::UnconfirmedUp,
        3 => LorawanMessageType::UnconfirmedDown,
        4 => LorawanMessageType::ConfirmedUp,
        5 => LorawanMessageType::ConfirmedDown,
        6 => LorawanMessageType::RejoinRequest,
        _ => LorawanMessageType::Proprietary,
    }
}

fn proprietary(payload: &[u8]) -> Option<LorawanFrame> {
    let (message, mic) = split_mic(payload)?;
    let mhdr = *message.first()?;
    Some(frame(mhdr, mic))
}

fn split_mic(payload: &[u8]) -> Option<(&[u8], &[u8])> {
    payload
        .len()
        .checked_sub(MIC_LEN)
        .map(|at| payload.split_at(at))
}

fn frame(mhdr: u8, mic: &[u8]) -> LorawanFrame {
    LorawanFrame {
        message_type: message_type(mhdr),
        major: mhdr & MAJOR_MASK,
        mic: hex(mic),
        mic_ok: None,
        dev_addr: None,
        adr: None,
        adr_ack_req: None,
        ack: None,
        pending_or_class_b: None,
        f_cnt: None,
        f_port: None,
        mac_commands: Vec::new(),
        frm_payload: None,
        decrypted: None,
        join_eui: None,
        dev_eui: None,
        dev_nonce: None,
        join_nonce: None,
        net_id: None,
        rx1_dr_offset: None,
        rx2_data_rate: None,
        rx_delay_s: None,
        cf_list_hz: Vec::new(),
    }
}

fn mic(key: &Aes, parts: &[&[u8]]) -> [u8; MIC_LEN] {
    let tag = crypto::cmac(key, &parts.concat());
    [tag[0], tag[1], tag[2], tag[3]]
}

fn msb_first_hex(little_endian: &[u8]) -> String {
    let mut bytes = little_endian.to_vec();
    bytes.reverse();
    hex(&bytes)
}

fn le_u16(bytes: Option<&[u8]>) -> Option<u16> {
    bytes?.try_into().ok().map(u16::from_le_bytes)
}

fn le_u24(bytes: Option<&[u8]>) -> Option<u32> {
    match bytes? {
        [low, middle, high] => Some(u32::from_le_bytes([*low, *middle, *high, 0])),
        _ => None,
    }
}

fn le_u32(bytes: Option<&[u8]>) -> Option<u32> {
    bytes?.try_into().ok().map(u32::from_le_bytes)
}
