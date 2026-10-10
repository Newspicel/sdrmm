use sdrmm_wire::{LoraKey, LorawanMessageType};

use super::{Keys, decode, join_request, mac, uplink};
use crate::lora::crypto::{self, Aes};

const NWK_S_KEY: &str = "44024241ed4ce9a68c6a8bc055233fd3";
const APP_S_KEY: &str = "ec925802ae430ca77fd3dd73cb2cc588";
const APP_KEY: [u8; 16] = [
    0x2b, 0x7e, 0x15, 0x16, 0x28, 0xae, 0xd2, 0xa6, 0xab, 0xf7, 0x15, 0x88, 0x09, 0xcf, 0x4f, 0x3c,
];

fn bytes(text: &str) -> Vec<u8> {
    crypto::hex(text).unwrap()
}

fn key(text: &str) -> [u8; 16] {
    bytes(text).try_into().unwrap()
}

fn session(dev_addr: &str) -> Keys {
    Keys::new(&[LoraKey::LorawanSession {
        dev_addr: dev_addr.to_owned(),
        nwk_s_key: NWK_S_KEY.to_owned(),
        app_s_key: APP_S_KEY.to_owned(),
    }])
    .unwrap()
}

fn app_key_text(key: &[u8; 16]) -> Keys {
    Keys::new(&[LoraKey::LorawanAppKey {
        app_key: crate::datalink::hex(key),
    }])
    .unwrap()
}

#[test]
fn the_published_uplink_verifies_and_decrypts() {
    let payload = bytes("40F17DBE4900020001954378762B11FF0D");
    let frame = decode(&payload, &session("49be7df1")).unwrap();
    assert_eq!(frame.message_type, LorawanMessageType::UnconfirmedUp);
    assert_eq!(frame.major, 0);
    assert_eq!(frame.dev_addr.as_deref(), Some("49be7df1"));
    assert_eq!(frame.f_cnt, Some(2));
    assert_eq!(frame.f_port, Some(1));
    assert_eq!(frame.mic, "2b11ff0d");
    assert_eq!(frame.mic_ok, Some(true));
    assert_eq!(frame.frm_payload.as_deref(), Some("95437876"));
    assert_eq!(frame.decrypted.as_deref(), Some("74657374"));
    assert_eq!(frame.adr, Some(false));
    assert_eq!(frame.adr_ack_req, Some(false));
}

#[test]
fn without_a_session_the_uplink_stays_encrypted() {
    let payload = bytes("40F17DBE4900020001954378762B11FF0D");
    let frame = decode(&payload, &Keys::default()).unwrap();
    assert_eq!(frame.mic_ok, None);
    assert_eq!(frame.decrypted, None);
    let other = decode(&payload, &session("26011bda")).unwrap();
    assert_eq!(other.mic_ok, None);
}

#[test]
fn a_tampered_uplink_fails_its_mic() {
    let mut payload = bytes("40F17DBE4900020001954378762B11FF0D");
    payload[9] ^= 1;
    let frame = decode(&payload, &session("49be7df1")).unwrap();
    assert_eq!(frame.mic_ok, Some(false));
    assert_eq!(frame.decrypted, None);
}

#[test]
fn synthesized_uplinks_round_trip() {
    let payload = uplink(
        0x49be_7df1,
        2,
        1,
        b"test",
        &key(NWK_S_KEY),
        &key(APP_S_KEY),
        false,
    );
    assert_eq!(payload, bytes("40F17DBE4900020001954378762B11FF0D"));
    let confirmed = uplink(
        0x49be_7df1,
        700,
        0,
        &[0x06, 200, 0x3f],
        &key(NWK_S_KEY),
        &key(APP_S_KEY),
        true,
    );
    let frame = decode(&confirmed, &session("49be7df1")).unwrap();
    assert_eq!(frame.message_type, LorawanMessageType::ConfirmedUp);
    assert_eq!(frame.f_cnt, Some(700));
    assert_eq!(frame.f_port, Some(0));
    assert_eq!(frame.mic_ok, Some(true));
    assert_eq!(frame.decrypted.as_deref(), Some("06c83f"));
    assert_eq!(frame.mac_commands, ["DevStatusAns battery 200 margin -1"]);
}

