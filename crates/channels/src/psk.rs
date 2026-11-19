use sdrmm_decoder::registry::Native;
use sdrmm_wire::FilterSpec;
use sdrmm_wire::{Control, EventContract, KindSpec, SettingField};
use sdrmm_wire::{HzRange, Modulation, SignalSignature};
mod model;
#[cfg(any(test, feature = "synth"))]
pub mod synth;
pub use model::*;
use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_modem::{
    constellation::tables,
    linear::{
        CarrierLoop, DifferentialDetector, LinearDemod, LinearParams, LinearTiming, PhaseDetector,
    },
    pulse::{self, Norm},
    varicode::VaricodeDecoder,
};
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily};

use sdrmm_decoder::{
    Channel, ChannelCtx, ChannelError, ChannelOutputs, NativeChannel, check_input_rate,
};

const INPUT_RATE_HZ: f64 = 8_000.0;
const FILTER_TAPS: usize = 257;
const TEXT_FLUSH_CHARS: usize = 64;
const BANDWIDTH_FACTOR: f64 = 1.3;
const LOCK_RATE: f32 = 0.01;
const LOCK_MIN: f32 = 0.78;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "psk".to_owned(),
    name: "PSK".to_owned(),
    summary: "PSK31 to PSK250 keyboard chat".to_owned(),
    family: DecoderFamily::AMATEUR,
    verification: sdrmm_wire::Verification::OwnModulator,
    bandwidth_hz: 650.0,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: false,
    emits: vec![PskText::KIND],
    ..ChannelDescriptor::default()
});

fn params(settings: &ChannelSettings) -> Result<PskParams, ChannelError> {
    sdrmm_decoder::settings_of::<PskParams>(settings)
}

fn demodulator(baud: f64) -> Result<LinearDemod, ChannelError> {
    let sps = (INPUT_RATE_HZ / baud).round() as usize;
    let constellation = tables::psk(2)
        .map_err(|error| ChannelError::InvalidSettings(format!("BPSK table: {error}")))?;
    let pulse = pulse::root_raised_cosine(sps as f64, 1.0, 4, Norm::Energy);
    let params = LinearParams::new(constellation, pulse.clone(), sps)
        .map_err(|error| ChannelError::InvalidSettings(format!("PSK waveform: {error}")))?;
    let carrier = CarrierLoop::new(PhaseDetector::MthPower { m: 2 }, 0.01).with_frequency_aid(0.01);
    Ok(LinearDemod::new(
        &params,
        &pulse,
        LinearTiming::CONTINUOUS,
        Some(carrier),
    ))
}

pub(crate) fn occupied_band(p: PskParams) -> (f64, f64) {
    let half = p.baud.rate() * BANDWIDTH_FACTOR;
    (-half, half)
}

pub(crate) fn channel_filter(p: PskParams) -> FilterSpec {
    let (_, half) = occupied_band(p);
    FilterSpec::lowpass(FILTER_TAPS, half)
}

pub struct PskChannel {
    demod: LinearDemod,
    differential: DifferentialDetector,
    decoder: VaricodeText,
    baud: PskBaud,
    invert: bool,
    symbols: Vec<Complex<f32>>,
    products: Vec<Complex<f32>>,
    lock: f32,
    symbols_seen: u32,
}

impl NativeChannel for PskChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }
    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let params = params(&settings)?;
        Ok(Self {
            demod: demodulator(params.baud.rate())?,
            differential: DifferentialDetector::new(),
            decoder: VaricodeText::default(),
            baud: params.baud,
            invert: params.invert,
            symbols: Vec::new(),
            products: Vec::new(),
            lock: 0.0,
            symbols_seen: 0,
        })
    }
}

impl Channel for PskChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let params = params(&settings)?;
        self.demod = demodulator(params.baud.rate())?;
        self.baud = params.baud;
        self.invert = params.invert;
        self.retuned();
        Ok(())
    }

    fn retuned(&mut self) {
        self.demod.reset();
        self.differential.reset();
        self.decoder = VaricodeText::default();
        self.symbols.clear();
        self.products.clear();
        self.lock = 0.0;
        self.symbols_seen = 0;
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.symbols.clear();
        self.demod.process(iq, &mut self.symbols);
        let baud = self.baud.rate();
        out.symbols.linear(
            &self.symbols,
            self.demod.params().constellation(),
            baud,
            self.demod.carrier_freq_cycles_per_symbol() * baud,
        );
        self.products.clear();
        self.differential.process(&self.symbols, &mut self.products);
        for product in &self.products {
            self.symbols_seen = self.symbols_seen.saturating_add(1);
            let rate = LOCK_RATE.max(1.0 / self.symbols_seen as f32);
            self.lock += rate * (binary_phase_fit(*product) - self.lock);
            let mut bit = product.re >= 0.0;
            if self.invert {
                bit = !bit;
            }
            if let Some(text) = self.decoder.feed(bit)
                && self.lock >= LOCK_MIN
            {
                out.events.push(DecoderEvent::of(&PskText {
                    baud: self.baud,
                    text,
                }));
            }
        }
    }
}

fn binary_phase_fit(product: Complex<f32>) -> f32 {
    let magnitude = product.norm();
    if magnitude > 0.0 {
        product.re.abs() / magnitude
    } else {
        0.0
    }
}

#[derive(Default)]
struct VaricodeText {
    decoder: VaricodeDecoder,
    text: String,
}

