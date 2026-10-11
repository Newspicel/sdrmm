use sdrmm_wire::decode::{AdsbMessage, PocsagMessage, PocsagPayload, RdsUpdate, RttyText};

use super::*;

const T0: i64 = 1_786_276_800_000;

fn at(offset_ms: i64) -> String {
    jiff::Timestamp::from_millisecond(T0 + offset_ms)
        .map(|stamp| stamp.to_string())
        .unwrap_or_default()
}

fn record(event: DecoderEvent, channel: u32, freq_hz: f64, offset_ms: i64) -> DecodedRecord {
    DecodedRecord {
        origin: None,
        device_set: 0,
        channel,
        at: at(offset_ms),
        freq_hz,
        event,
        sinks: Vec::new(),
    }
}

fn adsb(message: AdsbMessage, offset_ms: i64) -> DecodedRecord {
    record(DecoderEvent::Adsb(message), 1, 1_090_000_000.0, offset_ms)
}

fn plane(icao: &str) -> AdsbMessage {
    AdsbMessage {
        icao: icao.to_owned(),
        df: 17,
        raw: "8d4840d6".to_owned(),
        ..AdsbMessage::default()
    }
}

fn rds(update: RdsUpdate) -> DecodedRecord {
    record(DecoderEvent::Rds(update), 2, 100_300_000.0, 0)
}

fn station_rds() -> RdsUpdate {
    RdsUpdate {
        groups: 1,
        blocks: 4,
        pi: Some("D3C2".to_owned()),
        ..RdsUpdate::default()
    }
}

fn rtty(text: &str) -> DecodedRecord {
    record(
        DecoderEvent::Rtty(RttyText {
            text: text.to_owned(),
        }),
        3,
        14_083_000.0,
        0,
    )
}

fn raw_of(record: &DecodedRecord) -> Option<&str> {
    match &record.event {
        DecoderEvent::Adsb(message) => Some(message.raw.as_str()),
        _ => None,
    }
}

fn adsb_of(station: &Station) -> Option<&AdsbMessage> {
    match &station.event {
        DecoderEvent::Adsb(message) => Some(message),
        _ => None,
    }
}

fn count(decoded: &Decoded, kind: &str) -> Option<usize> {
    decoded.frames(kind).map(|slice| slice.len())
}

fn stations(decoded: &Decoded, kind: &str) -> Option<usize> {
    decoded.stations(kind).map(|found| found.len())
}

#[test]
fn the_ring_keeps_the_newest_frames_and_drops_the_oldest() {
    let overflow = 10;
    let mut decoded = Decoded::default();
    let batch = (0..RING_CAPACITY + overflow)
        .map(|i| {
            let mut message = plane("abc123");
            message.raw = format!("frame-{i}");
            adsb(message, i as i64)
        })
        .collect();
    decoded.publish(batch);

    let frames = decoded.frames("adsb").cloned().unwrap_or_default();
    assert_eq!(frames.len(), RING_CAPACITY);
    let newest = format!("frame-{}", RING_CAPACITY + overflow - 1);
    let oldest = format!("frame-{overflow}");
    assert_eq!(
        frames.front().and_then(|r| raw_of(r)),
        Some(newest.as_str())
    );
    assert_eq!(frames.back().and_then(|r| raw_of(r)), Some(oldest.as_str()));
    assert_eq!(decoded.received, (RING_CAPACITY + overflow) as u64);
}

#[test]
fn each_frame_lands_in_its_own_decoders_slice() {
    let mut decoded = Decoded::default();
    let mut named = station_rds();
    named.ps = Some("RADIO 1".to_owned());
    decoded.publish(vec![
        adsb(plane("abc123"), 0),
        rds(station_rds()),
        rds(named),
        rtty("CQ"),
    ]);
    assert_eq!(count(&decoded, "adsb"), Some(1));
    assert_eq!(count(&decoded, "rds"), Some(2));
    assert_eq!(count(&decoded, "rtty"), Some(1));
    assert_eq!(count(&decoded, "ais"), None);
}

