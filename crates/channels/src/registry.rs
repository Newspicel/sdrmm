//! Every decoder the server can open, built in or loaded as a plugin.
//!
//! The registry is control-plane state: channels are created from it and descriptors read from
//! it, but a running channel only holds the `ChannelRx` it was given.

use std::sync::{Arc, LazyLock, OnceLock, PoisonError, RwLock};

use sdrmm_wire::{
    ChannelDescriptor, ChannelParams, ChannelSettings, DEFAULT_FREQUENCY_HZ, FeedSpec, IdentSignal,
    KindSpec, SettingField, SignalSignature, SignalTrial,
};
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelRx, ChannelTx, NativeChannel, NativeTx,
    TxPayload,
};

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

    fn create_rx(
        &self,
        ctx: ChannelCtx,
        settings: &ChannelSettings,
    ) -> Result<Box<dyn ChannelRx>, ChannelError>;

    fn create_tx(
        &self,
        _ctx: ChannelCtx,
        settings: &ChannelSettings,
    ) -> Result<Box<dyn ChannelTx>, ChannelError> {
        Err(ChannelError::NoTransmitter(
            settings.params.type_id().to_owned(),
        ))
    }

    /// One message, filled into the descriptor's `message` form, as what the transmitter takes.
    fn compose(&self, _message: &serde_json::Value) -> Result<TxPayload, ChannelError> {
        Err(ChannelError::InvalidPayload(format!(
            "{} sends no messages",
            self.descriptor().type_id
        )))
    }

    fn layout(&self, params: &ChannelParams) -> Result<ChannelLayout, ChannelError>;

    fn filter(&self, params: &ChannelParams) -> Result<ChannelFilter, ChannelError>;

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
}

/// A channel following one granted call, in `slot` on a protocol whose carriers carry several,
/// deciphering with `keys` and playing through the vocoder chip on the named serial port, when
/// there is one.
pub type Follow = fn(
    slot: Option<u8>,
    ignore_crc: bool,
    keys: &[sdrmm_wire::VoiceKey],
    vocoder: Option<&str>,
) -> Result<ChannelParams, ChannelError>;

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

type CreateRx = fn(ChannelCtx, ChannelSettings) -> Result<Box<dyn ChannelRx>, ChannelError>;
type CreateTx = fn(ChannelCtx, ChannelSettings) -> Result<Box<dyn ChannelTx>, ChannelError>;
type Compose = fn(&serde_json::Value) -> Result<TxPayload, ChannelError>;

/// What a transmitter is fed: an audio wire, or messages the operator fills into a form.
#[derive(Clone, Copy)]
enum TxInput {
    Audio,
    Messages {
        form: fn() -> Vec<SettingField>,
        compose: Compose,
    },
}

/// A decoder compiled into this crate, whose settings are the struct `S`.
pub struct Native<S> {
    base: fn() -> &'static ChannelDescriptor,
    rx: CreateRx,
    tx: Option<(CreateTx, TxInput)>,
    band: fn(&S) -> (f64, f64),
    filter: fn(&S) -> Result<ChannelFilter, ChannelError>,
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

fn passthrough<S>(_: &S) -> Result<ChannelFilter, ChannelError> {
    Ok(ChannelFilter::Passthrough)
}

impl<S> Native<S>
where
    S: Serialize + DeserializeOwned + Default + Send + Sync + 'static,
{
    #[must_use]
    pub fn new<C: NativeChannel + 'static>(band: fn(&S) -> (f64, f64)) -> Self {
        Self {
            base: C::descriptor,
            rx: boxed::<C>,
            tx: None,
            band,
            filter: passthrough::<S>,
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
            complete: OnceLock::new(),
        }
    }

    #[must_use]
    pub fn tx<T: NativeTx + 'static>(mut self) -> Self {
        self.tx = Some((boxed_tx::<T>, TxInput::Audio));
        self
    }

    /// A transmitter that sends messages: `form` is what the operator fills in and `compose`
    /// turns one filled form into the payload.
    #[must_use]
    pub fn tx_messages<T: NativeTx + 'static>(
        mut self,
        form: fn() -> Vec<SettingField>,
        compose: Compose,
    ) -> Self {
        self.tx = Some((boxed_tx::<T>, TxInput::Messages { form, compose }));
        self
    }

    #[must_use]
    pub fn filter(mut self, filter: fn(&S) -> Result<ChannelFilter, ChannelError>) -> Self {
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
        descriptor.can_transmit = self.tx.is_some();
        descriptor.transmits_audio = matches!(self.tx, Some((_, TxInput::Audio)));
        if let Some((_, TxInput::Messages { form, .. })) = self.tx {
            descriptor.message = form();
        }
        descriptor.trunking = self.trunking.map(|trunking| (trunking.spec)());
        descriptor.settings = (self.form)();
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

    fn create_rx(
        &self,
        ctx: ChannelCtx,
        settings: &ChannelSettings,
    ) -> Result<Box<dyn ChannelRx>, ChannelError> {
        self.settings(&settings.params)?;
        (self.rx)(ctx, settings.clone())
    }

    fn create_tx(
        &self,
        ctx: ChannelCtx,
        settings: &ChannelSettings,
    ) -> Result<Box<dyn ChannelTx>, ChannelError> {
        let (create, _) = self
            .tx
            .ok_or_else(|| ChannelError::NoTransmitter(settings.params.type_id().to_owned()))?;
        self.settings(&settings.params)?;
        create(ctx, settings.clone())
    }

    fn compose(&self, message: &serde_json::Value) -> Result<TxPayload, ChannelError> {
        match self.tx {
            Some((_, TxInput::Messages { compose, .. })) => compose(message),
            _ => Err(ChannelError::InvalidPayload(format!(
                "{} sends no messages",
                (self.base)().type_id
            ))),
        }
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

    fn filter(&self, params: &ChannelParams) -> Result<ChannelFilter, ChannelError> {
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
}

fn boxed<C: NativeChannel + 'static>(
    ctx: ChannelCtx,
    settings: ChannelSettings,
) -> Result<Box<dyn ChannelRx>, ChannelError> {
    Ok(Box::new(C::new(ctx, settings)?))
}

fn boxed_tx<C: NativeTx + 'static>(
    ctx: ChannelCtx,
    settings: ChannelSettings,
) -> Result<Box<dyn ChannelTx>, ChannelError> {
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

    fn publish_kinds(&self) {
        let kinds = self.decoders.iter().flat_map(|decoder| decoder.kinds());
        let feeds = self.decoders.iter().flat_map(|decoder| decoder.feeds());
        sdrmm_wire::install_catalog(kinds.collect::<Vec<_>>(), feeds.collect::<Vec<_>>());
    }
}

