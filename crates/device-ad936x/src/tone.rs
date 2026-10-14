use sdrmm_device::DeviceError;
use sdrmm_wire::{DeviceSettings, ExtraSetting, Range};

use crate::{
    apply::Write,
    iio::{Client, Context, Direction},
};

pub(crate) const TONES: usize = 2;

const FREQUENCY: &str = "frequency";
const PHASE: &str = "phase";
const SCALE: &str = "scale";
const RAW: &str = "raw";
const GROUP: &str = "Tone";

const LEVEL: Range = Range {
    min: -60.0,
    max: 0.0,
    step: Some(1.0),
};

const COSINE_MILLIDEGREES: &str = "90000";
const SINE_MILLIDEGREES: &str = "0";
const NEGATIVE_SINE_MILLIDEGREES: &str = "180000";
const SCALE_SLACK: f64 = 1e-6;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Pair {
    i: String,
    q: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Tones {
    device: String,
    lanes: Vec<[Pair; TONES]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Tone {
    pub(crate) on: bool,
    pub(crate) hz: f64,
    pub(crate) dbfs: f64,
}

impl Tone {
    fn scale(self) -> f64 {
        if self.on {
            10f64.powf(self.dbfs / 20.0)
        } else {
            0.0
        }
    }
}

pub(crate) fn on_name(tone: usize) -> String {
    format!("tone_{}", tone + 1)
}

pub(crate) fn hz_name(tone: usize) -> String {
    format!("tone_{}_hz", tone + 1)
}

pub(crate) fn level_name(tone: usize) -> String {
    format!("tone_{}_dbfs", tone + 1)
}

pub(crate) fn owns(name: &str) -> bool {
    (0..TONES)
        .any(|tone| name == on_name(tone) || name == hz_name(tone) || name == level_name(tone))
}

impl Tones {
    pub(crate) fn read(context: &Context, device: &str) -> Option<Self> {
        let dds = context.device(device)?;
        let named = |name: String| {
            dds.channels
                .iter()
                .find(|channel| channel.output && channel.name.as_deref() == Some(name.as_str()))
                .map(|channel| channel.id.clone())
        };
        let mut lanes = Vec::new();
        for lane in 1.. {
            let pair = |tone: usize| -> Option<Pair> {
                Some(Pair {
                    i: named(format!("TX{lane}_I_F{}", tone + 1))?,
                    q: named(format!("TX{lane}_Q_F{}", tone + 1))?,
                })
            };
            let (Some(first), Some(second)) = (pair(0), pair(1)) else {
                break;
            };
            lanes.push([first, second]);
        }
        (!lanes.is_empty()).then(|| Self {
            device: device.to_string(),
            lanes,
        })
    }

    pub(crate) fn settings(&self, max_rate: f64) -> Vec<ExtraSetting> {
        let reach = Range {
            min: -(max_rate / 2.0).floor(),
            max: (max_rate / 2.0).floor(),
            step: None,
        };
        (0..TONES)
            .flat_map(|tone| {
                let label = format!("Tone {}", tone + 1);
                [
                    ExtraSetting::bool(on_name(tone), label.clone(), false),
                    ExtraSetting::range(hz_name(tone), format!("{label} offset"), reach, "Hz"),
                    ExtraSetting::range(level_name(tone), format!("{label} level"), LEVEL, "dBFS"),
                ]
            })
            .map(|setting| setting.in_group(GROUP))
            .collect()
    }

    pub(crate) fn plan(
        &self,
        delta: &DeviceSettings,
        next: &DeviceSettings,
        writes: &mut Vec<Write>,
    ) -> Result<(), DeviceError> {
        let touched = delta.extra.iter().any(|extra| owns(&extra.name));
        let tones = wanted(next);
        let retimed = delta.sample_rate.is_some() && tones.iter().any(|tone| tone.on);
        if !touched && !retimed {
            return Ok(());
        }
        check(&tones, next.sample_rate)?;
        for lane in &self.lanes {
            for (pair, tone) in lane.iter().zip(tones) {
                self.write(pair, tone, writes);
            }
        }
        Ok(())
    }

    fn write(&self, pair: &Pair, tone: Tone, writes: &mut Vec<Write>) {
        let attr = |channel: &str, attr: &str, value: String| Write::Attr {
            device: self.device.clone(),
            output: true,
            channel: channel.to_string(),
            attr: attr.to_string(),
            value,
        };
        let hz = format!("{:.0}", tone.hz.abs().round());
        let q_phase = if tone.hz < 0.0 {
            NEGATIVE_SINE_MILLIDEGREES
        } else {
            SINE_MILLIDEGREES
        };
        let scale = format!("{:.6}", tone.scale());
        if !tone.on {
            writes.extend([
                attr(&pair.i, SCALE, scale.clone()),
                attr(&pair.q, SCALE, scale),
            ]);
            return;
        }
        writes.extend([
            attr(&pair.i, FREQUENCY, hz.clone()),
            attr(&pair.q, FREQUENCY, hz),
            attr(&pair.i, PHASE, COSINE_MILLIDEGREES.to_string()),
            attr(&pair.q, PHASE, q_phase.to_string()),
            attr(&pair.i, RAW, "1".to_string()),
            attr(&pair.q, RAW, "1".to_string()),
            attr(&pair.i, SCALE, scale.clone()),
            attr(&pair.q, SCALE, scale),
        ]);
    }

    pub(crate) fn read_back(&self, client: &Client, name: &str) -> Option<serde_json::Value> {
        let tone = (0..TONES).find(|tone| owns_tone(name, *tone))?;
        let pair = &self.lanes.first()?[tone];
        let read = |channel: &str, attr: &str| -> Option<f64> {
            client
                .read_channel_attr(&self.device, Direction::Out, channel, attr)
                .inspect_err(|e| tracing::debug!("{}.{channel}.{attr}: {e}", self.device))
                .ok()?
                .trim()
                .parse()
                .ok()
        };
        let scale = read(&pair.i, SCALE)?;
        Some(if name == on_name(tone) {
            serde_json::Value::Bool(scale > 0.0)
        } else if name == level_name(tone) {
            let dbfs = if scale > 0.0 {
                (20.0 * scale.log10()).round()
            } else {
                LEVEL.min
            };
            serde_json::Value::from(dbfs.clamp(LEVEL.min, LEVEL.max))
        } else {
            let hz = read(&pair.i, FREQUENCY)?;
            let negative = read(&pair.q, PHASE).is_some_and(|phase| phase > 90_000.0);
            serde_json::Value::from(if negative { -hz } else { hz })
        })
    }
}

fn owns_tone(name: &str, tone: usize) -> bool {
    name == on_name(tone) || name == hz_name(tone) || name == level_name(tone)
}

pub(crate) fn wanted(settings: &DeviceSettings) -> [Tone; TONES] {
    let value = |name: String| {
        settings
            .extra
            .iter()
            .find(|extra| extra.name == name)
            .map(|extra| &extra.value)
    };
    std::array::from_fn(|tone| Tone {
        on: value(on_name(tone))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        hz: value(hz_name(tone))
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0),
        dbfs: value(level_name(tone))
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(LEVEL.min),
    })
}

pub(crate) fn any_on(settings: &DeviceSettings) -> bool {
    wanted(settings).iter().any(|tone| tone.on)
}

fn check(tones: &[Tone; TONES], rate: Option<f64>) -> Result<(), DeviceError> {
    for (index, tone) in tones.iter().enumerate() {
        if !tone.hz.is_finite() || !LEVEL.holds(tone.dbfs) {
            return Err(DeviceError::Unsupported(format!(
                "tone {} takes a level from {} to {} dBFS",
                index + 1,
                LEVEL.min,
                LEVEL.max
            )));
        }
        if let Some(rate) = rate.filter(|_| tone.on)
            && tone.hz.abs() > rate / 2.0
        {
            return Err(DeviceError::Unsupported(format!(
                "tone {} at {} Hz is outside the ±{:.0} Hz this rate reaches",
                index + 1,
                tone.hz,
                rate / 2.0
            )));
        }
    }
    let total: f64 = tones.iter().map(|tone| tone.scale()).sum();
    if total > 1.0 + SCALE_SLACK {
        return Err(DeviceError::Unsupported(
            "the tones together are above full scale; lower one of them".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::ExtraValue;
    use serde_json::json;

    use super::*;

    fn tones() -> Tones {
        let pair = |lane: usize, tone: usize| Pair {
            i: format!("altvoltage{}", lane * 4 + tone),
            q: format!("altvoltage{}", lane * 4 + tone + 2),
        };
        Tones {
            device: "cf-ad9361-dds-core-lpc".to_string(),
            lanes: vec![[pair(0, 0), pair(0, 1)], [pair(1, 0), pair(1, 1)]],
        }
    }

    fn settings(values: &[(String, serde_json::Value)]) -> DeviceSettings {
        DeviceSettings {
            sample_rate: Some(10e6),
            extra: values
                .iter()
                .map(|(name, value)| ExtraValue {
                    name: name.clone(),
                    value: value.clone(),
                })
                .collect(),
            ..DeviceSettings::default()
        }
    }

    fn written(writes: &[Write], channel: &str, attr: &str) -> Vec<String> {
        writes
            .iter()
            .filter_map(|write| match write {
                Write::Attr {
                    channel: c,
                    attr: a,
                    value,
                    ..
                } if c == channel && a == attr => Some(value.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_tone_is_a_cosine_and_a_sine_on_every_transmit_lane() {
        let next = settings(&[
            (on_name(0), json!(true)),
            (hz_name(0), json!(200_000.0)),
            (level_name(0), json!(-20.0)),
        ]);
        let mut writes = Vec::new();
        tones().plan(&next, &next, &mut writes).expect("planned");
        assert_eq!(
            writes.len(),
            2 * (8 + 2),
            "a silent tone only has its scale zeroed"
        );
        assert_eq!(written(&writes, "altvoltage0", PHASE), vec!["90000"]);
        assert_eq!(written(&writes, "altvoltage2", PHASE), vec!["0"]);
        assert_eq!(written(&writes, "altvoltage4", SCALE), vec!["0.100000"]);
        assert_eq!(written(&writes, "altvoltage1", SCALE), vec!["0.000000"]);
        assert!(written(&writes, "altvoltage1", FREQUENCY).is_empty());
    }

    #[test]
    fn a_negative_offset_turns_the_sine_around() {
        let next = settings(&[(on_name(1), json!(true)), (hz_name(1), json!(-50_000.0))]);
        let mut writes = Vec::new();
        tones().plan(&next, &next, &mut writes).expect("planned");
        assert_eq!(written(&writes, "altvoltage3", PHASE), vec!["180000"]);
        assert_eq!(written(&writes, "altvoltage1", FREQUENCY), vec!["50000"]);
    }

    #[test]
    fn tones_past_the_rate_or_above_full_scale_together_are_refused() {
        let far = settings(&[(on_name(0), json!(true)), (hz_name(0), json!(6e6))]);
        assert!(
            tones()
                .plan(&far, &far, &mut Vec::new())
                .expect_err("refused")
                .to_string()
                .contains("outside")
        );
        let loud = settings(&[
            (on_name(0), json!(true)),
            (level_name(0), json!(0.0)),
            (on_name(1), json!(true)),
            (level_name(1), json!(-6.0)),
        ]);
        assert!(
            tones()
                .plan(&loud, &loud, &mut Vec::new())
                .expect_err("refused")
                .to_string()
                .contains("full scale")
        );
    }

    #[test]
    fn a_rate_change_rewrites_a_running_tone_and_leaves_silent_ones_alone() {
        let running = settings(&[(on_name(0), json!(true))]);
        let rate = DeviceSettings {
            sample_rate: Some(5e6),
            ..DeviceSettings::default()
        };
        let mut writes = Vec::new();
        tones().plan(&rate, &running, &mut writes).expect("planned");
        assert!(!writes.is_empty());
        let silent = settings(&[]);
        let mut writes = Vec::new();
        tones().plan(&rate, &silent, &mut writes).expect("planned");
        assert!(writes.is_empty());
    }

    #[test]
    fn the_settings_are_grouped_and_reach_half_the_fastest_rate() {
        let settings = tones().settings(61_440_000.0);
        assert_eq!(settings.len(), 6);
        assert!(
            settings
                .iter()
                .all(|setting| setting.group() == Some(GROUP))
        );
        let ExtraSetting::Range { range, .. } = &settings[1] else {
            panic!("an offset is a range");
        };
        assert_eq!(range.max, 30_720_000.0);
        assert!(owns("tone_2_dbfs") && !owns("tone_3"));
    }
}
