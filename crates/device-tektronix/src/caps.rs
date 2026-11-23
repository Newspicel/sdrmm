use sdrmm_wire::{
    Agc, AgcReach, ArgumentOption, Capabilities, Coherence, DcArtifact, Duplex, ExtraSetting,
    GainKind, GainStage, NoiseSource, Range, StreamScope,
};

use crate::model::{Fitted, FrequencySource, Limits};

pub const GAIN: &str = "GAIN";
pub const ATTENUATOR: &str = "ATT";
pub const INPUT: &str = "RF";
pub const PREAMP: &str = "preamp";
pub const AUTO_ATTENUATION: &str = "auto_attenuation";
pub const FREQUENCY_SOURCE: &str = "frequency_reference";

/// The IQ stream runs at 56 MS/s or a power of two below it, thirteen rates in all.
const TOP_RATE_HZ: f64 = 56e6;
const RATE_COUNT: i32 = 13;

/// The fastest of the three slowest rates, which the API runs through a path of their own.
pub const LOW_PATH_TOP_HZ: f64 = 54_687.5;

/// The filter each rate is asked for. The API switches to the next rate up above rate / 1.4,
/// so the request stays a hair under that edge.
pub const BANDWIDTH_PER_RATE: f64 = 0.714;

/// The voltage on 50 ohms that the API calls 0 dBm.
pub const ZERO_DBM_VOLTS: f64 = 0.223_606_797_749_979;

/// The reference levels the manual documents; the API itself reports no range.
const REFERENCE_MIN_DBM: f64 = -130.0;
const REFERENCE_MAX_DBM: f64 = 30.0;

/// The attenuator's range as the manual gives it, a gain of zero or less.
const ATTENUATOR_MIN_DB: f64 = -51.0;

/// Every rate whose filter fits the widest stream this analyser offers, slowest first.
#[must_use]
pub fn rates(limits: &Limits) -> Vec<f64> {
    (0..RATE_COUNT)
        .rev()
        .map(|halvings| TOP_RATE_HZ / 2f64.powi(halvings))
        .filter(|rate| rate * BANDWIDTH_PER_RATE <= limits.bandwidth_max)
        .collect()
}

