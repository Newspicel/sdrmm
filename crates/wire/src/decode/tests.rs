use super::*;
use crate::{PskBaud, RadioClockStandard, SstvMode};

#[test]
fn an_sstv_picture_summarises_its_mode_size_and_outcome() {
    let complete = DecoderEvent::Sstv(SstvPicture {
        seq: 1,
        mode: SstvMode::MartinM1,
        width: 320,
        height: 256,
        lines: 256,
        complete: true,
        duration_ms: 114_300,
    });
    assert_eq!(complete.kind(), EventKind::Sstv);
    assert_eq!(
        complete.present().summary,
        "Martin M1 · 320×256 · complete in 114 s"
    );
    assert_eq!(complete.present().entity, None);

    let cut_short = DecoderEvent::Sstv(SstvPicture {
        seq: 2,
        mode: SstvMode::Robot36,
        width: 320,
        height: 240,
        lines: 96,
        complete: false,
        duration_ms: 14_500,
    });
    assert_eq!(
        cut_short.present().summary,
        "Robot 36 · 320×240 · 96 of 240 lines"
    );
}

#[test]
fn decoder_event_is_adjacently_tagged() {
    let ev = DecoderEvent::Pocsag(PocsagMessage {
        address: 1_234_567,
        function: 3,
        baud: 1_200,
        payload: PocsagPayload::Alpha,
        text: "TEST".to_owned(),
        errors_corrected: 1,
    });
    let json = serde_json::to_value(&ev).unwrap();
    assert_eq!(json["kind"], "pocsag");
    assert_eq!(json["data"]["address"], 1_234_567);
    assert_eq!(json["data"]["payload"], "alpha");
    assert_eq!(ev.kind(), EventKind::Pocsag);

    let back: DecoderEvent = serde_json::from_value(json).unwrap();
    assert_eq!(back, ev);
}

#[test]
fn eot_reports_carry_the_unit_as_a_tag() {
    let message = EotMessage {
        unit_address: 4_242,
        report: EotReport::Head(HotRequest {
            command: HotCommand::StatusRequest,
            code: 0x55,
            copies: 3,
        }),
        errors_corrected: 0,
    };
    let json = serde_json::to_value(&message).unwrap();
    assert_eq!(json["report"]["unit"], "head");
    assert_eq!(json["report"]["command"], "status_request");
    assert_eq!(message.summary(), "HOT 4242 · status request");
    let back: EotMessage = serde_json::from_value(json).unwrap();
    assert_eq!(back, message);
}