#[test]
fn join_requests_round_trip_and_check_the_app_key() {
    let payload = join_request(
        0x70b3_d57e_d000_0001,
        0x0004_a30b_001c_0530,
        0x1234,
        &APP_KEY,
    );
    assert_eq!(payload.len(), 23);
    let frame = decode(&payload, &app_key_text(&APP_KEY)).unwrap();
    assert_eq!(frame.message_type, LorawanMessageType::JoinRequest);
    assert_eq!(frame.join_eui.as_deref(), Some("70b3d57ed0000001"));
    assert_eq!(frame.dev_eui.as_deref(), Some("0004a30b001c0530"));
    assert_eq!(frame.dev_nonce, Some(0x1234));
    assert_eq!(frame.mic_ok, Some(true));
    assert_eq!(decode(&payload, &Keys::default()).unwrap().mic_ok, None);
    let wrong = app_key_text(&[7; 16]);
    assert_eq!(decode(&payload, &wrong).unwrap().mic_ok, Some(false));
}

fn join_accept(key: &[u8; 16], fields: &[u8]) -> Vec<u8> {
    let aes = Aes::new(key).unwrap();
    let mhdr = 0x20;
    let mut body = fields.to_vec();
    let tag = crypto::cmac(&aes, &[&[mhdr][..], fields].concat());
    body.extend_from_slice(&tag[..4]);
    for block in body.as_chunks_mut::<16>().0 {
        aes.decrypt(block);
    }
    [vec![mhdr], body].concat()
}

#[test]
fn join_accepts_decrypt_with_the_app_key() {
    let mut fields = vec![
        0x01, 0x02, 0x03, 0x13, 0x00, 0x00, 0xda, 0x1b, 0x01, 0x26, 0x23, 0x05,
    ];
    let cf_list = [
        0x18, 0x4f, 0x84, 0xe8, 0x56, 0x84, 0xb8, 0x5e, 0x84, 0, 0, 0, 0, 0, 0, 0,
    ];
    fields.extend_from_slice(&cf_list);
    let payload = join_accept(&APP_KEY, &fields);
    assert_eq!(payload.len(), 33);
    let frame = decode(&payload, &app_key_text(&APP_KEY)).unwrap();
    assert_eq!(frame.message_type, LorawanMessageType::JoinAccept);
    assert_eq!(frame.mic_ok, Some(true));
    assert_eq!(frame.join_nonce, Some(0x0003_0201));
    assert_eq!(frame.net_id.as_deref(), Some("000013"));
    assert_eq!(frame.dev_addr.as_deref(), Some("26011bda"));
    assert_eq!(frame.rx1_dr_offset, Some(2));
    assert_eq!(frame.rx2_data_rate, Some(3));
    assert_eq!(frame.rx_delay_s, Some(5));
    assert_eq!(
        frame.cf_list_hz,
        [867_100_000.0, 867_300_000.0, 867_500_000.0]
    );
    let locked = decode(&payload, &app_key_text(&[9; 16])).unwrap();
    assert_eq!(locked.mic_ok, Some(false));
    assert_eq!(locked.dev_addr, None);
    assert_eq!(locked.mic, crate::datalink::hex(&payload[29..]));
    assert_eq!(decode(&payload, &Keys::default()).unwrap().mic_ok, None);
}

#[test]
fn short_join_accepts_have_no_cf_list() {
    let fields = [
        0x01, 0x02, 0x03, 0x13, 0x00, 0x00, 0xda, 0x1b, 0x01, 0x26, 0x00, 0x00,
    ];
    let frame = decode(&join_accept(&APP_KEY, &fields), &app_key_text(&APP_KEY)).unwrap();
    assert_eq!(frame.rx_delay_s, Some(1));
    assert!(frame.cf_list_hz.is_empty());
}

#[test]
fn mac_commands_depend_on_direction() {
    assert_eq!(
        mac::parse(&[0x02, 20, 2, 0x03, 0x51, 0xff, 0x00, 0x01], false),
        [
            "LinkCheckAns margin 20 dB gw 2",
            "LinkADRReq DR5 TXPower 1 ChMask 00ff"
        ]
    );
    assert_eq!(
        mac::parse(&[0x02, 0x03, 0x07, 0x06, 254, 7, 0x0d], true),
        [
            "LinkCheckReq",
            "LinkADRAns ok",
            "DevStatusAns battery 254 margin 7",
            "DeviceTimeReq"
        ]
    );
    assert_eq!(mac::parse(&[0x03, 0x06], true), ["LinkADRAns rejected"]);
    assert_eq!(
        mac::parse(&[0x08, 0x01, 0x04, 0x02, 0x06], false),
        [
            "RXTimingSetupReq delay 1 s",
            "DutyCycleReq 1/4",
            "DevStatusReq"
        ]
    );
    assert_eq!(
        mac::parse(&[0x07, 3, 0x18, 0x4f, 0x84, 0x50], false),
        ["NewChannelReq ch 3 867.1 MHz DR0-5"]
    );
    assert_eq!(
        mac::parse(&[0x02, 0x80, 0x02], true),
        ["LinkCheckReq", "unknown 0x80"]
    );
    assert_eq!(mac::parse(&[0x03, 0x51], false), ["LinkADRReq truncated"]);
}

