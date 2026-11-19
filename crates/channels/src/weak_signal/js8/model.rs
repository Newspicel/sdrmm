use serde::{Deserialize, Serialize};

use sdrmm_wire::geo::grid_lat_lon;
use sdrmm_wire::present::{
    EventPayload, Field, Presentation, Spot, Unit, count, fields, number, text,
};
use sdrmm_wire::{Capability, EventContract, EventKind, Hue, KindSpec};

/// One JS8 frame as `JS8Call` shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Js8Message {
    /// What the frame says, or its raw characters when that needs `JS8Call`'s word dictionary.
    pub text: String,
    /// The twelve characters the frame's 72 bits spell.
    pub frame: String,
    pub frame_type: String,
    pub first: bool,
    pub last: bool,
    pub speed: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callsign: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<String>,
    pub snr_db: f32,
    pub audio_hz: f32,
    pub time_offset_s: f32,
    pub hard_errors: u32,
    /// UTC start of the period the frame was sent in, when the receiver's clock is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<String>,
}

impl EventContract for Js8Message {
    const KIND: EventKind = EventKind::named("js8");
}

impl EventPayload for Js8Message {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            entity: self.callsign.clone(),
            callsign: self.callsign.clone(),
            spot: self.spot(),
            fields: self.fields(),
            ..Presentation::of(format!(
                "{} · {:+.0} dB · {:.0} Hz",
                self.text, self.snr_db, self.audio_hz
            ))
        }
    }
}

impl Js8Message {
    fn spot(&self) -> Option<Spot> {
        let grid = self.grid.as_deref()?;
        Some(Spot {
            at: grid_lat_lon(grid)?,
            grid: grid.to_owned(),
            callsign: self.callsign.clone().unwrap_or_default(),
            snr_db: self.snr_db,
        })
    }

    fn fields(&self) -> Vec<Field> {
        fields(&[
            ("snr", "SNR", number(Some(self.snr_db), Unit::Db, 0)),
            ("audio", "Audio", number(Some(self.audio_hz), Unit::Hz, 1)),
            (
                "time_offset",
                "Time offset",
                number(Some(self.time_offset_s), Unit::Seconds, 2),
            ),
            ("speed", "Speed", text(Some(&self.speed))),
            ("frame_type", "Frame", text(Some(&self.frame_type))),
            ("to", "To", text(self.to.as_deref())),
            ("hard_errors", "Hard errors", count(Some(self.hard_errors))),
            (
                "slot",
                "Slot",
                text(self.slot.as_deref().and_then(|s| s.get(11..19))),
            ),
            ("raw", "Raw", text(Some(&self.frame))),
            ("message", "Message", text(Some(&self.text))),
        ])
    }
}

pub(crate) fn js8_kind() -> KindSpec {
    KindSpec::of::<Js8Message>("JS8", "Stations", Hue::Muted)
        .caps(&[Capability::Log, Capability::Station, Capability::Spot])
        .sighting()
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdrmm_wire::DecoderEvent;

    fn heartbeat() -> Js8Message {
        Js8Message {
            text: "KN4CRD: @HB HEARTBEAT EM73".to_owned(),
            frame: "8nYAa3LKsg0C".to_owned(),
            frame_type: "Heartbeat".to_owned(),
            first: true,
            last: true,
            speed: "Normal".to_owned(),
            callsign: Some("KN4CRD".to_owned()),
            to: None,
            grid: Some("EM73".to_owned()),
            snr_db: -12.0,
            audio_hz: 1_500.0,
            time_offset_s: 0.2,
            hard_errors: 3,
            slot: None,
        }
    }

    #[test]
    fn a_heartbeat_is_a_spotted_station() {
        let spec = js8_kind();
        let event = DecoderEvent::of(&heartbeat());
        assert_eq!(event.kind, spec.entry.kind);
        let view = spec
            .codec
            .present(&spec.entry.kind, &event.data())
            .expect("reads");
        assert_eq!(view.callsign.as_deref(), Some("KN4CRD"));
        assert!(view.spot.is_some());
        assert!(spec.entry.has(Capability::Spot) && spec.entry.has(Capability::Station));
        assert_eq!(view.field("Raw"), text(Some("8nYAa3LKsg0C")).as_ref());
    }
}
