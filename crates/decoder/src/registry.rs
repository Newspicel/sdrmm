//! Every decoder the server can open, built in or loaded as a plugin.
//!
//! The registry is control-plane state: channels are created from it and descriptors read from
//! it, but a running channel only holds the `Channel` it was given.

use std::sync::{Arc, LazyLock, OnceLock, PoisonError, RwLock};

use sdrmm_modem::synth::{BASIC, Catalog, EMPTY_CATALOG, Recipe, Signal};
use sdrmm_wire::{
    ChannelBand, ChannelDescriptor, ChannelParams, ChannelSettings, DEFAULT_FREQUENCY_HZ, FeedSpec,
    FilterSpec, IdentSignal, KindSpec, SettingField, SignalSignature, SignalTrial,
};
use serde::{Serialize, de::DeserializeOwned};

use crate::{Channel, ChannelCtx, ChannelError, ChannelFilter, NativeChannel};

/// Where a channel sits in its own baseband and what it needs fed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChannelLayout {
    pub input_rate_hz: f64,
    /// Lowest and highest occupied offset from the channel frequency.
    pub band: (f64, f64),
    pub audio_channels: u8,
}

/// Settings shaped to one identified signal.
#[derive(Clone, Debug, PartialEq)]
pub struct Tuned {
    pub params: ChannelParams,
    /// How far the channel tunes off the signal centre.
    pub offset_hz: f64,
}

pub trait Decoder: Send + Sync {
    /// The completed descriptor: form, defaults and ports included.
    fn descriptor(&self) -> &ChannelDescriptor;

    fn open(
        &self,
        ctx: ChannelCtx,
        settings: &ChannelSettings,
    ) -> Result<Box<dyn Channel>, ChannelError>;

    fn layout(&self, params: &ChannelParams) -> Result<ChannelLayout, ChannelError>;

    fn filter(&self, params: &ChannelParams) -> Result<FilterSpec, ChannelError>;

    /// `params` with every setting the decoder defaults filled in.
    fn normalize(&self, params: &ChannelParams) -> Result<ChannelParams, ChannelError>;

    fn kinds(&self) -> Vec<KindSpec> {
        Vec::new()
    }

    fn feeds(&self) -> Vec<FeedSpec> {
        Vec::new()
    }

    /// How a trunked system runs on this decoder, when its signalling carries trunking.
    fn trunking(&self) -> Option<Trunking> {
        None
    }

    /// `params` shaped to `signal`, as the spectrum monitor opens a decoder on what Signal ID
    /// found.
    fn tune(&self, params: &ChannelParams, _signal: &IdentSignal) -> Result<Tuned, ChannelError> {
        Ok(Tuned {
            params: params.clone(),
            offset_hz: 0.0,
        })
    }

    /// The test signals this decoder can synthesise, for a signal generator to offer.
    fn signals(&self) -> &'static [Signal] {
        &[]
    }

    /// The reference captures this decoder's fixtures are written from.
    fn recipes(&self) -> &'static [Recipe] {
        &[]
    }
}

/// A channel following one granted call, in `slot` on a protocol whose carriers carry several.
/// `calls` holds the trunk node's settings for its calls as this decoder's params; the decoder
/// keeps those its protocol has.
pub type Follow =
    fn(slot: Option<u8>, calls: &ChannelParams) -> Result<ChannelParams, ChannelError>;

/// A call the control channel granted a logical channel to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrunkGrant {
    pub logical_channel: u16,
    /// The timeslot, on a protocol that shares a carrier between calls.
    pub slot: Option<u8>,
    /// The talkgroup, or the radio on a private call.
    pub destination: u32,
    pub source: Option<u32>,
    pub group_call: Option<bool>,
}

/// A logical channel the signalling placed on the frequency it is received on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrunkPlacement {
    pub logical_channel: u16,
    pub freq_hz: u64,
}

