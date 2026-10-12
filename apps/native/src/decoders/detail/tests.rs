use std::collections::HashMap;

use serde_json::{Value, json};

use super::*;
use crate::decoders::testing::event;

fn detail_of(kind: &str, data: Value) -> EventDetail {
    event_detail(&event(kind, data))
}

fn fields_of(kind: &str, data: Value) -> HashMap<Cow<'static, str>, String> {
    detail_of(kind, data).fields.into_iter().collect()
}

fn broadcast_of(data: Value) -> EventDetail {
    broadcast_detail(&serde_json::from_value(data).expect("a broadcast status"))
}

fn broadcast_fields(data: Value) -> HashMap<Cow<'static, str>, String> {
    broadcast_of(data).fields.into_iter().collect()
}

fn labels(detail: &EventDetail) -> Vec<&str> {
    detail
        .fields
        .iter()
        .map(|(label, _)| label.as_ref())
        .collect()
}

fn has(shown: &HashMap<Cow<'static, str>, String>, expected: &[(&str, &str)]) {
    for (label, value) in expected {
        assert_eq!(
            shown.get(*label).map(String::as_str),
            Some(*value),
            "{label}"
        );
    }
}

#[test]
fn a_dect_base_station_breaks_into_identity_and_security() {
    let detail = detail_of(
        "dect",
        json!({
            "side": "rfp", "update": "capabilities",
            "identity": {
                "rfpi": "01234D5E6D", "pari": "02469ABCD", "arc": "a", "sari_available": false,
                "rpn": 5, "emc": 0x1234, "fpn": 0x1abcd, "multicell": true,
            },
            "carrier": 4, "carrier_hz": 1_890_432_000.0, "slot_pair": 2, "rf_carriers": 0x3ff,
            "capabilities": ["full_slot", "standard_authentication", "standard_ciphering"],
            "security": {
                "cipher_state": "active", "authentication_supported": true,
                "ciphering_supported": true, "encryption_events": 1,
            },
            "extended_carriers": false, "bursts": 40, "crc_errors": 2, "level_dbfs": -28.25,
        }),
    );
    let shown: HashMap<_, _> = detail.fields.iter().cloned().collect();
    has(
        &shown,
        &[
            ("RFPI", "01234D5E6D"),
            ("Access rights class", "A residential / small PBX"),
            ("Manufacturer code", "1234"),
            ("Cell", "multi-cell"),
            ("Frequency", "1.890432 GHz"),
            ("Authentication", "yes"),
            ("Ciphering", "yes"),
            ("Encryption", "encryption active"),
            ("Carriers available", "0, 1, 2, 3, 4, 5, 6, 7, 8, 9"),
        ],
    );
    assert!(
        detail
            .body
            .is_some_and(|body| body.contains("standard ciphering (DSC)"))
    );
}

#[test]
fn weak_signal_spots_show_timing_and_link_measurements() {
    let detail = detail_of(
        "wspr",
        json!({
            "text": "K1ABC FN42 37", "callsign": "K1ABC", "grid": "FN42", "power_dbm": 37,
            "snr_db": -21.0, "audio_hz": 1501.25, "time_offset_s": 0.75, "drift_hz": -0.2,
        }),
    );
    let shown: HashMap<_, _> = detail.fields.iter().cloned().collect();
    has(
        &shown,
        &[
            ("Callsign", "K1ABC"),
            ("Grid", "FN42"),
            ("Power", "37 dBm"),
            ("SNR", "-21 dB"),
            ("Time offset", "+0.75 s"),
            ("Drift", "-0.2 Hz"),
        ],
    );
    assert_eq!(detail.body.as_deref(), Some("K1ABC FN42 37"));
}

#[test]
fn selcall_shows_the_plan_code_and_duration() {
    let shown = fields_of(
        "selcall",
        json!({"system": "zvei1", "code": "A11D0", "tone_ms": 70}),
    );
    assert_eq!(shown.len(), 3);
    has(
        &shown,
        &[
            ("Tone plan", "ZVEI-1"),
            ("Code", "A11D0"),
            ("Tone duration", "70 ms"),
        ],
    );
}

