use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    fmt,
    marker::PhantomData,
    sync::Arc,
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, value::RawValue};
use utoipa::ToSchema;

use crate::{
    patch::Hue,
    present::{EventPayload, Presentation, overlay_value, unique_keys},
};

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, ToSchema,
)]
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
    /// The view carries an altitude: an aircraft or balloon a radar can be checked against.
    Track,
}

crate::open_name::open_name!(
    /// The icon an entity of a kind wears on the map.
    Symbol {
        PLANE = "plane",
        SHIP = "ship",
        STATION = "station",
        BALLOON = "balloon",
        DRONE = "drone",
        BEACON = "beacon",
        NODE = "node",
        PERSON = "person",
        VEHICLE = "vehicle",
        SATELLITE = "satellite",
        ANTENNA = "antenna",
        FIX = "fix",
        TARGET = "target",
        ECHO = "echo",
        DOT = "dot",
    }
);

/// How the frontend lays out a kind's events beyond the plain log. Each names fields by
/// `Field::key`, so a renamed label keeps its column.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum KindView {
    /// One row per entity, newest values merged, columns in this order.
    Table {
        columns: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sort: Option<SortKey>,
        /// The field that tells rows apart; unset uses the presentation's entity.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key: Option<String>,
    },
    /// The newest value of a field drawn on a range.
    Meter { field: String, min: f64, max: f64 },
    /// A field plotted over time, the last `window_s` seconds.
    Trend { fields: Vec<String>, window_s: u32 },
    /// The newest picture the kind carries.
    Picture,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SortKey {
    pub field: String,
    #[serde(default)]
    pub descending: bool,
}

impl KindView {
    #[must_use]
    pub fn table(columns: &[&str]) -> Self {
        Self::Table {
            columns: columns.iter().map(|&column| column.to_owned()).collect(),
            sort: None,
            key: None,
        }
    }

    #[must_use]
    pub fn sorted(self, field: &str, descending: bool) -> Self {
        match self {
            Self::Table { columns, key, .. } => Self::Table {
                columns,
                sort: Some(SortKey {
                    field: field.to_owned(),
                    descending,
                }),
                key,
            },
            other => other,
        }
    }

    #[must_use]
    pub fn keyed(self, field: &str) -> Self {
        match self {
            Self::Table { columns, sort, .. } => Self::Table {
                columns,
                sort,
                key: Some(field.to_owned()),
            },
            other => other,
        }
    }

    #[must_use]
    pub fn meter(field: &str, min: f64, max: f64) -> Self {
        Self::Meter {
            field: field.to_owned(),
            min,
            max,
        }
    }

    #[must_use]
    pub fn trend(fields: &[&str], window_s: u32) -> Self {
        Self::Trend {
            fields: fields.iter().map(|&field| field.to_owned()).collect(),
            window_s,
        }
    }

