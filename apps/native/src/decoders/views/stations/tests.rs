use serde_json::{Value, json};

use super::*;
use crate::decoders::testing::{event, record_at};

fn at(kind: &str, data: Value, second: u32) -> Arc<DecodedRecord> {
    Arc::new(record_at(
        event(kind, data),
        &format!("2026-05-01T12:00:{second:02}Z"),
        1,
    ))
}

fn newest_first_of(mut records: Vec<Arc<DecodedRecord>>) -> Vec<Arc<DecodedRecord>> {
    records.reverse();
    records
}

fn sonde(serial: &str, altitude_m: f64, second: u32) -> Arc<DecodedRecord> {
    at(
        "radiosonde",
        json!({ "sonde": "rs41", "serial": serial, "altitude_m": altitude_m, "errors_corrected": 0 }),
        second,
    )
}

#[test]
fn a_sonde_keeps_its_ceiling_and_marks_the_burst() {
    let records = newest_first_of(vec![
        at(
            "radiosonde",
            json!({ "sonde": "rs41", "serial": "S1", "altitude_m": 30_000.0, "temperature_c": -50.0, "errors_corrected": 0 }),
            1,
        ),
        sonde("S1", 31_000.0, 2),
        sonde("S1", 30_500.0, 3),
        sonde("S2", 1_000.0, 4),
    ]);
    let sondes = radiosonde_stations(&records);
    assert_eq!(sondes.len(), 2);
    assert_eq!(sondes[0].serial, "S2");
    let first = &sondes[1];
    assert_eq!(first.max_altitude_m, Some(31_000.0));
    assert_eq!(first.altitude_m, Some(30_500.0));
    assert!(first.burst);
    assert_eq!(first.temperature_c, Some(-50.0));
    assert_eq!(first.frames, 3);
    assert!(!sondes[0].burst);
    assert_eq!(radiosonde_log(&records, "S1", 2).len(), 2);
}

fn ble(address: Option<&str>, name: Option<&str>, repeats: u32, second: u32) -> Arc<DecodedRecord> {
    let mut data = json!({
        "pdu": "adv_ind", "phy": "le1m", "channel": 37, "level_dbfs": -40.0,
        "data": "0201061AFF4C000215", "repeats": repeats,
        "manufacturer": [{ "company_id": 76, "company": "Apple", "data": "02" }],
    });
    if let Some(address) = address {
        data["address"] = json!({ "address": address, "kind": "public" });
    }
    if let Some(name) = name {
        data["name"] = json!(name);
    }
    at("ble", data, second)
}

#[test]
fn ble_adverts_merge_by_station_and_count_repeats() {
    let records = newest_first_of(vec![
        ble(Some("AA:BB"), Some("Tag"), 2, 1),
        ble(Some("AA:BB"), None, 0, 2),
        ble(None, None, 0, 3),
    ]);
    let devices = ble_devices(&records);
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0].key, "ADV_IND 0201061AFF4C");
    let tag = &devices[1];
    assert_eq!(tag.name.as_deref(), Some("Tag"));
    assert_eq!(tag.vendor.as_deref(), Some("Apple"));
    assert_eq!(tag.heard, 4);
    assert_eq!(tag.channel, Some(37));
}

fn drone(messages: Value, transport: &str, second: u32) -> Arc<DecodedRecord> {
    at(
        "remote_id",
        json!({
            "transport": transport, "phy": "le1m", "address": "D2:11", "uas_id": "DRONE1",
            "level_dbfs": -50.0, "messages": messages,
        }),
        second,
    )
}

fn location(lat: f64, lon: f64) -> Value {
    json!({
        "type": "location", "status": "airborne", "lat": lat, "lon": lon,
        "height_m": 42.0, "speed_mps": 4.5, "height_reference": "takeoff",
    })
}

#[test]
fn a_drone_keeps_what_earlier_messages_said() {
    let records = newest_first_of(vec![
        drone(
            json!([
                { "type": "basic_id", "id_type": "serial_number", "ua_type": "rotorcraft", "uas_id": "DRONE1" },
                { "type": "system", "operator_location_type": "takeoff", "operator_lat": 47.0, "operator_lon": 8.0, "area_count": 1, "area_radius_m": 0 },
                { "type": "operator_id", "id_type": 0, "operator_id": "OP-1" },
            ]),
            "bluetooth_legacy",
            1,
        ),
        drone(json!([location(47.001, 8.0)]), "wifi_beacon", 2),
    ]);
    let drones = remote_id_drones(&records);
    assert_eq!(drones.len(), 1);
    let found = &drones[0];
    assert_eq!(found.key, "DRONE1");
    assert_eq!(found.kind, "Multirotor");
    assert_eq!(found.height_m, Some(42.0));
    assert_eq!(found.operator.as_deref(), Some("OP-1"));
    assert_eq!(found.links, vec!["BT4", "Wi-Fi beacon"]);
    assert_eq!(found.frames, 2);
    let pilot = found.pilot_m.unwrap_or_default();
    assert!((pilot - 111.2).abs() < 1.0, "{pilot}");
}

