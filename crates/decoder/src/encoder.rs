//! Every encoder the server can transmit with, built in or loaded as a plugin.
//!
//! Encoders are a registry of their own beside the decoders: a mode that can be sent has an
//! encoder, a mode that can be heard has a decoder, and neither needs the other.

use std::sync::{Arc, LazyLock, OnceLock, PoisonError, RwLock};

use num_complex::Complex;
use sdrmm_dsp::{Limiter, Peak};
use sdrmm_wire::{EncoderDescriptor, EncoderInput, SettingField};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};

mod error;

pub use error::{Carries, EncoderError};

/// How far ahead of a peak a voice encoder's limiter starts to pull its gain down.
const TX_LOOKAHEAD_S: f64 = 1e-3;
const TX_RELEASE_S: f64 = 0.1;

/// Holds a voice encoder's audio, or its envelope, to full scale without the splatter of a
/// hard clip.
pub fn tx_limiter<T: Peak>(rate: f64) -> Limiter<T> {
    Limiter::new(rate, 1.0, TX_LOOKAHEAD_S, TX_RELEASE_S)
}

/// What an encoder is handed to send.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TxPayload {
    Audio(Vec<f32>),
    Frame(Vec<u8>),
}

/// A running encoder: payloads in, baseband at its descriptor's rate out.
pub trait Modulator: Send {
    /// Queues `payload`, refusing what the mode cannot carry or a backlog past its bound.
    fn submit(&mut self, payload: TxPayload) -> Result<(), EncoderError>;

    /// Fills `out` with what is queued and returns how many samples it wrote, fewer once the
    /// queue runs dry.
    fn generate(&mut self, out: &mut [Complex<f32>]) -> usize;
}

/// Queues `payload` on `tx` and plays all of it out, as a signal generator renders one pass.
pub fn play(tx: &mut dyn Modulator, payload: TxPayload) -> Result<Vec<Complex<f32>>, EncoderError> {
    tx.submit(payload)?;
    Ok(burst(tx))
}

/// Everything `tx` has queued, played out.
#[must_use]
pub fn burst(tx: &mut dyn Modulator) -> Vec<Complex<f32>> {
    let mut iq = Vec::new();
    let mut block = vec![Complex::new(0.0, 0.0); 4_096];
    loop {
        let n = tx.generate(&mut block);
        iq.extend_from_slice(&block[..n]);
        if n < block.len() {
            return iq;
        }
    }
}

/// A modulator compiled into the build, opened on its encoder's settings struct.
pub trait NativeModulator: Modulator + Sized + 'static {
    type Settings: Serialize + DeserializeOwned + Default + Send + Sync + 'static;

    /// The baseband rate it produces.
    fn rate_hz() -> f64;

    fn new(settings: &Self::Settings) -> Result<Self, EncoderError>;
}

/// An encoder's settings as a node stores them: its type id and its own settings object.
#[derive(Clone, Debug, PartialEq)]
pub struct EncoderParams {
    pub type_id: String,
    pub settings: Value,
}

impl EncoderParams {
    /// Settings left to the encoder's defaults.
    #[must_use]
    pub fn bare(type_id: impl Into<String>) -> Self {
        Self {
            type_id: type_id.into(),
            settings: Value::Object(Map::new()),
        }
    }

    pub fn new(type_id: impl Into<String>, settings: &impl Serialize) -> Result<Self, String> {
        Ok(Self {
            type_id: type_id.into(),
            settings: serde_json::to_value(settings).map_err(|error| error.to_string())?,
        })
    }

    /// The settings as the encoder's own struct, absent fields taking its defaults.
    fn parse<S: DeserializeOwned>(&self) -> Result<S, EncoderError> {
        let parsed = match &self.settings {
            Value::Null => S::deserialize(&Value::Object(Map::new())),
            settings => S::deserialize(settings),
        };
        parsed.map_err(|error| EncoderError::settings(&self.type_id, error.to_string()))
    }
}

pub trait Encoder: Send + Sync {
    /// The completed descriptor: forms and defaults included.
    fn descriptor(&self) -> &EncoderDescriptor;