/// How a trunked system runs on a decoder: the channels it opens and how it reads the
/// protocol's signalling, so the trunk follower never needs to know which protocol it follows.
#[derive(Clone, Copy, Debug)]
pub struct Trunking {
    /// A channel hearing everything on a carrier: the control channel and channel-plan probes.
    pub watch: fn(ignore_crc: bool) -> Result<ChannelParams, ChannelError>,
    pub follow: Follow,
    /// The protocols the decoder recognises and how many calls share one carrier.
    pub spec: fn() -> sdrmm_wire::TrunkSpec,
    /// The calls a control burst grants.
    pub grants: fn(&sdrmm_wire::DvFrame) -> Vec<TrunkGrant>,
    /// Where a burst says logical channels transmit: the protocol's channel numbering.
    pub placements: fn(&sdrmm_wire::DvFrame) -> Vec<TrunkPlacement>,
}

/// The logical channels a burst's channel definitions place, for a decoder that works out the
/// protocol's channel numbering itself and announces the result on the frame.
#[must_use]
pub fn announced_placements(frame: &sdrmm_wire::DvFrame) -> Vec<TrunkPlacement> {
    frame
        .channel_definition
        .iter()
        .map(|definition| TrunkPlacement {
            logical_channel: definition.channel,
            freq_hz: definition.rx_hz,
        })
        .collect()
}

type Open = fn(ChannelCtx, ChannelSettings) -> Result<Box<dyn Channel>, ChannelError>;

/// A decoder compiled into the build, whose settings are the struct `S`.
pub struct Native<S> {
    base: fn() -> &'static ChannelDescriptor,
    open: Open,
    band: fn(&S) -> (f64, f64),
    filter: fn(&S) -> Result<FilterSpec, ChannelError>,
    input_rate: Option<fn(&S) -> f64>,
    audio_channels: fn(&S) -> u8,
    form: fn() -> Vec<SettingField>,
    home_hz: Option<f64>,
    kinds: fn() -> Vec<KindSpec>,
    feeds: fn() -> Vec<FeedSpec>,
    trunking: Option<Trunking>,
    settle: fn(&mut S),
    signatures: fn() -> Vec<SignalSignature>,
    trial: Option<fn() -> SignalTrial>,
    tune: Option<fn(&IdentSignal, &mut S) -> f64>,
    signals: &'static [Signal],
    recipes: &'static [Recipe],
    complete: OnceLock<ChannelDescriptor>,
}

fn settled<S>(_: &mut S) {}

fn no_signatures() -> Vec<SignalSignature> {
    Vec::new()
}

fn mono<S>(_: &S) -> u8 {
    1
}

fn no_form() -> Vec<SettingField> {
    Vec::new()
}

fn no_kinds() -> Vec<KindSpec> {
    Vec::new()
}

fn no_feeds() -> Vec<FeedSpec> {
    Vec::new()
}

