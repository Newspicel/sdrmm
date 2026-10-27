use sdrmm_device::{DeviceError, check_stream_settings};
use sdrmm_wire::{
    Agc, AgcReach, ArgumentOption, BandwidthSetting, Capabilities, Coherence, DcArtifact,
    DeviceSettings, Duplex, ExtraSetting, ExtraValue, GainKind, GainStage, GainUnit, GainValue,
    NoiseSource, Range, StreamScope,
};

use crate::{
    protocol::{BANDS_HZ, IfFilter, MAX_IF_GAIN_DB, RfFilter, SAMPLE_RATE_HZ},
    radio::{Config, MAX_PPM},
};

pub(crate) const ANTENNA: &str = "RX";
pub(crate) const LNA: &str = "LNA";
pub(crate) const MIXER: &str = "MIX";
pub(crate) const RF_FILTER: &str = "rf_filter";
pub(crate) const AUTO: &str = "auto";
pub(crate) const ON: f64 = 1.0;

fn switch(name: &str) -> GainStage {
    GainStage::named(
        name,
        GainKind::Amp,
        Range {
            min: 0.0,
            max: ON,
            step: Some(ON),
        },
    )
    .with_unit(GainUnit::Index)
    .with_agc(AgcReach::Never)
}

fn rf_filter_setting() -> ExtraSetting {
    let options = std::iter::once(ArgumentOption {
        value: AUTO.to_string(),
        label: Some("Auto".to_string()),
    })
    .chain(RfFilter::ALL.into_iter().map(|filter| ArgumentOption {
        value: filter.id().to_string(),
        label: Some(filter.label().to_string()),
    }))
    .collect();
    ExtraSetting::choice(RF_FILTER, "RF filter", options, AUTO)
}

pub(crate) fn capabilities() -> Capabilities {
    Capabilities {
        freq_ranges: BANDS_HZ
            .iter()
            .map(|band| Range {
                min: f64::from(*band.start()),
                max: f64::from(*band.end()),
                step: None,
            })
            .collect(),
        sample_rates: vec![f64::from(SAMPLE_RATE_HZ)],
        sample_rate_ranges: Vec::new(),
        gains: vec![
            switch(LNA),
            switch(MIXER),
            GainStage::new(
                GainKind::If,
                Range {
                    min: 0.0,
                    max: f64::from(MAX_IF_GAIN_DB),
                    step: Some(1.0),
                },
            )
            .with_agc(AgcReach::Never),
        ],
        antennas: vec![ANTENNA.to_string()],
        bandwidths: IfFilter::ALL.into_iter().map(IfFilter::hz).collect(),
        bandwidth_ranges: Vec::new(),
        bandwidth_auto: false,
        bias_tee: true,
        agc: Agc::None,
        extra: vec![rf_filter_setting()],
        ppm: true,
        duplex: Duplex::RxOnly,
        rx_streams: 1,
        tx_streams: 0,
        per_stream: StreamScope::default(),
        directional: None,
        dc_artifact: DcArtifact::Managed,
        hardware_sweep: false,
        coherence: Coherence::None,
        noise_source: NoiseSource::None,
        retune_keeps_phase: false,
        rx_inputs: Vec::new(),
        clean_fraction: None,
        bank: None,
    }
}

fn gain(stage: &str, value_db: f64) -> GainValue {
    GainValue {
        stage: stage.to_string(),
        value_db,
    }
}

const fn switched(on: bool) -> f64 {
    if on { ON } else { 0.0 }
}