#[test]
fn fields_a_frame_did_not_carry_are_left_out() {
    let bare = detail_of("adsb", json!({"icao": "3c6444", "df": 11, "raw": "5d"}));
    assert_eq!(labels(&bare), ["ICAO", "Downlink format", "Raw"]);
    let full = fields_of(
        "adsb",
        json!({
            "icao": "3c6444", "df": 17, "raw": "8d3c6444", "callsign": " DLH123 ",
            "altitude_ft": 37000, "ground_speed_kt": 451.4, "track_deg": 271.6,
            "vertical_rate_fpm": -1088, "lat": 52.52, "lon": 13.405,
        }),
    );
    has(
        &full,
        &[
            ("Callsign", "DLH123"),
            ("Altitude", "37,000 ft"),
            ("Position", "52.52000, 13.40500"),
            ("Ground speed", "451.4 kt"),
            ("Track", "272°"),
            ("Vertical rate", "-1088 ft/min"),
        ],
    );
}

#[test]
fn a_pocsag_ric_is_padded_and_its_function_named() {
    let detail = detail_of(
        "pocsag",
        json!({
            "address": 1234, "function": 3, "baud": 1200, "payload": "alpha",
            "text": "CALL 42", "errors_corrected": 2,
        }),
    );
    let shown: HashMap<_, _> = detail.fields.iter().cloned().collect();
    has(
        &shown,
        &[
            ("RIC", "0001234"),
            ("Function", "D (3)"),
            ("Baud", "1200"),
            ("Repaired", "2"),
        ],
    );
    assert_eq!(detail.body.as_deref(), Some("CALL 42"));
}

#[test]
fn navtex_keeps_its_text_and_header() {
    let detail = detail_of(
        "navtex",
        json!({
            "station": "D", "subject": "A", "subject_name": "Navigational warning", "serial": 7,
            "text": "GALE WARNING\nGERMAN BIGHT", "errors_corrected": 3, "complete": false,
        }),
    );
    let shown: HashMap<_, _> = detail.fields.iter().cloned().collect();
    has(
        &shown,
        &[
            ("Header", "DA07"),
            ("Subject", "Navigational warning"),
            ("Serial", "07"),
            ("Ended with NNNN", "no: flushed early"),
            ("Repaired", "3 characters"),
        ],
    );
    assert_eq!(detail.body.as_deref(), Some("GALE WARNING\nGERMAN BIGHT"));
}

#[test]
fn acars_names_direction_and_continuation() {
    let shown = fields_of(
        "acars",
        json!({
            "mode": "2", "registration": "D-AIBC", "flight": "LH0400 ", "label": "H1",
            "block_id": "3", "downlink": true, "seq_no": "M01A",
            "text": "POS N52.5 E013.4\nFL370", "more": true,
        }),
    );
    has(
        &shown,
        &[
            ("Registration", "D-AIBC"),
            ("Flight", "LH0400"),
            ("Direction", "downlink"),
            ("Sequence", "M01A"),
            ("Acknowledges", "NAK"),
            ("Continues", "yes: another block follows"),
        ],
    );
}

#[test]
fn not_encrypted_differs_from_unsaid() {
    let said = fields_of(
        "dv",
        json!({"mode": "dmr", "kind": "header", "errors_corrected": 0, "encrypted": false}),
    );
    assert_eq!(said.get("Encrypted").map(String::as_str), Some("no"));
    let silent = fields_of(
        "dv",
        json!({"mode": "dmr", "kind": "header", "errors_corrected": 0}),
    );
    assert!(!silent.contains_key("Encrypted"));
    assert!(!silent.contains_key("Trunking"));
    assert!(!silent.contains_key("Checksum"));
}

#[test]
fn dmr_metadata_is_exposed_and_packet_data_kept() {
    let detail = detail_of(
        "dv",
        json!({
            "mode": "dmr", "kind": "control", "errors_corrected": 2, "vendor": "hytera",
            "manufacturer_id": 8, "talker_alias": "Dispatcher", "lat": 52.52, "lon": 13.405,
            "position_error_m": 20, "channel": 407, "emergency": true, "algorithm_id": 5,
            "key_id": 42, "message_indicator": "001122334455667788",
            "slot_activity": [{"slot": 2, "activity": "group voice", "destination_hash": 0xab}],
            "data": "A1B2C3",
        }),
    );
    let shown: HashMap<_, _> = detail.fields.iter().cloned().collect();
    has(
        &shown,
        &[
            ("Vendor", "Hytera (0x08)"),
            ("Position error", "≤ 20 m"),
            ("Channel", "407"),
            ("Emergency", "yes"),
            ("Slot activity", "TS2 group voice (hash 0xAB)"),
            ("Algorithm", "0x05"),
            ("Key ID", "0x002A"),
            ("Repaired", "2 bits"),
        ],
    );
    assert_eq!(detail.body.as_deref(), Some("A1B2C3"));
}