impl<S> Native<S>
where
    S: Serialize + DeserializeOwned + Default + Send + Sync + 'static,
{
    #[must_use]
    pub fn new<C: NativeChannel + 'static>(band: fn(&S) -> (f64, f64)) -> Self {
        Self {
            base: C::descriptor,
            open: boxed::<C>,
            band,
            filter: |_| Ok(FilterSpec::Passthrough),
            input_rate: None,
            audio_channels: mono::<S>,
            form: no_form,
            home_hz: None,
            kinds: no_kinds,
            feeds: no_feeds,
            trunking: None,
            settle: settled::<S>,
            signatures: no_signatures,
            trial: None,
            tune: None,
            signals: &[],
            recipes: &[],
            complete: OnceLock::new(),
        }
    }

    #[must_use]
    pub fn filter(mut self, filter: fn(&S) -> Result<FilterSpec, ChannelError>) -> Self {
        self.filter = filter;
        self
    }

    #[must_use]
    pub fn input_rate(mut self, rate: fn(&S) -> f64) -> Self {
        self.input_rate = Some(rate);
        self
    }

    #[must_use]
    pub fn audio_channels(mut self, channels: fn(&S) -> u8) -> Self {
        self.audio_channels = channels;
        self
    }

    #[must_use]
    pub fn form(mut self, form: fn() -> Vec<SettingField>) -> Self {
        self.form = form;
        self
    }

    /// The one frequency the service lives on the world over.
    #[must_use]
    pub fn home(mut self, hz: f64) -> Self {
        self.home_hz = Some(hz);
        self
    }

    #[must_use]
    pub fn kinds(mut self, kinds: fn() -> Vec<KindSpec>) -> Self {
        self.kinds = kinds;
        self
    }

    #[must_use]
    pub fn feeds(mut self, feeds: fn() -> Vec<FeedSpec>) -> Self {
        self.feeds = feeds;
        self
    }

    /// Repairs settings that depend on each other, such as a width a new mode does not allow,
    /// before anything reads them.
    #[must_use]
    pub fn settle(mut self, settle: fn(&mut S)) -> Self {
        self.settle = settle;
        self
    }

    #[must_use]
    pub fn trunking(mut self, trunking: Trunking) -> Self {
        self.trunking = Some(trunking);
        self
    }

    /// How the decoder's signal shows on the air, so Signal ID can name it.
    #[must_use]
    pub fn signatures(mut self, signatures: fn() -> Vec<SignalSignature>) -> Self {
        self.signatures = signatures;
        self
    }

    #[must_use]
    pub fn trial(mut self, trial: fn() -> SignalTrial) -> Self {
        self.trial = Some(trial);
        self
    }

    /// Shapes settings to an identified signal and returns how far the channel tunes off the
    /// signal centre.
    #[must_use]
    pub fn tune(mut self, tune: fn(&IdentSignal, &mut S) -> f64) -> Self {
        self.tune = Some(tune);
        self
    }

    /// The test signals this decoder can synthesise.
    #[must_use]
    pub fn synth(mut self, signals: &'static [Signal]) -> Self {
        self.signals = signals;
        self
    }

    /// The reference captures this decoder's fixtures are written from.
    #[must_use]
    pub fn recipes(mut self, recipes: &'static [Recipe]) -> Self {
        self.recipes = recipes;
        self
    }

    fn settings(&self, params: &ChannelParams) -> Result<S, ChannelError> {
        let base = (self.base)();
        if params.type_id() != base.type_id {
            return Err(ChannelError::InvalidSettings(format!(
                "{} decoder got {} settings",
                base.type_id,
                params.type_id()
            )));
        }
        let mut settings: S = params.parse().map_err(ChannelError::InvalidSettings)?;
        (self.settle)(&mut settings);
        Ok(settings)
    }

    fn completed(&self) -> ChannelDescriptor {
        let mut descriptor = (self.base)().clone();
        descriptor.trunking = self.trunking.map(|trunking| (trunking.spec)());
        descriptor.settings = (self.form)();
        descriptor.band = (self.band)(&S::default()).into();
        descriptor.defaults = ChannelParams::new(&descriptor.type_id, &S::default())
            .ok()
            .map(|params| {
                ChannelSettings::new(self.home_hz.unwrap_or(DEFAULT_FREQUENCY_HZ), params)
            });
        descriptor.signatures = (self.signatures)();
        descriptor.identifiable = !descriptor.signatures.is_empty();
        descriptor.trial = self.trial.map(|trial| trial());
        descriptor.ports = sdrmm_wire::patch::channel_ports(&descriptor);
        descriptor
    }
}

