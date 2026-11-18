//! A block and a steer as the plugin ABI carries them, both ways, so the host and the plugin
//! SDK read and write the same layout.

use num_complex::Complex;
use sdrmm_wire::{
    CalPhase, CalSourceKind,
    plugin::processor::{
        ArrayHeader, BlockCal, BlockContext, BlockLane, BlockPose, BlockSteer, CONTEXT_LANES,
        STEER_OTHERS,
    },
};

use super::{ArrayBlock, CalView, CorrectionView, GeoFix, Pose, Steer};

const SOURCES: [CalSourceKind; 3] = [
    CalSourceKind::Noise,
    CalSourceKind::Pilot,
    CalSourceKind::Emitter,
];

const NONE: u32 = u32::MAX;

fn known(value: f64) -> Option<f64> {
    (!value.is_nan()).then_some(value)
}

fn known_f32(value: f32) -> Option<f32> {
    (!value.is_nan()).then_some(value)
}

fn code_of<T: PartialEq>(all: &[T], value: &T) -> u32 {
    all.iter()
        .position(|candidate| candidate == value)
        .and_then(|index| u32::try_from(index).ok())
        .unwrap_or(NONE)
}

fn cal_of(cal: &CalView) -> BlockCal {
    let (valid_low_hz, valid_high_hz) = cal.valid_hz.unwrap_or((f64::NAN, f64::NAN));
    BlockCal {
        phase: code_of(&CalPhase::ALL, &cal.phase),
        source: cal.source.map_or(NONE, |source| code_of(&SOURCES, &source)),
        phase_ready: cal.phase_ready,
        gain_ready: cal.gain_ready,
        generation: cal.generation,
        phase_sigma_deg: cal.phase_sigma_deg,
        gain_sigma_db: cal.gain_sigma_db,
        valid_low_hz,
        valid_high_hz,
    }
}

fn pose_of(pose: &Pose) -> BlockPose {
    let fix = pose.fix.unwrap_or(GeoFix {
        lat: f64::NAN,
        lon: f64::NAN,
        ..GeoFix::default()
    });
    BlockPose {
        heading_deg: pose.heading_deg.unwrap_or(f64::NAN),
        heading_sigma_deg: pose.heading_sigma_deg,
        yaw_rate_dps: pose.yaw_rate_dps.unwrap_or(f32::NAN),
        lat: fix.lat,
        lon: fix.lon,
        altitude_m: fix.altitude_m.unwrap_or(f64::NAN),
        accuracy_m: fix.accuracy_m.unwrap_or(f32::NAN),
        speed_mps: fix.speed_mps.unwrap_or(f32::NAN),
        moving: pose.moving,
        follows: pose.follows,
    }
}

fn steer_of(steer: Option<&Steer>) -> BlockSteer {
    let Some(steer) = steer else {
        return BlockSteer::default();
    };
    BlockSteer {
        present: true,
        same_array: steer.same_array,
        relative_deg: steer.relative_deg,
        true_deg: steer.true_deg.unwrap_or(f64::NAN),
        elevation_deg: steer.elevation_deg,
        sigma_deg: steer.sigma_deg,
        others: u32::from(steer.others),
        others_relative_deg: steer.others_relative_deg,
        others_true_deg: steer.others_true_deg.map(|deg| deg.unwrap_or(f64::NAN)),
        wall_ms: steer.wall_ms,
    }
}

/// `block` as a plugin reads it, with the newest `steer` and each lane's correction at
/// `offset_hz` from the array centre.
#[must_use]
pub fn block_context(
    block: &ArrayBlock<'_>,
    steer: Option<&Steer>,
    offset_hz: f64,
) -> BlockContext {
    let mut lane = [BlockLane::default(); CONTEXT_LANES];
    let lanes = block.lanes.len().min(CONTEXT_LANES);
    for (index, slot) in lane.iter_mut().enumerate().take(lanes) {
        let correction = block.correction.response(index, offset_hz);
        *slot = BlockLane {
            center_hz: block.centers_hz.get(index).copied().unwrap_or_default(),
            correction: [correction.re, correction.im],
        };
    }
    BlockContext {
        first_index: block.first_index,
        unix_ns: block.unix_ns,
        generation: block.generation,
        gap_before: block.gap_before,
        corrected: block.corrected,
        correction_generation: block.correction.generation,
        cal: cal_of(&block.cal),
        pose: pose_of(&block.pose),
        steer: steer_of(steer),
        lanes: u32::try_from(lanes).unwrap_or_default(),
        lane,
    }
}

