//! Lane meter: each lane's power and its phase against the first, and every lane summed into a
//! lane of its own, the array half of the example plugin. Written like a built-in processor.

use sdrmm_plugin::{
    Complex,
    array::{
        ArrayBlock, ArrayCtx, ArrayProcessor, Params, PluginReading, ProcessorAction,
        ProcessorFaults, ProcessorOutput, ResetCause, Settings,
    },
    decoder::ChannelError,
    defaults,
    wire::{
        ArrayNeeds, Face, NumberSpec, ProcessorFacets, ProcessorPort, ProcessorType, ReadoutSpec,
        SettingField, present::Unit,
    },
};
use serde::{Deserialize, Serialize};

pub const TYPE_ID: &str = "lane_meter";
pub const MAX_LANES: usize = 4;
/// The lane every lane is summed into.
pub const SUM_PORT: &str = "sum";
const SUM: usize = 0;
const POWER_KEYS: [&str; MAX_LANES] = ["power_1", "power_2", "power_3", "power_4"];
const PHASE_KEYS: [&str; MAX_LANES - 1] = ["phase_2", "phase_3", "phase_4"];
const TOO_MANY: &str = "The lane meter shows 4 lanes at most";

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

impl Settings for LaneMeterSettings {
    const TYPE_ID: &'static str = TYPE_ID;

    fn problem(&self) -> Option<&'static str> {
        (self.blocks == 0).then_some("Average out of range")
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
        facets: ProcessorFacets::default(),
        face: Face::fit(400),
        ports: vec![ProcessorPort::Lane {
            name: SUM_PORT.to_owned(),
            note: Some("Every lane summed".to_owned()),
        }],
        band: None,
        resolved_ports: Vec::new(),
    }
}

fn settings(params: &Params) -> Result<&LaneMeterSettings, ChannelError> {
    params
        .get::<LaneMeterSettings>()
        .ok_or_else(|| ChannelError::UnknownType(params.type_id().to_owned()))
}

pub struct LaneMeter {
    settings: LaneMeterSettings,
    lanes: usize,
    power: [f64; MAX_LANES],
    cross: [Complex<f64>; MAX_LANES],
    samples: usize,
    blocks: u32,
    faults: ProcessorFaults,
}

impl LaneMeter {
    fn clear(&mut self) {
        self.power = [0.0; MAX_LANES];
        self.cross = [Complex::new(0.0, 0.0); MAX_LANES];
        self.samples = 0;
        self.blocks = 0;
    }

    fn accumulate(&mut self, block: &ArrayBlock<'_>) {
        let first = block.lanes[0];
        for (index, lane) in block.lanes.iter().take(MAX_LANES).enumerate() {
            for (sample, reference) in lane.iter().zip(first) {
                let sample = Complex::new(f64::from(sample.re), f64::from(sample.im));
                let reference = Complex::new(f64::from(reference.re), f64::from(reference.im));
                self.power[index] += sample.norm_sqr();
                self.cross[index] += sample * reference.conj();
            }
        }
        self.samples += block.len();
        self.blocks += 1;
    }

    fn sum(block: &ArrayBlock<'_>, out: &mut ProcessorOutput<'_>) {
        let Some(mut lane) = out.lane(SUM) else {
            return;
        };
        let scale = 1.0 / block.lanes.len().max(1) as f32;
        for at in 0..block.len() {
            let total: Complex<f32> = block.lanes.iter().map(|lane| lane[at]).sum();
            lane.push(total * scale);
        }
    }

    fn report(&self, count: usize, out: &mut ProcessorOutput<'_>) {
        let Some(reading) = out.report_as::<PluginReading>() else {
            return;
        };
        let samples = self.samples.max(1) as f64;
        let power = self.power.iter().enumerate().map(|(lane, &total)| {
            let power = total / samples;
            if lane < count && power > 0.0 {
                10.0 * power.log10()
            } else {
                f64::NAN
            }
        });
        let phase = self.cross.iter().enumerate().skip(1).map(|(lane, cross)| {
            if lane < count {
                cross.arg().to_degrees()
            } else {
                f64::NAN
            }
        });
        reading.fill(power.chain(phase));
        out.publish_report();
    }
}

