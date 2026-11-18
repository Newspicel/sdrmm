use std::sync::LazyLock;

use sdrmm_wire::{ChannelParams, EncoderDescriptor};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::*;
use crate::encoder::{
    EncoderBase, EncoderError, EncoderParams, Modulator, NativeEncoder, NativeModulator, TxPayload,
    encoder, encoder_refusal, encoder_registry, install_encoders,
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct ToneParams {
    width_hz: f64,
}

static TONE: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "tone".to_owned(),
    input_rate_hz: 48_000.0,
    ..ChannelDescriptor::default()
});

struct ToneChannel;

impl Channel for ToneChannel {
    fn apply(&mut self, _: ChannelSettings) -> Result<(), ChannelError> {
        Ok(())
    }

    fn process(&mut self, _: &[Complex<f32>], _: &mut ChannelOutputs) {}
}

impl NativeChannel for ToneChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &TONE
    }

    fn new(ctx: ChannelCtx, _: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, Self::descriptor())?;
        Ok(Self)
    }
}

struct ToneModulator;

impl Modulator for ToneModulator {
    fn submit(&mut self, _: TxPayload) -> Result<(), EncoderError> {
        Ok(())
    }

    fn generate(&mut self, _: &mut [Complex<f32>]) -> usize {
        0
    }
}

impl NativeModulator for ToneModulator {
    type Settings = ToneParams;

    fn rate_hz() -> f64 {
        48_000.0
    }

    fn new(_: &ToneParams) -> Result<Self, EncoderError> {
        Ok(Self)
    }
}

static TONE_RECIPES: &[sdrmm_modem::synth::Recipe] = &[sdrmm_modem::synth::Recipe {
    stem: "tone_48k",
    rate: 48_000.0,
    note: "tone channel -> nothing",
    iq: || Ok(Vec::new()),
}];

fn built_in_decoders() -> Vec<Arc<dyn Decoder>> {
    vec![Arc::new(
        Native::<ToneParams>::new::<ToneChannel>(|p| (-p.width_hz / 2.0, p.width_hz / 2.0))
            .recipes(TONE_RECIPES),
    )]
}

fn built_in_encoders() -> Vec<Arc<dyn Encoder>> {
    let base = EncoderBase {
        type_id: "tone",
        name: "Tone",
        summary: "A test tone",
    };
    vec![Arc::new(NativeEncoder::new::<ToneModulator>(base, |p| {
        p.width_hz
    }))]
}

fn installed() {
    install(built_in_decoders, built_in_encoders);
}

fn tone(width_hz: f64) -> ChannelParams {
    ChannelParams::new("tone", &ToneParams { width_hz }).expect("tone settings")
}

struct Plugin(ChannelDescriptor);

impl Decoder for Plugin {
    fn descriptor(&self) -> &ChannelDescriptor {
        &self.0
    }

    fn open(
        &self,
        _: ChannelCtx,
        settings: &ChannelSettings,
    ) -> Result<Box<dyn Channel>, ChannelError> {
        Err(ChannelError::UnknownType(
            settings.params.type_id().to_owned(),
        ))
    }

    fn layout(&self, params: &ChannelParams) -> Result<ChannelLayout, ChannelError> {
        Err(ChannelError::UnknownType(params.type_id().to_owned()))
    }

    fn filter(&self, _: &ChannelParams) -> Result<sdrmm_wire::FilterSpec, ChannelError> {
        Ok(sdrmm_wire::FilterSpec::Passthrough)
    }

    fn normalize(&self, params: &ChannelParams) -> Result<ChannelParams, ChannelError> {
        Ok(params.clone())
    }
}

fn plugin(type_id: &str) -> Arc<dyn Decoder> {
    Arc::new(Plugin(ChannelDescriptor {
        type_id: type_id.to_owned(),
        ..ChannelDescriptor::default()
    }))
}

struct PluginEncoder(EncoderDescriptor);

impl Encoder for PluginEncoder {
    fn descriptor(&self) -> &EncoderDescriptor {
        &self.0
    }

    fn open(&self, params: &EncoderParams) -> Result<Box<dyn Modulator>, EncoderError> {
        Err(EncoderError::Unknown(params.type_id.clone()))
    }

    fn compose(&self, _: &Value) -> Result<TxPayload, EncoderError> {
        Err(EncoderError::NoMessages(self.0.type_id.clone()))
    }

    fn width_hz(&self, _: &EncoderParams) -> Result<f64, EncoderError> {
        Ok(self.0.bandwidth_hz)
    }
}

fn plugin_encoder(type_id: &str) -> Arc<dyn Encoder> {
    Arc::new(PluginEncoder(EncoderDescriptor {
        type_id: type_id.to_owned(),
        ..EncoderDescriptor::default()
    }))
}

