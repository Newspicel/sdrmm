//! Every built-in event payload serializes under the kind it is registered as.

use crate::acars::AcarsMessage;
use crate::adsb::AdsbMessage;
use crate::ais::AisMessage;
use crate::aprs::AprsPacket;
use crate::cw_skimmer::CwSkimmerSpot;
use crate::ermes::ErmesMessage;
use crate::flex::FlexMessage;
use crate::gnss::GnssFrame;
use sdrmm_decoder::link::DataLinkMessage;
use sdrmm_wire::PagerPayload;

use crate::ils::IlsReading;

use crate::morse::MorseText;
use crate::navtex::NavtexMessage;
use crate::nfm::ScramblerStatus;
use crate::pocsag::PocsagMessage;
use crate::pocsag::PocsagPayload;
use crate::psk::PskBaud;
use crate::psk::PskText;
use crate::radio_clock::RadioClockFrame;
use crate::radio_clock::RadioClockStandard;
use crate::rtty::RttyText;
use crate::wfm::rds::RdsUpdate;
use sdrmm_wire::ToneSquelchStatus;

use crate::selcall::SelcallSequence;

use crate::vor::VorReading;
use crate::weak_signal::WsjtMessage;
use crate::weak_signal::WsprSpot;

use crate::eot::{EotMessage, EotReport, HotCommand, HotRequest};
use sdrmm_wire::BroadcastData;
use sdrmm_wire::DecoderEvent;
use sdrmm_wire::EventKind;

#[test]
fn kind_matches_the_serialized_tag() {
    for ev in every_event_kind() {
        let json = serde_json::to_value(&ev).unwrap();
        assert_eq!(json["kind"], ev.kind().as_str());
    }
}

fn link() -> DataLinkMessage {
    DataLinkMessage {
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
        area: None,
        details: serde_json::Value::Null,
    }
}