#[test]
fn an_untouched_slice_stays_the_same_allocation() {
    let mut decoded = Decoded::default();
    decoded.publish(vec![rds(station_rds())]);
    let before = decoded.frames("rds").cloned();
    decoded.publish(vec![adsb(plane("abc123"), 0)]);
    let after = decoded.frames("rds").cloned();
    assert!(matches!((before, after), (Some(a), Some(b)) if Arc::ptr_eq(&a, &b)));
}

#[test]
fn a_broadcast_object_ring_holds_only_a_handful() {
    let mut decoded = Decoded::default();
    let batch = (0..20)
        .map(|i| {
            record(
                DecoderEvent::BroadcastData(sdrmm_wire::decode::BroadcastData {
                    protocol: None,
                    label: Vec::new(),
                    service_id: None,
                    name: format!("slide {i}"),
                    media_type: "image/png".to_owned(),
                    bytes: Vec::new(),
                }),
                0,
                0.0,
                i,
            )
        })
        .collect();
    decoded.publish(batch);
    assert_eq!(
        count(&decoded, "broadcast_data"),
        Some(BROADCAST_DATA_CAPACITY)
    );
}

#[test]
fn partial_aircraft_frames_merge_into_one_row() {
    let mut decoded = Decoded::default();
    let mut first = plane("abc123");
    first.callsign = Some("DLH400".to_owned());
    first.type_code = Some(4);
    let mut second = plane("abc123");
    second.lat = Some(52.5);
    second.lon = Some(13.4);
    second.altitude_ft = Some(37_000);
    second.type_code = Some(11);
    decoded.publish(vec![adsb(first, 0), adsb(second, 1_000)]);

    let found = decoded.stations("adsb").cloned().unwrap_or_default();
    assert_eq!(found.len(), 1);
    let station = found.get("abc123").cloned();
    assert_eq!(station.as_ref().map(|s| s.frames), Some(2));
    assert_eq!(station.as_ref().map(|s| s.last_seen_ms), Some(T0 + 1_000));
    let merged = station
        .as_deref()
        .and_then(adsb_of)
        .cloned()
        .unwrap_or_default();
    assert_eq!(merged.callsign.as_deref(), Some("DLH400"));
    assert_eq!(merged.lat, Some(52.5));
    assert_eq!(merged.lon, Some(13.4));
    assert_eq!(merged.altitude_ft, Some(37_000));
    assert_eq!(merged.type_code, Some(11));
}

#[test]
fn one_row_per_emitter_and_none_for_text_decoders() {
    let mut decoded = Decoded::default();
    decoded.publish(vec![
        adsb(plane("abc123"), 0),
        adsb(plane("def456"), 0),
        rds(station_rds()),
        rtty("CQ CQ"),
    ]);
    assert_eq!(stations(&decoded, "adsb"), Some(2));
    assert_eq!(stations(&decoded, "rds"), Some(1));
    assert_eq!(stations(&decoded, "rtty"), None);
}

#[test]
fn stations_unseen_past_the_horizon_age_out() {
    let mut decoded = Decoded::default();
    decoded.publish(vec![adsb(plane("stale"), 0), adsb(plane("fresh"), 60_000)]);
    assert!(decoded.stale(30_000, T0 + 60_000));
    decoded.age_out(30_000, T0 + 60_000);
    let left: Vec<String> = decoded
        .stations("adsb")
        .map(|found| found.keys().cloned().collect())
        .unwrap_or_default();
    assert_eq!(left, vec!["fresh".to_owned()]);
}

#[test]
fn a_fresh_picture_is_not_stale() {
    let mut decoded = Decoded::default();
    decoded.publish(vec![adsb(plane("fresh"), 0)]);
    assert!(!decoded.stale(30_000, T0 + 1_000));
}

#[test]
fn an_aged_out_station_returns_from_scratch() {
    let mut decoded = Decoded::default();
    let mut named = plane("abc123");
    named.callsign = Some("DLH400".to_owned());
    decoded.publish(vec![adsb(named, 0)]);
    decoded.age_out(30_000, T0 + 60_000);
    decoded.publish(vec![adsb(plane("abc123"), 90_000)]);
    let station = decoded
        .stations("adsb")
        .and_then(|found| found.get("abc123").cloned());
    assert_eq!(station.as_ref().map(|s| s.frames), Some(1));
    assert_eq!(
        station
            .as_deref()
            .and_then(adsb_of)
            .and_then(|m| m.callsign.clone()),
        None
    );
}