/// What a plugin keeps of each block's context, so it builds an [`ArrayBlock`] without
/// allocating once its room is made.
#[derive(Debug)]
pub struct BlockParts {
    pub first_index: u64,
    pub unix_ns: u64,
    pub generation: u32,
    pub gap_before: bool,
    pub corrected: bool,
    pub correction_generation: u32,
    pub centers_hz: Vec<f64>,
    /// One correction per lane, each a single-bin spectrum relative to lane 0.
    pub corrections: Vec<Vec<Complex<f32>>>,
    pub cal: CalView,
    pub pose: Pose,
    pub steer: Option<Steer>,
}

impl Default for BlockParts {
    fn default() -> Self {
        Self {
            first_index: 0,
            unix_ns: 0,
            generation: 0,
            gap_before: false,
            corrected: false,
            correction_generation: 0,
            centers_hz: Vec::with_capacity(CONTEXT_LANES),
            corrections: (0..CONTEXT_LANES)
                .map(|_| vec![Complex::new(1.0, 0.0)])
                .collect(),
            cal: CalView::default(),
            pose: Pose::default(),
            steer: None,
        }
    }
}

impl BlockParts {
    /// Takes `context` in place of what it held.
    pub fn read(&mut self, context: &BlockContext) {
        self.first_index = context.first_index;
        self.unix_ns = context.unix_ns;
        self.generation = context.generation;
        self.gap_before = context.gap_before;
        self.corrected = context.corrected;
        self.correction_generation = context.correction_generation;
        let lanes = (context.lanes as usize).min(CONTEXT_LANES);
        self.centers_hz.clear();
        for (lane, spectrum) in context.lane.iter().zip(&mut self.corrections).take(lanes) {
            self.centers_hz.push(lane.center_hz);
            if let Some(bin) = spectrum.first_mut() {
                *bin = Complex::new(lane.correction[0], lane.correction[1]);
            }
        }
        self.cal = read_cal(&context.cal);
        self.pose = read_pose(&context.pose);
        self.steer = read_steer(&context.steer);
    }

    /// The block of `lanes` these parts describe, at `sample_rate`.
    #[must_use]
    pub fn block<'a>(
        &'a self,
        lanes: &'a [&'a [Complex<f32>]],
        sample_rate: f64,
    ) -> ArrayBlock<'a> {
        let count = lanes.len().min(self.corrections.len());
        ArrayBlock {
            lanes,
            corrected: self.corrected,
            correction: CorrectionView::new(
                self.correction_generation,
                sample_rate,
                &self.corrections[..count],
            ),
            first_index: self.first_index,
            unix_ns: self.unix_ns,
            generation: self.generation,
            gap_before: self.gap_before,
            centers_hz: &self.centers_hz,
            cal: self.cal,
            pose: self.pose,
        }
    }
}

fn read_cal(cal: &BlockCal) -> CalView {
    let valid_hz = known(cal.valid_low_hz).zip(known(cal.valid_high_hz));
    CalView {
        phase: CalPhase::ALL
            .get(cal.phase as usize)
            .copied()
            .unwrap_or_default(),
        source: SOURCES.get(cal.source as usize).copied(),
        phase_ready: cal.phase_ready,
        gain_ready: cal.gain_ready,
        generation: cal.generation,
        phase_sigma_deg: cal.phase_sigma_deg,
        gain_sigma_db: cal.gain_sigma_db,
        valid_hz,
    }
}

fn read_pose(pose: &BlockPose) -> Pose {
    let fix = known(pose.lat)
        .zip(known(pose.lon))
        .map(|(lat, lon)| GeoFix {
            lat,
            lon,
            altitude_m: known(pose.altitude_m),
            accuracy_m: known_f32(pose.accuracy_m),
            speed_mps: known_f32(pose.speed_mps),
        });
    Pose {
        heading_deg: known(pose.heading_deg),
        heading_sigma_deg: pose.heading_sigma_deg,
        yaw_rate_dps: known_f32(pose.yaw_rate_dps),
        fix,
        moving: pose.moving,
        follows: pose.follows,
    }
}

fn read_steer(steer: &BlockSteer) -> Option<Steer> {
    steer.present.then(|| Steer {
        same_array: steer.same_array,
        relative_deg: steer.relative_deg,
        true_deg: known(steer.true_deg),
        elevation_deg: steer.elevation_deg,
        sigma_deg: steer.sigma_deg,
        others_relative_deg: steer.others_relative_deg,
        others_true_deg: steer.others_true_deg.map(known),
        others: u8::try_from(steer.others).unwrap_or(u8::MAX),
        wall_ms: steer.wall_ms,
    })
}

