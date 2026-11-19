use sdrmm_decoder::registry::Native;
use sdrmm_wire::FilterSpec;
use sdrmm_wire::{Control, NumberSpec, SettingField};
#[cfg(any(test, feature = "fixtures"))]
pub mod fixtures;
mod model;
pub use model::*;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_decoder::signal_id::{
    Catalog, PROBE_BANDWIDTH_HZ, PROBE_ENVELOPE, PROBED_CONFIDENCE, agreement, classify, confirm,
    features, framing, looks_analog, measure, shifts,
};
use sdrmm_dsp::flat_bandwidth_hz;
use sdrmm_dsp::measure::detect;
use sdrmm_wire::{
    ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily, EventKind, IdentReport,
    IdentSignal, Modulation,
};

use sdrmm_decoder::{
    Channel, ChannelCtx, ChannelError, ChannelOutputs, NativeChannel, check_input_rate,
};

const INPUT_RATE_HZ: f64 = 240_000.0;

const CHANNEL_TAPS: usize = 63;

const MAX_WINDOW: usize = 262_144;

const MIN_WINDOW: usize = 4 * detect::DETECT_FFT;

const STEADY_POWER_VARIATION: f64 = 0.3;

const OCCUPANCY_BLOCK: usize = 256;

const KEYED_SPAN_DB: f64 = 10.0;

const HF_TOP_HZ: f64 = 30_000_000.0;

const HF_GAP_HZ: f64 = 500.0;

const GAP_HZ: f64 = 12_000.0;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "ident".to_owned(),
    name: "Signal identifier".to_owned(),
    summary: "Guesses what an unknown signal is".to_owned(),
    family: DecoderFamily::UTILITY,
    verification: sdrmm_wire::Verification::OnAir,
    bandwidth_hz: MAX_IDENT_BANDWIDTH_HZ,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: false,
    emits: vec![EventKind::IDENT],
    ..ChannelDescriptor::default()
});

pub struct IdentChannel {
    params: IdentParams,
    frequency_hz: f64,
    window: Vec<Complex<f32>>,
    pending: usize,
    detector: detect::Detector,
    meter: features::Meter,
    tracker: agreement::Tracker,
    confirmer: confirm::Confirmer,
    catalog: Catalog,
    artifact_hz: Option<f64>,
    block_power: Vec<f64>,
    last_heard: Option<bool>,
}

fn params(settings: &ChannelSettings) -> Result<IdentParams, ChannelError> {
    settings
        .params
        .parse()
        .map_err(ChannelError::InvalidSettings)
}

fn check_params(p: &IdentParams) -> Result<(), ChannelError> {
    let widest = flat_bandwidth_hz(INPUT_RATE_HZ).min(MAX_IDENT_BANDWIDTH_HZ);
    if !(p.bandwidth_hz.is_finite() && (MIN_IDENT_BANDWIDTH_HZ..=widest).contains(&p.bandwidth_hz))
    {
        return Err(ChannelError::InvalidSettings(format!(
            "ident bandwidth must be in [{MIN_IDENT_BANDWIDTH_HZ}, {widest}] Hz, got {}",
            p.bandwidth_hz
        )));
    }
    Ok(())
}

pub(crate) fn occupied_band(p: &IdentParams) -> (f64, f64) {
    let half = p.bandwidth_hz / 2.0;
    (-half, half)
}

pub(crate) fn channel_filter(p: &IdentParams) -> Result<FilterSpec, ChannelError> {
    check_params(p)?;
    let (_, half) = occupied_band(p);
    Ok(FilterSpec::lowpass(CHANNEL_TAPS, half))
}

fn interval_samples(p: &IdentParams) -> usize {
    let wanted = (f64::from(p.interval_ms) / 1_000.0 * INPUT_RATE_HZ) as usize;
    wanted.max(MIN_WINDOW)
}

impl IdentChannel {
    fn restart(&mut self) {
        self.window.clear();
        self.pending = 0;
    }

    fn forget(&mut self) {
        self.restart();
        self.tracker.forget();
        self.confirmer.forget();
        self.last_heard = None;
    }

    fn worth_reporting(&self, report: &IdentReport) -> bool {
        !report.signals.is_empty() || self.last_heard != Some(false)
    }