#[test]
fn lost_frames_accumulate() {
    let mut decoded = Decoded::default();
    decoded.report_lost(12);
    decoded.report_lost(3);
    assert_eq!(decoded.lost, 15);
    decoded.publish(vec![adsb(plane("abc123"), 0), rds(station_rds())]);
    assert_eq!(decoded.lost, 15);
    assert_eq!(decoded.received, 2);
}

#[test]
fn dropping_frames_removes_only_the_matches_and_keeps_stations() {
    let mut decoded = Decoded::default();
    decoded.publish(vec![
        adsb(plane("abc123"), 0),
        adsb(plane("def456"), 1),
        rds(station_rds()),
        rtty("CQ"),
    ]);
    let rds_before = decoded.frames("rds").cloned();
    let dropped = decoded.drop_frames(|record| matches!(record.event.kind(), "adsb" | "rtty"));
    assert_eq!(dropped, 3);
    assert_eq!(count(&decoded, "adsb"), Some(0));
    assert_eq!(count(&decoded, "rtty"), Some(0));
    let rds_after = decoded.frames("rds").cloned();
    assert!(matches!((rds_before, rds_after), (Some(a), Some(b)) if Arc::ptr_eq(&a, &b)));
    assert_eq!(stations(&decoded, "adsb"), Some(2));
    assert_eq!(decoded.drop_frames(|record| record.channel == 99), 0);
}

#[test]
fn the_least_recently_seen_station_goes_once_the_cap_is_passed() {
    let overflow = 50;
    let mut decoded = Decoded::default();
    let batch = (0..STATION_CAPACITY + overflow)
        .map(|i| {
            record(
                DecoderEvent::Pocsag(PocsagMessage {
                    address: i as u32,
                    function: 3,
                    baud: 1200,
                    payload: PocsagPayload::Alpha,
                    text: format!("page {i}"),
                    errors_corrected: 0,
                }),
                3,
                466_230_000.0,
                i as i64 * 1000,
            )
        })
        .collect();
    decoded.publish(batch);
    let found = decoded.stations("pocsag").cloned().unwrap_or_default();
    assert_eq!(found.len(), STATION_CAPACITY);
    assert!(!found.contains_key("0"));
    assert!(found.contains_key(&(STATION_CAPACITY + overflow - 1).to_string()));
}

#[test]
fn a_backlog_rebuilds_stations_but_not_the_ring() {
    let mut decoded = Decoded::default();
    let mut first = plane("abc123");
    first.callsign = Some("DLH400".to_owned());
    let mut second = plane("abc123");
    second.lat = Some(52.5);
    second.lon = Some(13.4);
    let mut third = plane("def456");
    third.lat = Some(48.1);
    third.lon = Some(11.6);
    let touched = decoded.hydrate(&[adsb(first, 0), adsb(second, 1_000), adsb(third, 2_000)]);
    assert!(touched);
    assert_eq!(stations(&decoded, "adsb"), Some(2));
    let merged = decoded
        .stations("adsb")
        .and_then(|found| found.get("abc123").cloned());
    let message = merged
        .as_deref()
        .and_then(adsb_of)
        .cloned()
        .unwrap_or_default();
    assert_eq!(message.callsign.as_deref(), Some("DLH400"));
    assert_eq!(message.lat, Some(52.5));
    assert_eq!(count(&decoded, "adsb"), None);
    assert_eq!(decoded.received, 0);
}

#[test]
fn a_backlog_ignores_decoders_without_targets() {
    let mut decoded = Decoded::default();
    assert!(!decoded.hydrate(&[rtty("CQ CQ")]));
    assert_eq!(stations(&decoded, "rtty"), None);
}

#[test]
fn a_live_frame_the_backlog_carried_leaves_one_row() {
    let mut decoded = Decoded::default();
    let mut named = plane("abc123");
    named.callsign = Some("DLH400".to_owned());
    let duplicate = adsb(named, 0);
    decoded.hydrate(std::slice::from_ref(&duplicate));
    decoded.publish(vec![duplicate]);
    let station = decoded
        .stations("adsb")
        .and_then(|found| found.get("abc123").cloned());
    assert_eq!(stations(&decoded, "adsb"), Some(1));
    assert_eq!(station.as_ref().map(|s| s.last_seen_ms), Some(T0));
}