    fn open(&self, params: &EncoderParams) -> Result<Box<dyn Modulator>, EncoderError>;

    /// One message, filled into the descriptor's `message` form, as what the modulator takes.
    fn compose(&self, message: &Value) -> Result<TxPayload, EncoderError>;

    /// The width `params` keeps the signal inside.
    fn width_hz(&self, params: &EncoderParams) -> Result<f64, EncoderError>;
}

type Compose = fn(&Value) -> Result<TxPayload, EncoderError>;
type Open<S> = fn(&S) -> Result<Box<dyn Modulator>, EncoderError>;
/// A message encoder's form and how one filled form becomes a payload.
type Messages = (fn() -> Vec<SettingField>, Compose);

/// What names a built-in encoder.
#[derive(Clone, Copy, Debug)]
pub struct EncoderBase {
    pub type_id: &'static str,
    pub name: &'static str,
    pub summary: &'static str,
}

/// An encoder compiled into the build, whose settings are the struct `S`.
pub struct NativeEncoder<S> {
    base: EncoderBase,
    rate_hz: fn() -> f64,
    open: Open<S>,
    width: fn(&S) -> f64,
    form: fn() -> Vec<SettingField>,
    messages: Option<Messages>,
    complete: OnceLock<EncoderDescriptor>,
}

fn no_form() -> Vec<SettingField> {
    Vec::new()
}

fn boxed<M: NativeModulator>(settings: &M::Settings) -> Result<Box<dyn Modulator>, EncoderError> {
    Ok(Box::new(M::new(settings)?))
}

impl<S> NativeEncoder<S>
where
    S: Serialize + DeserializeOwned + Default + Send + Sync + 'static,
{
    /// An encoder modulating an audio wire with `M`, its signal `width` wide.
    #[must_use]
    pub fn new<M: NativeModulator<Settings = S>>(base: EncoderBase, width: fn(&S) -> f64) -> Self {
        Self {
            base,
            rate_hz: M::rate_hz,
            open: boxed::<M>,
            width,
            form: no_form,
            messages: None,
            complete: OnceLock::new(),
        }
    }

    #[must_use]
    pub fn form(mut self, form: fn() -> Vec<SettingField>) -> Self {
        self.form = form;
        self
    }

    /// Sends messages instead of audio: `form` is what the operator fills in and `compose`
    /// turns one filled form into the payload.
    #[must_use]
    pub fn messages(mut self, form: fn() -> Vec<SettingField>, compose: Compose) -> Self {
        self.messages = Some((form, compose));
        self
    }

    fn settings(&self, params: &EncoderParams) -> Result<S, EncoderError> {
        if params.type_id != self.base.type_id {
            return Err(EncoderError::WrongType {
                encoder: self.base.type_id.to_owned(),
                got: params.type_id.clone(),
            });
        }
        params.parse()
    }

    fn completed(&self) -> EncoderDescriptor {
        let defaults = S::default();
        EncoderDescriptor {
            type_id: self.base.type_id.to_owned(),
            name: self.base.name.to_owned(),
            summary: self.base.summary.to_owned(),
            rate_hz: (self.rate_hz)(),
            bandwidth_hz: (self.width)(&defaults),
            input: match self.messages {
                Some(_) => EncoderInput::Message,
                None => EncoderInput::Audio,
            },
            message: self.messages.map_or_default(|(form, _)| form()),
            settings: (self.form)(),
            defaults: serde_json::to_value(&defaults).unwrap_or_else(|_| Value::Object(Map::new())),
        }
    }
}

impl<S> Encoder for NativeEncoder<S>
where
    S: Serialize + DeserializeOwned + Default + Send + Sync + 'static,
{
    fn descriptor(&self) -> &EncoderDescriptor {
        self.complete.get_or_init(|| self.completed())
    }

    fn open(&self, params: &EncoderParams) -> Result<Box<dyn Modulator>, EncoderError> {
        (self.open)(&self.settings(params)?)
    }

    fn compose(&self, message: &Value) -> Result<TxPayload, EncoderError> {
        let (_, compose) = self
            .messages
            .ok_or_else(|| EncoderError::NoMessages(self.base.type_id.to_owned()))?;
        compose(message)
    }

    fn width_hz(&self, params: &EncoderParams) -> Result<f64, EncoderError> {
        Ok((self.width)(&self.settings(params)?))
    }
}