fn mesh(from: u32, content: Value, second: u32) -> Arc<DecodedRecord> {
    at(
        "lora",
        json!({
            "spreading_factor": 11, "bandwidth_hz": 250_000.0, "coding_rate": "4/5",
            "sync_word": 43, "implicit_header": false, "low_data_rate": false,
            "inverted_iq": false, "integrity": "crc_failed", "fec_corrected": 0,
            "snr_db": 7.5, "frequency_error_hz": 0.0, "payload": "00",
            "decoded": {
                "protocol": "meshtastic", "to": 4_294_967_295_u32, "from": from, "id": 1,
                "hop_limit": 3, "hop_start": 3, "want_ack": false, "via_mqtt": false,
                "channel_hash": 8, "next_hop": 0, "relay_node": 0, "encryption": "open",
                "content": content,
            },
        }),
        second,
    )
}

#[test]
fn lora_nodes_keep_their_name_and_read_the_message() {
    let records = newest_first_of(vec![
        mesh(
            0x1234,
            json!({ "type": "node_info", "id": "!00001234", "long_name": " Hilltop ", "short_name": "HT", "licensed": false }),
            1,
        ),
        mesh(0x1234, json!({ "type": "text", "text": "hello" }), 2),
    ]);
    let stations = lora_stations(&records);
    assert_eq!(stations.len(), 1);
    let node = &stations[0];
    assert_eq!(node.key, "!00001234");
    assert_eq!(node.protocol, "Meshtastic");
    assert_eq!(node.name.as_deref(), Some("Hilltop"));
    assert_eq!(node.message, "!00001234 → ^all · hello");
    assert_eq!(node.frames, 2);
}

fn weather(source: &str, data: Value, second: u32) -> Arc<DecodedRecord> {
    at(
        "aprs",
        json!({
            "source": source, "destination": "APRS", "info": "_", "tnc2": "x",
            "weather": data,
        }),
        second,
    )
}

#[test]
fn a_weather_station_folds_its_reports() {
    let records = newest_first_of(vec![
        weather(
            "WX1",
            json!({ "temperature_c": 12.0, "wind_gust_ms": 9.0, "humidity_pct": 80 }),
            1,
        ),
        weather(
            "WX1",
            json!({ "temperature_c": 8.0, "wind_gust_ms": 4.0 }),
            2,
        ),
        at(
            "aprs",
            json!({ "source": "CAR", "destination": "APRS", "info": "!", "tnc2": "x" }),
            3,
        ),
    ]);
    let stations = aprs_weather_stations(&records);
    assert_eq!(stations.len(), 1);
    let station = &stations[0];
    assert_eq!(station.min_temperature_c, Some(8.0));
    assert_eq!(station.max_temperature_c, Some(12.0));
    assert_eq!(station.max_gust_ms, Some(9.0));
    assert_eq!(station.weather.temperature_c, Some(8.0));
    assert_eq!(station.weather.humidity_pct, Some(80));
    assert_eq!(station.reports, 2);
}

#[test]
fn wind_names_where_it_blows_from() {
    let calm = AprsWeather::default();
    assert_eq!(wind(&calm), None);
    let blowing = AprsWeather {
        wind_speed_ms: Some(3.0),
        wind_dir_deg: Some(270),
        ..AprsWeather::default()
    };
    assert_eq!(wind(&blowing).as_deref(), Some("3.0 m/s from 270°"));
}

#[test]
fn the_latest_record_goes_by_time_not_order() {
    let records = vec![sonde("A", 1.0, 1), sonde("B", 1.0, 5), sonde("C", 1.0, 3)];
    let latest = latest_record(&records).map(|record| record.at.clone());
    assert_eq!(latest.as_deref(), Some("2026-05-01T12:00:05Z"));
    assert!(latest_record(&[]).is_none());
}

#[test]
fn a_dect_base_reads_its_voice_and_extended_security() {
    let base = at(
        "dect",
        json!({
            "side": "rfp", "update": "identity",
            "identity": { "rfpi": "0123456789", "pari": "1", "arc": "a", "sari_available": false, "rpn": 0 },
            "security": { "cipher_state": "clear", "authentication_supported": true, "dsaa2_supported": true },
            "voice": { "frames": 3, "playing": false },
            "handsets": [1, 2], "bursts": 10, "crc_errors": 0, "level_dbfs": -30.0,
        }),
        2,
    );
    let handset = at(
        "dect",
        json!({
            "side": "pp", "update": "identity",
            "identity": { "rfpi": "0123456789", "pari": "1", "arc": "a", "sari_available": false, "rpn": 0 },
            "security": { "cipher_state": "clear" },
            "voice": { "frames": 3, "playing": true },
            "bursts": 1, "crc_errors": 0, "level_dbfs": -60.0,
        }),
        1,
    );
    let stations = super::super::dect_stations(&[base, handset]);
    assert_eq!(stations.len(), 1);
    let station = &stations[0];
    assert_eq!(station.handsets, 2);
    assert_eq!(station.voice, Some(super::super::DectVoiceState::Muted));
    assert_eq!(
        super::super::dect_support(station.authentication, station.dsaa2, "DSAA2"),
        "yes · DSAA2"
    );
    assert_eq!(
        super::super::dect_support(station.ciphering, station.dsc2, "DSC2"),
        "-"
    );
}