fn every_event_kind() -> Vec<DecoderEvent> {
    vec![
        DecoderEvent::of(&RdsUpdate::default()),
        DecoderEvent::of(&PocsagMessage {
            address: 1,
            function: 0,
            baud: 512,
            payload: PocsagPayload::Tone,
            text: String::new(),
            errors_corrected: 0,
            encrypted: false,
        }),
        DecoderEvent::of(&FlexMessage {
            address: 1,
            payload: PagerPayload::Tone,
            text: String::new(),
            baud: 1_600,
            levels: 2,
            cycle: 0,
            frame: 0,
            phase: 'A',
            errors_corrected: 0,
            complete: true,
        }),
        DecoderEvent::of(&ErmesMessage {
            local_address: 1,
            message_number: 0,
            payload: PagerPayload::Tone,
            text: String::new(),
            urgent: false,
            alert: 0,
            errors_corrected: 0,
        }),
        DecoderEvent::of(&EotMessage {
            unit_address: 1,
            report: EotReport::Head(HotRequest {
                command: HotCommand::Emergency,
                code: 0xAA,
                copies: 3,
            }),
            errors_corrected: 0,
        }),
        DecoderEvent::of(&crate::sarsat::SarsatMessage {
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
        DecoderEvent::of(&crate::tetrapol::TetrapolMessage {
            channel: crate::tetrapol::TetrapolLogicalChannel::Traffic,
            frame: None,
            scrambling: 0,
            address: None,
            payload: crate::tetrapol::TetrapolPayload::GroupIdle { cause: 0 },
            data: String::new(),
            rejected: 0,
        }),
        DecoderEvent::of(&AdsbMessage::default()),
        DecoderEvent::of(&AisMessage::default()),
        DecoderEvent::of(&AprsPacket::default()),
        DecoderEvent::of(&crate::bss::BssPacket::default()),
        DecoderEvent::of(&RttyText {
            text: String::new(),
        }),
        DecoderEvent::of(&MorseText {
            text: String::new(),
            wpm: 0.0,
        }),
        DecoderEvent::of(&CwSkimmerSpot {
            offset_hz: 0.0,
            text: String::new(),
            wpm: 0.0,
            snr_db: 0.0,
        }),
        DecoderEvent::of(&SelcallSequence {
            system: crate::selcall::SelcallSystem::Ccir1,
            code: "12345".to_owned(),
            tone_ms: 100,
        }),
        DecoderEvent::of(&crate::dtmf::DtmfSequence {
            digits: "123".to_owned(),
            tone_ms: 40,
            gap_ms: Some(40),
        }),
        DecoderEvent::of(&NavtexMessage::default()),
        DecoderEvent::of(&AcarsMessage::default()),
        DecoderEvent::of(&ToneSquelchStatus::default()),
        DecoderEvent::of(&ScramblerStatus::default()),
        DecoderEvent::tagged(
            EventKind::named("ft8"),
            &WsjtMessage {
                text: String::new(),
                snr_db: 0.0,
                audio_hz: 0.0,
                time_offset_s: 0.0,
                hard_errors: 0,
                slot: None,
            },
        ),
        DecoderEvent::tagged(
            EventKind::named("ft4"),
            &WsjtMessage {
                text: String::new(),
                snr_db: 0.0,
                audio_hz: 0.0,
                time_offset_s: 0.0,
                hard_errors: 0,
                slot: None,
            },
        ),
        DecoderEvent::of(&PskText {
            baud: PskBaud::Psk31,
            text: String::new(),
        }),
        DecoderEvent::of(&WsprSpot {
            text: String::new(),
            callsign: String::new(),
            grid: None,
            power_dbm: 0,
            snr_db: 0.0,
            audio_hz: 0.0,
            time_offset_s: 0.0,
            drift_hz: 0.0,
            slot: None,
        }),
        DecoderEvent::of(&BroadcastData {
            protocol: None,
            label: Vec::new(),
            service_id: Some(1),
            name: "slide.png".to_owned(),
            media_type: "image/png".to_owned(),
            bytes: vec![137, 80, 78, 71],
        }),
        DecoderEvent::of(&RadioClockFrame {
            standard: RadioClockStandard::Dcf77,
            datetime: "2026-08-15T12:34:00+02:00".to_owned(),
            utc_offset_minutes: Some(120),
            dst: true,
            leap_warning: false,
            dut1_seconds: None,
            symbols: String::new(),
        }),
        DecoderEvent::of(&GnssFrame {
            prn: 1,
            doppler_hz: 0.0,
            code_phase_chips: 0.0,
            cn0_db_hz: 40.0,
            subframe: None,
            tow_seconds: None,
            week: None,
            words: Vec::new(),
        }),
        DecoderEvent::of(&VorReading {
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
        DecoderEvent::of(&IlsReading {
            component: crate::ils::IlsComponent::Localizer,
            modulation_90: 0.0,
            modulation_150: 0.0,
            ddm: 0.0,
            deviation_dots: 0.0,
            signal_db: 0.0,
        }),
        DecoderEvent::tagged(EventKind::named("dsc"), &link()),
        DecoderEvent::tagged(EventKind::named("inmarsat_stdc"), &link()),
        DecoderEvent::tagged(EventKind::named("inmarsat_aero"), &link()),
        DecoderEvent::tagged(EventKind::named("vdl2"), &link()),
        DecoderEvent::tagged(EventKind::named("hfdl"), &link()),
        DecoderEvent::tagged(EventKind::named("iridium"), &link()),
        DecoderEvent::of(&radar_event()),
        DecoderEvent::of(&crate::ism::IsmMessage::default()),
        DecoderEvent::of(&crate::wmbus::WmbusMessage::default()),
    ]
}

fn radar_event() -> sdrmm_wire::RadarTrackEvent {
    sdrmm_wire::RadarTrackEvent {
        track_id: 7,
        change: sdrmm_wire::TrackChange::Confirmed,
        range_km: 32.46,
        range_rate_mps: -365.2,
        doppler_hz: 120.0,
        snr_db: 17.0,
        bearing_deg: Some(98.0),
        lat: None,
        lon: None,
        target: Some("3C6444".to_owned()),
    }
}