impl<S> Decoder for Native<S>
where
    S: Serialize + DeserializeOwned + Default + Send + Sync + 'static,
{
    fn descriptor(&self) -> &ChannelDescriptor {
        self.complete.get_or_init(|| self.completed())
    }

    fn open(
        &self,
        ctx: ChannelCtx,
        settings: &ChannelSettings,
    ) -> Result<Box<dyn Channel>, ChannelError> {
        self.settings(&settings.params)?;
        (self.open)(ctx, settings.clone())
    }

    fn layout(&self, params: &ChannelParams) -> Result<ChannelLayout, ChannelError> {
        let settings = self.settings(params)?;
        Ok(ChannelLayout {
            input_rate_hz: self
                .input_rate
                .map_or((self.base)().input_rate_hz, |rate| rate(&settings)),
            band: (self.band)(&settings),
            audio_channels: (self.audio_channels)(&settings),
        })
    }

    fn filter(&self, params: &ChannelParams) -> Result<FilterSpec, ChannelError> {
        (self.filter)(&self.settings(params)?)
    }

    fn normalize(&self, params: &ChannelParams) -> Result<ChannelParams, ChannelError> {
        ChannelParams::new(params.type_id(), &self.settings(params)?)
            .map_err(ChannelError::InvalidSettings)
    }

    fn kinds(&self) -> Vec<KindSpec> {
        (self.kinds)()
    }

    fn feeds(&self) -> Vec<FeedSpec> {
        (self.feeds)()
    }

    fn trunking(&self) -> Option<Trunking> {
        self.trunking
    }

    fn tune(&self, params: &ChannelParams, signal: &IdentSignal) -> Result<Tuned, ChannelError> {
        let Some(tune) = self.tune else {
            return Ok(Tuned {
                params: params.clone(),
                offset_hz: 0.0,
            });
        };
        let mut settings = self.settings(params)?;
        let offset_hz = tune(signal, &mut settings);
        let params = ChannelParams::new(params.type_id(), &settings)
            .map_err(ChannelError::InvalidSettings)?;
        Ok(Tuned { params, offset_hz })
    }

    fn signals(&self) -> &'static [Signal] {
        self.signals
    }

    fn recipes(&self) -> &'static [Recipe] {
        self.recipes
    }
}

fn boxed<C: NativeChannel + 'static>(
    ctx: ChannelCtx,
    settings: ChannelSettings,
) -> Result<Box<dyn Channel>, ChannelError> {
    Ok(Box::new(C::new(ctx, settings)?))
}

/// The decoders open to new channels right now.
#[derive(Clone, Default)]
pub struct Registry {
    decoders: Vec<Arc<dyn Decoder>>,
}

impl Registry {
    #[must_use]
    pub fn new(decoders: Vec<Arc<dyn Decoder>>) -> Self {
        Self { decoders }
    }

    #[must_use]
    pub fn find(&self, type_id: &str) -> Option<&Arc<dyn Decoder>> {
        self.decoders
            .iter()
            .find(|decoder| decoder.descriptor().type_id == type_id)
    }

    pub fn decoder(&self, type_id: &str) -> Result<&Arc<dyn Decoder>, ChannelError> {
        self.find(type_id)
            .ok_or_else(|| ChannelError::UnknownType(type_id.to_owned()))
    }

    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn Decoder>> {
        self.decoders.iter()
    }

    #[must_use]
    pub fn descriptors(&self) -> Vec<ChannelDescriptor> {
        self.decoders
            .iter()
            .map(|decoder| decoder.descriptor().clone())
            .collect()
    }

    /// Publishes every decoder's event kinds and the plugins' own, so a kind a plugin brings
    /// without a decoder is known too.
    fn publish_kinds(&self) {
        let plugin_kinds = PLUGIN_KINDS
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let kinds = self.decoders.iter().flat_map(|decoder| decoder.kinds());
        let feeds = self.decoders.iter().flat_map(|decoder| decoder.feeds());
        sdrmm_wire::install_catalog(kinds.chain(plugin_kinds), feeds);
    }
}

static BUILT_IN: OnceLock<Vec<Arc<dyn Decoder>>> = OnceLock::new();

static SIGNALS: OnceLock<Catalog> = OnceLock::new();

