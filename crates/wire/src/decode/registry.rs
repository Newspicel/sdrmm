use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use utoipa::ToSchema;
use utoipa::openapi::{RefOr, schema::Schema};

use super::{
    AcarsMessage, AdsbMessage, AisMessage, AprsPacket, BroadcastData, BroadcastStatus,
    CwSkimmerSpot, DataLinkMessage, DectFrame, DvFrame, EotMessage, ErmesMessage, FlexMessage,
    GnssFrame, IdentReport, IlsReading, MorseText, NavtexMessage, PocsagMessage, PskText,
    RadioClockFrame, RdsUpdate, RttyText, ScramblerStatus, SelcallSequence, SstvPicture,
    ToneSquelchStatus, VorReading, WsjtMessage, WsprSpot,
};
use crate::{
    patch::Hue,
    present::{EventPayload, Presentation},
};
use Capability::{Bearing, Clip, Duration, Geo, Log, Picture, Spot, Station, Text, Voice};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Log,
    Station,
    Geo,
    Bearing,
    Spot,
    Voice,
    Duration,
    Text,
    Picture,
    Clip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Symbol {
    Plane,
    Ship,
    Station,
    Balloon,
    Drone,
    Beacon,
    Node,
    Person,
    Vehicle,
    Satellite,
    Antenna,
    Fix,
    Target,
    Echo,
    Dot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Lifetime {
    Instant,
    Sighting { ttl_s: u32 },
    Standing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KindInfo {
    pub name: &'static str,
    pub noun: &'static str,
    pub caps: &'static [Capability],
    pub symbol: Option<Symbol>,
    pub hue: Hue,
    pub lifetime: Lifetime,
}

const SIGHTING_S: u32 = 300;

impl KindInfo {
    const fn caps(self, caps: &'static [Capability]) -> Self {
        Self { caps, ..self }
    }

    const fn symbol(self, symbol: Symbol) -> Self {
        Self {
            symbol: Some(symbol),
            ..self
        }
    }

    const fn standing(self) -> Self {
        Self {
            lifetime: Lifetime::Standing,
            ..self
        }
    }

    const fn sighting(self) -> Self {
        Self {
            lifetime: Lifetime::Sighting { ttl_s: SIGHTING_S },
            ..self
        }
    }

    #[must_use]
    pub fn has(&self, cap: Capability) -> bool {
        self.caps.contains(&cap)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct EventKindEntry {
    pub kind: EventKind,
    pub name: String,
    pub noun: String,
    pub caps: Vec<Capability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<Symbol>,
    pub hue: Hue,
    pub lifetime: Lifetime,
}

const fn info(name: &'static str, noun: &'static str, hue: Hue) -> KindInfo {
    KindInfo {
        name,
        noun,
        caps: &[],
        symbol: None,
        hue,
        lifetime: Lifetime::Instant,
    }
}

macro_rules! decoder_events {
    ($( ($variant:ident, $tag:literal, $payload:ty, $info:expr) ),* $(,)?) => {
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
        #[serde(tag = "kind", content = "data", rename_all = "snake_case")]
        pub enum DecoderEvent {
            $( $variant($payload), )*
        }

        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
            ToSchema,
        )]
        #[serde(rename_all = "snake_case")]
        pub enum EventKind {
            $( $variant, )*
        }

        impl EventKind {
            pub const ALL: &'static [Self] = &[$( Self::$variant ),*];

            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $( Self::$variant => $tag, )*
                }
            }

            #[must_use]
            pub const fn info(self) -> &'static KindInfo {
                match self {
                    $( Self::$variant => {
                        const INFO: KindInfo = $info;
                        &INFO
                    } )*
                }
            }
        }

        impl DecoderEvent {
            #[must_use]
            pub const fn kind(&self) -> EventKind {
                match self {
                    $( Self::$variant(_) => EventKind::$variant, )*
                }
            }

            #[must_use]
            pub fn present(&self) -> Presentation {
                let mut view = match self {
                    $( Self::$variant(payload) => payload.present(EventKind::$variant), )*
                };
                if view.label.is_none() {
                    view.label.clone_from(&view.entity);
                }
                view
            }

            pub fn merge(&mut self, newer: &Self) -> bool {
                match (self, newer) {
                    $( (Self::$variant(older), Self::$variant(newer)) => {
                        EventPayload::merge(older, newer);
                        true
                    } )*
                    _ => false,
                }
            }

            pub fn strip_clip(&mut self) {
                match self {
                    $( Self::$variant(payload) => payload.strip_clip(), )*
                }
            }
        }
    };
}