#[test]
fn kind_matches_the_serialized_tag() {
    let link = || DataLinkMessage {
        message_type: "test".to_owned(),
        station: None,
        text: None,
        crc_ok: true,
        fec_corrected: None,
        snr_db: None,
        frequency_error_hz: None,
        lat: None,
        lon: None,
        raw: None,
        details: serde_json::Value::Null,
    };
    for ev in [
        DecoderEvent::Rds(RdsUpdate::default()),
        DecoderEvent::Pocsag(PocsagMessage {
            address: 1,
            function: 0,
            baud: 512,
            payload: PocsagPayload::Tone,
            text: String::new(),
            errors_corrected: 0,
        }),
        DecoderEvent::Flex(FlexMessage {
            address: 1,
            payload: PagerPayload::Tone,
            text: String::new(),
            baud: 1_600,
            levels: 2,
            cycle: 0,
            frame: 0,
            phase: 'A',
            errors_corrected: 0,
        }),
        DecoderEvent::Ermes(ErmesMessage {
            local_address: 1,
            message_number: 0,
            payload: PagerPayload::Tone,
            text: String::new(),
            urgent: false,
            alert: 0,
            errors_corrected: 0,
        }),
        DecoderEvent::Eot(EotMessage {
            unit_address: 1,
            report: EotReport::Head(HotRequest {
                command: HotCommand::Emergency,
                code: 0xAA,
                copies: 3,
            }),
            errors_corrected: 0,
        }),
        DecoderEvent::Sarsat(crate::sarsat::SarsatMessage {
            hex_id: "ADCD00800440401".to_owned(),
            message: String::new(),
            long: false,
            self_test: false,
            country: 366,
            protocol: crate::sarsat::SarsatProtocol::SerialUser,
            beacon: crate::sarsat::SarsatBeacon::Epirb,
            identity: None,
            lat: None,
            lon: None,
            homing: None,
            errors_corrected: 0,
            pdf2_ok: None,
        }),
        DecoderEvent::Tetrapol(crate::tetrapol::TetrapolMessage {
            channel: crate::tetrapol::TetrapolLogicalChannel::Traffic,
            frame: None,
            scrambling: 0,
            address: None,
            payload: crate::tetrapol::TetrapolPayload::GroupIdle { cause: 0 },
            data: String::new(),
            rejected: 0,
        }),
        DecoderEvent::Adsb(AdsbMessage::default()),
        DecoderEvent::Ais(AisMessage::default()),
        DecoderEvent::Aprs(AprsPacket::default()),
        DecoderEvent::Rtty(RttyText {
            text: String::new(),
        }),
        DecoderEvent::Morse(MorseText {
            text: String::new(),
            wpm: 0.0,
        }),
        DecoderEvent::CwSkimmer(CwSkimmerSpot {
            offset_hz: 0.0,
            text: String::new(),
            wpm: 0.0,
            snr_db: 0.0,
        }),
        DecoderEvent::Selcall(SelcallSequence {
            system: crate::channel::SelcallSystem::Ccir1,
            code: "12345".to_owned(),
            tone_ms: 100,
        }),
        DecoderEvent::Navtex(NavtexMessage::default()),
        DecoderEvent::Acars(AcarsMessage::default()),
        DecoderEvent::Tone(ToneSquelchStatus::default()),
        DecoderEvent::Scrambler(ScramblerStatus::default()),
        DecoderEvent::Ft8(WsjtMessage {
            text: String::new(),
            snr_db: 0.0,
            audio_hz: 0.0,
            time_offset_s: 0.0,
            hard_errors: 0,
        }),
        DecoderEvent::Ft4(WsjtMessage {
            text: String::new(),
            snr_db: 0.0,
            audio_hz: 0.0,
            time_offset_s: 0.0,
            hard_errors: 0,
        }),
        DecoderEvent::Psk(PskText {
            baud: PskBaud::Psk31,
            text: String::new(),
        }),
        DecoderEvent::Wspr(WsprSpot {
            text: String::new(),
            callsign: String::new(),
            grid: None,
            power_dbm: 0,
            snr_db: 0.0,
            audio_hz: 0.0,
            time_offset_s: 0.0,
            drift_hz: 0.0,
        }),
        DecoderEvent::BroadcastData(BroadcastData {
            protocol: None,
            label: Vec::new(),
            service_id: Some(1),
            name: "slide.png".to_owned(),
            media_type: "image/png".to_owned(),
            bytes: vec![137, 80, 78, 71],
        }),
        DecoderEvent::RadioClock(RadioClockFrame {
            standard: RadioClockStandard::Dcf77,
            datetime: "2026-08-15T12:34:00+02:00".to_owned(),
            utc_offset_minutes: Some(120),
            dst: true,
            leap_warning: false,
            dut1_seconds: None,
            symbols: String::new(),
        }),
        DecoderEvent::Gnss(GnssFrame {
            prn: 1,
            doppler_hz: 0.0,
            code_phase_chips: 0.0,
            cn0_db_hz: 40.0,
            subframe: None,
            tow_seconds: None,
            week: None,
            words: Vec::new(),
        }),
        DecoderEvent::Vor(VorReading {
            station: None,
            station_lat: None,
            station_lon: None,
            magnetic_declination_deg: 0.0,
            radial_deg: 0.0,
            variable_phase_deg: 0.0,
            reference_phase_deg: 0.0,
            signal_db: 0.0,
            confidence: 1.0,
        }),
        DecoderEvent::Ils(IlsReading {
            component: crate::channel::IlsComponent::Localizer,
            modulation_90: 0.0,
            modulation_150: 0.0,
            ddm: 0.0,
            deviation_dots: 0.0,
            signal_db: 0.0,
        }),
        DecoderEvent::Dsc(link()),
        DecoderEvent::InmarsatStdc(link()),
        DecoderEvent::InmarsatAero(link()),
        DecoderEvent::Vdl2(link()),
        DecoderEvent::Hfdl(link()),
        DecoderEvent::Iridium(link()),
        DecoderEvent::Radar(radar_event()),
        DecoderEvent::Ism(crate::ism::IsmMessage::default()),
        DecoderEvent::Wmbus(crate::wmbus::WmbusMessage::default()),
    ] {
        let json = serde_json::to_value(&ev).unwrap();
        assert_eq!(json["kind"], ev.kind().as_str());
    }
}