#[must_use]
pub fn bandwidth_for(rate: f64, limits: &Limits) -> f64 {
    (rate * BANDWIDTH_PER_RATE).clamp(limits.bandwidth_min, limits.bandwidth_max)
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

fn stage(name: &str, kind: GainKind, min: f64, max: f64) -> GainStage {
    let range = Range {
        min,
        max,
        step: Some(1.0),
    };
    let mut stage = GainStage::new(kind, range).with_agc(AgcReach::Never);
    stage.name = name.to_string();
    stage
}

fn gains(fitted: Fitted) -> Vec<GainStage> {
    let reference = stage(
        GAIN,
        GainKind::Tuner,
        -REFERENCE_MAX_DBM,
        -REFERENCE_MIN_DBM,
    );
    let attenuator = stage(ATTENUATOR, GainKind::Attenuator, ATTENUATOR_MIN_DB, 0.0);
    std::iter::once(reference)
        .chain(fitted.attenuator.then_some(attenuator))
        .collect()
}

fn extras(fitted: Fitted) -> Vec<ExtraSetting> {
    let sources = [FrequencySource::Internal, FrequencySource::External]
        .into_iter()
        .chain(fitted.gnss.then_some(FrequencySource::Gnss))
        .map(|source| ArgumentOption {
            value: source.name().to_string(),
            label: Some(source.label().to_string()),
        })
        .collect();
    [
        fitted
            .preamp
            .then(|| ExtraSetting::bool(PREAMP, "Preamp", false)),
        fitted
            .attenuator
            .then(|| ExtraSetting::bool(AUTO_ATTENUATION, "Auto attenuation", true)),
        Some(ExtraSetting::choice(
            FREQUENCY_SOURCE,
            "Frequency ref",
            sources,
            FrequencySource::Internal.name(),
        )),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[must_use]
pub fn capabilities(limits: &Limits, fitted: Fitted) -> Capabilities {
    Capabilities {
        freq_ranges: vec![Range {
            min: limits.center_min,
            max: limits.center_max,
            step: None,
        }],
        sample_rates: rates(limits),
        sample_rate_ranges: Vec::new(),
        gains: gains(fitted),
        antennas: vec![INPUT.to_string()],
        bandwidths: Vec::new(),
        bandwidth_ranges: Vec::new(),
        bandwidth_auto: false,
        bias_tee: false,
        agc: Agc::None,
        extra: extras(fitted),
        ppm: false,
        duplex: Duplex::RxOnly,
        rx_streams: 1,
        tx_streams: 0,
        per_stream: StreamScope::default(),
        directional: None,
        dc_artifact: DcArtifact::Operator,
        hardware_sweep: false,
        coherence: Coherence::None,
        noise_source: NoiseSource::None,
        retune_keeps_phase: false,
        rx_inputs: Vec::new(),
        clean_fraction: Some(BANDWIDTH_PER_RATE),
        bank: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSA306: Limits = Limits {
        center_min: 9e3,
        center_max: 6.2e9,
        bandwidth_min: 9765.625,
        bandwidth_max: 40e6,
    };

    #[test]
    fn an_rsa306_offers_thirteen_rates_up_to_56_megasamples() {
        let rates = rates(&RSA306);
        assert_eq!(rates.len(), 13);
        assert_eq!(rates.first(), Some(&13_671.875));
        assert_eq!(rates.last(), Some(&56e6));
    }

    #[test]
    fn a_narrower_analyser_loses_the_rates_its_filter_cannot_carry() {
        let narrow = Limits {
            bandwidth_max: 10e6,
            ..RSA306
        };
        assert_eq!(rates(&narrow).last(), Some(&14e6));
    }

    #[test]
    fn each_rate_asks_for_a_filter_inside_its_own_step() {
        for rate in rates(&RSA306) {
            let bandwidth = bandwidth_for(rate, &RSA306);
            assert!(bandwidth <= rate / 1.4, "{rate}");
            assert!(bandwidth > rate / 2.8 || rate == 13_671.875, "{rate}");
        }
        assert_eq!(bandwidth_for(13_671.875, &RSA306), RSA306.bandwidth_min);
    }

    #[test]
    fn gain_is_the_reference_level_reversed() {
        let caps = capabilities(&RSA306, Fitted::default());
        let stage = caps.stage(GAIN).expect("gain stage");
        assert_eq!((stage.range.min, stage.range.max), (-30.0, 130.0));
        assert_eq!(reference_for_gain(15.0), -15.0);
    }

    #[test]
    fn a_signal_at_the_reference_level_reads_as_full_scale() {
        let volts = ZERO_DBM_VOLTS * 10f64.powf(-30.0 / 20.0);
        assert!((volts as f32 * rx_scale(-30.0) - 1.0).abs() < 1e-6);
    }

    const EVERYTHING: Fitted = Fitted {
        preamp: true,
        attenuator: true,
        gnss: true,
    };

    fn extra_names(fitted: Fitted) -> Vec<String> {
        capabilities(&RSA306, fitted)
            .extra
            .iter()
            .map(|extra| extra.name().to_string())
            .collect()
    }

    fn sources(fitted: Fitted) -> Vec<String> {
        let caps = capabilities(&RSA306, fitted);
        let Some(ExtraSetting::Enum { options, .. }) = caps
            .extra
            .iter()
            .find(|extra| extra.name() == FREQUENCY_SOURCE)
        else {
            panic!("no frequency reference choice");
        };
        options.iter().map(|option| option.value.clone()).collect()
    }

    #[test]
    fn preamp_and_attenuator_are_offered_only_where_they_are_fitted() {
        assert_eq!(extra_names(Fitted::default()), [FREQUENCY_SOURCE]);
        assert_eq!(
            extra_names(EVERYTHING),
            [PREAMP, AUTO_ATTENUATION, FREQUENCY_SOURCE]
        );
        assert!(
            capabilities(&RSA306, Fitted::default())
                .stage(ATTENUATOR)
                .is_none()
        );
    }

    #[test]
    fn the_attenuator_is_a_gain_of_zero_or_less() {
        let caps = capabilities(&RSA306, EVERYTHING);
        let stage = caps.stage(ATTENUATOR).expect("attenuator stage");
        assert_eq!((stage.range.min, stage.range.max), (-51.0, 0.0));
    }

    #[test]
    fn gnss_is_a_frequency_reference_only_where_it_is_fitted() {
        assert_eq!(sources(Fitted::default()), ["internal", "external"]);
        assert_eq!(sources(EVERYTHING), ["internal", "external", "gnss"]);
    }

    #[test]
    fn the_analyser_only_receives() {
        let caps = capabilities(&RSA306, Fitted::default());
        assert_eq!(caps.duplex, Duplex::RxOnly);
        assert_eq!(caps.tx_streams, 0);
        assert_eq!(caps.antennas, vec![INPUT]);
    }
}