decoder_events! {
    (Transmission, "transmission", crate::Transmission,
        info("Transmission", "Transmissions", Hue::Muted).caps(&[Log, Duration, Clip])),
    (Rds, "rds", RdsUpdate, info("RDS", "Stations", Hue::Muted).caps(&[Log, Station]).sighting()),
    (Pocsag, "pocsag", PocsagMessage,
        info("POCSAG", "Pagers", Hue::Muted).caps(&[Log, Station]).sighting()),
    (Flex, "flex", FlexMessage, info("FLEX", "Pagers", Hue::Muted).caps(&[Log, Station]).sighting()),
    (Ermes, "ermes", ErmesMessage,
        info("ERMES", "Pagers", Hue::Muted).caps(&[Log, Station]).sighting()),
    (Eot, "eot", EotMessage,
        info("End-of-Train", "Trains", Hue::Muted).caps(&[Log, Station]).sighting()),
    (Sarsat, "sarsat", crate::sarsat::SarsatMessage,
        info("406 MHz beacon", "Beacons", Hue::Pink)
            .caps(&[Log, Station, Geo]).symbol(Symbol::Beacon).sighting()),
    (Tetrapol, "tetrapol", crate::tetrapol::TetrapolMessage,
        info("Tetrapol", "Radios", Hue::Muted).caps(&[Log, Station]).sighting()),
    (Adsb, "adsb", AdsbMessage,
        info("ADS-B", "Aircraft", Hue::Teal)
            .caps(&[Log, Station, Geo]).symbol(Symbol::Plane).sighting()),
    (Ais, "ais", AisMessage,
        info("AIS", "Vessels", Hue::Amber)
            .caps(&[Log, Station, Geo]).symbol(Symbol::Ship).sighting()),
    (Aprs, "aprs", AprsPacket,
        info("APRS", "Stations", Hue::Violet)
            .caps(&[Log, Station, Geo]).symbol(Symbol::Station).sighting()),
    (Rtty, "rtty", RttyText, info("RTTY", "RTTY", Hue::Muted).caps(&[Log, Text])),
    (Morse, "morse", MorseText, info("Morse", "Morse", Hue::Muted).caps(&[Log, Text])),
    (CwSkimmer, "cw_skimmer", CwSkimmerSpot,
        info("CW skimmer", "CW spots", Hue::Muted).caps(&[Log])),
    (Selcall, "selcall", SelcallSequence, info("Selcall", "Selcall", Hue::Muted).caps(&[Log])),
    (Navtex, "navtex", NavtexMessage,
        info("NAVTEX", "Stations", Hue::Muted).caps(&[Log, Station]).sighting()),
    (Acars, "acars", AcarsMessage,
        info("ACARS", "Aircraft", Hue::Teal).caps(&[Log, Station]).sighting()),
    (Tone, "tone", ToneSquelchStatus, info("Tone", "Tones", Hue::Muted).caps(&[Log])),
    (Scrambler, "scrambler", ScramblerStatus,
        info("Scrambler", "Scramblers", Hue::Muted).caps(&[Log])),
    (Dv, "dv", DvFrame,
        info("Digital voice", "Radios", Hue::Blue)
            .caps(&[Log, Station, Geo, Voice]).symbol(Symbol::Person).sighting()),
    (Call, "call", crate::rest::VoiceCall,
        info("Call", "Calls", Hue::Accent)
            .caps(&[Log, Station, Voice, Duration, Clip]).sighting()),
    (Ft8, "ft8", WsjtMessage,
        info("FT8", "Stations", Hue::Muted).caps(&[Log, Station, Spot]).sighting()),
    (Ft4, "ft4", WsjtMessage,
        info("FT4", "Stations", Hue::Muted).caps(&[Log, Station, Spot]).sighting()),
    (Psk, "psk", PskText, info("PSK", "PSK", Hue::Muted).caps(&[Log, Text])),
    (Wspr, "wspr", WsprSpot,
        info("WSPR", "Stations", Hue::Muted).caps(&[Log, Station, Spot]).sighting()),
    (Ident, "ident", IdentReport, info("Signal ID", "Signals", Hue::Muted).caps(&[Log])),
    (BroadcastData, "broadcast_data", BroadcastData,
        info("Broadcast data", "Services", Hue::Muted).caps(&[Log])),
    (Broadcast, "broadcast", BroadcastStatus, info("Broadcast", "Services", Hue::Muted)),
    (RadioClock, "radio_clock", RadioClockFrame,
        info("Radio clock", "Clocks", Hue::Muted).caps(&[Log, Station]).sighting()),
    (Gnss, "gnss", GnssFrame,
        info("GNSS lab", "Satellites", Hue::Muted).caps(&[Log, Station]).sighting()),
    (Sstv, "sstv", SstvPicture, info("SSTV", "Pictures", Hue::Orange).caps(&[Log, Picture])),
    (Vor, "vor", VorReading,
        info("VOR", "Beacons", Hue::Muted).caps(&[Log, Station]).sighting()),
    (Ils, "ils", IlsReading, info("ILS", "ILS", Hue::Muted).caps(&[Log])),
    (Dsc, "dsc", DataLinkMessage,
        info("DSC", "Vessels", Hue::Amber)
            .caps(&[Log, Station, Geo]).symbol(Symbol::Ship).sighting()),
    (InmarsatStdc, "inmarsat_stdc", DataLinkMessage,
        info("Inmarsat STD-C", "Vessels", Hue::Amber)
            .caps(&[Log, Station, Geo]).symbol(Symbol::Ship).sighting()),
    (InmarsatAero, "inmarsat_aero", DataLinkMessage,
        info("Inmarsat Aero", "Aircraft", Hue::Teal)
            .caps(&[Log, Station, Geo]).symbol(Symbol::Plane).sighting()),
    (Vdl2, "vdl2", DataLinkMessage,
        info("VDL Mode 2", "Aircraft", Hue::Teal)
            .caps(&[Log, Station, Geo]).symbol(Symbol::Plane).sighting()),
    (Hfdl, "hfdl", DataLinkMessage,
        info("HFDL", "Aircraft", Hue::Teal)
            .caps(&[Log, Station, Geo]).symbol(Symbol::Plane).sighting()),
    (Iridium, "iridium", DataLinkMessage,
        info("Iridium", "Bursts", Hue::Blue)
            .caps(&[Log, Station, Geo]).symbol(Symbol::Dot).sighting()),
    (Df, "df", crate::fusion::DfBearing,
        info("Bearing", "Bearings", Hue::Indigo)
            .caps(&[Log, Station, Geo, Bearing]).symbol(Symbol::Antenna).sighting()),
    (DfFix, "df_fix", crate::fusion::DfEstimate,
        info("Fix", "Fixes", Hue::Indigo)
            .caps(&[Log, Station, Geo]).symbol(Symbol::Fix).sighting()),
    (Radar, "radar", crate::radar::RadarTrackEvent,
        info("Radar", "Tracks", Hue::Orange)
            .caps(&[Log, Station, Geo]).symbol(Symbol::Echo).sighting()),
    (RadarSites, "radar_sites", crate::radar::RadarSites,
        info("Radar sites", "Sites", Hue::Orange)
            .caps(&[Station, Geo]).symbol(Symbol::Antenna).standing()),
    (Nav, "nav", crate::fusion::NavGuide,
        info("Guidance", "Targets", Hue::Indigo)
            .caps(&[Station, Geo]).symbol(Symbol::Target).standing()),
    (Satellite, "satellite", crate::satellite::SatelliteFix,
        info("Satellite", "Satellites", Hue::Pink)
            .caps(&[Station, Geo]).symbol(Symbol::Satellite).standing()),
    (Dect, "dect", DectFrame,
        info("DECT", "Base stations", Hue::Muted).caps(&[Log, Station]).sighting()),
    (Gsm, "gsm", crate::gsm::GsmFrame,
        info("GSM", "Cells", Hue::Muted).caps(&[Log, Station]).sighting()),
    (Apt, "apt", crate::weather::AptImage,
        info("NOAA APT", "Pictures", Hue::Orange).caps(&[Log, Picture])),
    (Lrpt, "lrpt", crate::weather::LrptImage,
        info("Meteor LRPT", "Pictures", Hue::Orange).caps(&[Log, Picture])),
    (Wefax, "wefax", crate::weather::WefaxPicture,
        info("WEFAX", "Pictures", Hue::Orange).caps(&[Log, Picture])),
    (Same, "same", crate::same::SameMessage,
        info("SAME", "Alerts", Hue::Orange).caps(&[Log, Station]).sighting()),
    (Radiosonde, "radiosonde", crate::weather::RadiosondeFrame,
        info("Radiosonde", "Sondes", Hue::Orange)
            .caps(&[Log, Station, Geo]).symbol(Symbol::Balloon).sighting()),
    (Lora, "lora", crate::lora::LoraFrame,
        info("LoRa", "Nodes", Hue::Green)
            .caps(&[Log, Station, Geo]).symbol(Symbol::Node).sighting()),
    (RemoteId, "remote_id", crate::remote_id::RemoteIdFrame,
        info("Remote ID", "Drones", Hue::Cyan)
            .caps(&[Log, Station, Geo]).symbol(Symbol::Drone).sighting()),
    (Ble, "ble", crate::ble::BleAdvert,
        info("BLE", "Devices", Hue::Muted).caps(&[Log, Station]).sighting()),
    (WifiOccupancy, "wifi_occupancy", crate::airtime::WifiOccupancyReport,
        info("Wi-Fi load", "Wi-Fi load", Hue::Muted).caps(&[Log])),
    (IsmSurvey, "ism_survey", crate::airtime::IsmSurveyReport,
        info("2.4 GHz", "2.4 GHz", Hue::Muted).caps(&[Log])),
    (Ism, "ism", crate::ism::IsmMessage,
        info("ISM sensor", "Sensors", Hue::Muted).caps(&[Log, Station]).sighting()),
    (Wmbus, "wmbus", crate::wmbus::WmbusMessage,
        info("wM-Bus", "Meters", Hue::Muted).caps(&[Log, Station]).sighting()),
    (NoiseFigure, "noise_figure", crate::measurement::NoiseFigureReport,
        info("Noise figure", "Noise figure", Hue::Muted).caps(&[Log])),
    (Per, "per", crate::measurement::PerReport, info("PER", "PER", Hue::Muted).caps(&[Log])),
    (Sid, "sid", crate::measurement::SidReport, info("SID", "SID", Hue::Muted).caps(&[Log])),
    (Stanag, "stanag", crate::stanag::StanagReport,
        info("STANAG", "STANAG", Hue::Muted).caps(&[Log])),
}

