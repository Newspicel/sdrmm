use std::sync::{Arc, Once};

use serde::{Deserialize, Serialize};

use super::{
    Capability, DecoderEvent, EventContract, EventKind, FeedEncoder, FeedSpec, KindSet, KindSpec,
    Symbol, install_catalog,
};
use crate::{
    LatLon,
    monitor::Transmission,
    patch::Hue,
    present::{EventPayload, Place, Presentation},
};

pub(crate) const STATION: EventKind = EventKind::named("probe_station");
pub(crate) const TEXT: EventKind = EventKind::named("probe_text");
pub(crate) const TRACK: EventKind = EventKind::named("probe_track");
pub(crate) const FEED: &str = "probe_feed";

/// A station that may know where it is, as an aircraft or ship decoder reports one.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct ProbeStation {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<LatLon>,
}

/// Free text, as a teleprinter decoder reports it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct ProbeText {
    pub text: String,
}

/// A flying target with a height, as a radar can be checked against.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct ProbeTrack {
    pub id: String,
    pub at: LatLon,
    pub altitude_m: f32,
}

impl EventContract for ProbeStation {
    const KIND: EventKind = STATION;
}

impl EventContract for ProbeText {
    const KIND: EventKind = TEXT;
}

impl EventContract for ProbeTrack {
    const KIND: EventKind = TRACK;
}

impl EventPayload for ProbeStation {
    fn present(&self, _kind: &EventKind) -> Presentation {
        let summary = self.name.clone().unwrap_or_else(|| self.id.clone());
        Presentation {
            entity: Some(self.id.clone()),
            place: self.at.map(Place::at),
            ..Presentation::of(summary)
        }
    }
}

impl EventPayload for ProbeText {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            text: Some(self.text.clone()),
            ..Presentation::of(self.text.clone())
        }
    }
}

impl EventPayload for ProbeTrack {
    fn present(&self, _kind: &EventKind) -> Presentation {
        Presentation {
            entity: Some(self.id.clone()),
            place: Some(Place::at(self.at).altitude(Some(self.altitude_m))),
            ..Presentation::of(self.id.clone())
        }
    }
}

impl ProbeStation {
    pub(crate) fn event(id: &str, name: Option<&str>, at: Option<LatLon>) -> DecoderEvent {
        DecoderEvent::of(&Self {
            id: id.to_owned(),
            name: name.map(str::to_owned),
            at,
        })
    }
}

impl ProbeText {
    pub(crate) fn event(text: &str) -> DecoderEvent {
        DecoderEvent::of(&Self {
            text: text.to_owned(),
        })
    }
}

/// A finished FM transmission with a clip, as the monitor reports one.
pub(crate) fn transmission() -> Transmission {
    serde_json::from_value(serde_json::json!({
        "id": 1, "state": "completed",
        "signal": {
            "modulation": "fm", "confidence": 0.9, "frequency_hz": 145_500_000.0,
            "center_offset_hz": 0.0, "bandwidth_hz": 12_500.0, "snr_db": 20.0,
            "features": {
                "envelope_variation": 0.1, "duty": 1.0, "keying_depth_db": 0.0,
                "spectral_asymmetry": 0.0, "carrier_db": 0.0, "spectral_flatness": 0.2,
                "frequency_levels": 0, "frequency_spread_hz": 3_000.0, "square_line_db": 0.0,
                "quartic_line_db": 0.0,
            },
        },
        "start_sample": 0, "end_sample": 48_000, "sample_rate_hz": 48_000.0, "duration_ms": 1_000,
        "audio": { "url": "/api/clips/1", "media_type": "audio/wav" },
    }))
    .expect("transmission fixture")
}

struct IdFeed;

impl FeedEncoder for IdFeed {
    fn encode(&self, event: &DecoderEvent) -> Result<Vec<u8>, String> {
        event
            .read::<ProbeStation>()
            .map(|station| station.id.into_bytes())
            .ok_or_else(|| "not a probe station".to_owned())
    }
}

pub(crate) fn kinds() -> Vec<KindSpec> {
    use Capability::{Geo, Log, Station, Text, Track};
    vec![
        KindSpec::of::<ProbeStation>("Probe station", "Stations", Hue::Teal)
            .caps(&[Log, Station, Geo])
            .symbol(Symbol::Plane)
            .sighting()
            .build(),
        KindSpec::of::<ProbeText>("Probe text", "Texts", Hue::Muted)
            .caps(&[Log, Text])
            .build(),
        KindSpec::of::<ProbeTrack>("Probe track", "Tracks", Hue::Orange)
            .caps(&[Log, Station, Geo, Track])
            .symbol(Symbol::Plane)
            .sighting()
            .build(),
    ]
}

pub(crate) fn feeds() -> Vec<FeedSpec> {
    vec![FeedSpec {
        format: FEED.to_owned(),
        name: "Probe feed".to_owned(),
        default_address: "127.0.0.1:30005".to_owned(),
        kinds: KindSet::EMPTY.with(STATION),
        heartbeat: Vec::new(),
        encoder: Arc::new(IdFeed),
    }]
}

/// Installs the probe kinds into the process catalog once. Every test that needs a decoder
/// kind calls this first and installs nothing else, so parallel tests see one catalog.
pub(crate) fn install() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| install_catalog(kinds(), feeds()));
}
