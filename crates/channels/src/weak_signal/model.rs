use sdrmm_wire::{Capability, DecoderSettings, EventContract, Hue, KindSpec};
use serde::{Deserialize, Serialize};

use sdrmm_wire::EventKind;

use sdrmm_wire::geo::grid_lat_lon;
use sdrmm_wire::present::EventPayload;
use sdrmm_wire::present::Field;
use sdrmm_wire::present::Presentation;
use sdrmm_wire::present::Spot;
use sdrmm_wire::present::Unit;
use sdrmm_wire::present::count;
use sdrmm_wire::present::fields;
use sdrmm_wire::present::number;
use sdrmm_wire::present::text;
use sdrmm_wire::present::trimmed;

fn default_wsjt_low_hz() -> f32 {
    200.0
}

fn default_wsjt_high_hz() -> f32 {
    3_000.0
}

fn default_wsjt_candidates() -> u16 {
    200
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WsjtParams {
    #[serde(default = "default_wsjt_low_hz")]
    pub audio_low_hz: f32,
    #[serde(default = "default_wsjt_high_hz")]
    pub audio_high_hz: f32,
    #[serde(default = "default_wsjt_candidates")]
    pub max_candidates: u16,
}

impl Default for WsjtParams {
    fn default() -> Self {
        Self {
            audio_low_hz: default_wsjt_low_hz(),
            audio_high_hz: default_wsjt_high_hz(),
            max_candidates: default_wsjt_candidates(),
        }
    }
}

fn default_wspr_low_hz() -> f32 {
    1_400.0
}

fn default_wspr_high_hz() -> f32 {
    1_600.0
}

fn default_wspr_candidates() -> u16 {
    200
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WsprParams {
    #[serde(default = "default_wspr_low_hz")]
    pub audio_low_hz: f32,
    #[serde(default = "default_wspr_high_hz")]
    pub audio_high_hz: f32,
    #[serde(default = "default_wspr_candidates")]
    pub max_candidates: u16,
}

impl Default for WsprParams {
    fn default() -> Self {
        Self {
            audio_low_hz: default_wspr_low_hz(),
            audio_high_hz: default_wspr_high_hz(),
            max_candidates: default_wspr_candidates(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WsjtMessage {
    pub text: String,
    pub snr_db: f32,
    pub audio_hz: f32,
    pub time_offset_s: f32,
    pub hard_errors: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WsprSpot {
    pub text: String,
    pub callsign: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<String>,
    pub power_dbm: i32,
    pub snr_db: f32,
    pub audio_hz: f32,
    pub time_offset_s: f32,
    pub drift_hz: f32,
}

const RR73: &str = "RR73";

impl EventPayload for WsjtMessage {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let sender = wsjt_sender(&self.text).map(str::to_owned);
        Presentation {
            entity: sender.clone(),
            callsign: sender,
            spot: message_spot(&self.text, self.snr_db),
            fields: wsjt_fields(self),
            ..Presentation::of(format!(
                "{} · {:+.0} dB · {:.0} Hz",
                self.text, self.snr_db, self.audio_hz
            ))
        }
    }
}

impl EventPayload for WsprSpot {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let callsign = trimmed(Some(&self.callsign));
        Presentation {
            entity: callsign.clone(),
            spot: self
                .grid
                .as_deref()
                .and_then(|grid| spot(grid, callsign.clone().unwrap_or_default(), self.snr_db)),
            callsign,
            fields: wspr_fields(self),
            ..Presentation::of(format!(
                "{} · {:+.0} dB · {:.0} Hz",
                self.text, self.snr_db, self.audio_hz
            ))
        }
    }
}

fn wsjt_fields(m: &WsjtMessage) -> Vec<Field> {
    fields(&[
        ("snr", "SNR", number(Some(m.snr_db), Unit::Db, 0)),
        ("audio", "Audio", number(Some(m.audio_hz), Unit::Hz, 1)),
        (
            "time_offset",
            "Time offset",
            number(Some(m.time_offset_s), Unit::Seconds, 2),
        ),
        ("hard_errors", "Hard errors", count(Some(m.hard_errors))),
        ("message", "Message", text(Some(&m.text))),
    ])
}

fn wspr_fields(s: &WsprSpot) -> Vec<Field> {
    fields(&[
        ("callsign", "Callsign", text(Some(&s.callsign))),
        ("grid", "Grid", text(s.grid.as_deref())),
        ("power", "Power", number(Some(s.power_dbm), Unit::Dbm, 0)),
        ("snr", "SNR", number(Some(s.snr_db), Unit::Db, 0)),
        ("audio", "Audio", number(Some(s.audio_hz), Unit::Hz, 1)),
        (
            "time_offset",
            "Time offset",
            number(Some(s.time_offset_s), Unit::Seconds, 2),
        ),
        ("drift", "Drift", number(Some(s.drift_hz), Unit::Hz, 1)),
        ("message", "Message", text(Some(&s.text))),
    ])
}

fn wsjt_sender(text: &str) -> Option<&str> {
    let mut words = text.split_whitespace();
    if words.next()? != "CQ" {
        return words.next();
    }
    words.find(|word| {
        word.bytes().any(|b| b.is_ascii_digit()) && word.bytes().any(|b| b.is_ascii_alphabetic())
    })
}

fn message_spot(text: &str, snr_db: f32) -> Option<Spot> {
    let upper = text.to_ascii_uppercase();
    let words: Vec<&str> = upper.split_whitespace().collect();
    let (grid, before) = match words.as_slice() {
        [.., before, grid] => (*grid, Some(*before)),
        [grid] => (*grid, None),
        [] => return None,
    };
    if grid == RR73 || !is_square(grid) {
        return None;
    }
    let callsign = before
        .map(|word| word.trim_start_matches('<').trim_end_matches('>'))
        .unwrap_or_default();
    spot(grid, callsign.to_owned(), snr_db)
}

fn is_square(word: &str) -> bool {
    matches!(
        word.as_bytes(),
        [b'A'..=b'R', b'A'..=b'R', b'0'..=b'9', b'0'..=b'9']
    )
}

fn spot(grid: &str, callsign: String, snr_db: f32) -> Option<Spot> {
    let grid = grid.trim().to_ascii_uppercase();
    Some(Spot {
        at: grid_lat_lon(&grid)?,
        grid,
        callsign,
        snr_db,
    })
}

pub(crate) fn ft8_kind() -> KindSpec {
    KindSpec::tagged::<WsjtMessage>(EventKind::named("ft8"), "FT8", "Stations", Hue::Muted)
        .caps(&[Capability::Log, Capability::Station, Capability::Spot])
        .sighting()
        .build()
}

pub(crate) fn ft4_kind() -> KindSpec {
    KindSpec::tagged::<WsjtMessage>(EventKind::named("ft4"), "FT4", "Stations", Hue::Muted)
        .caps(&[Capability::Log, Capability::Station, Capability::Spot])
        .sighting()
        .build()
}

pub(crate) fn wspr_kind() -> KindSpec {
    KindSpec::of::<WsprSpot>("WSPR", "Stations", Hue::Muted)
        .caps(&[Capability::Log, Capability::Station, Capability::Spot])
        .sighting()
        .build()
}

impl DecoderSettings for WsprParams {
    const TYPE_ID: &'static str = "wspr";
}

impl EventContract for WsprSpot {
    const KIND: EventKind = EventKind::named("wspr");
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use sdrmm_wire::DecoderEvent;

    fn message(text: &str) -> WsjtMessage {
        WsjtMessage {
            text: text.to_owned(),
            snr_db: 0.0,
            audio_hz: 0.0,
            time_offset_s: 0.0,
            hard_errors: 0,
        }
    }

    fn fixtures() -> Vec<(KindSpec, DecoderEvent)> {
        let call = message("CQ DL1ABC JO62");
        let spot = WsprSpot {
            text: "DL1ABC JO62 37".to_owned(),
            callsign: "DL1ABC".to_owned(),
            grid: Some("JO62".to_owned()),
            power_dbm: 37,
            snr_db: -21.0,
            audio_hz: 1_500.0,
            time_offset_s: 0.4,
            drift_hz: 0.0,
        };
        vec![
            (
                ft8_kind(),
                DecoderEvent::tagged(EventKind::named("ft8"), &call),
            ),
            (
                ft4_kind(),
                DecoderEvent::tagged(EventKind::named("ft4"), &call),
            ),
            (wspr_kind(), DecoderEvent::of(&spot)),
        ]
    }

    #[test]
    fn capabilities_match_the_presentation_both_ways() {
        for (spec, event) in fixtures() {
            let kind = &spec.entry.kind;
            assert_eq!(event.kind, *kind);
            let view = spec
                .codec
                .present(kind, &event.data())
                .expect("payload reads");
            let checks = [
                (Capability::Station, view.entity.is_some()),
                (Capability::Geo, view.place.is_some()),
                (Capability::Voice, view.voice.is_some()),
                (Capability::Duration, view.duration_ms.is_some()),
                (Capability::Text, view.text.is_some()),
                (Capability::Spot, view.spot.is_some()),
                (Capability::Bearing, view.bearing.is_some()),
            ];
            for (cap, shown) in checks {
                assert_eq!(spec.entry.has(cap), shown, "{kind} {cap:?}");
            }
            assert!(
                spec.entry.has(Capability::Clip) || view.clip.is_none(),
                "{kind} clip"
            );
            assert_eq!(
                spec.entry.has(Capability::Geo),
                spec.entry.symbol.is_some(),
                "{kind} symbol"
            );
        }
    }

    #[test]
    fn a_wsjt_message_names_its_sender() {
        let sender = |text: &str| message(text).present(&EventKind::named("ft8")).callsign;
        assert_eq!(sender("CQ DL1ABC JO62").as_deref(), Some("DL1ABC"));
        assert_eq!(sender("CQ DX DL1ABC JO62").as_deref(), Some("DL1ABC"));
        assert_eq!(sender("CQ POTA W1AW FN31").as_deref(), Some("W1AW"));
        assert_eq!(sender("W1AW DL1ABC -12").as_deref(), Some("DL1ABC"));
        assert_eq!(sender("CQ"), None);
    }
}
