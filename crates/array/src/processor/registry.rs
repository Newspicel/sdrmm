//! Every processor type a node can run, built in or loaded from a plugin. Control-plane state:
//! hosts are built from it, but a running processor only holds what it was given.

use std::sync::{Arc, LazyLock, OnceLock, PoisonError, RwLock};

use sdrmm_decoder::ChannelError;
use sdrmm_wire::{
    ProcessorParams, ProcessorType,
    array::{MAX_ARRAY_LANES, MIN_ARRAY_LANES},
    install_processor_types,
};

use super::{
    ArrayCtx, ArrayProcessor, Execution, Params, ProcessorDescriptor, Reading,
    plugins::{PluginDescriptor, ProcessorFactory},
};

pub type Descriptor = Arc<dyn ProcessorDescriptor>;

type BuiltIns = fn() -> Vec<Descriptor>;

static BUILT_IN: OnceLock<BuiltIns> = OnceLock::new();

static LOADED: LazyLock<RwLock<Vec<Arc<dyn ProcessorFactory>>>> = LazyLock::new(RwLock::default);

static CURRENT: LazyLock<RwLock<Arc<Vec<Descriptor>>>> = LazyLock::new(RwLock::default);

fn built_in() -> Vec<Descriptor> {
    BUILT_IN.get().map_or_else(Vec::new, |all| all())
}

fn loaded() -> Vec<Arc<dyn ProcessorFactory>> {
    LOADED
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// Builds every descriptor afresh, so ports that accept capabilities see every event kind
/// known now, and lists them in the catalog.
fn publish(factories: &[Arc<dyn ProcessorFactory>]) {
    let all: Vec<Descriptor> = built_in()
        .into_iter()
        .chain(
            factories
                .iter()
                .map(|factory| Arc::new(PluginDescriptor::new(Arc::clone(factory))) as Descriptor),
        )
        .collect();
    install_processor_types(
        all.iter()
            .map(|descriptor| descriptor.kind().clone())
            .collect(),
    );
    *CURRENT.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(all);
}

/// Makes `all` the built-in processors and lists them. The first call wins; processors loaded
/// before it stay.
pub fn install_built_ins(all: BuiltIns) {
    if BUILT_IN.set(all).is_ok() {
        publish(&loaded());
    }
}

/// Every processor type open to new nodes right now.
#[must_use]
pub fn descriptors() -> Arc<Vec<Descriptor>> {
    CURRENT
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

#[must_use]
pub fn descriptor(type_id: &str) -> Option<Descriptor> {
    descriptors()
        .iter()
        .find(|descriptor| descriptor.type_id() == type_id)
        .cloned()
}

/// The loaded processor factory of `type_id`.
#[must_use]
pub fn processor_plugin(type_id: &str) -> Option<Arc<dyn ProcessorFactory>> {
    loaded()
        .into_iter()
        .find(|factory| factory.kind().type_id == type_id)
}

/// Why a loaded processor of `kind` may not join the built-ins and the loaded types in `taken`,
/// if it may not.
#[must_use]
pub fn processor_refusal(kind: &ProcessorType, taken: &[&str]) -> Option<ChannelError> {
    let type_id = kind.type_id.as_str();
    if built_in()
        .iter()
        .any(|descriptor| descriptor.type_id() == type_id)
    {
        return Some(ChannelError::BuiltIn(type_id.to_owned()));
    }
    if taken.contains(&type_id) {
        return Some(ChannelError::LoadedTwice(type_id.to_owned()));
    }
    (kind.min_lanes < MIN_ARRAY_LANES
        || kind.max_lanes > MAX_ARRAY_LANES
        || kind.min_lanes > kind.max_lanes)
        .then(|| {
            ChannelError::InvalidSettings(format!(
                "processor type {type_id} takes {MIN_ARRAY_LANES} to {MAX_ARRAY_LANES} lanes"
            ))
        })
}

/// Loads `factories` in place of every one loaded before, and returns the type ids now built
/// by a different factory: their running processors still hold the old one. Refuses the whole
/// set, changing nothing, when [`processor_refusal`] refuses one of them.
pub fn install_plugins(
    factories: Vec<Arc<dyn ProcessorFactory>>,
) -> Result<Vec<String>, ChannelError> {
    let mut taken: Vec<&str> = Vec::with_capacity(factories.len());
    for factory in &factories {
        if let Some(error) = processor_refusal(factory.kind(), &taken) {
            return Err(error);
        }
        taken.push(&factory.kind().type_id);
    }
    let mut loaded = LOADED.write().unwrap_or_else(PoisonError::into_inner);
    let replaced = factories
        .iter()
        .filter(|factory| {
            loaded.iter().any(|old| {
                old.kind().type_id == factory.kind().type_id && !Arc::ptr_eq(old, factory)
            })
        })
        .map(|factory| factory.kind().type_id.clone())
        .collect();
    publish(&factories);
    *loaded = factories;
    Ok(replaced)
}

/// `params` as the settings of the type they name.
pub fn parse_params(params: &ProcessorParams) -> Result<Params, ChannelError> {
    if let Some(problem) = params.problem() {
        return Err(ChannelError::Refused(problem));
    }
    let descriptor = descriptor(params.type_id())
        .ok_or_else(|| ChannelError::UnknownType(params.type_id.clone()))?;
    descriptor
        .parse(params)
        .map_err(ChannelError::InvalidSettings)
}

/// The reading slot a processor of `params` fills, built afresh for every slot.
#[must_use]
pub fn reserved_reading(params: &Params) -> Option<Reading> {
    descriptor(params.type_id())?.reading()
}

pub fn create_processor(
    ctx: &ArrayCtx<'_>,
    params: &Params,
) -> Result<Box<dyn ArrayProcessor>, ChannelError> {
    let type_id = params.type_id();
    let descriptor =
        descriptor(type_id).ok_or_else(|| ChannelError::UnknownType(type_id.to_owned()))?;
    let kind = descriptor.kind();
    let lanes = u32::try_from(ctx.lanes).unwrap_or(u32::MAX);
    if lanes < kind.min_lanes {
        return Err(ChannelError::Refused("Too few elements"));
    }
    if lanes > kind.max_lanes {
        return Err(ChannelError::Refused("Too many elements"));
    }
    if let Some(problem) = params.problem() {
        return Err(ChannelError::Refused(problem));
    }
    if descriptor.execution(params, ctx) == Execution::Dedicated {
        return Err(ChannelError::Unsupported(format!(
            "{type_id} is built by the engine"
        )));
    }
    descriptor.create(ctx, params)
}
