use sdrmm_device::DeviceError;
use sdrmm_wire::{
    Agc, AgcReach, ArgumentOption, Capabilities, Coherence, DcArtifact, Duplex, ExtraSetting,
    GainKind, GainStage, NoiseSource, Range, StreamScope,
};

use crate::{
    api::{DevHandle, Rtsa},
    model::{Item, Kind},
};

pub const CENTER: &str = "main/centerfreq";
pub const SPAN: &str = "main/spanfreq";
pub const REFERENCE: &str = "main/reflevel";
pub const TX_GAIN: &str = "main/transgain";
pub const CLOCK: &str = "device/receiverclock";
pub const CHANNEL: &str = "device/receiverchannel";

pub const GAIN: &str = "GAIN";
pub const SINGLE_INPUT: &str = "RX";
pub const INPUTS: [&str; 2] = ["Rx1", "Rx2"];

/// The IQ modes deliver samples at one and a half times the span they are asked for.
pub const RATE_PER_SPAN: f64 = 1.5;

/// The voltage on 50 ohms that the API calls 0 dBm.
pub const ZERO_DBM_VOLTS: f64 = 0.223_606_797_749_979;

const FALLBACK_FREQ: Range = Range {
    min: 1e6,
    max: 6e9,
    step: None,
};
const FALLBACK_SPAN: (f64, f64) = (1e3, 40e6);
const FALLBACK_REFERENCE: (f64, f64) = (-40.0, 10.0);
const FALLBACK_TX_GAIN: (f64, f64) = (-60.0, 0.0);

/// The receiver clocks by the name the API gives them, with the rate each actually runs.
const CLOCKS: [(&str, f64); 8] = [
    ("46MHz", 46.08e6),
    ("61MHz", 61.44e6),
    ("77MHz", 76.8e6),
    ("92MHz", 92.16e6),
    ("122MHz", 122.88e6),
    ("184MHz", 184.32e6),
    ("245MHz", 245.76e6),
    ("492MHz", 491.52e6),
];

#[derive(Clone, Copy, Debug)]
pub struct ExtraPath {
    pub path: &'static str,
    pub name: &'static str,
    pub label: &'static str,
}

pub const EXTRAS: [ExtraPath; 3] = [
    ExtraPath {
        path: "calibration/preamp",
        name: "preamp",
        label: "Amplifier",
    },
    ExtraPath {
        path: "calibration/rffilter",
        name: "rf_filter",
        label: "RX filter",
    },
    ExtraPath {
        path: "device/usbcompression",
        name: "usb_compression",
        label: "USB compression",
    },
];

#[derive(Clone, Debug, PartialEq)]
pub struct Extra {
    pub path: &'static str,
    pub name: &'static str,
    pub label: &'static str,
    pub options: Vec<String>,
    pub current: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TxProfile {
    pub center: Option<Item>,
    pub span: Option<Item>,
    pub gain: Option<Item>,
}

/// What one SPECTRAN's configuration trees accept, read once when it is opened.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub kind: Kind,
    pub center: Option<Item>,
    pub span: Option<Item>,
    pub reference: Option<Item>,
    pub clock: Option<Item>,
    pub channel: Option<Item>,
    pub extras: Vec<Extra>,
    pub tx: Option<TxProfile>,
}

impl Profile {
    pub fn read_rx(api: &dyn Rtsa, dev: DevHandle, kind: Kind) -> Result<Self, DeviceError> {
        let mut extras = Vec::new();
        for extra in EXTRAS {
            let Some(item) = api.item(dev, extra.path)? else {
                continue;
            };
            let options: Vec<String> = item
                .enabled_options()
                .into_iter()
                .map(ToString::to_string)
                .collect();
            if options.len() < 2 {
                continue;
            }
            extras.push(Extra {
                path: extra.path,
                name: extra.name,
                label: extra.label,
                current: api.get_string(dev, extra.path).unwrap_or_default(),
                options,
            });
        }
        Ok(Self {
            kind,
            center: api.item(dev, CENTER)?,
            span: api.item(dev, SPAN)?,
            reference: api.item(dev, REFERENCE)?,
            clock: api.item(dev, CLOCK)?,
            channel: api.item(dev, CHANNEL)?,
            extras,
            tx: None,
        })
    }