fn parsed(kind: &str, data: serde_json::Value, offset_ms: i64) -> DecodedRecord {
    record(
        crate::decoders::testing::event(kind, data),
        1,
        433_000_000.0,
        offset_ms,
    )
}

fn meshtastic(content: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "spreading_factor": 11, "bandwidth_hz": 250_000.0, "coding_rate": "4/5",
        "sync_word": 43, "implicit_header": false, "low_data_rate": false,
        "inverted_iq": false, "integrity": "crc_ok", "fec_corrected": 0,
        "snr_db": 7.5, "frequency_error_hz": 0.0, "payload": "00",
        "decoded": {
            "protocol": "meshtastic", "to": 1, "from": 0x1234, "id": 1,
            "hop_limit": 3, "hop_start": 3, "want_ack": false, "via_mqtt": false,
            "channel_hash": 8, "next_hop": 0, "relay_node": 0, "encryption": "open",
            "content": content,
        },
    })
}

#[test]
fn sondes_mesh_nodes_drones_and_beacons_become_stations() {
    let mut decoded = Decoded::default();
    decoded.publish(vec![
        parsed(
            "radiosonde",
            serde_json::json!({ "sonde": "rs41", "serial": "S1", "errors_corrected": 0 }),
            0,
        ),
        parsed(
            "ble",
            serde_json::json!({
                "pdu": "adv_ind", "phy": "le1m", "level_dbfs": -40.0, "data": "00",
                "address": { "address": "AA:BB", "kind": "public" },
            }),
            0,
        ),
        parsed(
            "remote_id",
            serde_json::json!({
                "transport": "bluetooth_legacy", "phy": "le1m", "address": "D2:11",
                "level_dbfs": -50.0, "messages": [],
            }),
            0,
        ),
        parsed(
            "lora",
            meshtastic(serde_json::json!({ "type": "text", "text": "hi" })),
            0,
        ),
    ]);
    for kind in ["radiosonde", "ble", "remote_id", "lora"] {
        assert_eq!(stations(&decoded, kind), Some(1), "{kind}");
    }
}

#[test]
fn a_mesh_node_keeps_its_last_position() {
    let mut decoded = Decoded::default();
    decoded.publish(vec![parsed(
        "lora",
        meshtastic(serde_json::json!({ "type": "position", "lat": 47.5, "lon": 8.5 })),
        0,
    )]);
    decoded.publish(vec![parsed(
        "lora",
        meshtastic(serde_json::json!({ "type": "text", "text": "hi" })),
        1_000,
    )]);
    let position = decoded
        .stations("lora")
        .and_then(|found| found.get("!00001234").cloned())
        .and_then(|station| station.event.position());
    assert_eq!(position, Some((47.5, 8.5)));
}

#[test]
fn a_drone_keeps_messages_a_later_frame_left_out() {
    let mut decoded = Decoded::default();
    let frame = |messages: serde_json::Value, offset_ms| {
        parsed(
            "remote_id",
            serde_json::json!({
                "transport": "bluetooth_legacy", "phy": "le1m", "address": "D2:11",
                "uas_id": "DRONE1", "level_dbfs": -50.0, "messages": messages,
            }),
            offset_ms,
        )
    };
    decoded.publish(vec![frame(
        serde_json::json!([{ "type": "operator_id", "id_type": 0, "operator_id": "OP-1" }]),
        0,
    )]);
    decoded.publish(vec![frame(
        serde_json::json!([{ "type": "self_id", "description_type": 0, "text": "Survey" }]),
        1_000,
    )]);
    let merged = decoded
        .stations("remote_id")
        .and_then(|found| found.get("DRONE1").cloned());
    let Some(DecoderEvent::RemoteId(frame)) = merged.map(|station| station.event.clone()) else {
        panic!("the drone is a station");
    };
    assert_eq!(frame.messages.len(), 2);
}
