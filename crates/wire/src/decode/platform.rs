use std::sync::{Arc, LazyLock, PoisonError, RwLock};

use super::{
    BroadcastData, Capability, DvFrame, EventCatalog, EventContract, EventKind, FeedSpec,
    IdentReport, KindSpec, Symbol,
};
use crate::{
    fusion::{BearingFix, DfBearing, DfEstimate, NavGuide},
    monitor::Transmission,
    patch::Hue,
    radar::{RadarSites, RadarTrackEvent},
    rest::VoiceCall,
    satellite::SatelliteFix,
};
use Capability::{Bearing, Clip, Duration, Geo, Log, Station, Voice};

impl EventKind {
    pub const TRANSMISSION: Self = Self::named("transmission");
    pub const DV: Self = Self::named("dv");
    pub const CALL: Self = Self::named("call");
    pub const BROADCAST_DATA: Self = Self::named("broadcast_data");
    pub const DF: Self = Self::named("df");
    pub const DF_FIX: Self = Self::named("df_fix");
    pub const BEARING_FIX: Self = Self::named("bearing_fix");
    pub const RADAR: Self = Self::named("radar");
    pub const RADAR_SITES: Self = Self::named("radar_sites");
    pub const NAV: Self = Self::named("nav");
    pub const SATELLITE: Self = Self::named("satellite");
    pub const IDENT: Self = Self::named("ident");
}

macro_rules! contract {
    ($($payload:ty => $kind:ident),+ $(,)?) => {
        $(impl EventContract for $payload {
            const KIND: EventKind = EventKind::$kind;
        })+
    };
}

contract! {
    Transmission => TRANSMISSION,
    VoiceCall => CALL,
    DfBearing => DF,
    DfEstimate => DF_FIX,
    BearingFix => BEARING_FIX,
    RadarTrackEvent => RADAR,
    RadarSites => RADAR_SITES,
    NavGuide => NAV,
    SatelliteFix => SATELLITE,
    IdentReport => IDENT,
}

/// The kinds the platform itself raises or that several decoders share as a contract.
#[must_use]
pub fn platform_kinds() -> Vec<KindSpec> {
    vec![
        KindSpec::of::<Transmission>("Transmission", "Transmissions", Hue::Muted)
            .caps(&[Log, Duration, Clip])
            .build(),
        KindSpec::of::<DvFrame>("Digital voice", "Radios", Hue::Blue)
            .caps(&[Log, Station, Geo, Voice])
            .symbol(Symbol::PERSON)
            .sighting()
            .build(),
        KindSpec::of::<VoiceCall>("Call", "Calls", Hue::Accent)
            .caps(&[Log, Station, Voice, Duration, Clip])
            .sighting()
            .build(),
        KindSpec::of::<BroadcastData>("Broadcast data", "Services", Hue::Muted)
            .caps(&[Log])
            .build(),
        KindSpec::of::<DfBearing>("Bearing", "Bearings", Hue::Indigo)
            .caps(&[Log, Station, Geo, Bearing])
            .symbol(Symbol::ANTENNA)
            .sighting()
            .build(),
        KindSpec::of::<DfEstimate>("Fix", "Fixes", Hue::Indigo)
            .caps(&[Log, Station, Geo])
            .symbol(Symbol::FIX)
            .sighting()
            .build(),
        KindSpec::of::<BearingFix>("Bearing fix", "Fixes", Hue::Indigo)
            .caps(&[Log, Station, Geo])
            .symbol(Symbol::FIX)
            .sighting()
            .build(),
        KindSpec::of::<RadarTrackEvent>("Radar", "Tracks", Hue::Orange)
            .caps(&[Log, Station, Geo])
            .symbol(Symbol::ECHO)
            .sighting()
            .build(),
        KindSpec::of::<RadarSites>("Radar sites", "Sites", Hue::Orange)
            .caps(&[Station, Geo])
            .symbol(Symbol::ANTENNA)
            .standing()
            .build(),
        KindSpec::of::<NavGuide>("Guidance", "Targets", Hue::Indigo)
            .caps(&[Station, Geo])
            .symbol(Symbol::TARGET)
            .standing()
            .build(),
        KindSpec::of::<IdentReport>("Signal ID", "Signals", Hue::Muted)
            .caps(&[Log])
            .build(),
        KindSpec::of::<SatelliteFix>("Satellite", "Satellites", Hue::Pink)
            .caps(&[Station, Geo])
            .symbol(Symbol::SATELLITE)
            .standing()
            .build(),
    ]
}

static CATALOG: LazyLock<RwLock<Arc<EventCatalog>>> =
    LazyLock::new(|| RwLock::new(Arc::new(EventCatalog::new(platform_kinds()))));

/// The event kinds known right now. Read on the control plane only.
#[must_use]
pub fn catalog() -> Arc<EventCatalog> {
    CATALOG
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// Replaces the decoder kinds and feeds: the platform's own kinds stay, `decoders` join them.
pub fn install_catalog(
    decoders: impl IntoIterator<Item = KindSpec>,
    feeds: impl IntoIterator<Item = FeedSpec>,
) {
    let next =
        Arc::new(EventCatalog::new(platform_kinds().into_iter().chain(decoders)).with_feeds(feeds));
    *CATALOG.write().unwrap_or_else(PoisonError::into_inner) = next;
}

impl EventKind {
    #[must_use]
    pub fn has(&self, cap: Capability) -> bool {
        catalog().has(self, cap)
    }

    #[must_use]
    pub fn name(&self) -> String {
        catalog()
            .entry(self)
            .map_or_else(|| self.as_str().to_owned(), |entry| entry.name.clone())
    }

    #[must_use]
    pub fn parse(tag: &str) -> Option<Self> {
        catalog().parse(tag)
    }

    /// The kind as registered now; a kind nobody registers reads as a plain log entry.
    #[must_use]
    pub fn info(&self) -> super::EventKindEntry {
        catalog().entry(self).cloned().unwrap_or_else(|| {
            super::EventKindEntry::new(self.clone(), self.as_str(), self.as_str(), Hue::Muted)
        })
    }
}

impl super::DecoderEvent {
    #[must_use]
    pub fn present(&self) -> crate::present::Presentation {
        catalog().present(self)
    }

    pub fn merge(&mut self, newer: &Self) -> bool {
        catalog().merge(self, newer)
    }

    pub fn strip_clip(&mut self) {
        catalog().strip_clip(self);
    }
}

impl super::KindSet {
    #[must_use]
    pub fn all() -> Self {
        catalog().all()
    }

    #[must_use]
    pub fn with_cap(cap: Capability) -> Self {
        catalog().with_cap(cap)
    }
}