const _: () = assert!(EventKind::ALL.len() <= 128);

impl EventKind {
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.as_str() == text)
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        self.info().name
    }

    #[must_use]
    pub fn has(self, cap: Capability) -> bool {
        self.info().has(cap)
    }

    #[must_use]
    pub fn entry(self) -> EventKindEntry {
        let info = self.info();
        EventKindEntry {
            kind: self,
            name: info.name.to_owned(),
            noun: info.noun.to_owned(),
            caps: info.caps.to_vec(),
            symbol: info.symbol,
            hue: info.hue,
            lifetime: info.lifetime,
        }
    }

    #[must_use]
    pub fn entries() -> Vec<EventKindEntry> {
        Self::ALL.iter().map(|kind| kind.entry()).collect()
    }

    const fn bit(self) -> u128 {
        1 << self as u32
    }
}

impl fmt::Display for EventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct KindSet(u128);

impl KindSet {
    pub const EMPTY: Self = Self(0);

    #[must_use]
    pub fn all() -> Self {
        EventKind::ALL.iter().copied().collect()
    }

    #[must_use]
    pub fn with_cap(cap: Capability) -> Self {
        EventKind::ALL
            .iter()
            .copied()
            .filter(|kind| kind.has(cap))
            .collect()
    }

    #[must_use]
    pub const fn with(self, kind: EventKind) -> Self {
        Self(self.0 | kind.bit())
    }