    /// Every field key the view names, so a kind can be checked against what it presents.
    #[must_use]
    pub fn fields(&self) -> Vec<&str> {
        match self {
            Self::Table { columns, sort, key } => columns
                .iter()
                .map(String::as_str)
                .chain(sort.iter().map(|sort| sort.field.as_str()))
                .chain(key.as_deref())
                .collect(),
            Self::Meter { field, .. } => vec![field.as_str()],
            Self::Trend { fields, .. } => fields.iter().map(String::as_str).collect(),
            Self::Picture => Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Lifetime {
    Instant,
    Sighting { ttl_s: u32 },
    Standing,
}

pub const SIGHTING_S: u32 = 300;

/// The name an event kind goes by on the wire, in filters and in the registry.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, ToSchema)]
#[serde(transparent)]
#[schema(value_type = String)]
pub struct EventKind(Cow<'static, str>);

impl EventKind {
    #[must_use]
    pub const fn named(tag: &'static str) -> Self {
        Self(Cow::Borrowed(tag))
    }

    #[must_use]
    pub fn new(tag: impl Into<String>) -> Self {
        Self(Cow::Owned(tag.into()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for EventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EventKind({})", self.as_str())
    }
}

impl PartialEq<str> for EventKind {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for EventKind {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

/// One decoded event: the kind it is and that kind's own payload.
///
/// The payload travels as JSON text: building one on the DSP thread costs one buffer however many
/// fields it has, and the control plane parses it when it reads.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct DecoderEvent {
    pub kind: EventKind,
    #[schema(value_type = serde_json::Value)]
    data: Box<RawValue>,
}

/// A payload type bound to the event kind it travels as.
pub trait EventContract: Serialize + DeserializeOwned {
    const KIND: EventKind;
}

const EVENT_BUFFER: usize = 256;

/// `payload` as JSON text. A payload that cannot be serialized becomes `null`, which every reader
/// then rejects rather than reading half of it.
fn raw(payload: &impl Serialize) -> Box<RawValue> {
    let mut text = Vec::with_capacity(EVENT_BUFFER);
    serde_json::to_writer(&mut text, payload)
        .ok()
        .and_then(|()| String::from_utf8(text).ok())
        .and_then(|text| RawValue::from_string(text).ok())
        .unwrap_or_else(null)
}

fn null() -> Box<RawValue> {
    RawValue::from_string("null".to_owned()).unwrap_or_default()
}

impl<'de> Deserialize<'de> for DecoderEvent {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Stated {
            kind: EventKind,
            #[serde(default)]
            data: Value,
        }
        let stated = Stated::deserialize(deserializer)?;
        Ok(Self::tagged(stated.kind, &stated.data))
    }
}

impl PartialEq for DecoderEvent {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
            && (self.data.get() == other.data.get() || self.data() == other.data())
    }
}

impl DecoderEvent {
    #[must_use]
    pub fn of<T: EventContract>(payload: &T) -> Self {
        Self::tagged(T::KIND, payload)
    }

    /// `payload` as an event of `kind`, for a payload type several kinds share.
    #[must_use]
    pub fn tagged(kind: EventKind, payload: &impl Serialize) -> Self {
        Self {
            kind,
            data: raw(payload),
        }
    }

    /// The payload as a JSON value; `null` when it does not parse.
    #[must_use]
    pub fn data(&self) -> Value {
        serde_json::from_str(self.data.get()).unwrap_or(Value::Null)
    }

    /// The payload as JSON text, as it travels.
    #[must_use]
    pub fn raw_data(&self) -> &str {
        self.data.get()
    }

    /// Replaces the payload with `data`.
    pub fn set_data(&mut self, data: &Value) {
        self.data = raw(data);
    }

    /// The payload as `T`, whatever kind carries it.
    #[must_use]
    pub fn payload<T: DeserializeOwned>(&self) -> Option<T> {
        serde_json::from_str(self.data.get()).ok()
    }

    #[must_use]
    pub fn kind(&self) -> EventKind {
        self.kind.clone()
    }

    #[must_use]
    pub fn is<T: EventContract>(&self) -> bool {
        self.kind == T::KIND
    }

    /// The payload as `T`, only when this event is of `T`'s kind.
    #[must_use]
    pub fn read<T: EventContract>(&self) -> Option<T> {
        if self.is::<T>() { self.payload() } else { None }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct EventKindEntry {
    pub kind: EventKind,
    pub name: String,
    pub noun: String,
    pub caps: Vec<Capability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<Symbol>,
    pub hue: Hue,
    pub lifetime: Lifetime,
    /// A decode of this kind shows the signal is the decoder's. Free text a decoder reads out of
    /// any noise does not.
    #[serde(default = "verifies_by_default")]
    pub verifies: bool,
    /// A reading of this kind stands only inside its decoder's allocation: the decoder measures
    /// whatever carrier it is given.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allocated: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub views: Vec<KindView>,
}

fn verifies_by_default() -> bool {
    true
}

impl EventKindEntry {
    #[must_use]
    pub fn new(kind: EventKind, name: &str, noun: &str, hue: Hue) -> Self {
        Self {
            kind,
            name: name.to_owned(),
            noun: noun.to_owned(),
            caps: Vec::new(),
            symbol: None,
            hue,
            lifetime: Lifetime::Instant,
            verifies: true,
            allocated: false,
            views: Vec::new(),
        }
    }

    #[must_use]
    pub fn view(mut self, view: KindView) -> Self {
        self.views.push(view);
        self
    }

    #[must_use]
    pub fn caps(mut self, caps: &[Capability]) -> Self {
        self.caps = caps.to_vec();
        self
    }

    #[must_use]
    pub fn symbol(mut self, symbol: Symbol) -> Self {
        self.symbol = Some(symbol);
        self
    }

    #[must_use]
    pub fn standing(mut self) -> Self {
        self.lifetime = Lifetime::Standing;
        self
    }

    #[must_use]
    pub fn sighting(mut self) -> Self {
        self.lifetime = Lifetime::Sighting { ttl_s: SIGHTING_S };
        self
    }

    #[must_use]
    pub fn tentative(mut self) -> Self {
        self.verifies = false;
        self
    }

    #[must_use]
    pub fn allocated(mut self) -> Self {
        self.allocated = true;
        self
    }

    #[must_use]
    pub fn has(&self, cap: Capability) -> bool {
        self.caps.contains(&cap)
    }
}

/// How the platform reads one kind's payloads without knowing their type.
pub trait KindCodec: Send + Sync {
    fn present(&self, kind: &EventKind, data: &Value) -> Result<Presentation, String>;

    fn merge(&self, older: &mut Value, newer: &Value) {
        overlay_value(older, newer);
    }

    fn strip_clip(&self, _data: &mut Value) {}
}

struct Typed<T>(PhantomData<fn() -> T>);

impl<T: EventPayload> KindCodec for Typed<T> {
    fn present(&self, kind: &EventKind, data: &Value) -> Result<Presentation, String> {
        let payload: T = serde_json::from_value(data.clone())
            .map_err(|error| format!("{kind} payload: {error}"))?;
        Ok(payload.present(kind))
    }

    fn merge(&self, older: &mut Value, newer: &Value) {
        let (Ok(mut base), Ok(top)) = (
            serde_json::from_value::<T>(older.clone()),
            serde_json::from_value::<T>(newer.clone()),
        ) else {
            *older = newer.clone();
            return;
        };
        base.merge(&top);
        if let Ok(merged) = serde_json::to_value(base) {
            *older = merged;
        }
    }

    fn strip_clip(&self, data: &mut Value) {
        if let Ok(mut payload) = serde_json::from_value::<T>(data.clone()) {
            payload.strip_clip();
            if let Ok(stripped) = serde_json::to_value(payload) {
                *data = stripped;
            }
        }
    }
}

/// An event kind as the registry holds it: what it is called and how to read it.
#[derive(Clone)]
pub struct KindSpec {
    pub entry: EventKindEntry,
    pub codec: Arc<dyn KindCodec>,
}

impl KindSpec {
    #[must_use]
    pub fn of<T: EventPayload + EventContract + 'static>(
        name: &str,
        noun: &str,
        hue: Hue,
    ) -> KindSpecBuilder {
        Self::tagged::<T>(T::KIND, name, noun, hue)
    }

    /// A kind whose payload type other kinds share.
    #[must_use]
    pub fn tagged<T: EventPayload + 'static>(
        kind: EventKind,
        name: &str,
        noun: &str,
        hue: Hue,
    ) -> KindSpecBuilder {
        KindSpecBuilder {
            entry: EventKindEntry::new(kind, name, noun, hue),
            codec: Arc::new(Typed::<T>(PhantomData)),
        }
    }

    /// A kind read through its own codec, as a plugin supplies one.
    #[must_use]
    pub fn with_codec(entry: EventKindEntry, codec: Arc<dyn KindCodec>) -> Self {
        Self { entry, codec }
    }
}

impl fmt::Debug for KindSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("KindSpec").field(&self.entry).finish()
    }
}

pub struct KindSpecBuilder {
    entry: EventKindEntry,
    codec: Arc<dyn KindCodec>,
}

impl KindSpecBuilder {
    #[must_use]
    pub fn caps(mut self, caps: &[Capability]) -> Self {
        self.entry = self.entry.caps(caps);
        self
    }

    #[must_use]
    pub fn symbol(mut self, symbol: Symbol) -> Self {
        self.entry = self.entry.symbol(symbol);
        self
    }

    #[must_use]
    pub fn standing(mut self) -> Self {
        self.entry = self.entry.standing();
        self
    }

    #[must_use]
    pub fn sighting(mut self) -> Self {
        self.entry = self.entry.sighting();
        self
    }

    #[must_use]
    pub fn tentative(mut self) -> Self {
        self.entry = self.entry.tentative();
        self
    }

    #[must_use]
    pub fn allocated(mut self) -> Self {
        self.entry = self.entry.allocated();
        self
    }

    #[must_use]
    pub fn view(mut self, view: KindView) -> Self {
        self.entry = self.entry.view(view);
        self
    }

    #[must_use]
    pub fn build(self) -> KindSpec {
        KindSpec {
            entry: self.entry,
            codec: self.codec,
        }
    }
}

/// Turns events into the bytes of a wire format other software reads, such as an aircraft
/// feed.
pub trait FeedEncoder: Send + Sync {
    /// The frame for `event`, one of the feed's kinds, or why it cannot be framed.
    fn encode(&self, event: &DecoderEvent) -> Result<Vec<u8>, String>;
}

/// A feed format a decoder offers, with the kinds it encodes.
#[derive(Clone)]
pub struct FeedSpec {
    pub format: String,
    pub name: String,
    pub default_address: String,
    pub kinds: KindSet,
    /// Sent to idle clients so they know the feed is alive; empty sends nothing.
    pub heartbeat: Vec<u8>,
    pub encoder: Arc<dyn FeedEncoder>,
}

impl fmt::Debug for FeedSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FeedSpec")
            .field("format", &self.format)
            .field("kinds", &self.kinds)
            .finish_non_exhaustive()
    }
}

/// A feed format as the frontend lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FeedEntry {
    pub format: String,
    pub name: String,
    pub default_address: String,
    pub kinds: KindSet,
}