fn radar_event() -> crate::radar::RadarTrackEvent {
    crate::radar::RadarTrackEvent {
        track_id: 7,
        change: crate::radar::TrackChange::Confirmed,
        range_km: 32.46,
        range_rate_mps: -365.2,
        doppler_hz: 120.0,
        snr_db: 17.0,
        bearing_deg: Some(98.0),
        lat: None,
        lon: None,
        icao: Some("3C6444".to_owned()),
    }
}

fn bearing() -> crate::fusion::DfBearing {
    serde_json::from_value(serde_json::json!({
        "bearing_deg": 87.4,
        "confidence": 0.83,
        "sigma_deg": 4.6,
        "lat": 52.5,
        "lon": 13.4,
        "node": "df"
    }))
    .expect("a bearing")
}

#[test]
fn a_df_bearing_states_its_spread_and_confidence() {
    let event = DecoderEvent::Df(bearing());
    assert_eq!(event.present().summary, "87° ±5° · 83%");
    assert_eq!(
        event
            .present()
            .place
            .map(|place| (place.at.lat, place.at.lon)),
        Some((52.5, 13.4))
    );
    assert_eq!(event.kind(), EventKind::Df);
}

#[test]
fn a_processor_record_names_no_channel_and_its_node() {
    let record = DecodedRecord {
        origin: Some(crate::EventOrigin {
            node: "df".to_owned(),
            transmission: 0,
        }),
        device_set: 3,
        channel: NO_CHANNEL,
        at: "2026-09-28T12:00:00Z".to_owned(),
        freq_hz: 433_920_000.0,
        event: DecoderEvent::Df(bearing()),
        sinks: Vec::new(),
        callsign: None,
        view: None,
    };
    let json = serde_json::to_value(&record).expect("record json");
    assert_eq!(json["channel"], u64::from(u32::MAX));
    assert_eq!(json["origin"]["node"], "df");
    let back: DecodedRecord = serde_json::from_value(json).expect("record back");
    assert_eq!(back, record);
}

#[test]
fn a_radar_track_event_names_its_track_and_place() {
    let event = DecoderEvent::Radar(radar_event());
    assert_eq!(event.present().summary, "T7 confirmed 32.5 km -365 m/s");
    assert_eq!(
        event
            .present()
            .place
            .map(|place| (place.at.lat, place.at.lon)),
        None
    );
    assert_eq!(event.present().entity.as_deref(), Some("T7"));
    assert_eq!(event.present().label.as_deref(), Some("3C6444"));
    let placed = DecoderEvent::Radar(crate::radar::RadarTrackEvent {
        change: crate::radar::TrackChange::Lost,
        range_rate_mps: 12.0,
        lat: Some(52.4),
        lon: Some(13.2),
        ..radar_event()
    });
    assert_eq!(placed.present().summary, "T7 lost 32.5 km +12 m/s");
    assert_eq!(
        placed
            .present()
            .place
            .map(|place| (place.at.lat, place.at.lon)),
        Some((52.4, 13.2))
    );
    let json = serde_json::to_value(&placed).unwrap();
    assert_eq!(json["kind"], "radar");
    assert_eq!(json["data"]["track_id"], 7);
    assert_eq!(
        serde_json::from_value::<DecoderEvent>(json).unwrap(),
        placed
    );
}

#[test]
fn navtex_subject_names_match_the_standard_table() {
    const NAMED: [(char, &str); 16] = [
        ('A', "Navigational warning"),
        ('B', "Meteorological warning"),
        ('C', "Ice report"),
        ('D', "Search and rescue / piracy"),
        ('E', "Meteorological forecast"),
        ('F', "Pilot service"),
        ('G', "AIS"),
        ('H', "LORAN"),
        ('J', "SATNAV"),
        ('K', "Other electronic navaid"),
        ('L', "Navigational warning (additional)"),
        ('T', "Test transmission"),
        ('V', "Notice to fishermen"),
        ('W', "Environmental"),
        ('X', "Special service"),
        ('Y', "Special service"),
    ];
    for (subject, name) in NAMED {
        assert_eq!(
            NavtexMessage::subject_name(subject),
            Some(name),
            "{subject}"
        );
    }
    assert_eq!(NavtexMessage::subject_name('Z'), Some("No message on hand"));
    for unassigned in ['I', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'U'] {
        assert_eq!(
            NavtexMessage::subject_name(unassigned),
            None,
            "{unassigned} is unassigned and must stay unnamed"
        );
    }
}