static BUILT_IN: LazyLock<Vec<Arc<dyn Decoder>>> = LazyLock::new(crate::builtin::decoders);

static CURRENT: LazyLock<RwLock<Arc<Registry>>> = LazyLock::new(|| {
    let registry = Registry::new(BUILT_IN.clone());
    registry.publish_kinds();
    RwLock::new(Arc::new(registry))
});

/// The registry as it stands now.
#[must_use]
pub fn registry() -> Arc<Registry> {
    CURRENT
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// Why a plugin decoder of `type_id` may not load beside the built-ins and the plugin types in
/// `taken`, if it may not.
#[must_use]
pub fn plugin_refusal(type_id: &str, taken: &[&str]) -> Option<ChannelError> {
    if BUILT_IN
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
    let registry = Registry::new(BUILT_IN.iter().cloned().chain(plugins).collect());
    registry.publish_kinds();
    *current = Arc::new(registry);
    Ok(replaced)
}

/// Makes sure the decoders' event kinds are known before anything reads the event catalog.
pub fn init() {
    LazyLock::force(&CURRENT);
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

pub fn channel_filter(params: &ChannelParams) -> Result<ChannelFilter, ChannelError> {
    registry().decoder(params.type_id())?.filter(params)
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
) -> Result<Box<dyn ChannelRx>, ChannelError> {
    check(settings)?;
    registry()
        .decoder(settings.params.type_id())?
        .create_rx(ctx, settings)
}

pub fn create_tx(
    ctx: ChannelCtx,
    settings: &ChannelSettings,
) -> Result<Box<dyn ChannelTx>, ChannelError> {
    registry()
        .decoder(settings.params.type_id())?
        .create_tx(ctx, settings)
}

pub fn compose(type_id: &str, message: &serde_json::Value) -> Result<TxPayload, ChannelError> {
    registry().decoder(type_id)?.compose(message)
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

#[cfg(test)]
mod tests {
    use super::*;

    struct Plugin(ChannelDescriptor);

    impl Decoder for Plugin {
        fn descriptor(&self) -> &ChannelDescriptor {
            &self.0
        }

        fn create_rx(
            &self,
            _: ChannelCtx,
            settings: &ChannelSettings,
        ) -> Result<Box<dyn ChannelRx>, ChannelError> {
            Err(ChannelError::UnknownType(
                settings.params.type_id().to_owned(),
            ))
        }

        fn layout(&self, params: &ChannelParams) -> Result<ChannelLayout, ChannelError> {
            Err(ChannelError::UnknownType(params.type_id().to_owned()))
        }

        fn filter(&self, _: &ChannelParams) -> Result<ChannelFilter, ChannelError> {
            Ok(ChannelFilter::Passthrough)
        }

        fn normalize(&self, params: &ChannelParams) -> Result<ChannelParams, ChannelError> {
            Ok(params.clone())
        }
    }

    fn plugin(type_id: &str) -> Arc<dyn Decoder> {
        Arc::new(Plugin(ChannelDescriptor {
            type_id: type_id.to_owned(),
            ..ChannelDescriptor::default()
        }))
    }

    #[test]
    fn a_plugin_may_not_take_a_built_in_or_a_taken_type() {
        let built_in = BUILT_IN[0].descriptor().type_id.clone();
        assert!(matches!(
            plugin_refusal(&built_in, &[]),
            Some(ChannelError::BuiltIn(id)) if id == built_in
        ));
        assert!(matches!(
            plugin_refusal("twin", &["twin"]),
            Some(ChannelError::LoadedTwice(id)) if id == "twin"
        ));
        assert!(plugin_refusal("twin", &["other"]).is_none());
    }

    #[test]
    fn a_clashing_set_installs_nothing() {
        let built_in = BUILT_IN[0].descriptor().type_id.clone();
        for refused in [
            vec![plugin("clash_ok"), plugin(&built_in)],
            vec![plugin("clash_twin"), plugin("clash_twin")],
        ] {
            assert!(install_plugins(refused).is_err());
        }
        let current = registry();
        assert!(current.find("clash_ok").is_none());
        assert!(current.find("clash_twin").is_none());
        let kept = current.find(&built_in).expect("the built-in stays");
        assert!(Arc::ptr_eq(kept, &BUILT_IN[0]));
    }
}
