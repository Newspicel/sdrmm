//! What a processor type declares: its catalog entry and the facets the engine and server ask
//! of it. Shared behaviour is a facet here, never a match on a type id.

use sdrmm_decoder::ChannelError;
use sdrmm_wire::{
    ArrayNeeds, Face, ProcessorFacets, ProcessorParams, ProcessorType, STEER_PORT, StreamKind,
    ViewSpec,
    array::{MAX_ARRAY_LANES, MIN_ARRAY_LANES},
};
use serde::{Deserialize, Serialize};

use std::sync::Arc;

use super::{
    ArrayCtx, ArrayProcessor, Built, DedicatedFn, Execution, LaneFormat, MakeEnricher, Params,
    ProcessorAction, ProcessorNeeds, Reading, RunnerHost, Settings, TuningNeed,
};

const DEFAULT_FACE_WIDTH: u16 = 400;

/// Whether a processor may run its heavy kernels on an accelerator.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuUse {
    #[default]
    Auto,
    Off,
}

sdrmm_wire::labelled! {
    GpuUse {
        Auto => "Auto": "GPU when it is faster",
        Off => "off",
    }
}

/// The mission a phone runs on a processor node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissionShape {
    Bearings,
    Radar { transmitter: &'static str },
}

pub type CreateFn = fn(&ArrayCtx<'_>, &Params) -> Result<Box<dyn ArrayProcessor>, ChannelError>;

/// A processor type, built in or loaded: its catalog entry and every facet it declares.
pub trait ProcessorDescriptor: Send + Sync {
    /// What the catalog lists: name, form, face, ports and views.
    fn kind(&self) -> &ProcessorType;

    /// `params` as this type's own settings.
    fn parse(&self, params: &ProcessorParams) -> Result<Params, String>;

    /// A fresh reading slot with its room reserved, when the type reports.
    fn reading(&self) -> Option<Reading>;

    fn needs(&self, params: &Params) -> ProcessorNeeds;

    /// The band it listens to, as an offset and width around the array centre.
    fn band(&self, params: &Params) -> Option<(f64, f64)>;

    fn tuning(&self, params: &Params) -> TuningNeed;

    fn lane_format(&self, params: &Params, ctx: &ArrayCtx<'_>, port: usize) -> LaneFormat;

    fn execution(&self, params: &Params, ctx: &ArrayCtx<'_>) -> Execution;

    /// Whether a running processor takes `next` without a rebuild.
    fn in_place(&self, previous: &Params, next: &Params) -> bool;

    fn gpu(&self, params: &Params) -> GpuUse;

    /// Room for one block's events as JSON; zero for a type that hands on none.
    fn event_bytes(&self) -> usize;

    /// Control-plane work its reports need from the patch around the node.
    fn enricher(&self) -> Option<MakeEnricher>;

    fn mission(&self) -> Option<MissionShape>;

    fn create(
        &self,
        ctx: &ArrayCtx<'_>,
        params: &Params,
    ) -> Result<Box<dyn ArrayProcessor>, ChannelError>;

    /// The runner of a type whose execution is dedicated, on threads and kernels `host` gives.
    fn dedicated(
        &self,
        _ctx: &ArrayCtx<'_>,
        _params: &Params,
        _host: &Arc<dyn RunnerHost>,
    ) -> Result<Built, ChannelError> {
        Err(ChannelError::Unsupported(format!(
            "{} has no runner",
            self.name()
        )))
    }

    fn type_id(&self) -> &str {
        &self.kind().type_id
    }

    fn name(&self) -> &str {
        &self.kind().name
    }

    /// The lanes it writes, in port order.
    fn lane_ports(&self) -> Vec<&str> {
        self.kind().lane_ports().collect()
    }

    fn steer_port(&self) -> Option<&'static str> {
        self.kind().steers().then_some(STEER_PORT)
    }

    /// The surface it streams, if it draws one.
    fn surface(&self) -> Option<StreamKind> {
        self.kind().facets.views.iter().find_map(|view| match view {
            ViewSpec::Surface { stream } => Some(*stream),
            _ => None,
        })
    }

    /// Whether an operator may send it `action`; any other is refused before the DSP.
    fn takes(&self, action: ProcessorAction) -> bool {
        self.kind()
            .facets
            .actions
            .iter()
            .any(|spec| spec.action == action)
    }
}

/// A built-in processor type: its catalog entry and plain functions for each facet.
pub struct Native {
    pub kind: ProcessorType,
    pub parse: fn(&ProcessorParams) -> Result<Params, String>,
    pub reading: fn() -> Option<Reading>,
    pub needs: fn(&Params) -> ProcessorNeeds,
    pub band: fn(&Params) -> Option<(f64, f64)>,
    pub tuning: fn(&Params) -> TuningNeed,
    pub lane_format: fn(&Params, &ArrayCtx<'_>, usize) -> LaneFormat,
    pub execution: fn(&Params, &ArrayCtx<'_>) -> Execution,
    pub in_place: fn(&Params, &Params) -> bool,
    pub gpu: fn(&Params) -> GpuUse,
    pub event_bytes: usize,
    pub enricher: Option<MakeEnricher>,
    pub mission: Option<MissionShape>,
    pub create: Option<CreateFn>,
    pub dedicated: Option<DedicatedFn>,
}

impl ProcessorDescriptor for Native {
    fn kind(&self) -> &ProcessorType {
        &self.kind
    }

    fn parse(&self, params: &ProcessorParams) -> Result<Params, String> {
        (self.parse)(params)
    }

    fn reading(&self) -> Option<Reading> {
        (self.reading)()
    }

    fn needs(&self, params: &Params) -> ProcessorNeeds {
        (self.needs)(params)
    }

    fn band(&self, params: &Params) -> Option<(f64, f64)> {
        (self.band)(params)
    }

    fn tuning(&self, params: &Params) -> TuningNeed {
        (self.tuning)(params)
    }

    fn lane_format(&self, params: &Params, ctx: &ArrayCtx<'_>, port: usize) -> LaneFormat {
        (self.lane_format)(params, ctx, port)
    }

    fn execution(&self, params: &Params, ctx: &ArrayCtx<'_>) -> Execution {
        (self.execution)(params, ctx)
    }

    fn in_place(&self, previous: &Params, next: &Params) -> bool {
        (self.in_place)(previous, next)
    }

    fn gpu(&self, params: &Params) -> GpuUse {
        (self.gpu)(params)
    }

    fn event_bytes(&self) -> usize {
        self.event_bytes
    }

    fn enricher(&self) -> Option<MakeEnricher> {
        self.enricher
    }

    fn mission(&self) -> Option<MissionShape> {
        self.mission
    }

    fn create(
        &self,
        ctx: &ArrayCtx<'_>,
        params: &Params,
    ) -> Result<Box<dyn ArrayProcessor>, ChannelError> {
        let create = self.create.ok_or_else(|| {
            ChannelError::Unsupported(format!("{} is not built yet", self.kind.name))
        })?;
        create(ctx, params)
    }

    fn dedicated(
        &self,
        ctx: &ArrayCtx<'_>,
        params: &Params,
        host: &Arc<dyn RunnerHost>,
    ) -> Result<Built, ChannelError> {
        let dedicated = self.dedicated.ok_or_else(|| {
            ChannelError::Unsupported(format!("{} has no runner", self.kind.name))
        })?;
        dedicated(ctx, params, (self.gpu)(params), host)
    }
}

/// `params` parsed as `S`, absent fields taking its defaults.
pub fn parse<S: Settings>(params: &ProcessorParams) -> Result<Params, String> {
    params.parse::<S>().map(Params::new)
}

/// The catalog entry of a built-in type whose settings are `S`, before its own facets.
#[must_use]
pub fn kind_of<S: Settings>(name: &str, summary: &str) -> ProcessorType {
    ProcessorType {
        type_id: S::TYPE_ID.to_owned(),
        name: name.to_owned(),
        summary: summary.to_owned(),
        min_lanes: MIN_ARRAY_LANES,
        max_lanes: MAX_ARRAY_LANES,
        needs: ArrayNeeds::default(),
        settings: Vec::new(),
        readouts: Vec::new(),
        emits: Vec::new(),
        defaults: written(&S::default()),
        facets: ProcessorFacets::default(),
        face: Face::fit(DEFAULT_FACE_WIDTH),
        ports: Vec::new(),
        band: None,
        resolved_ports: Vec::new(),
    }
}

/// `value` as JSON reads it back, so an `f32` keeps the short form it is written in.
fn written(value: &impl Serialize) -> serde_json::Value {
    serde_json::to_string(value)
        .and_then(|text| serde_json::from_str(&text))
        .unwrap_or_default()
}

pub fn boxed<P: ArrayProcessor + 'static>(
    ctx: &ArrayCtx<'_>,
    params: &Params,
) -> Result<Box<dyn ArrayProcessor>, ChannelError> {
    Ok(Box::new(P::new(ctx, params)?))
}