impl ArrayProcessor for LaneMeter {
    fn new(ctx: &ArrayCtx<'_>, params: &Params) -> Result<Self, ChannelError> {
        if ctx.lanes > MAX_LANES {
            return Err(ChannelError::Refused(TOO_MANY));
        }
        Ok(Self {
            settings: settings(params)?.clone(),
            lanes: ctx.lanes,
            power: [0.0; MAX_LANES],
            cross: [Complex::new(0.0, 0.0); MAX_LANES],
            samples: 0,
            blocks: 0,
            faults: ProcessorFaults::default(),
        })
    }

    fn apply(&mut self, params: &Params) -> Result<(), ChannelError> {
        self.settings = settings(params)?.clone();
        self.clear();
        Ok(())
    }

    fn retune(&mut self, ctx: &ArrayCtx<'_>) -> Result<(), ChannelError> {
        if ctx.lanes > MAX_LANES {
            return Err(ChannelError::Refused(TOO_MANY));
        }
        self.lanes = ctx.lanes;
        self.clear();
        Ok(())
    }

    fn reset(&mut self, _: ResetCause) {
        self.clear();
    }

    fn action(&mut self, _: ProcessorAction) -> Result<(), ChannelError> {
        Err(ChannelError::Refused("No actions"))
    }

    fn process(&mut self, block: &ArrayBlock<'_>, out: &mut ProcessorOutput<'_>) {
        if block.is_empty() || !self.faults.lanes_match(block, self.lanes) {
            return;
        }
        Self::sum(block, out);
        self.accumulate(block);
        if self.blocks >= self.settings.blocks.max(1) {
            self.report(block.lanes.len(), out);
            self.clear();
        }
    }

    fn faults(&self) -> ProcessorFaults {
        self.faults
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_plugin::{
        Hosted,
        wire::plugin::processor::{ArrayFacts, ArrayOpen, BlockContext},
    };

    use super::*;

    fn opened(lanes: u32, blocks: u32) -> Result<Hosted, String> {
        Hosted::open::<LaneMeter, LaneMeterSettings>(
            &kind(),
            ArrayOpen {
                facts: ArrayFacts {
                    lanes,
                    sample_rate: 48_000.0,
                    max_block: 1_024,
                    ..ArrayFacts::default()
                },
                settings: sdrmm_plugin::serde_json::json!({ "blocks": blocks }),
            },
        )
    }

    fn run(meter: &mut Hosted, phases_deg: &[f64], amplitude: f32, len: usize) -> Vec<f64> {
        let input = meter.input(phases_deg.len(), len);
        for (lane, deg) in input.chunks_exact_mut(len).zip(phases_deg) {
            let shift = Complex::from_polar(amplitude, deg.to_radians() as f32);
            for (n, sample) in lane.iter_mut().enumerate() {
                *sample = Complex::from_polar(1.0f32, n as f32 * 0.1) * shift;
            }
        }
        let context = BlockContext {
            lanes: u32::try_from(phases_deg.len()).unwrap_or_default(),
            ..BlockContext::default()
        };
        meter.context().copy_from_slice(&context.to_bytes());
        let staged = meter.process(len);
        assert_eq!(
            staged.lanes[SUM].len(),
            2 * len,
            "the sum lane carries every sample"
        );
        if staged.header.report == 0 {
            Vec::new()
        } else {
            staged.values.clone()
        }
    }

    #[test]
    fn a_report_carries_each_lanes_power_and_phase() {
        let mut meter = opened(3, 2).expect("opens");
        assert!(run(&mut meter, &[0.0, 90.0, -45.0], 0.5, 256).is_empty());
        let values = run(&mut meter, &[0.0, 90.0, -45.0], 0.5, 256);
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
        assert!(opened(5, 8).is_err());
    }

    #[test]
    fn its_declaration_lists_every_readout_once_and_its_sum_lane() {
        let kind = kind();
        assert_eq!(kind.readouts.len(), 2 * MAX_LANES - 1);
        assert_eq!(kind.defaults["blocks"], 8);
        assert_eq!(kind.lane_ports().collect::<Vec<_>>(), [SUM_PORT]);
    }
}
