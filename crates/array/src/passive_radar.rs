mod assemble;
mod caf;
mod cpi;
mod dab_remod;
mod enrich;
mod form;
mod front;
mod model;
mod plan;
mod present;
mod reference;
mod report;
mod runner;
mod surface;
#[cfg(test)]
mod tests;

use sdrmm_wire::{
    EventKind, Face, ProcessorFacets, ProcessorPort, ProcessorType, RadarUpdate, StreamKind,
    ViewSpec,
};

use crate::{
    Execution, GpuUse, MissionShape, Native, Params, ProcessorAction, ProcessorNeeds, Reading,
    Settings, TuningNeed, kind_of, never_in_place, no_lane_format, parse,
};

pub use assemble::{CpiAssembler, CpiJob};
pub use caf::{
    CafBackend, CafError, CpuCaf, CubeOut, GateChunk, SpectraShard, doppler_stage, gather_stage,
    residual_stage, solve_stage, spectra_stage,
};
pub use cpi::{CpiOutcome, CpiStage};
pub use dab_remod::DabRemod;
pub use front::{FRONT_CHUNK, FrontStage, FrontStats, InLanes, JobSink};
pub use model::*;
pub use plan::{
    AOA_GRID_STEP_DEG, Alphas, AoaPlan, CancellerPlan, DAB_FRAME_LATENCY, FrontPlan, LiveParams,
    MAX_LOOKS, PlanChange, PlanError, RadarCtx, RadarPlan, change, line_axis, params_of, plan,
    plan_for, positions_of,
};
pub use reference::ReferenceCleaner;
pub use report::{
    AoaPod, DetectionPod, RadarCounters, RadarPod, TrackEventPod, TrackPod, axes_of, fill_update,
    track_events,
};
pub use runner::{CafBackendKind, RadarWorker, RunnerPlan};
pub use surface::{RangeDopplerSurface, fill_surface, level};

/// Where the transmitter stands, from a GPS node fixed there.
pub const TX_PORT: &str = "tx";
/// Tracks with altitude, used as truth.
pub const TRUTH_PORT: &str = "truth";

pub fn descriptor() -> Native {
    Native {
        kind: ProcessorType {
            settings: PassiveRadarParams::form(),
            emits: vec![EventKind::RADAR, EventKind::RADAR_SITES],
            facets: ProcessorFacets::new(
                &[
                    ViewSpec::Surface {
                        stream: StreamKind::RangeDoppler,
                    },
                    ViewSpec::Table,
                ],
                &[ProcessorAction::ClearTracks],
            ),
            face: Face::sized(560, 540).min(420, 380),
            ports: vec![
                ProcessorPort::Position {
                    name: TX_PORT.to_owned(),
                    note: Some("GPS node fixed at the transmitter".to_owned()),
                },
                ProcessorPort::Tracks {
                    name: TRUTH_PORT.to_owned(),
                    note: Some("Tracks with altitude used as truth".to_owned()),
                },
                ProcessorPort::Events {
                    note: Some("Track events".to_owned()),
                },
            ],
            ..kind_of::<PassiveRadarParams>("Passive radar", "Aircraft echoes of FM, DAB or DVB-T")
        },
        parse: parse::<PassiveRadarParams>,
        reading: || {
            Some(Reading::new(
                PassiveRadarParams::TYPE_ID,
                RadarUpdate::reserved(),
            ))
        },
        needs: |_| ProcessorNeeds::TIME,
        band: |_| None,
        tuning: |_| TuningNeed::Together,
        lane_format: no_lane_format,
        execution: |_, _| Execution::Dedicated,
        in_place: never_in_place,
        gpu,
        event_bytes: 0,
        enricher: Some(enrich::enricher),
        mission: Some(MissionShape::Radar {
            transmitter: TX_PORT,
        }),
        create: None,
        dedicated: Some(runner::dedicated),
    }
}

fn gpu(params: &Params) -> GpuUse {
    params_of(params).map_or(GpuUse::Off, |radar| radar.gpu)
}
