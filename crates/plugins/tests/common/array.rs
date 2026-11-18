//! Runs a plugin processor on blocks of synthetic lanes the way the engine's worker does.

use num_complex::Complex;
use sdrmm_array::{
    ArrayBlock, ArrayCtx, ArrayProcessor, CalView, CorrectionView, EventBytes, LaneBuffer,
    OutputSlots, OutputTally, PluginReading, PluginSettings, Pose, ProcessorFactory,
    ProcessorOutput, Reading, Steer,
};
use sdrmm_decoder::ChannelError;
use sdrmm_wire::{ArrayGeometry, ArrayTuningMode, Coherence, ProcessorType, SurfaceFrame};

pub const RATE_HZ: f64 = 48_000.0;
const EVENT_ROOM: usize = 4_096;

/// One lane per phase, each the same tone shifted by its phase.
pub fn lanes(phases_deg: &[f64], len: usize) -> Vec<Vec<Complex<f32>>> {
    phases_deg
        .iter()
        .map(|deg| {
            let shift = Complex::from_polar(0.5f32, deg.to_radians() as f32);
            (0..len)
                .map(|n| Complex::from_polar(1.0f32, n as f32 * 0.05) * shift)
                .collect()
        })
        .collect()
}

pub struct Rig {
    centers: Vec<f64>,
    positions: Vec<[f64; 3]>,
    geometry: ArrayGeometry,
    pub reading: Reading,
    pub events: EventBytes,
    pub surface: Option<SurfaceFrame>,
    pub lanes: Vec<LaneBuffer>,
    pub steer: Option<Steer>,
}

impl Rig {
    pub fn new(kind: &ProcessorType, lanes: usize) -> Self {
        Self {
            centers: vec![100e6; lanes],
            positions: (0..lanes)
                .map(|lane| [lane as f64 * 0.5, 0.0, 0.0])
                .collect(),
            geometry: ArrayGeometry::default(),
            reading: Reading::new(&kind.type_id, PluginReading::reserved(kind)),
            events: EventBytes::with_capacity(EVENT_ROOM),
            surface: None,
            lanes: kind.lane_ports().map(|_| LaneBuffer::new(4_096)).collect(),
            steer: None,
        }
    }

    pub fn ctx(&self) -> ArrayCtx<'_> {
        ArrayCtx {
            node: "processor",
            lanes: self.centers.len(),
            sample_rate: RATE_HZ,
            center_hz: self.centers[0],
            lane_centers_hz: &self.centers,
            geometry: &self.geometry,
            positions_m: &self.positions,
            manifold: None,
            tier: Coherence::TimeSync,
            tuning: ArrayTuningMode::Together,
            max_block: 1_024,
        }
    }

    pub fn open(
        &self,
        factory: &dyn ProcessorFactory,
        settings: serde_json::Value,
    ) -> Result<Box<dyn ArrayProcessor>, ChannelError> {
        factory.create(&self.ctx(), &PluginSettings { settings })
    }

    pub fn run(
        &mut self,
        processor: &mut dyn ArrayProcessor,
        lanes: &[Vec<Complex<f32>>],
    ) -> OutputTally {
        let view: Vec<&[Complex<f32>]> = lanes.iter().map(Vec::as_slice).collect();
        let block = ArrayBlock {
            lanes: &view,
            corrected: false,
            correction: CorrectionView::identity(),
            first_index: 0,
            unix_ns: 0,
            generation: 0,
            gap_before: false,
            centers_hz: &self.centers,
            cal: CalView::default(),
            pose: Pose::default(),
        };
        for lane in &mut self.lanes {
            lane.clear();
        }
        let mut out = ProcessorOutput::new(OutputSlots {
            report: Some(&mut self.reading),
            surface: self.surface.as_mut(),
            bearings: &mut [],
            lanes: &mut self.lanes,
        })
        .with_events(Some(&mut self.events));
        processor.process(&block, &mut out);
        let tally = out.tally();
        self.steer = tally.steer;
        tally
    }

    pub fn plugin_reading(&self) -> &PluginReading {
        match self.reading.get::<PluginReading>() {
            Some(reading) => reading,
            None => panic!("a plugin reading, got {}", self.reading.type_id()),
        }
    }

    pub fn value(&self, key: &str) -> Option<f64> {
        self.plugin_reading()
            .readouts
            .iter()
            .find(|readout| readout.key == key)
            .and_then(|readout| readout.value)
    }
}
