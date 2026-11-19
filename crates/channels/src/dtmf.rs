use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_decoder::registry::Native;
use sdrmm_decoder::{
    Channel, ChannelCtx, ChannelError, ChannelOutputs, NativeChannel, check_input_rate,
};
use sdrmm_dsp::{DtmfDetector, DtmfPress};
use sdrmm_modem::analog::{AngleDemod, AngleDetector, AngleKind, AngleParams, AngleRx};
use sdrmm_wire::{
    ChannelDescriptor, ChannelSettings, Control, DecoderEvent, DecoderFamily, EventContract,
    FilterSpec, HzRange, KindSpec, Modulation, SettingField, SignalSignature,
};

mod model;
#[cfg(any(test, feature = "synth"))]
pub mod synth;
pub use model::*;

const INPUT_RATE_HZ: f64 = 48_000.0;
const BANDWIDTH_HZ: f64 = 12_500.0;
const DEVIATION_HZ: f64 = 2_500.0;
const CHANNEL_TAPS: usize = 129;
/// Longer runs go out as one sequence each, so the buffer never grows.
const MAX_DIGITS: usize = 64;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "dtmf".to_owned(),
    name: "DTMF".to_owned(),
    summary: "Touch-tone digits over FM".to_owned(),
    family: DecoderFamily::PAGING,
    verification: sdrmm_wire::Verification::OnAir,
    bandwidth_hz: BANDWIDTH_HZ,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: false,
    emits: vec![DtmfSequence::KIND],
    ..ChannelDescriptor::default()
});

pub struct DtmfChannel {
    demod: AngleDemod,
    audio: Vec<f32>,
    decoder: Decoder,
}

fn params(settings: &ChannelSettings) -> Result<DtmfParams, ChannelError> {
    sdrmm_decoder::settings_of::<DtmfParams>(settings)
}

fn occupied_band() -> (f64, f64) {
    (-BANDWIDTH_HZ / 2.0, BANDWIDTH_HZ / 2.0)
}

fn demodulator() -> AngleDemod {
    AngleDemod::new(
        &AngleParams::new(
            AngleKind::Fm {
                deviation: DEVIATION_HZ / INPUT_RATE_HZ,
            },
            3_000.0 / INPUT_RATE_HZ,
        ),
        &AngleRx::detector_only(AngleDetector::Discriminator),
    )
}

impl NativeChannel for DtmfChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        Ok(Self {
            demod: demodulator(),
            audio: Vec::new(),
            decoder: Decoder::new(params(&settings)?, INPUT_RATE_HZ),
        })
    }
}

impl Channel for DtmfChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        self.decoder.apply(params(&settings)?);
        Ok(())
    }

    fn retuned(&mut self) {
        self.demod = demodulator();
        self.decoder.reset();
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.demod.process(iq, &mut self.audio);
        for &sample in &self.audio {
            self.decoder.push(sample, out);
        }
    }
}

/// Groups the detector's presses into sequences split by a silence of the channel's gap.
struct Decoder {
    params: DtmfParams,
    rate: f64,
    detector: DtmfDetector,
    gap: u64,
    presses: Vec<DtmfPress>,
    spans: Vec<u64>,
}

impl Decoder {
    fn new(params: DtmfParams, rate: f64) -> Self {
        Self {
            params,
            rate,
            detector: DtmfDetector::new(rate, params.pace.timing()),
            gap: (rate * f64::from(params.gap_ms) / 1_000.0).round() as u64,
            presses: Vec::with_capacity(MAX_DIGITS),
            spans: Vec::with_capacity(MAX_DIGITS),
        }
    }

    fn apply(&mut self, params: DtmfParams) {
        if params != self.params {
            *self = Self::new(params, self.rate);
        }
    }

    fn reset(&mut self) {
        self.detector.reset();
        self.presses.clear();
    }

    fn push(&mut self, sample: f32, out: &mut ChannelOutputs) {
        if let Some(press) = self.detector.push(sample) {
            let apart = self
                .presses
                .last()
                .is_some_and(|last| press.start.saturating_sub(last.end) >= self.gap);
            if apart || self.presses.len() == MAX_DIGITS {
                self.finish(out);
            }
            self.presses.push(press);
        } else if !self.detector.busy()
            && self
                .presses
                .last()
                .is_some_and(|last| self.detector.clock().saturating_sub(last.end) >= self.gap)
        {
            self.finish(out);
        }
    }

    fn finish(&mut self, out: &mut ChannelOutputs) {
        if self.presses.is_empty() {
            return;
        }
        let digits = self
            .presses
            .iter()
            .map(|press| press.key.symbol())
            .collect();
        self.spans.clear();
        self.spans
            .extend(self.presses.iter().map(DtmfPress::samples));
        let tone_ms = self.median_ms().unwrap_or_default();
        self.spans.clear();
        self.spans.extend(
            self.presses
                .windows(2)
                .map(|pair| pair[1].start.saturating_sub(pair[0].end)),
        );
        let gap_ms = self.median_ms();
        out.events.push(DecoderEvent::of(&DtmfSequence {
            digits,
            tone_ms,
            gap_ms,
        }));
        self.presses.clear();
    }

    fn median_ms(&mut self) -> Option<u32> {
        self.spans.sort_unstable();
        let median = *self.spans.get(self.spans.len() / 2)?;
        Some((median as f64 * 1_000.0 / self.rate).round() as u32)
    }
}

const DTMF_BANDS: &[HzRange] = &[HzRange::mhz(136.0, 174.0), HzRange::mhz(400.0, 470.0)];

fn dtmf_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "DTMF (tone pairs over FM)",
            &[Modulation::Fm],
            HzRange::new(6_000.0, 25_000.0),
            "pairs of audio tones inside an FM channel",
        )
        .deviation(HzRange::new(1_000.0, 5_500.0))
        .on(DTMF_BANDS)
        .prior(0.9),
    ]
}

fn dtmf_trial() -> sdrmm_wire::SignalTrial {
    let settings = DtmfPace::ALL
        .into_iter()
        .filter_map(|pace| {
            sdrmm_wire::ChannelParams::of(&DtmfParams {
                pace,
                ..DtmfParams::default()
            })
            .ok()
        })
        .collect();
    sdrmm_wire::SignalTrial::proven_by(DtmfSequence::KIND).trying(settings)
}

pub(crate) fn dtmf_decoder() -> Native<DtmfParams> {
    let decoder = Native::new::<DtmfChannel>(|_| occupied_band())
        .filter(|_| Ok(FilterSpec::lowpass(CHANNEL_TAPS, BANDWIDTH_HZ / 2.0)))
        .signatures(dtmf_signatures)
        .trial(dtmf_trial)
        .form(dtmf_form)
        .kinds(dtmf_kinds);
    cfg_select! {
        any(test, feature = "synth") => decoder.synth(synth::SIGNALS),
        _ => decoder,
    }
}

fn dtmf_form() -> Vec<SettingField> {
    vec![
        SettingField::new("pace", "Timing", Control::choice::<DtmfPace>())
            .title("Q.24 takes tones from 40 ms, Fast from 20 ms"),
        SettingField::new(
            "gap_ms",
            "Gap",
            Control::values(&DTMF_GAPS_MS, |ms| format!("{} s", f64::from(ms) / 1_000.0)),
        )
        .title("Silence that ends a sequence"),
    ]
}

fn dtmf_kinds() -> Vec<KindSpec> {
    vec![dtmf_kind()]
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;
