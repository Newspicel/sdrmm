use serde_json::json;

use super::{DecoderEvent, EventKind};

const FIXTURES: &[(EventKind, &str)] = &[
    (
        EventKind::Transmission,
        r#"{"id":1,"state":"completed","signal":{"modulation":"fm","confidence":0.9,"frequency_hz":145500000.0,"center_offset_hz":0.0,"bandwidth_hz":12500.0,"snr_db":20.0,"features":{"envelope_variation":0.1,"duty":1.0,"keying_depth_db":0.0,"spectral_asymmetry":0.0,"carrier_db":0.0,"spectral_flatness":0.2,"frequency_levels":0,"frequency_spread_hz":3000.0,"square_line_db":0.0,"quartic_line_db":0.0}},"start_sample":0,"end_sample":48000,"sample_rate_hz":48000.0,"duration_ms":1000,"audio":{"url":"/api/clips/1","media_type":"audio/wav"}}"#,
    ),
    (
        EventKind::Rds,
        r#"{"pi":"D3C2","ps":"RADIO 1","groups":4,"blocks":16,"block_errors":0}"#,
    ),
    (
        EventKind::Pocsag,
        r#"{"address":1234567,"function":3,"baud":1200,"payload":"alpha","text":"TEST","errors_corrected":0}"#,
    ),
    (
        EventKind::Flex,
        r#"{"address":1234567,"payload":"alpha","text":"TEST","baud":1600,"levels":2,"cycle":1,"frame":2,"phase":"A","errors_corrected":0}"#,
    ),
    (
        EventKind::Ermes,
        r#"{"local_address":4242,"message_number":1,"payload":"alpha","text":"TEST","urgent":false,"alert":0,"errors_corrected":0}"#,
    ),
    (
        EventKind::Eot,
        r#"{"unit_address":4242,"report":{"unit":"head","command":"status_request","code":85,"copies":3},"errors_corrected":0}"#,
    ),
    (
        EventKind::Sarsat,
        r#"{"hex_id":"ADCD00800440401","message":"FFFED0","long":true,"self_test":false,"country":366,"protocol":"standard_location","beacon":"epirb","lat":43.5,"lon":-1.5,"errors_corrected":0}"#,
    ),
    (
        EventKind::Tetrapol,
        r#"{"channel":"signalling","scrambling":0,"address":{"kind":"group","id":393},"payload":{"message":"voice","seconds":2.0},"data":"00","rejected":0}"#,
    ),
    (
        EventKind::Adsb,
        r#"{"icao":"3c6444","df":17,"callsign":"DLH400  ","altitude_ft":37000,"lat":52.5,"lon":13.4,"track_deg":-90.0,"raw":"8d3c6444"}"#,
    ),
    (
        EventKind::Ais,
        r#"{"mmsi":211234560,"msg_type":1,"ais_channel":"A","name":"ELBE","lat":53.5,"lon":9.9,"cog_deg":120.0,"heading_deg":511,"nmea":"!AIVDM"}"#,
    ),
    (
        EventKind::Aprs,
        r#"{"source":"DL1ABC-9","destination":"APRS","info":"!","lat":48.1,"lon":11.5,"course_deg":90.0,"altitude_ft":1000,"tnc2":"DL1ABC-9>APRS:!"}"#,
    ),
    (EventKind::Rtty, r#"{"text":"CQ CQ DE DL1ABC"}"#),
    (EventKind::Morse, r#"{"text":"CQ DE DL1ABC","wpm":20.0}"#),
    (
        EventKind::CwSkimmer,
        r#"{"offset_hz":350.0,"text":"CQ DE DL1ABC","wpm":24.0,"snr_db":12.0}"#,
    ),
    (
        EventKind::Selcall,
        r#"{"system":"zvei1","code":"12345","tone_ms":70}"#,
    ),
    (
        EventKind::Navtex,
        r#"{"station":"S","subject":"A","subject_name":"Navigational warning","serial":12,"text":"GALE","errors_corrected":0,"complete":true}"#,
    ),
    (
        EventKind::Acars,
        r#"{"mode":"2","registration":".D-ABCD","label":"H1","block_id":"1","downlink":true,"flight":"LH400","text":"POS","more":false}"#,
    ),
    (EventKind::Tone, r#"{"ctcss_hz":88.5,"open":true}"#),
    (
        EventKind::Scrambler,
        r#"{"inversion_hz":3300.0,"confidence":0.8}"#,
    ),
    (
        EventKind::Dv,
        r#"{"mode":"dmr","kind":"voice","source":2621001,"destination":505,"group_call":true,"encrypted":false,"emergency":false,"lat":52.5,"lon":13.4,"errors_corrected":0}"#,
    ),
    (
        EventKind::Call,
        r#"{"id":1,"node":"dmr","started_at":"2026-08-16T10:00:00Z","ended_at":"2026-08-16T10:00:02Z","duration_ms":2000,"device_set":1,"channel":2,"freq_hz":451125000.0,"mode":"dmr","source":2621001,"destination":505,"group_call":true,"encrypted":false,"emergency":false,"audio":{"url":"/api/clips/1","media_type":"audio/wav"}}"#,
    ),
    (
        EventKind::Ft8,
        r#"{"text":"CQ DL1ABC JO62","snr_db":-12.0,"audio_hz":1500.0,"time_offset_s":0.1,"hard_errors":0}"#,
    ),
    (
        EventKind::Ft4,
        r#"{"text":"W1AW DL1ABC JO62","snr_db":-8.0,"audio_hz":900.0,"time_offset_s":0.0,"hard_errors":0}"#,
    ),
    (EventKind::Psk, r#"{"baud":"psk31","text":"CQ DE DL1ABC"}"#),
    (
        EventKind::Wspr,
        r#"{"text":"DL1ABC JO62 37","callsign":"DL1ABC","grid":"JO62","power_dbm":37,"snr_db":-20.0,"audio_hz":1500.0,"time_offset_s":0.0,"drift_hz":0.0}"#,
    ),
    (EventKind::Ident, r#"{"snr_db":12.0}"#),
    (
        EventKind::BroadcastData,
        r#"{"name":"slide.jpg","media_type":"image/jpeg","bytes":[1,2,3]}"#,
    ),
    (
        EventKind::RadioClock,
        r#"{"standard":"dcf77","datetime":"2026-08-15T12:34:00+02:00","symbols":""}"#,
    ),
    (
        EventKind::Gnss,
        r#"{"prn":7,"doppler_hz":1000.0,"code_phase_chips":158.3,"cn0_db_hz":44.5}"#,
    ),
    (
        EventKind::Sstv,
        r#"{"seq":1,"mode":"martin_m1","width":320,"height":256,"lines":256,"complete":true,"duration_ms":114300}"#,
    ),
    (
        EventKind::Vor,
        r#"{"station":"TGO","station_lat":52.5,"station_lon":13.3,"magnetic_declination_deg":3.0,"radial_deg":90.0,"variable_phase_deg":0.0,"reference_phase_deg":0.0,"signal_db":-40.0,"confidence":0.9}"#,
    ),
    (
        EventKind::Ils,
        r#"{"component":"localizer","modulation_90":0.2,"modulation_150":0.2,"ddm":0.01,"deviation_dots":0.5,"signal_db":-50.0}"#,
    ),
    (
        EventKind::Dsc,
        r#"{"message_type":"distress","station":"211234560","crc_ok":true,"lat":54.0,"lon":10.0,"details":null}"#,
    ),
    (
        EventKind::InmarsatStdc,
        r#"{"message_type":"position","station":"421234567","crc_ok":true,"lat":10.0,"lon":60.0,"details":null}"#,
    ),
    (
        EventKind::InmarsatAero,
        r#"{"message_type":"acars","station":".D-ABCD","crc_ok":true,"lat":50.0,"lon":-30.0,"details":null}"#,
    ),
    (
        EventKind::Vdl2,
        r#"{"message_type":"acars","station":"3C6444","crc_ok":true,"lat":51.0,"lon":9.0,"details":null}"#,
    ),
    (
        EventKind::Hfdl,
        r#"{"message_type":"position","station":"3C6444","crc_ok":true,"lat":45.0,"lon":-20.0,"details":null}"#,
    ),
    (
        EventKind::Iridium,
        r#"{"message_type":"ring alert","station":"beam 12","crc_ok":true,"lat":47.0,"lon":8.0,"details":null}"#,
    ),
    (
        EventKind::Df,
        r#"{"bearing_deg":87.4,"confidence":0.83,"sigma_deg":4.6,"lat":52.5,"lon":13.4,"station_id":"roof","node":"df","others":[{"bearing_deg":212.0,"sigma_deg":9.0,"confidence":0.3}]}"#,
    ),
    (
        EventKind::DfFix,
        r#"{"lat":52.51,"lon":13.38,"ellipse_major_m":120.0,"ellipse_minor_m":40.0,"ellipse_bearing_deg":30.0,"converged":true,"samples":12}"#,
    ),
    (
        EventKind::Broadcast,
        r#"{"system":"dab_plus","locked":true,"snr_db":18.5,"frequency_error_hz":12.0,"label":"Radio Eins","dynamic_label":"Now playing"}"#,
    ),
    (
        EventKind::RadarSites,
        r#"{"geometry":{"receiver":{"lat":52.5,"lon":13.4,"altitude_m":40.0},"transmitter":{"lat":52.52,"lon":13.41,"altitude_m":360.0},"baseline_km":2.3},"ranges_km":[12.5]}"#,
    ),
    (
        EventKind::Nav,
        r#"{"target":{"lat":52.51,"lon":13.38,"kind":"estimate","revision":3,"distance_m":850.0,"bearing_deg":42.0},"from":{"lat":52.5,"lon":13.37}}"#,
    ),
    (
        EventKind::Satellite,
        r#"{"name":"ISS (ZARYA)","catalog":"25544","ground":{"at":{"lat":51.0,"lon":10.0},"altitude_km":420.0,"footprint_km":2200.0},"ground_track":[{"lat":50.0,"lon":9.0},{"lat":51.0,"lon":10.0}],"visible":true}"#,
    ),
    (
        EventKind::Radar,
        r#"{"track_id":7,"change":"confirmed","range_km":32.5,"range_rate_mps":-365.0,"doppler_hz":120.0,"snr_db":14.0,"lat":52.4,"lon":13.2,"icao":"3C6444"}"#,
    ),
    (
        EventKind::Dect,
        r#"{"side":"rfp","update":"identity","identity":{"rfpi":"0012345678","pari":"0012345","arc":"a","sari_available":false,"rpn":0},"security":{"cipher_state":"clear"},"bursts":10,"level_dbfs":-40.0}"#,
    ),
    (
        EventKind::Gsm,
        r#"{"bsic":10,"frame_number":1234,"timeslot":0,"channel":"bcch","cgi":"262-01-1234-5678","message":{"type":"other","message_type":32},"snr_db":15.0,"frequency_error_hz":10.0,"raw":"00"}"#,
    ),
    (
        EventKind::Apt,
        r#"{"seq":1,"lines":1200,"complete":true,"duration_ms":600000}"#,
    ),
    (
        EventKind::Lrpt,
        r#"{"seq":1,"mode":"qpsk72","width":1568,"lines":800,"complete":true,"duration_ms":600000,"apids":[64,65,66],"frames":100,"frames_corrected":2,"frames_failed":0,"packets_lost":0}"#,
    ),
    (
        EventKind::Wefax,
        r#"{"seq":1,"ioc":"ioc576","lpm":"lpm120","width":1809,"lines":1200,"complete":true,"duration_ms":600000}"#,
    ),
    (
        EventKind::Same,
        r#"{"body":{"header":"alert","originator":"WXR","originator_name":"National Weather Service","event":"TOR","event_name":"Tornado Warning","locations":[{"part":0,"state":48,"county":201,"name":"Harris, TX"}],"valid_minutes":30,"issued_day":123,"issued_hour":22,"issued_minute":0,"station":"KEC55/NW"},"copies":3,"raw":"ZCZC-WXR-TOR-048201+0030-1232200-KEC55/NW-"}"#,
    ),
    (
        EventKind::Radiosonde,
        r#"{"sonde":"rs41","serial":"S1234567","lat":52.2,"lon":14.1,"altitude_m":12000.0,"heading_deg":45.0,"errors_corrected":0,"forecast":{"burst":{"lat":52.4,"lon":14.3},"burst_altitude_m":31000.0,"burst_seen":false,"landing":{"lat":52.3,"lon":14.6},"landing_in_s":5400.0,"path":[{"lat":52.2,"lon":14.1},{"lat":52.4,"lon":14.3},{"lat":52.3,"lon":14.6}]}}"#,
    ),
    (
        EventKind::Lora,
        r#"{"spreading_factor":11,"bandwidth_hz":250000.0,"coding_rate":"4/5","sync_word":43,"implicit_header":false,"low_data_rate":false,"inverted_iq":false,"integrity":"crc_ok","fec_corrected":0,"snr_db":1.0,"frequency_error_hz":0.0,"payload":"00","decoded":{"protocol":"meshtastic","to":4294967295,"from":161,"id":1,"hop_limit":3,"hop_start":3,"want_ack":false,"via_mqtt":false,"channel_hash":8,"next_hop":0,"relay_node":0,"encryption":"channel","content":{"type":"position","lat":47.5,"lon":8.25}}}"#,
    ),
    (
        EventKind::RemoteId,
        r#"{"transport":"bluetooth_legacy","phy":"le1m","address":"D4:5A:21:0C:7E:19","uas_id":"1581F5FJD239C00DW22E","level_dbfs":-42.0,"messages":[{"type":"location","status":"airborne","lat":52.5163,"lon":13.3777,"height_m":84.0,"height_reference":"takeoff","track_deg":74.0}]}"#,
    ),
    (
        EventKind::Ble,
        r#"{"pdu":"adv_ind","phy":"le1m","address":{"address":"C0:FF:EE:00:00:01","kind":"public"},"level_dbfs":-50.0,"data":"00"}"#,
    ),
    (
        EventKind::WifiOccupancy,
        r#"{"window_ms":1000,"measured":1.0,"channels":[]}"#,
    ),
    (
        EventKind::IsmSurvey,
        r#"{"window_ms":1000,"low_hz":2400000000.0,"high_hz":2483500000.0,"measured":1.0,"busy":0.4,"kinds":[]}"#,
    ),
    (
        EventKind::Ism,
        r#"{"model":"Acurite-Tower","id":"1234","modulation":"ook","coding":"pwm","integrity":"crc","bits":56,"raw":"00","snr_db":15.0}"#,
    ),
    (
        EventKind::Wmbus,
        r#"{"mode":"t","format":"a","control":68,"manufacturer":"BMT","id":"18162333","version":19,"device_type":7,"frame":"4e44","snr_db":15.0}"#,
    ),
    (
        EventKind::NoiseFigure,
        r#"{"window_ms":1000,"level_dbfs":-60.0,"settled_s":2.0}"#,
    ),
    (
        EventKind::Per,
        r#"{"received":990,"errored":10,"missed":0,"expected":1000,"per":0.01,"bits":8000,"bit_errors":2}"#,
    ),
    (
        EventKind::Sid,
        r#"{"window_ms":1000,"floor_dbfs_hz":-140.0,"stations":[]}"#,
    ),
    (
        EventKind::Stanag,
        r#"{"confidence":0.9,"audio_hz":1800.0,"bandwidth_hz":3000.0}"#,
    ),
];

fn fixture_json(kind: EventKind) -> Option<&'static str> {
    FIXTURES
        .iter()
        .find(|(fixture, _)| *fixture == kind)
        .map(|(_, data)| *data)
}

pub(crate) fn fixture(kind: EventKind) -> DecoderEvent {
    let data: serde_json::Value = fixture_json(kind)
        .and_then(|data| serde_json::from_str(data).ok())
        .unwrap_or_else(|| panic!("no fixture for {kind}"));
    serde_json::from_value(json!({ "kind": kind.as_str(), "data": data }))
        .unwrap_or_else(|error| panic!("{kind} fixture does not parse: {error}"))
}

pub(crate) fn fixture_count() -> usize {
    FIXTURES.len()
}
