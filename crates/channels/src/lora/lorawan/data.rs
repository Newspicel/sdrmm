use sdrmm_wire::{LorawanFrame, LorawanMessageType};

use super::{Keys, Session, frame, le_u16, le_u32, mac, mic, split_mic};
use crate::datalink::hex;
use crate::lora::crypto::{self, Aes};

const MIN_DATA_LEN: usize = 12;
const OPTS_START: usize = 8;
const MIC_BLOCK_TAG: u8 = 0x49;
const CIPHER_BLOCK_TAG: u8 = 0x01;
const ADR: u8 = 0x80;
const ADR_ACK_REQ: u8 = 0x40;
const ACK: u8 = 0x20;
const PENDING_OR_CLASS_B: u8 = 0x10;
const OPTS_LEN: u8 = 0x0f;

#[derive(Clone, Copy)]
pub(super) struct Link {
    pub(super) uplink: bool,
    pub(super) dev_addr: u32,
    pub(super) f_cnt: u32,
}

struct Header<'a> {
    dev_addr: u32,
    control: u8,
    f_cnt: u16,
    f_opts: &'a [u8],
    f_port: Option<u8>,
    frm_payload: &'a [u8],
}

fn header(message: &[u8]) -> Option<Header<'_>> {
    let dev_addr = le_u32(message.get(1..5))?;
    let control = *message.get(5)?;
    let f_cnt = le_u16(message.get(6..8))?;
    let opts_end = OPTS_START + usize::from(control & OPTS_LEN);
    let f_opts = message.get(OPTS_START..opts_end)?;
    let (f_port, frm_payload) = match message.get(opts_end..)?.split_first() {
        Some((port, rest)) => (Some(*port), rest),
        None => (None, &[][..]),
    };
    if f_port == Some(0) && !f_opts.is_empty() {
        return None;
    }
    Some(Header {
        dev_addr,
        control,
        f_cnt,
        f_opts,
        f_port,
        frm_payload,
    })
}

pub(super) fn decode(
    payload: &[u8],
    message_type: LorawanMessageType,
    keys: &Keys,
) -> Option<LorawanFrame> {
    if payload.len() < MIN_DATA_LEN {
        return None;
    }
    let (message, received_mic) = split_mic(payload)?;
    let header = header(message)?;
    let uplink = message_type.uplink();
    let mut frame = frame(*message.first()?, received_mic);
    describe_header(&mut frame, &header, uplink);
    let link = Link {
        uplink,
        dev_addr: header.dev_addr,
        f_cnt: u32::from(header.f_cnt),
    };
    let mut sessions = keys
        .sessions
        .iter()
        .filter(|session| session.dev_addr == header.dev_addr)
        .peekable();
    if sessions.peek().is_none() {
        return Some(frame);
    }
    let verified =
        sessions.find(|session| data_mic(&session.nwk_s_key, link, message) == received_mic);
    frame.mic_ok = Some(verified.is_some());
    if let Some(session) = verified {
        decrypt(&mut frame, &header, session, link);
    }
    Some(frame)
}

fn describe_header(frame: &mut LorawanFrame, header: &Header<'_>, uplink: bool) {
    frame.dev_addr = Some(hex(&header.dev_addr.to_be_bytes()));
    frame.adr = Some(header.control & ADR != 0);
    frame.adr_ack_req = uplink.then_some(header.control & ADR_ACK_REQ != 0);
    frame.ack = Some(header.control & ACK != 0);
    frame.pending_or_class_b = Some(header.control & PENDING_OR_CLASS_B != 0);
    frame.f_cnt = Some(header.f_cnt);
    frame.f_port = header.f_port;
    frame.mac_commands = mac::parse(header.f_opts, uplink);
    frame.frm_payload = header.f_port.map(|_| hex(header.frm_payload));
}

fn decrypt(frame: &mut LorawanFrame, header: &Header<'_>, session: &Session, link: Link) {
    let Some(port) = header.f_port else {
        return;
    };
    let mut plain = header.frm_payload.to_vec();
    if port == 0 {
        crypt(&session.nwk_s_key, link, &mut plain);
        frame.mac_commands.extend(mac::parse(&plain, link.uplink));
    } else {
        crypt(&session.app_s_key, link, &mut plain);
    }
    frame.decrypted = Some(hex(&plain));
}

fn block(tag: u8, link: Link, last: u8) -> [u8; 16] {
    let mut block = [0; 16];
    block[0] = tag;
    block[5] = u8::from(!link.uplink);
    block[6..10].copy_from_slice(&link.dev_addr.to_le_bytes());
    block[10..14].copy_from_slice(&link.f_cnt.to_le_bytes());
    block[15] = last;
    block
}

pub(super) fn data_mic(nwk_s_key: &Aes, link: Link, message: &[u8]) -> [u8; 4] {
    let length = u8::try_from(message.len()).unwrap_or(u8::MAX);
    mic(nwk_s_key, &[&block(MIC_BLOCK_TAG, link, length), message])
}

pub(super) fn crypt(key: &Aes, link: Link, data: &mut [u8]) {
    crypto::ctr(key, &block(CIPHER_BLOCK_TAG, link, 1), data);
}
