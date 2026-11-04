use sdrmm_wire::{Capability, EventContract, Hue, KindSpec};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Field;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::count;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::flag;
use sdrmm_wire::present::number;
use sdrmm_wire::present::repaired;
use sdrmm_wire::present::text_of;

fn default_eot_bandwidth_hz() -> f64 {
    12_500.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EotParams {
    #[serde(default = "default_eot_bandwidth_hz")]
    pub bandwidth_hz: f64,
}

impl Default for EotParams {
    fn default() -> Self {
        Self {
            bandwidth_hz: default_eot_bandwidth_hz(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EotBattery {
    NotMonitored,
    VeryLow,
    Low,
    Ok,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EotArming {
    Normal,
    Arming,
    Armed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HotCommand {
    StatusRequest,
    Emergency,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EotStatus {
    pub message_type: u8,
    pub arming: EotArming,
    pub pressure_psig: u8,
    pub battery: EotBattery,
    pub battery_charge_pct: u8,
    pub valve_ok: bool,
    pub confirmed: bool,
    pub turbine: bool,
    pub motion: bool,
    pub marker_light: bool,
    pub marker_battery_low: bool,
    pub discretionary: bool,
    pub chaining: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotRequest {
    pub command: HotCommand,
    pub code: u8,
    pub copies: u8,
}

impl HotRequest {
    #[must_use]
    pub fn command_label(&self) -> String {
        match self.command {
            HotCommand::StatusRequest => "status request".to_owned(),
            HotCommand::Emergency => "EMERGENCY".to_owned(),
            HotCommand::Other => format!("command {:#04x}", self.code),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "unit", rename_all = "snake_case")]
pub enum EotReport {
    Rear(EotStatus),
    Head(HotRequest),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EotMessage {
    pub unit_address: u32,
    pub report: EotReport,
    pub errors_corrected: u32,
}

impl EotMessage {
    #[must_use]
    pub fn summary(&self) -> String {
        match &self.report {
            EotReport::Rear(s) => {
                let mut parts = vec![
                    format!("EOT {}", self.unit_address),
                    format!("{} psig", s.pressure_psig),
                    if s.motion { "moving" } else { "stopped" }.to_owned(),
                ];
                if s.arming != EotArming::Normal {
                    parts.push(format!("{:?}", s.arming).to_lowercase());
                }
                parts.join(" · ")
            }
            EotReport::Head(h) => format!("HOT {} · {}", self.unit_address, h.command_label()),
        }
    }
}

impl EventPayload for EotMessage {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            entity: Some(self.unit_address.to_string()),
            fields: eot_fields(self),
            ..Presentation::of(self.summary())
        }
    }
}

fn eot_fields(m: &EotMessage) -> Vec<Field> {
    let mut shown = match &m.report {
        EotReport::Head(request) => hot_fields(m, request),
        EotReport::Rear(status) => rear_fields(m, status),
    };
    shown.extend(fields(&[("Repaired", repaired(m.errors_corrected))]));
    shown
}

fn hot_fields(m: &EotMessage, request: &HotRequest) -> Vec<Field> {
    fields(&[
        ("Unit", text_of("Head (HOT)")),
        ("Address", text_of(m.unit_address)),
        ("Command", text_of(request.command_label())),
        ("Copies", text_of(format!("{}/3", request.copies))),
    ])
}

fn rear_fields(m: &EotMessage, s: &EotStatus) -> Vec<Field> {
    fields(&[
        ("Unit", text_of("Rear (EOT)")),
        ("Address", text_of(m.unit_address)),
        ("Brake pipe", number(Some(s.pressure_psig), Unit::Psi, 0)),
        ("Motion", flag(Some(s.motion))),
        ("Marker light", flag(Some(s.marker_light))),
        (
            "Marker battery",
            text_of(if s.marker_battery_low { "low" } else { "ok" }),
        ),
        ("Battery", text_of(eot_battery(s.battery))),
        (
            "Charge",
            number(Some(s.battery_charge_pct), Unit::Percent, 0),
        ),
        (
            "Valve circuit",
            text_of(if s.valve_ok { "ok" } else { "fail" }),
        ),
        ("Turbine", flag(Some(s.turbine))),
        ("Arming", text_of(eot_arming(s.arming))),
        ("Message type", count(Some(s.message_type))),
    ])
}

fn eot_battery(battery: EotBattery) -> &'static str {
    match battery {
        EotBattery::NotMonitored => "not monitored",
        EotBattery::VeryLow => "very low",
        EotBattery::Low => "low",
        EotBattery::Ok => "ok",
    }
}

fn eot_arming(arming: EotArming) -> &'static str {
    match arming {
        EotArming::Normal => "normal",
        EotArming::Arming => "arming",
        EotArming::Armed => "armed",
    }
}

pub(crate) fn eot_kind() -> KindSpec {
    KindSpec::of::<EotMessage>("End-of-Train", "Trains", Hue::Muted)
        .caps(&[Capability::Log, Capability::Station])
        .sighting()
        .build()
}

impl EventContract for EotMessage {
    const KIND: EventKind = EventKind::named("eot");
}

impl sdrmm_wire::DecoderSettings for EotParams {
    const TYPE_ID: &'static str = "eot";
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use crate::acars::AcarsMessage;
    use crate::adsb::AdsbMessage;
    use crate::ais::AisMessage;
    use crate::aprs::AprsPacket;
    use crate::cw_skimmer::CwSkimmerSpot;
    use crate::datalink::DataLinkMessage;
    use crate::ermes::ErmesMessage;
    use crate::flex::FlexMessage;
    use crate::flex::PagerPayload;
    use crate::gnss::GnssFrame;
    use sdrmm_wire::present::text;

    use crate::ils::IlsReading;

    use crate::morse::MorseText;
    use crate::navtex::NavtexMessage;
    use crate::nfm::ScramblerStatus;
    use crate::nfm::ToneSquelchStatus;
    use crate::pocsag::PocsagMessage;
    use crate::pocsag::PocsagPayload;
    use crate::psk::PskBaud;
    use crate::psk::PskText;
    use crate::radio_clock::RadioClockFrame;
    use crate::radio_clock::RadioClockStandard;
    use crate::rds::RdsUpdate;
    use crate::rtty::RttyText;

    use crate::selcall::SelcallSequence;

    use crate::vor::VorReading;
    use crate::weak_signal::WsjtMessage;
    use crate::weak_signal::WsprSpot;

    use sdrmm_wire::BroadcastData;
    use sdrmm_wire::DecoderEvent;

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
            area: None,
            details: serde_json::Value::Null,
        };
        for ev in [
            DecoderEvent::of(&RdsUpdate::default()),
            DecoderEvent::of(&PocsagMessage {
                address: 1,
                function: 0,
                baud: 512,
                payload: PocsagPayload::Tone,
                text: String::new(),
                errors_corrected: 0,
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
        ] {
            let json = serde_json::to_value(&ev).unwrap();
            assert_eq!(json["kind"], ev.kind().as_str());
        }
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

    fn eot(json: serde_json::Value) -> Presentation {
        serde_json::from_value::<EotMessage>(json)
            .unwrap()
            .present(&EventKind::named("eot"))
    }

    #[test]
    fn an_eot_rear_unit_reports_its_brake_pipe_and_battery() {
        let view = eot(serde_json::json!({
            "unit_address": 23456, "errors_corrected": 1,
            "report": {
                "unit": "rear", "message_type": 0, "arming": "normal", "pressure_psig": 87,
                "battery": "low", "battery_charge_pct": 63, "valve_ok": true, "confirmed": false,
                "turbine": true, "motion": true, "marker_light": true,
                "marker_battery_low": false, "discretionary": false, "chaining": 3,
            },
        }));
        assert_eq!(
            view.field("Brake pipe"),
            number(Some(87), Unit::Psi, 0).as_ref()
        );
        assert_eq!(view.field("Battery"), text(Some("low")).as_ref());
        assert_eq!(view.field("Repaired"), count(Some(1)).as_ref());
    }

    #[test]
    fn a_hot_command_names_its_code_and_copies() {
        let view = eot(serde_json::json!({
            "unit_address": 23456, "errors_corrected": 0,
            "report": { "unit": "head", "command": "other", "code": 60, "copies": 2 },
        }));
        assert_eq!(view.field("Command"), text(Some("command 0x3c")).as_ref());
        assert_eq!(view.field("Copies"), text(Some("2/3")).as_ref());
        assert_eq!(view.field("Repaired"), None);
    }
}