/// The encoders open to new transmitters right now.
#[derive(Clone, Default)]
pub struct EncoderRegistry {
    encoders: Vec<Arc<dyn Encoder>>,
}

impl EncoderRegistry {
    #[must_use]
    pub fn new(encoders: Vec<Arc<dyn Encoder>>) -> Self {
        Self { encoders }
    }

    #[must_use]
    pub fn find(&self, type_id: &str) -> Option<&Arc<dyn Encoder>> {
        self.encoders
            .iter()
            .find(|encoder| encoder.descriptor().type_id == type_id)
    }

    pub fn encoder(&self, type_id: &str) -> Result<&Arc<dyn Encoder>, EncoderError> {
        self.find(type_id)
            .ok_or_else(|| EncoderError::Unknown(type_id.to_owned()))
    }

    #[must_use]
    pub fn descriptors(&self) -> Vec<EncoderDescriptor> {
        self.encoders
            .iter()
            .map(|encoder| encoder.descriptor().clone())
            .collect()
    }
}

static BUILT_IN: OnceLock<Vec<Arc<dyn Encoder>>> = OnceLock::new();

static CURRENT: LazyLock<RwLock<Arc<EncoderRegistry>>> = LazyLock::new(RwLock::default);

fn built_in() -> &'static [Arc<dyn Encoder>] {
    BUILT_IN.get().map_or(&[], Vec::as_slice)
}

/// Makes `encoders` the built-in encoders. The first call wins; plugins installed before it stay.
pub fn install_built_ins(encoders: fn() -> Vec<Arc<dyn Encoder>>) {
    let mut current = CURRENT.write().unwrap_or_else(PoisonError::into_inner);
    if BUILT_IN.get().is_some() {
        return;
    }
    let built_in = BUILT_IN.get_or_init(encoders);
    let encoders = built_in.iter().chain(&current.encoders).cloned().collect();
    *current = Arc::new(EncoderRegistry::new(encoders));
}

/// The encoder registry as it stands now.
#[must_use]
pub fn encoder_registry() -> Arc<EncoderRegistry> {
    CURRENT
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

#[must_use]
pub fn encoders() -> Vec<EncoderDescriptor> {
    encoder_registry().descriptors()
}

pub fn encoder(type_id: &str) -> Result<Arc<dyn Encoder>, EncoderError> {
    encoder_registry().encoder(type_id).cloned()
}

/// Why a plugin encoder of `type_id` may not load beside the built-ins and the plugin encoders
/// in `taken`, if it may not.
#[must_use]
pub fn encoder_refusal(type_id: &str, taken: &[&str]) -> Option<EncoderError> {
    if built_in()
        .iter()
        .any(|encoder| encoder.descriptor().type_id == type_id)
    {
        Some(EncoderError::BuiltIn(type_id.to_owned()))
    } else if taken.contains(&type_id) {
        Some(EncoderError::LoadedTwice(type_id.to_owned()))
    } else {
        None
    }
}

/// Opens `plugins` beside the built-in encoders, replacing every plugin encoder loaded before.
/// Refuses the whole set, changing nothing, when [`encoder_refusal`] refuses one of them.
pub fn install_encoders(plugins: Vec<Arc<dyn Encoder>>) -> Result<(), EncoderError> {
    let mut taken: Vec<&str> = Vec::with_capacity(plugins.len());
    for plugin in &plugins {
        let type_id = plugin.descriptor().type_id.as_str();
        if let Some(refusal) = encoder_refusal(type_id, &taken) {
            return Err(refusal);
        }
        taken.push(type_id);
    }
    let mut current = CURRENT.write().unwrap_or_else(PoisonError::into_inner);
    let registry = EncoderRegistry::new(built_in().iter().cloned().chain(plugins).collect());
    *current = Arc::new(registry);
    Ok(())
}