static PLUGIN_KINDS: RwLock<Vec<KindSpec>> = RwLock::new(Vec::new());

static CURRENT: LazyLock<RwLock<Arc<Registry>>> = LazyLock::new(RwLock::default);

fn built_in() -> &'static [Arc<dyn Decoder>] {
    BUILT_IN.get().map_or(&[], Vec::as_slice)
}

/// Makes `decoders` the built-in decoders and publishes their event kinds. The first call wins;
/// plugins installed before it stay.
pub fn install_built_ins(decoders: fn() -> Vec<Arc<dyn Decoder>>) {
    let mut current = CURRENT.write().unwrap_or_else(PoisonError::into_inner);
    if BUILT_IN.get().is_some() {
        return;
    }
    let built_in = BUILT_IN.get_or_init(decoders);
    let registry = Registry::new(built_in.iter().chain(current.iter()).cloned().collect());
    registry.publish_kinds();
    *current = Arc::new(registry);
}

/// The registry as it stands now.
#[must_use]
pub fn registry() -> Arc<Registry> {
    CURRENT
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// The signals a generator offers: the basic ones, then every built-in decoder's, starting on
/// the first decoder's first. Empty until the built-ins are installed.
#[must_use]
pub fn signal_catalog() -> &'static Catalog {
    if BUILT_IN.get().is_none() {
        return &EMPTY_CATALOG;
    }
    SIGNALS.get_or_init(|| {
        let decoders: Vec<Signal> = built_in()
            .iter()
            .flat_map(|decoder| decoder.signals().iter().copied())
            .collect();
        Catalog {
            default_id: decoders.first().map_or("", |signal| signal.id),
            signals: BASIC.iter().copied().chain(decoders).collect(),
        }
    })
}

/// Every built-in decoder's reference captures, in palette order.
#[must_use]
pub fn recipes() -> Vec<&'static Recipe> {
    built_in()
        .iter()
        .flat_map(|decoder| decoder.recipes().iter())
        .collect()
}

/// The built-in reference capture named `stem`.
#[must_use]
pub fn recipe(stem: &str) -> Option<&'static Recipe> {
    built_in()
        .iter()
        .flat_map(|decoder| decoder.recipes().iter())
        .find(|recipe| recipe.stem == stem)
}

/// Why a plugin decoder of `type_id` may not load beside the built-ins and the plugin types in
/// `taken`, if it may not.
#[must_use]
pub fn plugin_refusal(type_id: &str, taken: &[&str]) -> Option<ChannelError> {
    if built_in()
        .iter()
        .any(|decoder| decoder.descriptor().type_id == type_id)
    {
        Some(ChannelError::BuiltIn(type_id.to_owned()))
    } else if taken.contains(&type_id) {
        Some(ChannelError::LoadedTwice(type_id.to_owned()))
    } else {
        None
    }
}

/// Opens `plugins` beside the built-in decoders, replacing every plugin loaded before, and
/// returns the type ids now on a different decoder than before: their running channels still
/// hold the old one. Refuses the whole set, changing nothing, when a plugin takes a built-in's
/// type id or one another plugin took; [`plugin_refusal`] finds which to leave out first.
pub fn install_plugins(plugins: Vec<Arc<dyn Decoder>>) -> Result<Vec<String>, ChannelError> {
    let mut taken: Vec<&str> = Vec::with_capacity(plugins.len());
    for plugin in &plugins {
        let type_id = plugin.descriptor().type_id.as_str();
        if let Some(refusal) = plugin_refusal(type_id, &taken) {
            return Err(refusal);
        }
        taken.push(type_id);
    }
    let mut current = CURRENT.write().unwrap_or_else(PoisonError::into_inner);
    let replaced = plugins
        .iter()
        .filter(|plugin| {
            current
                .find(&plugin.descriptor().type_id)
                .is_some_and(|old| !Arc::ptr_eq(old, plugin))
        })
        .map(|plugin| plugin.descriptor().type_id.clone())
        .collect();
    let registry = Registry::new(built_in().iter().cloned().chain(plugins).collect());
    registry.publish_kinds();
    *current = Arc::new(registry);
    Ok(replaced)
}