#[test]
fn a_trunked_burst_says_its_side_and_checksum() {
    let checked = fields_of(
        "dv",
        json!({
            "mode": "dmr", "kind": "control", "errors_corrected": 0,
            "trunk_protocol": "tier_three", "control_channel": true, "crc_verified": true,
        }),
    );
    has(
        &checked,
        &[
            ("Trunking", "Tier III · control channel"),
            ("Checksum", "verified"),
        ],
    );
    let unchecked = fields_of(
        "dv",
        json!({
            "mode": "dmr", "kind": "control", "errors_corrected": 0,
            "trunk_protocol": "tier_three", "control_channel": false, "crc_verified": false,
        }),
    );
    has(
        &unchecked,
        &[
            ("Trunking", "Tier III · traffic channel"),
            ("Checksum", "not verified: read on error correction alone"),
        ],
    );
}

#[test]
fn rds_reads_as_fields_with_radiotext_as_body() {
    let detail = detail_of(
        "rds",
        json!({
            "groups": 100, "blocks": 402, "block_errors": 2, "pi": "D389", "ps": "RADIO 1 ",
            "pty_name": "Pop Music", "tp": true, "music": false,
            "alt_freqs_hz": [98_000_000.0, 100_500_000.0], "radiotext": "Now playing something",
        }),
    );
    let shown: HashMap<_, _> = detail.fields.iter().cloned().collect();
    has(
        &shown,
        &[
            ("Station", "RADIO 1"),
            ("Programme type", "Pop Music"),
            ("Traffic programme", "yes"),
            ("Content", "speech"),
            ("Alternative frequencies", "98 MHz, 100.5 MHz"),
            ("Block errors", "2"),
        ],
    );
    assert_eq!(detail.body.as_deref(), Some("Now playing something"));
}

#[test]
fn broadcast_audio_failures_show_regardless_of_lock() {
    let shown = broadcast_fields(json!({
        "system": "dab", "locked": true, "snr_db": 20.0, "frequency_error_hz": 0.0,
        "audio_frames_ok": 41, "audio_frames_bad": 2,
        "audio_error": "Broadcast audio input queue overflow",
    }));
    has(
        &shown,
        &[
            ("Lock", "locked"),
            ("Audio frames", "41"),
            ("Audio failures", "2"),
            ("Audio error", "Broadcast audio input queue overflow"),
        ],
    );
}

#[test]
fn a_broadcast_acquisition_invents_no_multiplex_metadata() {
    let detail = broadcast_of(json!({
        "system": "dvb_s2", "locked": true, "snr_db": 18.26, "frequency_error_hz": -32.4,
        "symbol_rate": 333000.0,
    }));
    let shown: Vec<(&str, &str)> = detail
        .fields
        .iter()
        .map(|(label, value)| (label.as_ref(), value.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            ("System", "DVB-S2"),
            ("Lock", "locked"),
            ("SNR", "18.3 dB"),
            ("Frequency error", "-32 Hz"),
            ("Symbol rate", "333000 Bd"),
        ]
    );
    assert_eq!(detail.body, None);
}

#[test]
fn aprs_unpacks_its_monitor_line() {
    let detail = detail_of(
        "aprs",
        json!({
            "source": "DL1ABC-9", "destination": "S32U6T", "path": ["WIDE1-1", "WIDE2-1"],
            "info": "`(_fn\"Oj/", "tnc2": "DL1ABC-9>S32U6T:`(_fn\"Oj/", "lat": 52.52,
            "lon": 13.405, "course_deg": 251.0, "speed_kt": 20.0, "altitude_ft": 1500,
            "mic_e_message": "En Route",
        }),
    );
    let shown: HashMap<_, _> = detail.fields.iter().cloned().collect();
    has(
        &shown,
        &[
            ("Path", "WIDE1-1 → WIDE2-1"),
            ("Course", "251°"),
            ("Speed", "20.0 kt"),
            ("Altitude", "1,500 ft"),
            ("Mic-E message", "En Route"),
        ],
    );
    assert_eq!(detail.body.as_deref(), Some("DL1ABC-9>S32U6T:`(_fn\"Oj/"));
}

#[test]
fn an_empty_text_frame_adds_nothing() {
    let detail = detail_of("rtty", json!({"text": ""}));
    assert!(detail.fields.is_empty());
    assert_eq!(detail.body, None);
}

#[test]
fn a_utc_offset_carries_its_sign() {
    assert_eq!(utc_offset(Some(120)).as_deref(), Some("UTC+02:00"));
    assert_eq!(utc_offset(Some(-330)).as_deref(), Some("UTC−05:30"));
    assert_eq!(utc_offset(None), None);
}