/// Every event kind the running server knows: the platform's own and those of every decoder
/// registered now, with the feed formats they offer.
#[derive(Clone, Debug, Default)]
pub struct EventCatalog {
    kinds: BTreeMap<EventKind, KindSpec>,
    feeds: Vec<FeedSpec>,
}

impl EventCatalog {
    #[must_use]
    pub fn new(specs: impl IntoIterator<Item = KindSpec>) -> Self {
        Self {
            kinds: specs
                .into_iter()
                .map(|spec| (spec.entry.kind.clone(), spec))
                .collect(),
            feeds: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_feeds(mut self, feeds: impl IntoIterator<Item = FeedSpec>) -> Self {
        self.feeds.extend(feeds);
        self
    }

    #[must_use]
    pub fn feeds(&self) -> &[FeedSpec] {
        &self.feeds
    }

    #[must_use]
    pub fn feed(&self, format: &str) -> Option<&FeedSpec> {
        self.feeds.iter().find(|feed| feed.format == format)
    }

    #[must_use]
    pub fn feed_entries(&self) -> Vec<FeedEntry> {
        self.feeds
            .iter()
            .map(|feed| FeedEntry {
                format: feed.format.clone(),
                name: feed.name.clone(),
                default_address: feed.default_address.clone(),
                kinds: feed.kinds.clone(),
            })
            .collect()
    }

    #[must_use]
    pub fn feed_kinds(&self, format: &str) -> KindSet {
        self.feed(format)
            .map(|feed| feed.kinds.clone())
            .unwrap_or_default()
    }

    #[must_use]
    pub fn entry(&self, kind: &EventKind) -> Option<&EventKindEntry> {
        self.kinds.get(kind).map(|spec| &spec.entry)
    }

    #[must_use]
    pub fn entries(&self) -> Vec<EventKindEntry> {
        self.kinds.values().map(|spec| spec.entry.clone()).collect()
    }

    #[must_use]
    pub fn parse(&self, tag: &str) -> Option<EventKind> {
        self.kinds.keys().find(|kind| kind.as_str() == tag).cloned()
    }

    #[must_use]
    pub fn has(&self, kind: &EventKind, cap: Capability) -> bool {
        self.entry(kind).is_some_and(|entry| entry.has(cap))
    }

    #[must_use]
    pub fn all(&self) -> KindSet {
        self.kinds.keys().cloned().collect()
    }

    #[must_use]
    pub fn with_cap(&self, cap: Capability) -> KindSet {
        self.kinds
            .values()
            .filter(|spec| spec.entry.has(cap))
            .map(|spec| spec.entry.kind.clone())
            .collect()
    }

    /// How `event` reads in a log. A kind nobody registered, or a payload its codec rejects,
    /// still presents, naming what is wrong instead of hiding the event.
    #[must_use]
    pub fn present(&self, event: &DecoderEvent) -> Presentation {
        let presented = match self.kinds.get(&event.kind) {
            Some(spec) => spec.codec.present(&event.kind, &event.data()),
            None => Err(format!("unknown event kind {}", event.kind)),
        };
        let mut view = presented.unwrap_or_else(Presentation::of);
        view.fields = unique_keys(std::mem::take(&mut view.fields));
        if view.label.is_none() {
            view.label.clone_from(&view.entity);
        }
        view
    }

    /// Folds `newer` into `older` when both are of one kind.
    pub fn merge(&self, older: &mut DecoderEvent, newer: &DecoderEvent) -> bool {
        if older.kind != newer.kind {
            return false;
        }
        let mut merged = older.data();
        let top = newer.data();
        match self.kinds.get(&older.kind) {
            Some(spec) => spec.codec.merge(&mut merged, &top),
            None => overlay_value(&mut merged, &top),
        }
        older.set_data(&merged);
        true
    }

    pub fn strip_clip(&self, event: &mut DecoderEvent) {
        if let Some(spec) = self.kinds.get(&event.kind) {
            let mut data = event.data();
            spec.codec.strip_clip(&mut data);
            event.set_data(&data);
        }
    }
}

/// A set of event kinds, sorted, as a filter or a port states it.
#[derive(Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct KindSet(BTreeSet<EventKind>);

impl utoipa::PartialSchema for KindSet {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        <Vec<String> as utoipa::PartialSchema>::schema()
    }
}

impl ToSchema for KindSet {}

impl KindSet {
    pub const EMPTY: Self = Self(BTreeSet::new());

    #[must_use]
    pub fn with(mut self, kind: EventKind) -> Self {
        self.0.insert(kind);
        self
    }

    #[must_use]
    pub fn contains(&self, kind: &EventKind) -> bool {
        self.0.contains(kind)
    }

    #[must_use]
    pub fn union(&self, other: &Self) -> Self {
        self.0.union(&other.0).cloned().collect()
    }

    #[must_use]
    pub fn intersection(&self, other: &Self) -> Self {
        self.0.intersection(&other.0).cloned().collect()
    }

    #[must_use]
    pub fn without(&self, other: &Self) -> Self {
        self.0.difference(&other.0).cloned().collect()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &EventKind> {
        self.0.iter()
    }
}

impl FromIterator<EventKind> for KindSet {
    fn from_iter<I: IntoIterator<Item = EventKind>>(kinds: I) -> Self {
        Self(kinds.into_iter().collect())
    }
}

impl fmt::Debug for KindSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

#[cfg(test)]
mod tests;
