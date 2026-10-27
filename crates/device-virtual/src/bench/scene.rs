use sdrmm_dsp::scene::{Draw, LaneImpairments, MAX_BENCH_LANES, Pilot, Scene, all_finite};
use sdrmm_wire::{Coherence, DcArtifact, NoiseSource, StreamScope};

const IMPAIRMENT_SEED: u64 = 0xB1A5_ED00_0000_0005;
const DONGLE2_START_OFFSET: i64 = 480_000;
const KRAKEN_OFFSET_SPAN: f64 = 20_000.0;

#[derive(Clone, Debug, PartialEq)]
pub struct BenchDeviceSpec {
    pub key: String,
    pub label: String,
    pub first_element: usize,
    pub lanes: Vec<LaneImpairments>,
    pub coherence: Coherence,
    pub noise_source: NoiseSource,
    pub noise_from: Option<String>,
    pub pilot: Option<Pilot>,
    pub dc_artifact: DcArtifact,
    pub retune_keeps_phase: bool,
    pub per_stream: StreamScope,
}

impl BenchDeviceSpec {
    pub(crate) fn problem(&self, scene: &Scene) -> Option<String> {
        if self.lanes.is_empty() || self.lanes.len() > MAX_BENCH_LANES {
            return Some(format!("{} needs 1 to {MAX_BENCH_LANES} lanes", self.key));
        }
        let pilot_sound = self
            .pilot
            .is_none_or(|pilot| all_finite(&[pilot.offset_hz, pilot.power_dbfs]));
        if !pilot_sound {
            return Some(format!("{}: the pilot must be finite", self.key));
        }
        let lane_problem = self
            .lanes
            .iter()
            .enumerate()
            .find_map(|(lane, impairments)| {
                impairments
                    .problem()
                    .map(|problem| format!("{} lane {lane}: {problem}", self.key))
            });
        lane_problem.or_else(|| self.coverage(scene))
    }

    pub(crate) fn coverage(&self, scene: &Scene) -> Option<String> {
        let end = self.first_element + self.lanes.len();
        (end > scene.positions.len()).then(|| {
            format!(
                "{} needs elements {} to {}, the scene has {}",
                self.key,
                self.first_element,
                end.saturating_sub(1),
                scene.positions.len()
            )
        })
    }
}

fn kraken_lanes(draw: &mut Draw) -> Vec<LaneImpairments> {
    (0..5)
        .map(|_| LaneImpairments {
            start_offset: draw.within(-KRAKEN_OFFSET_SPAN, KRAKEN_OFFSET_SPAN).round() as i64,
            frac_delay: draw.unit(),
            phase_deg: draw.within(0.0, 360.0),
            gain_db: draw.within(-1.5, 1.5),
            phase_per_db: 0.3,
            scramble_on_retune: true,
            ..LaneImpairments::default()
        })
        .collect()
}

fn array_lanes(draw: &mut Draw) -> Vec<LaneImpairments> {
    (0..4)
        .map(|_| LaneImpairments {
            frac_delay: draw.within(0.0, 0.1),
            phase_deg: draw.within(0.0, 360.0),
            gain_db: draw.within(-1.0, 1.0),
            ..LaneImpairments::default()
        })
        .collect()
}

fn dongle_lane(draw: &mut Draw, start_offset: i64) -> Vec<LaneImpairments> {
    vec![LaneImpairments {
        start_offset,
        frac_delay: draw.unit(),
        phase_deg: draw.within(0.0, 360.0),
        phase_per_db: 0.3,
        scramble_on_retune: true,
        ..LaneImpairments::default()
    }]
}

const BANK_SCOPE: StreamScope = StreamScope {
    tuning: true,
    gain: true,
    antenna: false,
    agc: false,
};

const SHARED_TUNING_SCOPE: StreamScope = StreamScope {
    tuning: false,
    gain: true,
    antenna: false,
    agc: false,
};

#[must_use]
pub fn default_devices() -> Vec<BenchDeviceSpec> {
    let mut draw = Draw::new(IMPAIRMENT_SEED);
    vec![
        BenchDeviceSpec {
            key: "kraken5".to_owned(),
            label: "Kraken bench ×5 (virtual)".to_owned(),
            first_element: 0,
            lanes: kraken_lanes(&mut draw),
            coherence: Coherence::TimeSync,
            noise_source: NoiseSource::Isolated,
            noise_from: None,
            pilot: None,
            dc_artifact: DcArtifact::Managed,
            retune_keeps_phase: false,
            per_stream: BANK_SCOPE,
        },
        BenchDeviceSpec {
            key: "array4".to_owned(),
            label: "Coherent Array ×4 (virtual)".to_owned(),
            first_element: 5,
            lanes: array_lanes(&mut draw),
            coherence: Coherence::PhaseCoherent,
            noise_source: NoiseSource::None,
            noise_from: None,
            pilot: Some(Pilot {
                offset_hz: 100_000.0,
                power_dbfs: -25.0,
            }),
            dc_artifact: DcArtifact::Operator,
            retune_keeps_phase: true,
            per_stream: SHARED_TUNING_SCOPE,
        },
        dongle(
            "dongle1",
            "Dongle 1 (virtual)",
            9,
            dongle_lane(&mut draw, 0),
        ),
        BenchDeviceSpec {
            noise_source: NoiseSource::None,
            noise_from: Some("dongle1".to_owned()),
            ..dongle(
                "dongle2",
                "Dongle 2 (virtual)",
                10,
                dongle_lane(&mut draw, DONGLE2_START_OFFSET),
            )
        },
    ]
}

fn dongle(key: &str, label: &str, element: usize, lanes: Vec<LaneImpairments>) -> BenchDeviceSpec {
    BenchDeviceSpec {
        key: key.to_owned(),
        label: label.to_owned(),
        first_element: element,
        lanes,
        coherence: Coherence::TimeSync,
        noise_source: NoiseSource::Isolated,
        noise_from: None,
        pilot: None,
        dc_artifact: DcArtifact::Managed,
        retune_keeps_phase: false,
        per_stream: StreamScope::default(),
    }
}