#[test]
fn a_hot_emergency_reads_in_capitals() {
    let detail = detail_of(
        "eot",
        json!({
            "unit_address": 12345,
            "report": { "unit": "head", "command": "emergency", "code": 170, "copies": 3 },
            "errors_corrected": 0,
            "rejected": 0,
        }),
    );
    let shown: HashMap<_, _> = detail.fields.iter().cloned().collect();
    has(
        &shown,
        &[
            ("Command", "EMERGENCY"),
            ("Copies", "3/3"),
            ("Address", "12345"),
        ],
    );
    assert_eq!(detail.body, None);
}

#[test]
fn a_radar_track_names_its_change_and_aircraft() {
    let shown = fields_of(
        "radar",
        json!({
            "track_id": 7, "change": "confirmed", "range_km": 12.5, "range_rate_mps": -80.0,
            "doppler_hz": 120.0, "snr_db": 14.0, "icao": "3C6444",
        }),
    );
    has(
        &shown,
        &[
            ("Bistatic range", "12.50 km"),
            ("Range rate", "-80 m/s"),
            ("Aircraft", "3C6444"),
        ],
    );
    assert!(
        shown
            .get("Track")
            .is_some_and(|track| track.starts_with("T7 "))
    );
}

#[test]
fn an_eot_rear_unit_reads_its_brake_pipe() {
    let shown = fields_of(
        "eot",
        json!({
            "unit_address": 12345, "errors_corrected": 1, "rejected": 0,
            "report": {
                "unit": "rear", "message_type": 1, "arming": "armed", "pressure_psig": 88,
                "battery": "very_low", "battery_charge_pct": 40, "valve_ok": true,
                "confirmed": true, "turbine": false, "motion": true, "marker_light": true,
                "marker_battery_low": false, "discretionary": false, "chaining": 0,
            },
        }),
    );
    has(
        &shown,
        &[
            ("Unit", "Rear (EOT)"),
            ("Brake pipe", "88 psig"),
            ("Battery", "very low"),
            ("Arming", "armed"),
            ("Repaired", "1"),
        ],
    );
    assert!(!shown.contains_key("Rejected"));
}

#[test]
fn a_hot_request_names_an_unknown_command() {
    let shown = fields_of(
        "eot",
        json!({
            "unit_address": 7, "errors_corrected": 0, "rejected": 0,
            "report": { "unit": "head", "command": "other", "code": 10, "copies": 2 },
        }),
    );
    has(
        &shown,
        &[
            ("Unit", "Head (HOT)"),
            ("Command", "command 0x0a"),
            ("Copies", "2/3"),
        ],
    );
}

#[test]
fn weather_pictures_and_sondes_break_down() {
    let apt = fields_of(
        "apt",
        json!({ "seq": 1, "lines": 900, "complete": false, "duration_ms": 450_000, "channel_a": "ch2" }),
    );
    has(
        &apt,
        &[
            ("Channels", "ch 2 + ?"),
            ("State", "cut short"),
            ("Took", "450.0 s"),
        ],
    );
    let sonde = fields_of(
        "radiosonde",
        json!({
            "sonde": "rs41", "serial": "S1234", "altitude_m": 12_345.4, "climb_ms": 5.0,
            "humidity_pct": 54.6, "errors_corrected": 0, "rejected": 2,
        }),
    );
    has(
        &sonde,
        &[
            ("Type", "RS41"),
            ("Altitude", "12,345 m"),
            ("Climb", "+5.0 m/s"),
            ("Humidity", "55%"),
        ],
    );
}

#[test]
fn an_aprs_weather_report_adds_its_readings() {
    let shown = fields_of(
        "aprs",
        json!({
            "source": "WX1", "destination": "APRS", "info": "_", "tnc2": "WX1>APRS:_",
            "weather": { "wind_dir_deg": 90, "wind_speed_ms": 2.0, "temperature_c": 21.5, "luminosity_wm2": 300 },
        }),
    );
    has(
        &shown,
        &[
            ("Wind", "2.0 m/s from 90°"),
            ("Temperature", "21.5 °C"),
            ("Luminosity", "300 W/m²"),
        ],
    );
}

