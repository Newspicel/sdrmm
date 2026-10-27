use std::{f64::consts::TAU, sync::LazyLock};

use num_complex::Complex;
use sdrmm_wire::{
    ChannelDescriptor, ChannelParams, ChannelSettings, DecoderEvent, DecoderFamily, EventKind,
    VorParams, VorReading,
};

use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, check_input_rate, datalink,
};

const RATE: f64 = 48_000.0;
const HALF_BANDWIDTH: f64 = 12_000.0;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "vor".to_owned(),
    name: "VOR".to_owned(),
    summary: "Bearing to a VOR beacon".to_owned(),
    family: DecoderFamily::Aviation,
    bandwidth_hz: HALF_BANDWIDTH * 2.0,
    input_rate_hz: RATE,
    has_audio: false,
    emits: vec![EventKind::Vor],
    ..ChannelDescriptor::default()
});

#[derive(Clone, Copy, Default)]
struct ToneSum {
    i: f64,
    q: f64,
    signal: f64,
}

impl ToneSum {
    fn add(&mut self, signal: f64, cos_30: f64, sin_30: f64) {
        self.i += signal * cos_30;
        self.q += signal * sin_30;
        self.signal += signal;
    }

    fn phasor(&self, basis: Complex<f64>, count: f64) -> Complex<f64> {
        let mean = self.signal / count;
        Complex::new(self.i - mean * basis.re, self.q - mean * basis.im)
    }
}

pub struct VorChannel {
    params: VorParams,
    phase_30: f64,
    phase_subcarrier: f64,
    power: f64,
    subcarrier: [Complex<f64>; 4],
    previous_subcarrier: Complex<f64>,
    basis_30: Complex<f64>,
    variable: ToneSum,
    reference: ToneSum,
    subcarrier_level: f64,
    samples: usize,
    settled: bool,
}

fn params(settings: &ChannelSettings) -> Result<&VorParams, ChannelError> {
    match &settings.params {
        ChannelParams::Vor(params) => Ok(params),
        other => Err(ChannelError::InvalidSettings(format!(
            "VOR channel got {} params",
            other.type_id()
        ))),
    }
}

fn check_params(params: &VorParams) -> Result<(), ChannelError> {
    datalink::check_navaid_report_ms(params.report_ms)?;
    match (params.station_lat, params.station_lon) {
        (None, None) | (Some(_), Some(_)) => Ok(()),
        _ => Err(ChannelError::InvalidSettings(
            "VOR station latitude and longitude must be given together".to_owned(),
        )),
    }
}

pub(crate) fn occupied_band() -> (f64, f64) {
    (-HALF_BANDWIDTH, HALF_BANDWIDTH)
}

pub(crate) fn channel_filter() -> ChannelFilter {
    datalink::channel_filter(RATE, HALF_BANDWIDTH)
}

impl ChannelRx for VorChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let params = params(&settings)?.clone();
        check_params(&params)?;
        Ok(Self::build(params))
    }

    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let params = params(&settings)?.clone();
        check_params(&params)?;
        self.params = params;
        Ok(())
    }

    fn retuned(&mut self) {
        let params = self.params.clone();
        *self = Self::build(params);
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        let alpha_dc = 1.0 - (-TAU * 3.0 / RATE).exp();
        let alpha_subcarrier = 1.0 - (-TAU * 1_200.0 / RATE).exp();
        let step_30 = TAU * 30.0 / RATE;
        let step_subcarrier = TAU * 9_960.0 / RATE;
        for sample in iq {
            let envelope = f64::from(sample.norm());
            self.power += alpha_dc * (envelope * envelope - self.power);
            let (sin_30, cos_30) = self.phase_30.sin_cos();
            self.basis_30 += Complex::new(cos_30, sin_30);
            self.variable.add(envelope, cos_30, sin_30);
            let (sin_subcarrier, cos_subcarrier) = self.phase_subcarrier.sin_cos();
            let mut filtered = Complex::new(envelope * cos_subcarrier, -envelope * sin_subcarrier);
            for stage in &mut self.subcarrier {
                *stage += (filtered - *stage) * alpha_subcarrier;
                filtered = *stage;
            }
            let discriminator = (filtered * self.previous_subcarrier.conj()).arg() * RATE / TAU;
            self.previous_subcarrier = filtered;
            self.reference.add(discriminator, cos_30, sin_30);
            self.subcarrier_level += filtered.norm();
            self.phase_30 = (self.phase_30 + step_30) % TAU;
            self.phase_subcarrier = (self.phase_subcarrier + step_subcarrier) % TAU;
            self.samples += 1;
            if self.samples >= self.report_samples() {
                if self.settled {
                    self.report(out);
                }
                self.settled = true;
                self.clear_window();
            }
        }
    }
}

impl VorChannel {
    fn build(params: VorParams) -> Self {
        Self {
            params,
            phase_30: 0.0,
            phase_subcarrier: 0.0,
            power: 0.0,
            subcarrier: [Complex::default(); 4],
            previous_subcarrier: Complex::default(),
            basis_30: Complex::default(),
            variable: ToneSum::default(),
            reference: ToneSum::default(),
            subcarrier_level: 0.0,
            samples: 0,
            settled: false,
        }
    }