    pub fn read_tx(api: &dyn Rtsa, dev: DevHandle) -> Result<TxProfile, DeviceError> {
        Ok(TxProfile {
            center: api.item(dev, CENTER)?,
            span: api.item(dev, SPAN)?,
            gain: api.item(dev, TX_GAIN)?,
        })
    }

    #[must_use]
    pub fn freq_range(&self) -> Range {
        bounded(self.center.as_ref()).map_or(FALLBACK_FREQ, |(min, max)| Range {
            min,
            max,
            step: None,
        })
    }

    /// The fastest the receiver clock allows: a span may not exceed the clock over 1.5, and the
    /// rate is the span times 1.5.
    #[must_use]
    pub fn top_clock_hz(&self) -> Option<f64> {
        self.kind.fixed_clock_hz().or_else(|| {
            let clock = self.clock.as_ref()?;
            clock
                .enabled_options()
                .into_iter()
                .filter_map(clock_hz)
                .max_by(f64::total_cmp)
        })
    }

    #[must_use]
    pub fn rate_range(&self) -> Range {
        let (span_min, span_max) = bounded(self.span.as_ref()).unwrap_or(FALLBACK_SPAN);
        let mut max = span_max * RATE_PER_SPAN;
        if let Some(clock) = self.top_clock_hz() {
            max = max.min(clock);
        }
        Range {
            min: span_min * RATE_PER_SPAN,
            max,
            step: None,
        }
    }

    #[must_use]
    pub fn reference_range(&self) -> (f64, f64, f64) {
        let (min, max) = bounded(self.reference.as_ref()).unwrap_or(FALLBACK_REFERENCE);
        (min, max, step_of(self.reference.as_ref()))
    }