    fn analyse(&mut self) -> IdentReport {
        let dominated = self.fills_the_span();
        let survey = self.detector.measure(
            &self.window,
            INPUT_RATE_HZ,
            &detect::Search {
                half_span_hz: self.params.bandwidth_hz / 2.0,
                threshold_db: self.params.threshold_db,
                gap_hz: self.gap_hz(),
                dominated,
                artifact_hz: self.artifact_hz,
            },
        );
        let signals = survey
            .bands
            .iter()
            .map(|band| self.describe(band))
            .collect();
        self.tracker.sweep();
        self.confirmer.sweep();
        IdentReport {
            snr_db: survey.peak_db - survey.floor_db,
            signals,
        }
    }

    fn describe(&mut self, band: &detect::Band) -> IdentSignal {
        let frequency_hz = self.frequency_hz + band.center_hz;
        let dial = (self.frequency_hz > 0.0).then_some(frequency_hz);
        let located = IdentSignal {
            modulation: Modulation::Unknown,
            frequency_hz,
            center_offset_hz: band.center_hz,
            bandwidth_hz: band.bandwidth_hz,
            snr_db: band.snr_db,
            ..IdentSignal::default()
        };
        let Some(zoom) = features::zoom(&self.window, INPUT_RATE_HZ, band) else {
            return located;
        };
        let waveform = self.meter.measure(&zoom, band);
        let mut verdict = self
            .tracker
            .settle(band, classify::classify(band, &waveform));
        let mut candidates = self
            .catalog
            .candidates(verdict.modulation, &measure(band, &waveform, dial));
        framing::confirm(
            &self.catalog,
            &mut candidates,
            &self.window,
            INPUT_RATE_HZ,
            band,
        );
        if looks_analog(verdict.modulation)
            && band.bandwidth_hz <= PROBE_BANDWIDTH_HZ
            && waveform.envelope_variation <= PROBE_ENVELOPE
            && let Some(probe) = framing::probe(&self.catalog, &self.window, INPUT_RATE_HZ, band)
        {
            verdict.modulation = probe.modulation;
            verdict.confidence = verdict.confidence.max(PROBED_CONFIDENCE);
            verdict.sideband = None;
            candidates.splice(0..0, probe.matches);
        }
        self.confirmer.confirm(
            &mut candidates,
            &self.catalog,
            &self.window,
            INPUT_RATE_HZ,
            band,
            frequency_hz,
        );

        IdentSignal {
            modulation: verdict.modulation,
            confidence: verdict.confidence,
            sideband: verdict.sideband,
            symbol_rate_hz: waveform.symbol_rate_hz,
            deviation_hz: shifts(verdict.modulation).then_some(waveform.deviation_hz),
            burst_ms: waveform.burst_ms,
            burst_period_ms: waveform.burst_period_ms,
            ofdm_symbol_us: waveform.ofdm_symbol_us,
            ofdm_guard_us: waveform.ofdm_guard_us,
            candidates,
            features: features(band, &waveform),
            ..located
        }
    }

    fn gap_hz(&self) -> f64 {
        if self.frequency_hz > 0.0 && self.frequency_hz < HF_TOP_HZ {
            HF_GAP_HZ
        } else {
            GAP_HZ
        }
    }

    fn fills_the_span(&mut self) -> bool {
        steady(&self.window)
            || self.keyed_blocks()
            || self
                .meter
                .cyclic_prefix(&self.window, INPUT_RATE_HZ, self.params.bandwidth_hz)
                .is_some()
    }

    fn keyed_blocks(&mut self) -> bool {
        self.block_power.clear();
        self.block_power.extend(
            self.window
                .as_chunks::<OCCUPANCY_BLOCK>()
                .0
                .iter()
                .map(|block| block.iter().map(|s| f64::from(s.norm_sqr())).sum::<f64>()),
        );
        if self.block_power.len() < 8 {
            return false;
        }
        let quiet = quantile(&mut self.block_power, 0.1);
        let loud = quantile(&mut self.block_power, 0.9);
        quiet > 0.0 && 10.0 * (loud / quiet).log10() >= KEYED_SPAN_DB
    }
}

fn steady(iq: &[Complex<f32>]) -> bool {
    let mut sum = 0.0f64;
    let mut sum_sq = 0.0f64;
    for sample in iq {
        let power = f64::from(sample.norm_sqr());
        sum += power;
        sum_sq += power * power;
    }
    let n = iq.len() as f64;
    if n < 2.0 || sum <= 0.0 {
        return false;
    }
    let mean = sum / n;
    let variation = (sum_sq / n - mean * mean).max(0.0).sqrt() / mean;
    variation < STEADY_POWER_VARIATION
}