    fn report_samples(&self) -> usize {
        (RATE * f64::from(self.params.report_ms) / 1_000.0) as usize
    }

    fn report(&self, out: &mut ChannelOutputs) {
        let count = self.samples as f64;
        let variable = self.variable.phasor(self.basis_30, count);
        let reference = self.reference.phasor(self.basis_30, count);
        let mean_envelope = (self.variable.signal / count).max(1e-9);
        let variable_amplitude = 2.0 * variable.norm() / count;
        let reference_deviation = 2.0 * reference.norm() / count;
        let modulation = variable_amplitude / mean_envelope;
        let subcarrier = self.subcarrier_level / count / mean_envelope;
        let confidence = (modulation / 0.3)
            .min(1.0)
            .min((reference_deviation / 480.0).min(1.0))
            .min((subcarrier / 0.15).min(1.0))
            .max(0.0) as f32;
        if confidence < 0.15 {
            return;
        }
        let variable_phase = variable.arg().to_degrees();
        let reference_phase = reference.arg().to_degrees();
        let alpha = 1.0 - (-TAU * 1_200.0 / RATE).exp();
        let omega = TAU * 30.0 / RATE;
        let pole = 1.0 - alpha;
        let filter_lag = 4.0
            * (pole * omega.sin())
                .atan2(1.0 - pole * omega.cos())
                .to_degrees();
        let radial = (variable_phase - reference_phase + filter_lag).rem_euclid(360.0);
        out.events.push(DecoderEvent::Vor(VorReading {
            station: self.params.station.clone(),
            station_lat: self.params.station_lat,
            station_lon: self.params.station_lon,
            magnetic_declination_deg: self.params.magnetic_declination_deg,
            radial_deg: radial,
            variable_phase_deg: variable_phase.rem_euclid(360.0),
            reference_phase_deg: reference_phase.rem_euclid(360.0),
            signal_db: (10.0 * self.power.max(1e-12).log10()) as f32,
            confidence,
        }));
    }