    #[must_use]
    pub const fn contains(self, kind: EventKind) -> bool {
        self.0 & kind.bit() != 0
    }

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[must_use]
    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    #[must_use]
    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[must_use]
    pub const fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    pub fn iter(self) -> impl Iterator<Item = EventKind> {
        EventKind::ALL
            .iter()
            .copied()
            .filter(move |kind| self.contains(*kind))
    }
}

impl FromIterator<EventKind> for KindSet {
    fn from_iter<I: IntoIterator<Item = EventKind>>(kinds: I) -> Self {
        kinds.into_iter().fold(Self::EMPTY, Self::with)
    }
}

impl fmt::Debug for KindSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

impl Serialize for KindSet {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.iter())
    }
}

impl<'de> Deserialize<'de> for KindSet {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Vec::<EventKind>::deserialize(deserializer)?
            .into_iter()
            .collect())
    }
}

impl utoipa::PartialSchema for KindSet {
    fn schema() -> RefOr<Schema> {
        <Vec<EventKind> as utoipa::PartialSchema>::schema()
    }
}

impl ToSchema for KindSet {
    fn schemas(schemas: &mut Vec<(String, RefOr<Schema>)>) {
        schemas.push((
            <EventKind as ToSchema>::name().into_owned(),
            <EventKind as utoipa::PartialSchema>::schema(),
        ));
        <EventKind as ToSchema>::schemas(schemas);
    }
}

#[cfg(test)]
mod tests;