pub(crate) fn settings(config: &Config) -> DeviceSettings {
    DeviceSettings {
        center_hz: Some(config.center_hz()),
        ppm: Some(config.ppm),
        sample_rate: Some(f64::from(SAMPLE_RATE_HZ)),
        antenna: Some(ANTENNA.to_string()),
        bandwidth: Some(BandwidthSetting::Manual {
            hz: config.if_filter.hz(),
        }),
        bias_tee: Some(config.bias_tee),
        gains: vec![
            gain(LNA, switched(config.lna)),
            gain(MIXER, switched(config.mixer)),
            GainValue::new(GainKind::If, f64::from(config.if_gain_db)),
        ],
        extra: vec![ExtraValue {
            name: RF_FILTER.to_string(),
            value: config.rf_filter.map_or(AUTO, RfFilter::id).into(),
        }],
        ..DeviceSettings::default()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Writes {
    pub(crate) center_hz: Option<u32>,
    pub(crate) ppm: Option<f64>,
    pub(crate) lna: Option<bool>,
    pub(crate) mixer: Option<bool>,
    pub(crate) if_gain_db: Option<u8>,
    pub(crate) if_filter: Option<IfFilter>,
    pub(crate) bias_tee: Option<bool>,
    pub(crate) rf_filter: Option<Option<RfFilter>>,
}

fn unsupported(reason: impl Into<String>) -> DeviceError {
    DeviceError::Unsupported(reason.into())
}

pub(crate) fn plan(delta: &DeviceSettings, caps: &Capabilities) -> Result<Writes, DeviceError> {
    check_stream_settings(delta, caps)?;
    let mut writes = Writes {
        center_hz: delta.center_hz.map(center_hz).transpose()?,
        ppm: delta.ppm.map(ppm).transpose()?,
        if_filter: delta.bandwidth.map(if_filter).transpose()?,
        bias_tee: delta.bias_tee,
        ..Writes::default()
    };
    if let Some(rate) = delta.sample_rate
        && (rate - f64::from(SAMPLE_RATE_HZ)).abs() >= 1.0
    {
        return Err(unsupported(format!(
            "{rate} Hz: a Pro+ runs at 192 kS/s only"
        )));
    }
    if let Some(antenna) = &delta.antenna
        && antenna != ANTENNA
    {
        return Err(unsupported(format!(
            "this radio has one input, {ANTENNA}, not {antenna}"
        )));
    }
    if delta.agc.as_ref().is_some_and(|agc| agc.on) {
        return Err(unsupported("this radio has no AGC"));
    }
    for value in &delta.gains {
        plan_gain(value, caps, &mut writes)?;
    }
    for extra in &delta.extra {
        writes.rf_filter = Some(rf_filter(extra)?);
    }
    Ok(writes)
}

fn center_hz(hz: f64) -> Result<u32, DeviceError> {
    if !sdrmm_wire::any_range_holds(&capabilities().freq_ranges, hz) {
        return Err(unsupported(format!(
            "{hz} Hz is outside 150 kHz to 260 MHz and 410 MHz to 2.05 GHz"
        )));
    }
    Ok(hz.round() as u32)
}

fn ppm(ppm: f64) -> Result<f64, DeviceError> {
    if ppm.is_finite() && (-MAX_PPM..=MAX_PPM).contains(&ppm) {
        return Ok(ppm);
    }
    Err(unsupported(format!("ppm {ppm} outside ±{MAX_PPM}")))
}

fn if_filter(bandwidth: BandwidthSetting) -> Result<IfFilter, DeviceError> {
    bandwidth
        .hz()
        .and_then(IfFilter::from_hz)
        .ok_or_else(|| unsupported(format!("no {bandwidth:?} IF filter")))
}

fn plan_gain(
    value: &GainValue,
    caps: &Capabilities,
    writes: &mut Writes,
) -> Result<(), DeviceError> {
    let stage = caps
        .stage(&value.stage)
        .ok_or_else(|| unsupported(format!("no {} stage", value.stage)))?;
    let db = value.value_db;
    if !db.is_finite() || db < stage.range.min || db > stage.range.max {
        return Err(unsupported(format!(
            "{} takes {} to {} dB",
            stage.name, stage.range.min, stage.range.max
        )));
    }
    match stage.name.as_str() {
        LNA => writes.lna = Some(db > 0.0),
        MIXER => writes.mixer = Some(db > 0.0),
        _ => writes.if_gain_db = Some(db.round() as u8),
    }
    Ok(())
}

fn rf_filter(extra: &ExtraValue) -> Result<Option<RfFilter>, DeviceError> {
    if extra.name != RF_FILTER {
        return Err(unsupported(format!("no {} setting", extra.name)));
    }
    match extra.value.as_str() {
        Some(AUTO) => Ok(None),
        Some(id) => RfFilter::from_id(id)
            .map(Some)
            .ok_or_else(|| unsupported(format!("no {id} RF filter"))),
        None => Err(unsupported("the RF filter is named by text")),
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::AgcSetting;

    use super::*;

    fn plan_of(delta: DeviceSettings) -> Result<Writes, DeviceError> {
        plan(&delta, &capabilities())
    }

    fn extra(value: serde_json::Value) -> DeviceSettings {
        DeviceSettings {
            extra: vec![ExtraValue {
                name: RF_FILTER.to_string(),
                value,
            }],
            ..DeviceSettings::default()
        }
    }

    #[test]
    fn both_tuning_windows_are_separate_ranges() {
        let caps = capabilities();
        assert_eq!(caps.freq_ranges.len(), 2, "the gap must stay a gap");
        for (hz, ok) in [
            (150e3, true),
            (100e3, false),
            (260e6, true),
            (300e6, false),
            (410e6, true),
            (2.05e9, true),
            (2.1e9, false),
        ] {
            let delta = DeviceSettings {
                center_hz: Some(hz),
                ..DeviceSettings::default()
            };
            assert_eq!(plan_of(delta).is_ok(), ok, "{hz} Hz");
        }
    }

    #[test]
    fn only_the_native_rate_is_offered() {
        let caps = capabilities();
        assert_eq!(caps.sample_rates, vec![192_000.0]);
        let other = DeviceSettings {
            sample_rate: Some(96_000.0),
            ..DeviceSettings::default()
        };
        assert!(plan_of(other).is_err());
        let native = DeviceSettings {
            sample_rate: Some(192_000.0),
            ..DeviceSettings::default()
        };
        assert_eq!(plan_of(native).expect("native"), Writes::default());
    }

    #[test]
    fn lna_and_mixer_are_switches_and_if_gain_is_a_slider() {
        let caps = capabilities();
        for name in [LNA, MIXER] {
            let stage = caps.stage(name).expect(name);
            assert!(stage.is_switch());
            assert_eq!(stage.unit, GainUnit::Index);
            assert_eq!(stage.setting_count(), 2);
        }
        let if_gain = caps.stage(GainKind::If.name()).expect("if");
        assert!(!if_gain.is_switch());
        assert_eq!(if_gain.setting_count(), 60);
    }

    #[test]
    fn gains_become_firmware_writes() {
        let writes = plan_of(DeviceSettings {
            gains: vec![
                gain(LNA, 0.0),
                gain(MIXER, ON),
                GainValue::new(GainKind::If, 30.4),
            ],
            ..DeviceSettings::default()
        })
        .expect("plan");
        assert_eq!(writes.lna, Some(false));
        assert_eq!(writes.mixer, Some(true));
        assert_eq!(writes.if_gain_db, Some(30));
    }

    #[test]
    fn a_gain_beyond_its_stage_or_an_unknown_stage_is_refused() {
        for value in [
            GainValue::new(GainKind::If, 60.0),
            GainValue::new(GainKind::If, f64::NAN),
            gain(LNA, ON + 1.0),
            GainValue::new(GainKind::Attenuator, -6.0),
        ] {
            let delta = DeviceSettings {
                gains: vec![value.clone()],
                ..DeviceSettings::default()
            };
            assert!(plan_of(delta).is_err(), "{value:?}");
        }
    }

    #[test]
    fn the_if_filter_is_the_bandwidth_menu() {
        let caps = capabilities();
        assert_eq!(caps.bandwidths.len(), 8);
        assert!(!caps.bandwidth_auto);
        let pick = |bandwidth| {
            plan_of(DeviceSettings {
                bandwidth: Some(bandwidth),
                ..DeviceSettings::default()
            })
        };
        assert_eq!(
            pick(BandwidthSetting::Manual { hz: 600e3 })
                .expect("600k")
                .if_filter,
            Some(IfFilter::Khz600)
        );
        assert!(pick(BandwidthSetting::Manual { hz: 250e3 }).is_err());
        assert!(pick(BandwidthSetting::Auto).is_err());
    }

    #[test]
    fn the_rf_filter_is_auto_or_one_of_the_firmware_bands() {
        assert_eq!(
            plan_of(extra(AUTO.into())).expect("auto").rf_filter,
            Some(None)
        );
        assert_eq!(
            plan_of(extra("145".into())).expect("145").rf_filter,
            Some(Some(RfFilter::Mhz145))
        );
        assert!(plan_of(extra("9000".into())).is_err());
        assert!(plan_of(extra(true.into())).is_err());
        let other = DeviceSettings {
            extra: vec![ExtraValue {
                name: "agc".to_string(),
                value: true.into(),
            }],
            ..DeviceSettings::default()
        };
        assert!(plan_of(other).is_err());
    }

    #[test]
    fn the_rf_filter_menu_offers_auto_and_every_band() {
        let caps = capabilities();
        let Some(ExtraSetting::Enum {
            options, default, ..
        }) = caps.extra.first()
        else {
            panic!("rf filter menu");
        };
        assert_eq!(default, AUTO);
        assert_eq!(options.len(), RfFilter::ALL.len() + 1);
    }

    #[test]
    fn agc_can_only_be_off() {
        let with = |agc| DeviceSettings {
            agc: Some(agc),
            ..DeviceSettings::default()
        };
        assert!(plan_of(with(AgcSetting::off())).is_ok());
        assert!(plan_of(with(AgcSetting::switched(true))).is_err());
    }

    #[test]
    fn ppm_and_antenna_are_bounded() {
        let ppm = |value| {
            plan_of(DeviceSettings {
                ppm: Some(value),
                ..DeviceSettings::default()
            })
        };
        assert_eq!(ppm(-1.5).expect("ppm").ppm, Some(-1.5));
        assert!(ppm(201.0).is_err());
        assert!(ppm(f64::INFINITY).is_err());
        let antenna = DeviceSettings {
            antenna: Some("TX".to_string()),
            ..DeviceSettings::default()
        };
        assert!(plan_of(antenna).is_err());
    }

    #[test]
    fn settings_report_every_control() {
        let reported = settings(&Config {
            tuned_hz: 145_800_000,
            lna: true,
            mixer: false,
            if_gain_db: 12,
            if_filter: IfFilter::Khz300,
            rf_filter: Some(RfFilter::Mhz145),
            bias_tee: true,
            ..Config::default()
        });
        assert_eq!(reported.center_hz, Some(145_800_000.0));
        assert_eq!(reported.sample_rate, Some(192_000.0));
        assert_eq!(reported.gain(LNA), Some(ON));
        assert_eq!(reported.gain(MIXER), Some(0.0));
        assert_eq!(reported.gain(GainKind::If.name()), Some(12.0));
        assert_eq!(
            reported.bandwidth,
            Some(BandwidthSetting::Manual { hz: 300e3 })
        );
        assert_eq!(reported.bias_tee, Some(true));
        assert_eq!(reported.extra[0].value, serde_json::Value::from("145"));
    }

    #[test]
    fn reported_settings_plan_back_to_the_same_state() {
        let config = Config {
            tuned_hz: 433_920_000,
            mixer: false,
            if_gain_db: 40,
            if_filter: IfFilter::Mhz5,
            bias_tee: true,
            ..Config::default()
        };
        let writes = plan_of(settings(&config)).expect("round trip");
        assert_eq!(writes.center_hz, Some(433_920_000));
        assert_eq!(writes.lna, Some(true));
        assert_eq!(writes.mixer, Some(false));
        assert_eq!(writes.if_gain_db, Some(40));
        assert_eq!(writes.if_filter, Some(IfFilter::Mhz5));
        assert_eq!(writes.bias_tee, Some(true));
        assert_eq!(writes.rf_filter, Some(None));
    }
}