impl VaricodeText {
    fn feed(&mut self, bit: bool) -> Option<String> {
        self.text.extend(self.decoder.push(bit));
        let line_end = self.text.ends_with('\n') || self.text.ends_with('\r');
        if !self.text.is_empty() && (line_end || self.text.chars().count() >= TEXT_FLUSH_CHARS) {
            Some(std::mem::take(&mut self.text))
        } else {
            None
        }
    }
}

const PSK_SPOTS: &[HzRange] = &[
    HzRange::spot_khz(3_580.0, 3.0),
    HzRange::spot_khz(7_040.0, 3.0),
    HzRange::spot_khz(7_070.0, 3.0),
    HzRange::spot_khz(10_142.0, 3.0),
    HzRange::spot_khz(14_070.0, 3.0),
    HzRange::spot_khz(18_100.0, 3.0),
    HzRange::spot_khz(21_070.0, 3.0),
    HzRange::spot_khz(24_920.0, 3.0),
    HzRange::spot_khz(28_120.0, 3.0),
];

fn psk_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "PSK31",
            &[Modulation::Psk2],
            HzRange::new(20.0, 120.0),
            "31.25 baud phase reversals in a 60 Hz slot",
        )
        .symbol_rate(HzRange::about(31.25, 0.1))
        .on(PSK_SPOTS),
        SignalSignature::new(
            "PSK63",
            &[Modulation::Psk2],
            HzRange::new(50.0, 200.0),
            "62.5 baud phase reversals in a 120 Hz slot",
        )
        .symbol_rate(HzRange::about(62.5, 0.1))
        .on(PSK_SPOTS),
        SignalSignature::new(
            "PSK125",
            &[Modulation::Psk2],
            HzRange::new(100.0, 400.0),
            "125 baud phase reversals in a 250 Hz slot",
        )
        .symbol_rate(HzRange::about(125.0, 0.1))
        .on(PSK_SPOTS),
    ]
}

pub(crate) fn psk_decoder() -> Native<PskParams> {
    let decoder = Native::new::<PskChannel>(|p| occupied_band(*p))
        .filter(|p| Ok(channel_filter(*p)))
        .home(14_070_000.0)
        .signatures(psk_signatures)
        .form(psk_form)
        .kinds(psk_kinds);
    cfg_select! {
        any(test, feature = "synth") => decoder.synth(synth::SIGNALS),
        _ => decoder,
    }
}

pub(crate) fn psk_form() -> Vec<SettingField> {
    vec![
        SettingField::new("baud", "Mode", Control::choice::<PskBaud>()).title("PSK symbol rate"),
        SettingField::invert(),
    ]
}

fn psk_kinds() -> Vec<KindSpec> {
    vec![psk_kind()]
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests {
    use sdrmm_modem::varicode::VARICODE;
    use sdrmm_wire::ChannelParams;

    use super::*;
    use crate::synth;
    use sdrmm_decoder_test_support::settings;

    #[test]
    fn decoder_recovers_varicode_and_flushes_a_line() {
        let mut decoder = VaricodeText::default();
        let mut output = None;
        for ch in "CQ de DL1ABC\n".bytes() {
            for bit in VARICODE[usize::from(ch)].bytes().chain(*b"00") {
                output = decoder.feed(bit == b'1').or(output);
            }
        }
        assert_eq!(output.as_deref(), Some("CQ de DL1ABC\n"));
    }

    fn decoded(baud: PskBaud, iq: &[Complex<f32>]) -> Vec<PskText> {
        let mut channel = PskChannel::new(
            ChannelCtx {
                input_rate: INPUT_RATE_HZ,
            },
            settings(
                ChannelParams::of(&PskParams {
                    baud,
                    invert: false,
                })
                .expect("psk settings"),
            ),
        )
        .unwrap();
        let mut out = ChannelOutputs::default();
        for block in iq.chunks(997) {
            channel.process(block, &mut out);
        }
        out.events
            .into_iter()
            .filter_map(|event| event.read::<PskText>())
            .collect()
    }

    #[test]
    fn a_quiet_band_decodes_to_nothing() {
        for seed in [0x0c0f_fee1, 0x1234_5678, 0xdead_beef] {
            let mut iq = sdrmm_modem::synth::silence((30.0 * INPUT_RATE_HZ) as usize);
            sdrmm_modem::synth::add_noise(&mut iq, seed, 0.3);
            assert_eq!(
                decoded(PskBaud::Psk31, &iq),
                Vec::new(),
                "seed {seed:#x} read text out of noise"
            );
        }
    }

    #[test]
    fn every_baud_fixture_decodes_across_ragged_blocks() {
        for baud in [
            PskBaud::Psk31,
            PskBaud::Psk63,
            PskBaud::Psk125,
            PskBaud::Psk250,
        ] {
            let iq = synth::psk::transmission("CQ de DL1ABC\n", baud.rate())
                .expect("a synthetic signal");
            let events = decoded(baud, &iq);
            let text: String = events.iter().map(|e| e.text.as_str()).collect();
            assert!(text.contains("DL1ABC"), "{baud:?} decoded {text:?}");
            assert!(events.iter().all(|e| e.baud == baud), "{baud:?} tagging");
        }
    }

    #[test]
    fn occupied_band_tracks_the_selected_baud() {
        for baud in [PskBaud::Psk31, PskBaud::Psk250] {
            let (low, high) = occupied_band(PskParams {
                baud,
                invert: false,
            });
            assert!(
                (high - baud.rate() * BANDWIDTH_FACTOR).abs() < 1e-9,
                "{baud:?}"
            );
            assert!((low + high).abs() < 1e-9, "{baud:?}");
            assert!(
                high * 2.0 <= DESCRIPTOR.bandwidth_hz,
                "{baud:?} fits nominal"
            );
        }
    }
}
