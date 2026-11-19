use serde::{Deserialize, Serialize};

use sdrmm_wire::geo::grid_lat_lon;
use sdrmm_wire::present::{
    EventPayload, Field, Presentation, Spot, Unit, count, fields, flag, number, text, trimmed,
};
use sdrmm_wire::{Capability, EventContract, EventKind, Hue, KindSpec};

use crate::weak_signal::WsjtMessage;

/// An FST4 message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fst4Message {
    pub text: String,
    pub snr_db: f32,
    pub audio_hz: f32,
    pub time_offset_s: f32,
    pub hard_errors: u32,
    /// Read with the first word taken as CQ, as WSJT-X's first a-priori pass.
    #[serde(default)]
    pub cq_assumed: bool,
    /// UTC start of the period the message was sent in, when the receiver's clock is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<String>,
}

/// An FST4W beacon.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fst4wSpot {
    pub text: String,
    pub callsign: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_dbm: Option<i32>,
    pub snr_db: f32,
    pub audio_hz: f32,
    pub time_offset_s: f32,
    pub hard_errors: u32,
    /// UTC start of the period the beacon was sent in, when the receiver's clock is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<String>,
}

impl EventContract for Fst4Message {
    const KIND: EventKind = EventKind::named("fst4");
}

impl EventContract for Fst4wSpot {
    const KIND: EventKind = EventKind::named("fst4w");
}

impl EventPayload for Fst4Message {
    fn present(&self, kind: &EventKind) -> Presentation {
        let message = WsjtMessage {
            text: self.text.clone(),
            snr_db: self.snr_db,
            audio_hz: self.audio_hz,
            time_offset_s: self.time_offset_s,
            hard_errors: self.hard_errors,
            slot: self.slot.clone(),
        };
        let mut view = message.present(kind);
        let at = view.fields.len().saturating_sub(1);
        view.fields.splice(
            at..at,
            fields(&[("cq_assumed", "CQ assumed", flag(Some(self.cq_assumed)))]),
        );
        view
    }
}

impl EventPayload for Fst4wSpot {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let callsign = trimmed(Some(&self.callsign));
        Presentation {
            entity: callsign.clone(),
            spot: self.spot(callsign.clone().unwrap_or_default()),
            callsign,
            fields: self.fields(),
            ..Presentation::of(format!(
                "{} · {:+.0} dB · {:.0} Hz",
                self.text, self.snr_db, self.audio_hz
            ))
        }
    }
}

impl Fst4wSpot {
    fn spot(&self, callsign: String) -> Option<Spot> {
        let grid = self.grid.as_deref()?.trim().to_ascii_uppercase();
        Some(Spot {
            at: grid_lat_lon(&grid)?,
            grid,
            callsign,
            snr_db: self.snr_db,
        })
    }

    fn fields(&self) -> Vec<Field> {
        fields(&[
            ("callsign", "Callsign", text(Some(&self.callsign))),
            ("grid", "Grid", text(self.grid.as_deref())),
            ("power", "Power", number(self.power_dbm, Unit::Dbm, 0)),
            ("snr", "SNR", number(Some(self.snr_db), Unit::Db, 0)),
            ("audio", "Audio", number(Some(self.audio_hz), Unit::Hz, 1)),
            (
                "time_offset",
                "Time offset",
                number(Some(self.time_offset_s), Unit::Seconds, 2),
            ),
            ("hard_errors", "Hard errors", count(Some(self.hard_errors))),
            (
                "slot",
                "Slot",
                text(self.slot.as_deref().and_then(|slot| slot.get(11..19))),
            ),
            ("message", "Message", text(Some(&self.text))),
        ])
    }
}

pub(crate) fn fst4_kind() -> KindSpec {
    KindSpec::of::<Fst4Message>("FST4", "Stations", Hue::Muted)
        .caps(&[Capability::Log, Capability::Station, Capability::Spot])
        .sighting()
        .build()
}

pub(crate) fn fst4w_kind() -> KindSpec {
    KindSpec::of::<Fst4wSpot>("FST4W", "Stations", Hue::Muted)
        .caps(&[Capability::Log, Capability::Station, Capability::Spot])
        .sighting()
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdrmm_wire::DecoderEvent;
    use sdrmm_wire::present::FieldValue;

    #[test]
    fn a_message_read_with_cq_assumed_says_so() {
        let message = Fst4Message {
            text: "CQ N3HSK FM28".to_owned(),
            snr_db: -20.0,
            audio_hz: 1_135.0,
            time_offset_s: 0.3,
            hard_errors: 9,
            cq_assumed: true,
            slot: None,
        };
        let view = fst4_kind()
            .codec
            .present(&Fst4Message::KIND, &DecoderEvent::of(&message).data())
            .expect("payload reads");
        assert_eq!(view.callsign.as_deref(), Some("N3HSK"));
        assert!(view.spot.is_some());
        let assumed = view
            .fields
            .iter()
            .find(|field| field.key == "cq_assumed")
            .expect("a CQ assumed field");
        assert_eq!(assumed.value, FieldValue::Flag(true));
        assert_eq!(
            view.fields.last().map(|field| field.label.as_str()),
            Some("Message")
        );
    }

    #[test]
    fn a_beacon_names_its_station_and_where_it_is() {
        let spot = Fst4wSpot {
            text: "DL0HOT JO60 30".to_owned(),
            callsign: "DL0HOT".to_owned(),
            grid: Some("JO60".to_owned()),
            power_dbm: Some(30),
            snr_db: -44.0,
            audio_hz: 1_433.0,
            time_offset_s: 0.5,
            hard_errors: 30,
            slot: None,
        };
        let view = fst4w_kind()
            .codec
            .present(&Fst4wSpot::KIND, &DecoderEvent::of(&spot).data())
            .expect("payload reads");
        assert_eq!(view.entity.as_deref(), Some("DL0HOT"));
        assert_eq!(view.spot.map(|spot| spot.grid).as_deref(), Some("JO60"));
    }
}