fn quantile(values: &mut [f64], fraction: f64) -> f64 {
    let index = ((values.len() - 1) as f64 * fraction) as usize;
    let (_, value, _) = values.select_nth_unstable_by(index, f64::total_cmp);
    *value
}

impl NativeChannel for IdentChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }
    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let params = params(&settings)?;
        check_params(&params)?;
        Ok(Self {
            window: Vec::with_capacity(interval_samples(&params).min(MAX_WINDOW)),
            params,
            frequency_hz: settings.frequency_hz,
            pending: 0,
            detector: detect::Detector::new(),
            meter: features::Meter::new(),
            tracker: agreement::Tracker::new(),
            confirmer: confirm::Confirmer::new(),
            catalog: Catalog::registered(),
            artifact_hz: None,
            block_power: Vec::with_capacity(MAX_WINDOW / OCCUPANCY_BLOCK),
            last_heard: None,
        })
    }
}

impl Channel for IdentChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let params = params(&settings)?;
        check_params(&params)?;
        if interval_samples(&params) != interval_samples(&self.params)
            || settings.frequency_hz != self.frequency_hz
        {
            self.forget();
        }
        self.params = params;
        self.frequency_hz = settings.frequency_hz;
        Ok(())
    }

    fn retuned(&mut self) {
        self.forget();
    }

    fn lo_artifact_at(&mut self, offset_hz: Option<f64>) {
        if offset_hz != self.artifact_hz {
            self.artifact_hz = offset_hz;
            self.forget();
        }
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        let interval = interval_samples(&self.params);
        let mut rest = iq;
        while !rest.is_empty() {
            let take = (interval - self.pending).min(rest.len());
            self.window.extend_from_slice(&rest[..take]);
            self.pending += take;
            rest = &rest[take..];
            if self.window.len() > MAX_WINDOW {
                self.window.drain(..self.window.len() - MAX_WINDOW);
            }
            if self.pending >= interval {
                let report = self.analyse();
                if self.worth_reporting(&report) {
                    self.last_heard = Some(!report.signals.is_empty());
                    out.events.push(DecoderEvent::of(&report));
                }
                self.restart();
            }
        }
    }
}

pub(crate) fn ident_decoder() -> Native<IdentParams> {
    Native::new::<IdentChannel>(occupied_band)
        .filter(channel_filter)
        .form(ident_form)
}

const BANDWIDTHS_HZ: [f64; 4] = [12_500.0, 50_000.0, 100_000.0, 192_000.0];

pub(crate) fn ident_form() -> Vec<SettingField> {
    vec![
        SettingField::new(
            "bandwidth_hz",
            "Width",
            Control::values(&BANDWIDTHS_HZ, sdrmm_wire::units::hertz),
        )
        .title("Search bandwidth"),
        SettingField::new(
            "interval_ms",
            "Every",
            NumberSpec::new(
                f64::from(MIN_IDENT_INTERVAL_MS),
                f64::from(MAX_IDENT_INTERVAL_MS),
                250.0,
            )
            .unit("ms"),
        )
        .title("Milliseconds of signal each report is measured from"),
        SettingField::new(
            "threshold_db",
            "Detect",
            NumberSpec::new(
                f64::from(MIN_IDENT_THRESHOLD_DB),
                f64::from(MAX_IDENT_THRESHOLD_DB),
                1.0,
            )
            .unit("dB"),
        )
        .title("Decibels above the noise floor a signal must reach"),
    ]
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests {
    use sdrmm_decoder::{Channel, ChannelCtx, ChannelOutputs, NativeChannel};
    use sdrmm_decoder_test_support::{complex_noise, settings};
    use sdrmm_wire::ChannelParams;

    use super::{INPUT_RATE_HZ, IdentChannel, IdentParams, MAX_WINDOW};

    #[test]
    fn a_long_interval_holds_no_more_than_one_window() {
        let long = IdentParams {
            interval_ms: 2_000,
            ..IdentParams::default()
        };
        let params = ChannelParams::new("ident", &long).expect("ident settings");
        let ctx = ChannelCtx {
            input_rate: INPUT_RATE_HZ,
        };
        let mut channel = IdentChannel::new(ctx, settings(params)).expect("ident channel");
        let mut out = ChannelOutputs::default();
        channel.process(&complex_noise(0x77c2, 0.02, MAX_WINDOW + 1), &mut out);
        assert_eq!(channel.window.len(), MAX_WINDOW);
        assert!(out.events.is_empty());
    }
}
