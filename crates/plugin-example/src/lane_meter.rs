//! Lane meter: each lane's power and its phase against the first, the array half of the
//! example plugin.

use sdrmm_plugin::{
    ArrayOutputs, Complex, Lanes, Processor,
    processor::defaults,
    wire::{
        ArrayNeeds, NumberSpec, ProcessorType, ReadoutSpec, SettingField,
        plugin::processor::ArrayFacts, present::Unit,
    },
};
use serde::{Deserialize, Serialize};

pub const TYPE_ID: &str = "lane_meter";
pub const MAX_LANES: usize = 4;
const POWER_KEYS: [&str; MAX_LANES] = ["power_1", "power_2", "power_3", "power_4"];
const PHASE_KEYS: [&str; MAX_LANES - 1] = ["phase_2", "phase_3", "phase_4"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LaneMeterSettings {
    /// Blocks averaged into one report.
    pub blocks: u32,
}

impl Default for LaneMeterSettings {
    fn default() -> Self {
        Self { blocks: 8 }
    }
}

/// The processor as the manifest declares it.
#[must_use]
pub fn kind() -> ProcessorType {
    let power = (0..MAX_LANES)
        .map(|lane| ReadoutSpec::new(POWER_KEYS[lane], &format!("P{}", lane + 1), Unit::Dbfs, 1));
    let phase = (1..MAX_LANES).map(|lane| {
        ReadoutSpec::new(
            PHASE_KEYS[lane - 1],
            &format!("φ{}", lane + 1),
            Unit::Degrees,
            0,
        )
    });
    ProcessorType {
        type_id: TYPE_ID.to_owned(),
        name: "Lane meter".to_owned(),
        summary: "Power and phase of each lane".to_owned(),
        min_lanes: 2,
        max_lanes: MAX_LANES as u32,
        needs: ArrayNeeds {
            time: true,
            ..ArrayNeeds::default()
        },
        settings: vec![
            SettingField::new("blocks", "Average", NumberSpec::new(1.0, 256.0, 1.0))
                .title("Blocks averaged into one report"),
        ],
        readouts: power.chain(phase).collect(),
        emits: Vec::new(),
        defaults: defaults::<LaneMeterSettings>(),
    }
}

pub struct LaneMeter {
    settings: LaneMeterSettings,
    power: [f64; MAX_LANES],
    cross: [Complex<f64>; MAX_LANES],
    samples: usize,
    blocks: u32,
}

impl LaneMeter {
    fn clear(&mut self) {
        self.power = [0.0; MAX_LANES];
        self.cross = [Complex::new(0.0, 0.0); MAX_LANES];
        self.samples = 0;
        self.blocks = 0;
    }

    fn accumulate(&mut self, lanes: Lanes<'_>) {
        let first = lanes.lane(0);
        for (index, lane) in lanes.iter().take(MAX_LANES).enumerate() {
            for (sample, reference) in lane.iter().zip(first) {
                let sample = Complex::new(f64::from(sample.re), f64::from(sample.im));
                let reference = Complex::new(f64::from(reference.re), f64::from(reference.im));
                self.power[index] += sample.norm_sqr();
                self.cross[index] += sample * reference.conj();
            }
        }
        self.samples += lanes.len();
        self.blocks += 1;
    }

    fn report(&self, count: usize, out: &mut ArrayOutputs) {
        let samples = self.samples.max(1) as f64;
        for lane in 0..MAX_LANES {
            let power = self.power[lane] / samples;
            if lane < count && power > 0.0 {
                out.set(lane, 10.0 * power.log10());
            } else {
                out.unset(lane);
            }
        }
        for lane in 1..MAX_LANES {
            let slot = MAX_LANES + lane - 1;
            if lane < count {
                out.set(slot, self.cross[lane].arg().to_degrees());
            } else {
                out.unset(slot);
            }
        }
    }
}

impl Processor for LaneMeter {
    type Settings = LaneMeterSettings;

    fn open(facts: &ArrayFacts, settings: LaneMeterSettings) -> Result<Self, String> {
        if facts.lanes as usize > MAX_LANES {
            return Err(format!("the lane meter shows {MAX_LANES} lanes at most"));
        }
        Ok(Self {
            settings,
            power: [0.0; MAX_LANES],
            cross: [Complex::new(0.0, 0.0); MAX_LANES],
            samples: 0,
            blocks: 0,
        })
    }

    fn apply(&mut self, settings: LaneMeterSettings) -> Result<(), String> {
        self.settings = settings;
        self.clear();
        Ok(())
    }

    fn retune(&mut self, facts: &ArrayFacts) -> Result<(), String> {
        if facts.lanes as usize > MAX_LANES {
            return Err(format!("the lane meter shows {MAX_LANES} lanes at most"));
        }
        self.clear();
        Ok(())
    }

    fn reset(&mut self, _: sdrmm_plugin::processor::Reset) {
        self.clear();
    }

    fn process(&mut self, lanes: Lanes<'_>, out: &mut ArrayOutputs) {
        if lanes.is_empty() {
            return;
        }
        self.accumulate(lanes);
        if self.blocks >= self.settings.blocks.max(1) {
            self.report(lanes.count(), out);
            self.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(lanes: u32) -> ArrayFacts {
        ArrayFacts {
            lanes,
            sample_rate: 48_000.0,
            ..ArrayFacts::default()
        }
    }

    fn block(phases_deg: &[f64], amplitude: f32, len: usize) -> Vec<Complex<f32>> {
        phases_deg
            .iter()
            .flat_map(|deg| {
                let shift = Complex::from_polar(amplitude, deg.to_radians() as f32);
                (0..len).map(move |n| {
                    let tone = Complex::from_polar(1.0f32, n as f32 * 0.1);
                    tone * shift
                })
            })
            .collect()
    }

    #[test]
    fn a_report_carries_each_lanes_power_and_phase() {
        let settings = LaneMeterSettings { blocks: 2 };
        let mut meter = LaneMeter::open(&facts(3), settings).expect("opens");
        let samples = block(&[0.0, 90.0, -45.0], 0.5, 256);
        let mut out = ArrayOutputs::default();
        meter.process(Lanes::new(&samples, 3), &mut out);
        assert!(!out.reports());
        meter.process(Lanes::new(&samples, 3), &mut out);
        assert!(out.reports());
        let values = out.values();
        assert!(
            (values[0] - 20.0 * 0.5f64.log10()).abs() < 0.1,
            "{}",
            values[0]
        );
        assert!(
            (values[MAX_LANES] - 90.0).abs() < 0.5,
            "{}",
            values[MAX_LANES]
        );
        assert!((values[MAX_LANES + 1] + 45.0).abs() < 0.5);
        assert!(values[3].is_nan() && values[MAX_LANES + 2].is_nan());
    }

    #[test]
    fn more_lanes_than_it_shows_are_refused() {
        assert!(LaneMeter::open(&facts(5), LaneMeterSettings::default()).is_err());
    }

    #[test]
    fn its_declaration_lists_every_readout_once() {
        let kind = kind();
        assert_eq!(kind.readouts.len(), 2 * MAX_LANES - 1);
        assert_eq!(kind.defaults["blocks"], 8);
    }
}
