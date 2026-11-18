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
    shown.extend(fields(&[(
        "repaired",
        "Repaired",
        repaired(m.errors_corrected),
    )]));
    shown
}

fn hot_fields(m: &EotMessage, request: &HotRequest) -> Vec<Field> {
    fields(&[
        ("unit", "Unit", text_of("Head (HOT)")),
        ("address", "Address", text_of(m.unit_address)),
        ("command", "Command", text_of(request.command_label())),
        ("copies", "Copies", text_of(format!("{}/3", request.copies))),
    ])
}

fn rear_fields(m: &EotMessage, s: &EotStatus) -> Vec<Field> {
    fields(&[
        ("unit", "Unit", text_of("Rear (EOT)")),
        ("address", "Address", text_of(m.unit_address)),
        (
            "brake_pipe",
            "Brake pipe",
            number(Some(s.pressure_psig), Unit::Psi, 0),
        ),
        ("motion", "Motion", flag(Some(s.motion))),
        ("marker_light", "Marker light", flag(Some(s.marker_light))),
        (
            "marker_battery",
            "Marker battery",
            text_of(if s.marker_battery_low { "low" } else { "ok" }),
        ),
        ("battery", "Battery", text_of(eot_battery(s.battery))),
        (
            "charge",
            "Charge",
            number(Some(s.battery_charge_pct), Unit::Percent, 0),
        ),
        (
            "valve_circuit",
            "Valve circuit",
            text_of(if s.valve_ok { "ok" } else { "fail" }),
        ),
        ("turbine", "Turbine", flag(Some(s.turbine))),
        ("arming", "Arming", text_of(eot_arming(s.arming))),
        ("message_type", "Message type", count(Some(s.message_type))),
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
    use sdrmm_wire::present::text;

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