#[test]
fn a_meshtastic_frame_lists_route_content_and_metrics() {
    let detail = detail_of(
        "lora",
        json!({
            "spreading_factor": 11, "bandwidth_hz": 250_000.0, "coding_rate": "4/5",
            "sync_word": 43, "implicit_header": false, "low_data_rate": false,
            "inverted_iq": false, "integrity": "crc_ok", "fec_corrected": 0,
            "snr_db": 7.5, "frequency_error_hz": -120.0, "payload": "AB",
            "decoded": {
                "protocol": "meshtastic", "to": 4_294_967_295_u32, "from": 0x1234, "id": 1,
                "hop_limit": 1, "hop_start": 3, "want_ack": false, "via_mqtt": false,
                "channel_hash": 8, "next_hop": 0, "relay_node": 0, "encryption": "channel",
                "content": { "type": "telemetry", "kind": "device", "metrics": [{ "name": "Battery", "value": 87.0 }] },
            },
        }),
    );
    let shown: HashMap<_, _> = detail.fields.iter().cloned().collect();
    has(
        &shown,
        &[
            ("Protocol", "Meshtastic"),
            ("Sync word", "0x2B"),
            ("From", "!00001234"),
            ("To", "^all"),
            ("Hops", "2 of 3"),
            ("Encryption", "channel"),
            ("Battery", "87"),
            ("Frequency error", "-120 Hz"),
        ],
    );
    assert_eq!(labels(&detail).last(), Some(&"Payload"));
}

#[test]
fn a_drone_frame_names_type_pilot_and_operator() {
    let detail = detail_of(
        "remote_id",
        json!({
            "transport": "bluetooth_legacy", "phy": "le1m", "address": "D2:11",
            "uas_id": "DRONE1", "level_dbfs": -41.0,
            "messages": [
                { "type": "basic_id", "id_type": "serial_number", "ua_type": "rotorcraft", "uas_id": "DRONE1" },
                { "type": "operator_id", "id_type": 0, "operator_id": "OP-1" },
                { "type": "self_id", "description_type": 0, "text": "Survey" },
            ],
        }),
    );
    let shown: HashMap<_, _> = detail.fields.iter().cloned().collect();
    has(
        &shown,
        &[
            ("Type", "Multirotor"),
            ("Operator", "OP-1"),
            ("Link", "BT4"),
            ("Level", "-41.0 dBFS"),
        ],
    );
    assert_eq!(detail.body.as_deref(), Some("Survey"));
}

#[test]
fn a_ble_advert_lists_makers_by_name() {
    let detail = detail_of(
        "ble",
        json!({
            "pdu": "adv_ind", "phy": "le1m", "channel": 38, "level_dbfs": -40.0, "data": "0201",
            "address": { "address": "AA:BB", "kind": "random_static" },
            "flags": { "limited": false, "general": true, "le_only": true },
            "manufacturer": [{ "company_id": 76, "company": "Apple", "data": "1005" }, { "company_id": 1, "data": "00" }],
            "beacon": { "type": "find_my", "maintained": true },
        }),
    );
    let shown: HashMap<_, _> = detail.fields.iter().cloned().collect();
    has(
        &shown,
        &[
            ("Address type", "random static"),
            ("Apple", "1005"),
            ("Company 0x0001", "00"),
            ("Beacon", "Find My"),
            ("Beacon data", "owner nearby"),
            ("Flags", "general, LE only"),
        ],
    );
    assert_eq!(detail.body.as_deref(), Some("0201"));
}

#[test]
fn airtime_reports_list_channels_and_kinds() {
    let wifi = fields_of(
        "wifi_occupancy",
        json!({
            "window_ms": 1000, "measured": 0.98,
            "channels": [{ "band": "ghz2_4", "number": 6, "centre_hz": 2.437e9, "busy": 0.05, "level_dbfs": -40.0, "floor_dbfs": -80.0 }],
        }),
    );
    has(
        &wifi,
        &[
            ("2.4 GHz ch 6", "5.0% busy · -40 dBFS · floor -80 dBFS"),
            ("Measured", "98%"),
        ],
    );
    let ism = fields_of(
        "ism_survey",
        json!({
            "window_ms": 1000, "low_hz": 2.4e9, "high_hz": 2.42e9, "measured": 1.0, "busy": 0.3,
            "kinds": [{ "kind": "bluetooth", "bursts": 12, "airtime": 0.02, "mean_us": 366.4, "peak_dbfs": -30.0, "centres_mhz": [2402.0] }],
        }),
    );
    has(
        &ism,
        &[
            ("Busy", "30%"),
            ("Bluetooth", "12 bursts · 2.0% · 366 µs · 2402 MHz"),
        ],
    );
    assert!(!ism.contains_key("Dropped"));
}
