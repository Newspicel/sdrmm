use super::*;
use crate::{decode::EventKind, present::EventPayload};

fn lora(decoded: serde_json::Value) -> crate::present::Presentation {
    let mut frame = serde_json::json!({
        "spreading_factor": 11, "bandwidth_hz": 250000.0, "coding_rate": "4/5",
        "sync_word": 43, "implicit_header": false, "low_data_rate": false,
        "inverted_iq": false, "integrity": "crc_ok", "fec_corrected": 2, "snr_db": -7.5,
        "frequency_error_hz": -1210.0, "payload": "ffffffff",
    });
    if !decoded.is_null() {
        frame["decoded"] = decoded;
    }
    serde_json::from_value::<LoraFrame>(frame)
        .unwrap()
        .present(EventKind::Lora)
}

#[test]
fn a_raw_frame_shows_its_modem_and_payload() {
    let view = lora(serde_json::Value::Null);
    assert_eq!(
        view.labels(),
        [
            "SF",
            "Bandwidth",
            "Coding rate",
            "Sync word",
            "Header",
            "IQ",
            "Integrity",
            "FEC repaired",
            "SNR",
            "Frequency error",
            "Payload"
        ]
    );
    assert_eq!(view.field("SF"), text(Some("SF11")).as_ref());
    assert_eq!(view.field("Sync word"), text(Some("0x2B")).as_ref());
    assert_eq!(view.field("Integrity"), text(Some("CRC ok")).as_ref());
    assert_eq!(
        view.field("Bandwidth"),
        number(Some(250_000.0), Unit::Hz, 0).as_ref()
    );
}

#[test]
fn a_meshtastic_text_names_nodes_channel_and_hops() {
    let view = lora(serde_json::json!({
        "protocol": "meshtastic", "to": 0xffff_ffff_u32, "from": 0xa1b2_c3d4_u32, "id": 0x1234,
        "hop_limit": 1, "hop_start": 3, "want_ack": false, "via_mqtt": false,
        "channel_hash": 8, "next_hop": 0, "relay_node": 0xd4, "encryption": "channel",
        "channel": "LongFast", "port": 1, "port_name": "TEXT_MESSAGE_APP",
        "content": { "type": "text", "text": "hello mesh" },
    }));
    assert_eq!(view.field("Protocol"), text(Some("Meshtastic")).as_ref());
    assert_eq!(view.field("From"), text(Some("!a1b2c3d4")).as_ref());
    assert_eq!(view.field("To"), text(Some("^all")).as_ref());
    assert_eq!(view.field("Packet ID"), text(Some("0x00001234")).as_ref());
    assert_eq!(view.field("Channel"), text(Some("LongFast")).as_ref());
    assert_eq!(view.field("Port"), text(Some("TEXT_MESSAGE_APP")).as_ref());
    assert_eq!(view.field("Hops"), text(Some("2 of 3")).as_ref());
    assert_eq!(view.field("Relay"), text(Some("0xD4")).as_ref());
    assert_eq!(view.field("Next hop"), None);
    assert_eq!(view.field("Text"), text(Some("hello mesh")).as_ref());
}

#[test]
fn a_meshtastic_position_lists_its_fix() {
    let view = lora(serde_json::json!({
        "protocol": "meshtastic", "to": 0xffff_ffff_u32, "from": 1, "id": 1,
        "hop_limit": 3, "hop_start": 3, "want_ack": false, "via_mqtt": false,
        "channel_hash": 8, "next_hop": 0, "relay_node": 0, "encryption": "channel",
        "content": { "type": "position", "lat": 47.5, "lon": 8.25, "altitude_m": 410,
            "satellites": 9, "time": 1_700_000_000_u32 },
    }));
    assert_eq!(view.field("From"), text(Some("!00000001")).as_ref());
    assert_eq!(view.field("Channel"), text(Some("0x08")).as_ref());
    assert_eq!(view.field("Hops"), text(Some("0 of 3")).as_ref());
    assert_eq!(
        view.field("Position"),
        position(Some(47.5), Some(8.25)).as_ref()
    );
    assert_eq!(
        view.field("Altitude"),
        number(Some(410), Unit::Metres, 0).as_ref()
    );
    assert_eq!(view.field("Satellites"), count(Some(9)).as_ref());
    assert_eq!(
        view.field("Fix time"),
        Some(&FieldValue::Time("2023-11-14T22:13:20Z".to_owned()))
    );
}