/// Writes `steer` into the steer words of `header`.
pub fn write_steer(header: &mut ArrayHeader, steer: Option<&Steer>) {
    let Some(steer) = steer else {
        header.steer = 0;
        return;
    };
    header.steer = 1;
    header.steer_same_array = u32::from(steer.same_array);
    header.steer_relative_deg = steer.relative_deg as f32;
    header.steer_true_deg = steer.true_deg.map_or(f32::NAN, |deg| deg as f32);
    header.steer_elevation_deg = steer.elevation_deg as f32;
    header.steer_sigma_deg = steer.sigma_deg;
    header.steer_others = u32::from(steer.others);
    header.steer_others_relative_deg = steer.others_relative_deg.map(|deg| deg as f32);
    header.steer_others_true_deg = steer
        .others_true_deg
        .map(|deg| deg.map_or(f32::NAN, |deg| deg as f32));
}

/// The steer `header` carries, if it carries one.
#[must_use]
pub fn steer_of_header(header: &ArrayHeader, wall_ms: u64) -> Option<Steer> {
    (header.steer != 0).then(|| {
        let mut others_relative_deg = [0.0; STEER_OTHERS];
        let mut others_true_deg = [None; STEER_OTHERS];
        for (index, deg) in header.steer_others_relative_deg.iter().enumerate() {
            others_relative_deg[index] = f64::from(*deg);
            others_true_deg[index] = known_f32(header.steer_others_true_deg[index]).map(f64::from);
        }
        Steer {
            same_array: header.steer_same_array != 0,
            relative_deg: f64::from(header.steer_relative_deg),
            true_deg: known_f32(header.steer_true_deg).map(f64::from),
            elevation_deg: f64::from(header.steer_elevation_deg),
            sigma_deg: header.steer_sigma_deg,
            others_relative_deg,
            others_true_deg,
            others: u8::try_from(header.steer_others).unwrap_or(u8::MAX),
            wall_ms,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_reads_back_as_the_plugin_sees_it() {
        let samples = [Complex::new(1.0f32, 0.0); 8];
        let lanes: [&[Complex<f32>]; 2] = [&samples, &samples];
        let spectra = vec![
            vec![Complex::new(1.0f32, 0.0); 4],
            vec![Complex::from_polar(0.5f32, 1.0); 4],
        ];
        let block = ArrayBlock {
            lanes: &lanes,
            corrected: false,
            correction: CorrectionView::new(4, 48_000.0, &spectra),
            first_index: 99,
            unix_ns: 5,
            generation: 2,
            gap_before: true,
            centers_hz: &[100e6, 100.2e6],
            cal: CalView {
                phase: CalPhase::Solved,
                source: Some(CalSourceKind::Pilot),
                phase_ready: true,
                valid_hz: Some((1.0, 2.0)),
                ..CalView::default()
            },
            pose: Pose {
                heading_deg: Some(45.0),
                fix: Some(GeoFix {
                    lat: 52.0,
                    lon: 13.0,
                    ..GeoFix::default()
                }),
                ..Pose::default()
            },
        };
        let steer = Steer {
            relative_deg: 30.0,
            true_deg: Some(75.0),
            others: 1,
            others_relative_deg: [10.0, 0.0, 0.0],
            ..Steer::default()
        };
        let context =
            BlockContext::from_bytes(&block_context(&block, Some(&steer), 0.0).to_bytes());
        let mut parts = BlockParts::default();
        parts.read(&context);
        let read = parts.block(&lanes, 48_000.0);
        assert_eq!(read.first_index, 99);
        assert!(read.gap_before);
        assert_eq!(read.centers_hz, block.centers_hz);
        assert_eq!(read.cal, block.cal);
        assert_eq!(read.pose, block.pose);
        assert_eq!(parts.steer, Some(steer));
        let lane = read.correction.response(1, 0.0);
        assert!((lane.norm() - 0.5).abs() < 1e-5 && (lane.arg() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_steer_reads_back_from_the_header() {
        let steer = Steer {
            relative_deg: 12.0,
            true_deg: None,
            sigma_deg: 2.0,
            others: 2,
            others_relative_deg: [1.0, 2.0, 0.0],
            others_true_deg: [Some(3.0), None, None],
            wall_ms: 7,
            ..Steer::default()
        };
        let mut header = ArrayHeader::default();
        write_steer(&mut header, Some(&steer));
        assert_eq!(steer_of_header(&header, 7), Some(steer));
        write_steer(&mut header, None);
        assert_eq!(steer_of_header(&header, 7), None);
    }
}
