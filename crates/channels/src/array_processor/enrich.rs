//! Work on a processor's reports that needs the patch around its node: sites, truth and the
//! events its reports raise. It runs on the control plane, never on the DSP thread.

use jiff::Timestamp;
use sdrmm_wire::{
    DecoderEvent, PositionFix, ProcessorAction, ProcessorParams, ProcessorReading,
    present::TrackFix,
};

/// Where a processor's array stands, as its position input places it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ArrayPlace {
    pub lat: f64,
    pub lon: f64,
    pub altitude_m: Option<f64>,
    pub heading_deg: Option<f64>,
}

/// What an enricher reads of the patch around its node.
pub struct EnrichCtx<'a> {
    pub params: &'a ProcessorParams,
    pub array: Option<ArrayPlace>,
    /// Fixes wired to the processor's position inputs, by port.
    pub positions: &'a [(String, PositionFix)],
}

impl EnrichCtx<'_> {
    #[must_use]
    pub fn position(&self, port: &str) -> Option<&PositionFix> {
        self.positions
            .iter()
            .find(|(wired, _)| wired == port)
            .map(|(_, fix)| fix)
    }
}

/// Why a processor cannot run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    NoArray,
    Refused(String),
}

/// An event an enricher hands its host to publish under the processor's node.
#[derive(Clone, Debug, PartialEq)]
pub struct Raised {
    pub event: DecoderEvent,
    pub at: String,
    pub freq_hz: f64,
}

pub trait ReportEnricher: Send {
    /// Completes `reading` and returns the events it raises, in publishing order.
    fn complete(&mut self, ctx: &EnrichCtx<'_>, reading: &mut ProcessorReading) -> Vec<Raised>;

    /// The reading a processor shows while it cannot run.
    fn refused(&mut self, refusal: &Refusal) -> (ProcessorReading, Vec<Raised>);

    /// The reading a freshly wired processor shows before its first report.
    fn waiting(&mut self, ctx: &EnrichCtx<'_>) -> ProcessorReading;

    /// A decoded track fix routed to the processor's node.
    fn observe(&mut self, _at: Timestamp, _fix: &TrackFix) {}

    /// An action the processor took; `latest` is the reading its node shows now.
    fn act(&mut self, _action: ProcessorAction, _latest: Option<&mut ProcessorReading>) {}

    /// Reports lost before they reached the host.
    fn lagged(&mut self, _count: u64) {}
}

pub type MakeEnricher = fn() -> Box<dyn ReportEnricher>;
