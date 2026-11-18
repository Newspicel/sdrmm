//! Signal ID: names a band found in a spectrum by its modulation and measures, and offers the
//! registered decoders that may read it. Everything it matches against comes from the
//! registry: each decoder's signatures, frame syncs and chirp sweeps.

pub mod agreement;
pub mod chirp;
pub mod classify;
pub mod confirm;
pub mod features;
pub mod framing;

use num_complex::Complex;
use sdrmm_dsp::measure::detect;
use sdrmm_wire::{IdentFeatures, IdentSignal, Modulation, SignalMeasure};

pub use crate::catalog::Catalog;
use detect::Band;
use features::{Meter, Waveform};

/// The widest band a probe for keyed framing listens to.
pub const PROBE_BANDWIDTH_HZ: f64 = 30_000.0;

/// The confidence a modulation found by its framing carries.
pub const PROBED_CONFIDENCE: f32 = 0.9;

/// The most envelope variation a signal may show and still be probed for framing.
pub const PROBE_ENVELOPE: f32 = 0.3;

/// What a catalog matches: the band's width and the waveform's measures.
#[must_use]
pub fn measure(band: &Band, waveform: &Waveform, frequency_hz: Option<f64>) -> SignalMeasure {
    SignalMeasure {
        bandwidth_hz: band.bandwidth_hz,
        symbol_rate_hz: waveform.symbol_rate_hz,
        deviation_hz: waveform.deviation_hz,
        ofdm_symbol_us: waveform.ofdm_symbol_us,
        burst_ms: waveform.burst_ms,
        frequency_hz,
    }
}

/// The features a report shows of `band` and `waveform`.
#[must_use]
pub fn features(band: &Band, waveform: &Waveform) -> IdentFeatures {
    IdentFeatures {
        envelope_variation: waveform.envelope_variation,
        duty: waveform.duty,
        keying_depth_db: waveform.on_off_db,
        spectral_asymmetry: band.skew,
        carrier_db: band.carrier_db,
        spectral_flatness: band.flatness,
        frequency_levels: waveform.frequency_levels,
        frequency_spread_hz: waveform.frequency_spread_hz,
        square_line_db: waveform.square_line_db,
        quartic_line_db: waveform.quartic_line_db,
    }
}

/// A modulation an analog voice could be, worth probing for keyed framing.
#[must_use]
pub const fn looks_analog(modulation: Modulation) -> bool {
    matches!(
        modulation,
        Modulation::Fm | Modulation::Am | Modulation::Unknown | Modulation::NoiseLike
    )
}

/// A modulation that moves the carrier's frequency, so its deviation means something.
#[must_use]
pub const fn shifts(modulation: Modulation) -> bool {
    matches!(
        modulation,
        Modulation::Fm | Modulation::Fsk2 | Modulation::Fsk4 | Modulation::Fsk8
    )
}

/// Names a band found in a spectrum: its modulation, measures and the decoders that may read
/// it. Its scratch carries from one band to the next.
#[derive(Default)]
pub struct Identifier {
    meter: Meter,
    chirps: chirp::Chirps,
}

impl Identifier {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `band` of `iq`, sampled at `rate` around `center`, matched against `catalog`.
    pub fn identify(
        &mut self,
        iq: &[Complex<f32>],
        rate: f64,
        band: &Band,
        center: f64,
        catalog: &Catalog,
    ) -> IdentSignal {
        let mut signal = IdentSignal {
            frequency_hz: center + band.center_hz,
            center_offset_hz: band.center_hz,
            bandwidth_hz: band.bandwidth_hz,
            snr_db: band.snr_db,
            ..IdentSignal::default()
        };
        let Some(zoom) = features::zoom(iq, rate, band) else {
            return signal;
        };
        let waveform = self.meter.measure(&zoom, band);
        let chirp = self.chirps.find(&zoom, band, catalog.chirps());
        let verdict = chirp.map_or_else(|| classify::classify(band, &waveform), chirp::verdict);
        signal.modulation = verdict.modulation;
        signal.confidence = verdict.confidence;
        signal.sideband = verdict.sideband;
        signal.symbol_rate_hz = chirp
            .map(|chirp| chirp.symbol_rate_hz())
            .or(waveform.symbol_rate_hz);
        signal.bandwidth_hz = chirp.map_or(band.bandwidth_hz, |chirp| chirp.bandwidth_hz);
        signal.deviation_hz = shifts(verdict.modulation).then_some(waveform.deviation_hz);
        signal.burst_ms = waveform.burst_ms;
        signal.burst_period_ms = waveform.burst_period_ms;
        signal.ofdm_symbol_us = waveform.ofdm_symbol_us;
        signal.ofdm_guard_us = waveform.ofdm_guard_us;
        signal.features = features(band, &waveform);
        signal.candidates = catalog.candidates(
            verdict.modulation,
            &measure(band, &waveform, Some(signal.frequency_hz)),
        );
        framing::confirm(catalog, &mut signal.candidates, iq, rate, band);
        if (looks_analog(signal.modulation) || signal.modulation == Modulation::Ook)
            && band.bandwidth_hz <= PROBE_BANDWIDTH_HZ
            && waveform.envelope_variation <= PROBE_ENVELOPE
            && let Some(probe) = framing::probe(catalog, iq, rate, band)
        {
            signal.modulation = probe.modulation;
            signal.confidence = PROBED_CONFIDENCE;
            signal.candidates.splice(0..0, probe.matches);
        }
        signal
    }
}