/// Makes `kinds` the event kinds the loaded plugins bring, in place of those loaded before.
pub fn install_plugin_kinds(kinds: Vec<KindSpec>) {
    *PLUGIN_KINDS.write().unwrap_or_else(PoisonError::into_inner) = kinds;
    registry().publish_kinds();
}

/// The trunking of `type_id`, or of the first decoder that trunks when `type_id` is empty.
#[must_use]
pub fn trunking(type_id: &str) -> Option<(String, Trunking)> {
    registry().iter().find_map(|decoder| {
        let id = &decoder.descriptor().type_id;
        let trunking = decoder.trunking()?;
        (type_id.is_empty() || id == type_id).then(|| (id.clone(), trunking))
    })
}

#[must_use]
pub fn descriptors() -> Vec<ChannelDescriptor> {
    registry().descriptors()
}

#[must_use]
pub fn descriptor(type_id: &str) -> Option<ChannelDescriptor> {
    registry()
        .find(type_id)
        .map(|decoder| decoder.descriptor().clone())
}

/// A fresh channel of `type_id`, on its home frequency when it has one.
#[must_use]
pub fn default_settings(type_id: &str) -> Option<ChannelSettings> {
    descriptor(type_id).and_then(|descriptor| descriptor.defaults)
}

pub fn layout(params: &ChannelParams) -> Result<ChannelLayout, ChannelError> {
    registry().decoder(params.type_id())?.layout(params)
}

/// The filter `params` asks for, built at the decoder's input rate.
pub fn channel_filter(params: &ChannelParams) -> Result<ChannelFilter, ChannelError> {
    let registry = registry();
    let decoder = registry.decoder(params.type_id())?;
    let spec = decoder.filter(params)?;
    ChannelFilter::new(&spec, decoder.layout(params)?.input_rate_hz)
}

pub fn tune(params: &ChannelParams, signal: &IdentSignal) -> Result<Tuned, ChannelError> {
    registry().decoder(params.type_id())?.tune(params, signal)
}

pub fn normalize(params: &ChannelParams) -> Result<ChannelParams, ChannelError> {
    registry().decoder(params.type_id())?.normalize(params)
}

/// Every setting checked against the decoder's form.
pub fn check(settings: &ChannelSettings) -> Result<(), ChannelError> {
    let registry = registry();
    let decoder = registry.decoder(settings.params.type_id())?;
    settings
        .check_limits(&decoder.descriptor().settings)
        .map_err(ChannelError::InvalidSettings)?;
    decoder.normalize(&settings.params).map(|_| ())
}

pub fn create(
    ctx: ChannelCtx,
    settings: &ChannelSettings,
) -> Result<Box<dyn Channel>, ChannelError> {
    check(settings)?;
    registry()
        .decoder(settings.params.type_id())?
        .open(ctx, settings)
}

/// Where `params` puts a channel around its frequency, as its decoder lays it out.
pub fn channel_band(params: &ChannelParams) -> Result<ChannelBand, ChannelError> {
    layout(params).map(|layout| layout.band.into())
}

/// The band `params` occupies around the channel frequency; an unknown decoder occupies none.
#[must_use]
pub fn occupied_band(params: &ChannelParams) -> (f64, f64) {
    layout(params).map_or((0.0, 0.0), |layout| layout.band)
}

#[must_use]
pub fn input_rate(params: &ChannelParams) -> f64 {
    layout(params).map_or(0.0, |layout| layout.input_rate_hz)
}

#[must_use]
pub fn audio_channels(params: &ChannelParams) -> u8 {
    layout(params).map_or(1, |layout| layout.audio_channels)
}