#[test]
fn a_native_decoder_lays_out_its_own_settings() {
    installed();
    let params = tone(10_000.0);
    assert_eq!(occupied_band(&params), (-5_000.0, 5_000.0));
    assert_eq!(input_rate(&params), 48_000.0);
    assert_eq!(audio_channels(&params), 1);
    let defaults = default_settings("tone").expect("tone has defaults");
    assert_eq!(defaults.params.type_id(), "tone");
}

#[test]
fn a_native_decoder_refuses_another_decoders_settings() {
    installed();
    let registry = registry();
    let decoder = registry.decoder("tone").expect("tone is built in");
    let other = ChannelParams::new("other", &ToneParams::default()).expect("settings");
    assert!(matches!(
        decoder.layout(&other),
        Err(ChannelError::InvalidSettings(_))
    ));
}

#[test]
fn create_opens_a_built_in_at_its_rate_only() {
    installed();
    let settings = ChannelSettings::new(0.0, tone(1_000.0));
    assert!(
        create(
            ChannelCtx {
                input_rate: 48_000.0
            },
            &settings
        )
        .is_ok()
    );
    assert!(matches!(
        create(
            ChannelCtx {
                input_rate: 96_000.0
            },
            &settings
        ),
        Err(ChannelError::InvalidSettings(_))
    ));
}

#[test]
fn a_decoder_declares_the_recipes_its_fixtures_are_written_from() {
    installed();
    let stems: Vec<_> = recipes().iter().map(|recipe| recipe.stem).collect();
    assert_eq!(stems, ["tone_48k"]);
    assert_eq!(recipe("tone_48k").map(|recipe| recipe.rate), Some(48_000.0));
    assert!(recipe("missing").is_none());
}

#[test]
fn an_unknown_type_occupies_nothing_and_opens_nothing() {
    installed();
    let params = ChannelParams::new("nowhere", &ToneParams::default()).expect("settings");
    assert_eq!(occupied_band(&params), (0.0, 0.0));
    assert!(descriptor("nowhere").is_none());
    assert!(matches!(
        create(
            ChannelCtx { input_rate: 0.0 },
            &ChannelSettings::new(0.0, params)
        ),
        Err(ChannelError::UnknownType(_))
    ));
}

#[test]
fn a_plugin_may_not_take_a_built_in_or_a_taken_type() {
    installed();
    assert!(matches!(
        plugin_refusal("tone", &[]),
        Some(ChannelError::BuiltIn(id)) if id == "tone"
    ));
    assert!(matches!(
        plugin_refusal("twin", &["twin"]),
        Some(ChannelError::LoadedTwice(id)) if id == "twin"
    ));
    assert!(plugin_refusal("twin", &["other"]).is_none());
}

#[test]
fn a_clashing_decoder_set_installs_nothing() {
    installed();
    for refused in [
        vec![plugin("clash_ok"), plugin("tone")],
        vec![plugin("clash_twin"), plugin("clash_twin")],
    ] {
        assert!(install_plugins(refused).is_err());
    }
    let current = registry();
    assert!(current.find("clash_ok").is_none());
    assert!(current.find("clash_twin").is_none());
    assert!(current.find("tone").is_some());
}

#[test]
fn an_encoder_refuses_another_encoders_settings_and_an_audio_one_composes_nothing() {
    installed();
    let tone = encoder("tone").expect("tone encodes");
    assert!(matches!(
        tone.open(&EncoderParams::bare("am")),
        Err(EncoderError::WrongType { got, .. }) if got == "am"
    ));
    assert!(matches!(
        tone.compose(&serde_json::json!({})),
        Err(EncoderError::NoMessages(id)) if id == "tone"
    ));
    assert!(matches!(
        encoder("nowhere"),
        Err(EncoderError::Unknown(id)) if id == "nowhere"
    ));
}

#[test]
fn a_clashing_encoder_set_installs_nothing() {
    installed();
    assert!(matches!(
        encoder_refusal("tone", &[]),
        Some(EncoderError::BuiltIn(id)) if id == "tone"
    ));
    for refused in [
        vec![plugin_encoder("clash_ok"), plugin_encoder("tone")],
        vec![plugin_encoder("clash_twin"), plugin_encoder("clash_twin")],
    ] {
        assert!(install_encoders(refused).is_err());
    }
    let current = encoder_registry();
    assert!(current.find("clash_ok").is_none());
    assert!(current.find("clash_twin").is_none());
    assert!(current.find("tone").is_some());
}

#[test]
fn a_setting_outside_its_range_is_refused_with_its_bounds() {
    assert!(crate::check_range("report interval", "ms", 500, 100..=1_000).is_ok());
    let refused =
        crate::check_range("report interval", "ms", 50, 100..=1_000).expect_err("below the range");
    assert_eq!(
        refused.to_string(),
        "invalid settings: report interval must be in [100, 1000] ms, got 50"
    );
}