    #[must_use]
    pub fn antennas(&self) -> Vec<String> {
        let inputs: Vec<String> = self
            .channel
            .as_ref()
            .map(|item| {
                item.enabled_options()
                    .into_iter()
                    .filter(|option| INPUTS.contains(option))
                    .map(ToString::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if inputs.is_empty() {
            vec![SINGLE_INPUT.to_string()]
        } else {
            inputs
        }
    }

    #[must_use]
    pub fn selects_input(&self) -> bool {
        self.antennas()
            .iter()
            .any(|antenna| antenna != SINGLE_INPUT)
    }

    /// The slowest receiver clock that still carries `rate`, so the USB link moves no more than
    /// it has to. `None` when this SPECTRAN has no clock to choose.
    #[must_use]
    pub fn clock_for(&self, rate: f64) -> Option<&str> {
        let clock = self.clock.as_ref()?;
        let mut usable: Vec<(&str, f64)> = clock
            .enabled_options()
            .into_iter()
            .filter_map(|name| clock_hz(name).map(|hz| (name, hz)))
            .collect();
        usable.sort_by(|a, b| a.1.total_cmp(&b.1));
        usable
            .iter()
            .find(|(_, hz)| *hz >= rate)
            .or(usable.last())
            .map(|(name, _)| *name)
    }

    #[must_use]
    pub fn tx_gain_range(&self) -> Option<(f64, f64, f64)> {
        let tx = self.tx.as_ref()?;
        let (min, max) = bounded(tx.gain.as_ref()).unwrap_or(FALLBACK_TX_GAIN);
        Some((min, max, step_of(tx.gain.as_ref())))
    }

    #[must_use]
    pub fn tx_span_max(&self) -> Option<f64> {
        let tx = self.tx.as_ref()?;
        Some(bounded(tx.span.as_ref()).map_or(FALLBACK_SPAN.1, |(_, max)| max))
    }

    #[must_use]
    pub fn capabilities(&self) -> Capabilities {
        let (ref_min, ref_max, ref_step) = self.reference_range();
        let mut gains = vec![
            GainStage::new(
                GainKind::Tuner,
                Range {
                    min: -ref_max,
                    max: -ref_min,
                    step: Some(ref_step),
                },
            )
            .with_agc(AgcReach::Never),
        ];
        gains[0].name = GAIN.to_string();
        if let Some((min, max, step)) = self.tx_gain_range() {
            gains.push(
                GainStage::new(
                    GainKind::Tx,
                    Range {
                        min,
                        max,
                        step: Some(step),
                    },
                )
                .with_agc(AgcReach::Never),
            );
        }
        let transmits = self.tx.is_some();
        Capabilities {
            freq_ranges: vec![self.freq_range()],
            sample_rates: Vec::new(),
            sample_rate_ranges: vec![self.rate_range()],
            gains,
            antennas: self.antennas(),
            bandwidths: Vec::new(),
            bandwidth_ranges: Vec::new(),
            bandwidth_auto: false,
            bias_tee: false,
            agc: Agc::None,
            extra: self.extras.iter().map(extra_setting).collect(),
            ppm: false,
            duplex: if transmits {
                Duplex::Half
            } else {
                Duplex::RxOnly
            },
            rx_streams: 1,
            tx_streams: u32::from(transmits),
            per_stream: StreamScope::default(),
            directional: None,
            dc_artifact: DcArtifact::Operator,
            hardware_sweep: false,
            coherence: Coherence::None,
            noise_source: NoiseSource::None,
            retune_keeps_phase: false,
            rx_inputs: Vec::new(),
            clean_fraction: Some(1.0 / RATE_PER_SPAN),
            bank: None,
        }
    }
}

fn extra_setting(extra: &Extra) -> ExtraSetting {
    let default = if extra.options.contains(&extra.current) {
        extra.current.clone()
    } else {
        extra.options.first().cloned().unwrap_or_default()
    };
    ExtraSetting::choice(
        extra.name,
        extra.label,
        extra.options.iter().map(ArgumentOption::plain).collect(),
        default,
    )
}

fn bounded(item: Option<&Item>) -> Option<(f64, f64)> {
    item.filter(|item| item.has_range())
        .map(|item| (item.min, item.max))
}

fn step_of(item: Option<&Item>) -> f64 {
    item.map(|item| item.step)
        .filter(|step| step.is_finite() && *step > 0.0)
        .unwrap_or(1.0)
}

#[must_use]
pub fn clock_hz(name: &str) -> Option<f64> {
    CLOCKS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, hz)| *hz)
}

/// The receive gain the operator sees is the reference level turned around: lowering the level
/// the analyser treats as full scale raises everything below it.
#[must_use]
pub fn reference_for_gain(gain_db: f64) -> f64 {
    -gain_db
}

/// The factor that puts a signal at the reference level at full scale. The API delivers volts on
/// 50 ohms, with 0 dBm at [`ZERO_DBM_VOLTS`].
#[must_use]
pub fn rx_scale(reference_dbm: f64) -> f32 {
    (1.0 / (ZERO_DBM_VOLTS * 10f64.powf(reference_dbm / 20.0))) as f32
}

/// Full scale on the transmit side is 0 dBm before the transmitter gain.
#[must_use]
pub fn tx_scale() -> f32 {
    ZERO_DBM_VOLTS as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v6() -> Profile {
        let mut clock = Item::choice(&["92MHz", "122MHz", "184MHz", "245MHz", "61MHz", "46MHz"]);
        clock.disabled = 1 << 2;
        Profile {
            kind: Kind::V6,
            center: Some(Item::number(1e6, 6e9, 0.0)),
            span: Some(Item::number(10e3, 200e6, 0.0)),
            reference: Some(Item::number(-20.0, 10.0, 1.0)),
            clock: Some(clock),
            channel: Some(Item::choice(&["Rx1", "Rx2", "Rx1+Rx2", "auto"])),
            extras: vec![Extra {
                path: "calibration/preamp",
                name: "preamp",
                label: "Amplifier",
                options: vec!["Auto".to_string(), "None".to_string()],
                current: "Auto".to_string(),
            }],
            tx: Some(TxProfile {
                center: Some(Item::number(1e6, 6e9, 0.0)),
                span: Some(Item::number(10e3, 100e6, 0.0)),
                gain: Some(Item::number(-100.0, 0.0, 0.5)),
            }),
        }
    }

    #[test]
    fn the_rate_range_is_the_span_range_held_under_the_fastest_clock() {
        let rates = v6().rate_range();
        assert!((rates.min - 15e3).abs() < 1e-6);
        assert_eq!(
            rates.max, 245.76e6,
            "184 MHz is disabled, 245 MHz is the top"
        );
    }

    #[test]
    fn an_eco_rate_stops_at_its_fixed_clock() {
        let mut eco = v6();
        eco.kind = Kind::V6Eco;
        eco.clock = None;
        assert_eq!(eco.rate_range().max, 61.44e6);
    }

    #[test]
    fn the_slowest_clock_that_carries_the_rate_is_chosen() {
        let profile = v6();
        assert_eq!(profile.clock_for(2e6), Some("46MHz"));
        assert_eq!(profile.clock_for(50e6), Some("61MHz"));
        assert_eq!(profile.clock_for(150e6), Some("245MHz"));
        assert_eq!(profile.clock_for(400e6), Some("245MHz"));
    }

    #[test]
    fn gain_is_the_reference_level_reversed() {
        let caps = v6().capabilities();
        let stage = caps.stage(GAIN).expect("gain stage");
        assert_eq!((stage.range.min, stage.range.max), (-10.0, 20.0));
        assert_eq!(reference_for_gain(15.0), -15.0);
    }

    #[test]
    fn a_signal_at_the_reference_level_reads_as_full_scale() {
        let volts = ZERO_DBM_VOLTS * 10f64.powf(-30.0 / 20.0);
        assert!((volts as f32 * rx_scale(-30.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_two_inputs_of_a_v6_are_its_antennas() {
        let profile = v6();
        assert_eq!(profile.antennas(), vec!["Rx1", "Rx2"]);
        assert!(profile.selects_input());
        let eco = Profile {
            channel: None,
            ..v6()
        };
        assert_eq!(eco.antennas(), vec![SINGLE_INPUT]);
        assert!(!eco.selects_input());
    }

    #[test]
    fn a_transmitter_makes_the_radio_half_duplex_with_a_tx_stage() {
        let caps = v6().capabilities();
        assert_eq!(caps.duplex, Duplex::Half);
        assert_eq!(caps.tx_streams, 1);
        assert!(caps.gains.iter().any(|stage| stage.kind == GainKind::Tx));
        let receiver = Profile { tx: None, ..v6() }.capabilities();
        assert_eq!(receiver.duplex, Duplex::RxOnly);
        assert_eq!(receiver.tx_streams, 0);
    }

    #[test]
    fn extras_offer_what_the_device_lists_with_its_current_choice() {
        let caps = v6().capabilities();
        let [
            ExtraSetting::Enum {
                name,
                options,
                default,
                ..
            },
        ] = caps.extra.as_slice()
        else {
            panic!("one enum extra");
        };
        assert_eq!(name, "preamp");
        assert_eq!(options.len(), 2);
        assert_eq!(default, "Auto");
    }

    #[test]
    fn a_tree_without_ranges_falls_back_to_the_documented_limits() {
        let bare = Profile {
            kind: Kind::V6Eco,
            center: None,
            span: None,
            reference: None,
            clock: None,
            channel: None,
            extras: Vec::new(),
            tx: None,
        };
        assert_eq!(bare.freq_range(), FALLBACK_FREQ);
        assert_eq!(bare.rate_range().max, FALLBACK_SPAN.1 * RATE_PER_SPAN);
    }
}