#[test]
fn meshtastic_telemetry_names_each_metric() {
    let view = lora(serde_json::json!({
        "protocol": "meshtastic", "to": 1, "from": 2, "id": 3,
        "hop_limit": 3, "hop_start": 3, "want_ack": false, "via_mqtt": false,
        "channel_hash": 8, "next_hop": 0, "relay_node": 0, "encryption": "open",
        "content": { "type": "telemetry", "kind": "device",
            "metrics": [{ "name": "battery_level", "value": 87.0 }] },
    }));
    assert_eq!(view.field("Telemetry"), text(Some("device")).as_ref());
    assert_eq!(
        view.field("battery_level"),
        number(Some(87.0), Unit::None, 2).as_ref()
    );
}

#[test]
fn lorawan_shows_header_counters_and_mic_check() {
    let view = lora(serde_json::json!({
        "protocol": "lorawan", "message_type": "confirmed_up", "major": 0,
        "mic": "a1b2c3d4", "mic_ok": true, "dev_addr": "26011BDA", "adr": true,
        "f_cnt": 42, "f_port": 10, "mac_commands": ["LinkCheckReq"], "decrypted": "0102",
        "cf_list_hz": [867_100_000.0, 867_300_000.0],
    }));
    assert_eq!(view.field("Protocol"), text(Some("LoRaWAN")).as_ref());
    assert_eq!(view.field("Type"), text(Some("Confirmed uplink")).as_ref());
    assert_eq!(view.field("DevAddr"), text(Some("26011BDA")).as_ref());
    assert_eq!(view.field("FCnt"), count(Some(42)).as_ref());
    assert_eq!(view.field("FPort"), count(Some(10)).as_ref());
    assert_eq!(view.field("MIC"), text(Some("a1b2c3d4 ok")).as_ref());
    assert_eq!(view.field("ADR"), flag(Some(true)).as_ref());
    assert_eq!(
        view.field("MAC commands"),
        text(Some("LinkCheckReq")).as_ref()
    );
    assert_eq!(view.field("Decrypted"), text(Some("0102")).as_ref());
    assert_eq!(
        view.field("Channels"),
        text(Some("867.1 MHz, 867.3 MHz")).as_ref()
    );
}

#[test]
fn a_meshcore_advert_shows_its_place_and_signature() {
    let view = lora(serde_json::json!({
        "protocol": "meshcore", "route": "flood", "payload_type": "advert", "version": 1,
        "path": ["a1", "b2"],
        "content": { "type": "advert", "public_key": "00112233", "timestamp": 1_700_000_000_u32,
            "node_type": "repeater", "name": "Hilltop", "lat": 51.5, "lon": -0.12,
            "signature_ok": true },
    }));
    assert_eq!(view.field("Protocol"), text(Some("MeshCore")).as_ref());
    assert_eq!(view.field("Route"), text(Some("flood")).as_ref());
    assert_eq!(view.field("Type"), text(Some("Advert")).as_ref());
    assert_eq!(view.field("Path"), text(Some("a1 → b2")).as_ref());
    assert_eq!(view.field("Name"), text(Some("Hilltop")).as_ref());
    assert_eq!(view.field("Node type"), text(Some("repeater")).as_ref());
    assert_eq!(
        view.field("Position"),
        position(Some(51.5), Some(-0.12)).as_ref()
    );
    assert_eq!(
        view.field("Advertised"),
        Some(&FieldValue::Time("2023-11-14T22:13:20Z".to_owned()))
    );
    assert_eq!(view.field("Signature"), text(Some("verified")).as_ref());
}