    fn clear_window(&mut self) {
        self.basis_30 = Complex::default();
        self.variable = ToneSum::default();
        self.reference = ToneSum::default();
        self.subcarrier_level = 0.0;
        self.samples = 0;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::testutil::{
        THROUGHPUT_ATTEMPTS, fastest_seconds, realtime_budget, run_events, settings,
    };

    const ANNEX_10_TONE_HZ: f64 = 30.0;
    const ANNEX_10_SUBCARRIER_HZ: f64 = 9_960.0;
    const ANNEX_10_MODULATION_DEPTH: f64 = 0.30;
    const ANNEX_10_DEVIATION_RATIO: f64 = 16.0;
    const ANNEX_10_DEVIATION_RATIO_LIMITS: [f64; 3] = [15.0, 16.0, 17.0];
    const PATTERN_LAGS: f64 = -1.0;
    const PATTERN_LEADS: f64 = 1.0;
    const PERF_SECONDS: f64 = 5.0;
    const MIN_REALTIME_SPEEDUP: f64 = 320.0;

    fn annex_10_conventional_vor(
        bearing_deg: f64,
        deviation_ratio: f64,
        rotation: f64,
        seconds: usize,
    ) -> Vec<Complex<f32>> {
        (0..RATE as usize * seconds)
            .map(|index| {
                let time = index as f64 / RATE;
                let reference = TAU * ANNEX_10_TONE_HZ * time;
                let variable = reference + rotation * bearing_deg.to_radians();
                let subcarrier =
                    TAU * ANNEX_10_SUBCARRIER_HZ * time + deviation_ratio * reference.sin();
                let envelope = 1.0
                    + ANNEX_10_MODULATION_DEPTH * variable.cos()
                    + ANNEX_10_MODULATION_DEPTH * subcarrier.cos();
                Complex::new(envelope as f32, 0.0)
            })
            .collect()
    }

    fn last_reading(iq: &[Complex<f32>]) -> VorReading {
        let mut channel = VorChannel::new(
            ChannelCtx { input_rate: RATE },
            settings(ChannelParams::Vor(VorParams::default())),
        )
        .expect("channel");
        run_events(&mut channel, iq)
            .into_iter()
            .filter_map(|event| match event {
                DecoderEvent::Vor(reading) => Some(reading),
                _ => None,
            })
            .next_back()
            .expect("VOR reading")
    }

    fn bearing_error(reading: &VorReading, expected_deg: f64) -> f64 {
        (reading.radial_deg - expected_deg + 180.0).rem_euclid(360.0) - 180.0
    }

    #[test]
    fn an_annex_10_conventional_vor_signal_reads_its_own_bearing() {
        for bearing in [0.0, 17.0, 90.0, 213.0, 345.0] {
            let iq = annex_10_conventional_vor(bearing, ANNEX_10_DEVIATION_RATIO, PATTERN_LAGS, 2);
            let reading = last_reading(&iq);
            let error = bearing_error(&reading, bearing);
            assert!(error.abs() < 0.5, "{bearing} => {}", reading.radial_deg);
            assert!(reading.confidence > 0.8, "{bearing} => {reading:?}");
        }
    }

    #[test]
    fn a_pattern_turning_the_other_way_reads_the_reciprocal_bearing() {
        for bearing in [37.0, 120.0, 250.0] {
            let iq = annex_10_conventional_vor(bearing, ANNEX_10_DEVIATION_RATIO, PATTERN_LEADS, 2);
            let reading = last_reading(&iq);
            let error = bearing_error(&reading, 360.0 - bearing);
            assert!(error.abs() < 0.5, "{bearing} => {}", reading.radial_deg);
        }
    }

    #[test]
    fn both_ends_of_the_annex_10_deviation_ratio_tolerance_still_read_the_bearing() {
        for ratio in ANNEX_10_DEVIATION_RATIO_LIMITS {
            let iq = annex_10_conventional_vor(123.0, ratio, PATTERN_LAGS, 2);
            let reading = last_reading(&iq);
            let error = bearing_error(&reading, 123.0);
            assert!(error.abs() < 1.0, "ratio {ratio} => {}", reading.radial_deg);
            assert!(reading.confidence > 0.3, "ratio {ratio} => {reading:?}");
        }
    }

    #[test]
    fn measures_the_radial_from_an_analytic_vor_signal() {
        for radial in [0.0, 90.0, 123.0, 270.0] {
            let params = VorParams {
                station: Some("TST".to_owned()),
                ..VorParams::default()
            };
            let mut channel = VorChannel::new(
                ChannelCtx { input_rate: RATE },
                settings(ChannelParams::Vor(params)),
            )
            .expect("channel");
            let events = run_events(&mut channel, &crate::synth::vor::transmission(radial, 2));
            let reading = events
                .iter()
                .filter_map(|event| match event {
                    DecoderEvent::Vor(reading) => Some(reading),
                    _ => None,
                })
                .next_back()
                .expect("VOR reading");
            let error = (reading.radial_deg - radial + 180.0).rem_euclid(360.0) - 180.0;
            assert!(error.abs() < 0.5, "{radial} => {}", reading.radial_deg);
            assert!(reading.confidence > 0.8);
        }
    }

    #[test]
    fn measures_the_radial_at_the_shortest_report_interval() {
        for radial in [0.0, 37.0, 123.0, 301.0] {
            let params = VorParams {
                report_ms: sdrmm_wire::MIN_NAVAID_REPORT_MS,
                ..VorParams::default()
            };
            let mut channel = VorChannel::new(
                ChannelCtx { input_rate: RATE },
                settings(ChannelParams::Vor(params)),
            )
            .expect("channel");
            let events = run_events(&mut channel, &crate::synth::vor::transmission(radial, 2));
            let reading = events
                .iter()
                .filter_map(|event| match event {
                    DecoderEvent::Vor(reading) => Some(reading),
                    _ => None,
                })
                .next_back()
                .expect("VOR reading");
            let error = (reading.radial_deg - radial + 180.0).rem_euclid(360.0) - 180.0;
            assert!(error.abs() < 1.0, "{radial} => {}", reading.radial_deg);
        }
    }

    #[test]
    fn a_report_interval_outside_the_wire_range_is_rejected() {
        for report_ms in [0, sdrmm_wire::MAX_NAVAID_REPORT_MS + 1] {
            let params = VorParams {
                report_ms,
                ..VorParams::default()
            };
            assert!(
                VorChannel::new(
                    ChannelCtx { input_rate: RATE },
                    settings(ChannelParams::Vor(params.clone())),
                )
                .is_err(),
                "{report_ms}"
            );
            let mut channel = VorChannel::new(
                ChannelCtx { input_rate: RATE },
                settings(ChannelParams::Vor(VorParams::default())),
            )
            .expect("channel");
            assert!(
                channel.apply(settings(ChannelParams::Vor(params))).is_err(),
                "{report_ms}"
            );
        }
    }

    #[test]
    fn processing_keeps_ahead_of_the_channel_rate() {
        let iq = crate::synth::vor::transmission(45.0, PERF_SECONDS as usize);
        let elapsed = fastest_seconds(THROUGHPUT_ATTEMPTS, || {
            let mut channel = VorChannel::new(
                ChannelCtx { input_rate: RATE },
                settings(ChannelParams::Vor(VorParams::default())),
            )
            .expect("channel");
            let started = Instant::now();
            let events = run_events(&mut channel, &iq);
            assert!(!events.is_empty());
            started.elapsed().as_secs_f64()
        });
        assert!(
            elapsed < realtime_budget(PERF_SECONDS / MIN_REALTIME_SPEEDUP),
            "{PERF_SECONDS} s of signal took {elapsed:.4} s, {:.1}x realtime",
            PERF_SECONDS / elapsed
        );
    }
}
