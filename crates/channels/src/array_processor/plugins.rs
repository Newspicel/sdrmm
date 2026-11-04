//! Array processors loaded at run time, such as a plugin's, beside the built-in ones. They all
//! share one descriptor; what differs between them travels in their [`PluginParams`].

use std::sync::{Arc, LazyLock, PoisonError, RwLock};

use sdrmm_wire::{
    ArrayNeeds, PluginParams, ProcessorParams, ProcessorType,
    array::{MAX_ARRAY_LANES, MIN_ARRAY_LANES},
    install_processor_types,
};

use super::{
    ArrayCtx, ArrayProcessor, Execution, ProcessorDescriptor, ProcessorNeeds, TuningNeed,
    no_lane_format, registrations,
};
use crate::ChannelError;

/// Builds the processors of one loaded type.
pub trait ProcessorFactory: Send + Sync {
    fn kind(&self) -> &ProcessorType;

    fn create(
        &self,
        ctx: &ArrayCtx<'_>,
        params: &PluginParams,
    ) -> Result<Box<dyn ArrayProcessor>, ChannelError>;
}

type Loaded = Arc<Vec<Arc<dyn ProcessorFactory>>>;

static LOADED: LazyLock<RwLock<Loaded>> = LazyLock::new(|| RwLock::new(Arc::new(Vec::new())));

pub(super) static DESCRIPTOR: ProcessorDescriptor = ProcessorDescriptor {
    type_id: "plugin",
    name: "Processor",
    min_lanes: MIN_ARRAY_LANES,
    max_lanes: MAX_ARRAY_LANES,
    lane_ports: &[],
    steer_port: None,
    surface: None,
    settings: Vec::new,
    needs,
    band: |_| None,
    tuning: |_| TuningNeed::Any,
    lane_format: no_lane_format,
    execution: |_, ctx| Execution::Worker {
        batch: ctx.max_block,
    },
    in_place: same_type,
};

const fn needs_of(needs: ArrayNeeds) -> ProcessorNeeds {
    ProcessorNeeds {
        time: needs.time,
        phase: needs.phase,
        gain: needs.gain,
        geometry: needs.geometry,
    }
}

fn needs(params: &ProcessorParams) -> ProcessorNeeds {
    match params {
        ProcessorParams::Plugin(plugin) => needs_of(plugin.needs),
        _ => ProcessorNeeds::default(),
    }
}

fn same_type(previous: &ProcessorParams, next: &ProcessorParams) -> bool {
    matches!(
        (previous, next),
        (ProcessorParams::Plugin(a), ProcessorParams::Plugin(b))
            if a.type_id == b.type_id && a.needs == b.needs
    )
}

/// The loaded processor of `type_id`.
#[must_use]
pub fn processor_plugin(type_id: &str) -> Option<Arc<dyn ProcessorFactory>> {
    LOADED
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .find(|factory| factory.kind().type_id == type_id)
        .cloned()
}

fn refusal(kind: &ProcessorType, taken: &[&str]) -> Option<ChannelError> {
    let type_id = kind.type_id.as_str();
    if registrations()
        .iter()
        .any(|entry| entry.descriptor.type_id == type_id)
        || type_id == DESCRIPTOR.type_id
    {
        return Some(ChannelError::InvalidSettings(format!(
            "processor type {type_id} is already a built-in processor"
        )));
    }
    if taken.contains(&type_id) {
        return Some(ChannelError::InvalidSettings(format!(
            "processor type {type_id} is loaded twice"
        )));
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
/// by a different factory: their running processors still hold the old one.
pub fn install_processors(
    factories: Vec<Arc<dyn ProcessorFactory>>,
) -> Result<Vec<String>, ChannelError> {
    let mut taken: Vec<&str> = Vec::with_capacity(factories.len());
    for factory in &factories {
        if let Some(error) = refusal(factory.kind(), &taken) {
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
    install_processor_types(factories.iter().map(|f| f.kind().clone()).collect());
    *loaded = Arc::new(factories);
    Ok(replaced)
}

pub(super) fn create(
    ctx: &ArrayCtx<'_>,
    params: &PluginParams,
) -> Result<Box<dyn ArrayProcessor>, ChannelError> {
    let factory = processor_plugin(&params.type_id)
        .ok_or_else(|| ChannelError::UnknownType(params.type_id.clone()))?;
    let kind = factory.kind();
    let lanes = u32::try_from(ctx.lanes).unwrap_or(u32::MAX);
    if lanes < kind.min_lanes {
        return Err(ChannelError::Refused("Too few elements"));
    }
    if lanes > kind.max_lanes {
        return Err(ChannelError::Refused("Too many elements"));
    }
    factory.create(ctx, params)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Named(ProcessorType);

    impl ProcessorFactory for Named {
        fn kind(&self) -> &ProcessorType {
            &self.0
        }

        fn create(
            &self,
            _: &ArrayCtx<'_>,
            _: &PluginParams,
        ) -> Result<Box<dyn ArrayProcessor>, ChannelError> {
            Err(ChannelError::UnknownType(self.0.type_id.clone()))
        }
    }

    fn named(type_id: &str) -> Arc<dyn ProcessorFactory> {
        Arc::new(Named(ProcessorType {
            type_id: type_id.to_owned(),
            name: type_id.to_owned(),
            summary: String::new(),
            min_lanes: MIN_ARRAY_LANES,
            max_lanes: MAX_ARRAY_LANES,
            needs: ArrayNeeds::default(),
            settings: Vec::new(),
            readouts: Vec::new(),
            emits: Vec::new(),
            defaults: serde_json::Value::Null,
        }))
    }

    #[test]
    fn a_loaded_type_may_not_shadow_a_built_in_or_load_twice() {
        for refused in [
            vec![named("df")],
            vec![named("plugin")],
            vec![named("twin"), named("twin")],
        ] {
            assert!(install_processors(refused).is_err());
        }
        assert!(processor_plugin("twin").is_none());
    }

    #[test]
    fn a_loaded_type_shares_the_plugin_descriptor() {
        let params = ProcessorParams::Plugin(PluginParams {
            type_id: "meter".to_owned(),
            settings: serde_json::Value::Null,
            needs: ArrayNeeds {
                phase: true,
                ..ArrayNeeds::default()
            },
        });
        assert!((DESCRIPTOR.needs)(&params).phase);
        assert!((DESCRIPTOR.in_place)(&params, &params));
        let other = ProcessorParams::Plugin(PluginParams {
            type_id: "other".to_owned(),
            ..PluginParams::default()
        });
        assert!(!(DESCRIPTOR.in_place)(&params, &other));
    }
}