#[test]
fn f_opts_are_read_from_data_frames() {
    let payload = bytes(&["40", "DA1B0126", "A2", "0500", "020D", "11223344"].concat());
    let frame = decode(&payload, &Keys::default()).unwrap();
    assert_eq!(frame.dev_addr.as_deref(), Some("26011bda"));
    assert_eq!(frame.adr, Some(true));
    assert_eq!(frame.ack, Some(true));
    assert_eq!(frame.f_cnt, Some(5));
    assert_eq!(frame.f_port, None);
    assert_eq!(frame.frm_payload, None);
    assert_eq!(frame.mac_commands, ["LinkCheckReq", "DeviceTimeReq"]);
    assert_eq!(frame.mic, "11223344");
}

#[test]
fn downlinks_report_pending_and_no_adr_ack_req() {
    let payload = bytes("60DA1B012670010001AABBCCDD");
    let frame = decode(&payload, &Keys::default()).unwrap();
    assert_eq!(frame.message_type, LorawanMessageType::UnconfirmedDown);
    assert_eq!(frame.adr_ack_req, None);
    assert_eq!(frame.pending_or_class_b, Some(true));
    assert_eq!(frame.ack, Some(true));
    assert_eq!(frame.f_port, Some(1));
}

#[test]
fn rejoin_and_proprietary_frames_parse_minimally() {
    let rejoin = bytes(&["C000", "130000", "30051C000BA30400", "0700", "01020304"].concat());
    assert_eq!(rejoin.len(), 19);
    let frame = decode(&rejoin, &Keys::default()).unwrap();
    assert_eq!(frame.message_type, LorawanMessageType::RejoinRequest);
    assert_eq!(frame.net_id.as_deref(), Some("000013"));
    assert_eq!(frame.dev_eui.as_deref(), Some("0004a30b001c0530"));
    assert_eq!(frame.dev_nonce, Some(7));
    let proprietary = decode(&[0xe1, 1, 2, 3, 4, 5], &Keys::default()).unwrap();
    assert_eq!(proprietary.message_type, LorawanMessageType::Proprietary);
    assert_eq!(proprietary.major, 1);
    assert_eq!(proprietary.mic, "02030405");
}

#[test]
fn malformed_frames_are_rejected() {
    let keys = session("49be7df1");
    let valid = bytes("40F17DBE4900020001954378762B11FF0D");
    assert!(decode(&[], &keys).is_none());
    assert!(decode(&valid[..11], &keys).is_none());
    assert!(decode(&[[0x41].as_slice(), &valid[1..]].concat(), &keys).is_none());
    assert!(decode(&bytes("40F17DBE490F0200112233445566"), &keys).is_none());
    assert!(decode(&bytes("40F17DBE490102000300AABBCCDD"), &keys).is_none());
    assert!(decode(&bytes("00010203"), &keys).is_none());
    assert!(decode(&[0x20; 20], &keys).is_none());
    assert!(
        decode(
            &[0xc0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            &keys
        )
        .is_none()
    );
}

#[test]
fn arbitrary_bytes_never_panic() {
    let keys = Keys::new(&[
        LoraKey::LorawanSession {
            dev_addr: "00000000".to_owned(),
            nwk_s_key: NWK_S_KEY.to_owned(),
            app_s_key: APP_S_KEY.to_owned(),
        },
        LoraKey::LorawanAppKey {
            app_key: NWK_S_KEY.to_owned(),
        },
    ])
    .unwrap();
    let mut state = 0x1234_5678_u32;
    for length in 0..64 {
        for _ in 0..64 {
            let payload: Vec<u8> = (0..length)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    state.to_le_bytes()[0]
                })
                .collect();
            let _ = decode(&payload, &keys);
            let _ = mac::parse(&payload, length % 2 == 0);
        }
    }
}

#[test]
fn malformed_keys_are_reported() {
    let short = LoraKey::LorawanAppKey {
        app_key: "0011".to_owned(),
    };
    assert!(Keys::new(&[short]).is_err());
    let address = LoraKey::LorawanSession {
        dev_addr: "xyz".to_owned(),
        nwk_s_key: NWK_S_KEY.to_owned(),
        app_s_key: APP_S_KEY.to_owned(),
    };
    assert!(Keys::new(&[address]).is_err());
    let other = LoraKey::MeshtasticChannel {
        name: "LongFast".to_owned(),
        psk: "not hex".to_owned(),
    };
    assert!(Keys::new(&[other]).is_ok());
}