#[test]
fn navtex_header_needs_the_whole_group() {
    let mut msg = NavtexMessage {
        station: Some('D'),
        subject: Some('A'),
        serial: Some(7),
        ..NavtexMessage::default()
    };
    assert_eq!(msg.header().as_deref(), Some("DA07"));
    msg.serial = None;
    assert_eq!(msg.header(), None);
}

#[test]
fn clock_and_gnss_summaries_keep_the_measurements_used_by_the_live_log() {
    let clock = DecoderEvent::RadioClock(RadioClockFrame {
        standard: RadioClockStandard::Dcf77,
        datetime: "2026-08-15T12:34:00+02:00".to_owned(),
        utc_offset_minutes: Some(120),
        dst: true,
        leap_warning: false,
        dut1_seconds: None,
        symbols: String::new(),
    });
    assert_eq!(clock.present().summary, "DCF77 · 2026-08-15T12:34:00+02:00");
    assert_eq!(clock.present().entity.as_deref(), Some("DCF77"));

    let gnss = DecoderEvent::Gnss(GnssFrame {
        prn: 7,
        doppler_hz: 1_000.0,
        code_phase_chips: 158.34,
        cn0_db_hz: 44.5,
        subframe: None,
        tow_seconds: None,
        week: None,
        words: Vec::new(),
    });
    assert_eq!(
        gnss.present().summary,
        "GPS PRN 7 · +1000 Hz · 44.5 dB-Hz · acquired"
    );
    assert_eq!(gnss.present().entity.as_deref(), Some("GPS-7"));
}

#[test]
fn a_mic_e_summary_names_the_message_beside_the_monitor_line() {
    let mut packet = AprsPacket {
        tnc2: "DL1ABC-9>S32U6T:`(_fn\"Oj/".to_owned(),
        ..AprsPacket::default()
    };
    assert_eq!(
        DecoderEvent::Aprs(packet.clone()).present().summary,
        "DL1ABC-9>S32U6T:`(_fn\"Oj/"
    );
    packet.mic_e_message = Some("Returning".to_owned());
    assert_eq!(
        DecoderEvent::Aprs(packet).present().summary,
        "DL1ABC-9>S32U6T:`(_fn\"Oj/ · Returning"
    );
}

#[test]
fn position_is_reported_only_when_both_coordinates_are_known() {
    let mut a = AdsbMessage {
        icao: "3C6444".to_owned(),
        lat: Some(52.5),
        ..AdsbMessage::default()
    };
    assert_eq!(
        DecoderEvent::Adsb(a.clone())
            .present()
            .place
            .map(|place| (place.at.lat, place.at.lon)),
        None
    );
    a.lon = Some(13.4);
    assert_eq!(
        DecoderEvent::Adsb(a)
            .present()
            .place
            .map(|place| (place.at.lat, place.at.lon)),
        Some((52.5, 13.4))
    );
    assert_eq!(
        DecoderEvent::Rtty(RttyText {
            text: "CQ".to_owned()
        })
        .present()
        .place,
        None
    );
}

#[test]
fn decoded_record_roundtrips() {
    let rec = DecodedRecord {
        origin: None,
        sinks: Vec::new(),
        callsign: None,
        device_set: 1,
        channel: 2,
        at: "2026-08-09T12:00:00Z".to_owned(),
        freq_hz: 1_090_000_000.0,
        event: DecoderEvent::Adsb(AdsbMessage {
            icao: "3C6444".to_owned(),
            df: 17,
            type_code: Some(4),
            callsign: Some("DLH123".to_owned()),
            raw: "8D3C6444".to_owned(),
            ..AdsbMessage::default()
        }),
        view: None,
    };
    let json = serde_json::to_value(&rec).unwrap();
    assert_eq!(json["event"]["kind"], "adsb");
    assert_eq!(json["freq_hz"], 1_090_000_000.0);
    let back: DecodedRecord = serde_json::from_value(json).unwrap();
    assert_eq!(back, rec);
}
