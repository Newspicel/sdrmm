use crate::registry::Native;
use sdrmm_wire::{Control, KindSpec, NumberSpec, PresetOption, SettingField};
use sdrmm_wire::{HzRange, Modulation, SignalSignature};
use serde_json::json;
mod model;
pub use model::*;
pub(crate) mod lorawan;
pub(crate) mod meshcore;
pub(crate) mod meshtastic;
pub(crate) mod phy;
mod protocol;
fn lora_signatures() -> Vec<SignalSignature> {
    vec![SignalSignature::new(
        "LoRa",
        &[Modulation::Chirp],
        HzRange::new(7_000.0, 550_000.0),
        "chirps sweeping the whole channel",
    )]
}

/// Takes the heard chirp width and, from its symbol rate, the spreading factor.
fn lora_tune(signal: &sdrmm_wire::IdentSignal, p: &mut LoraParams) -> f64 {
    p.bandwidth = LoraBandwidth::nearest(signal.bandwidth_hz);
    p.spreading_factor = signal
        .symbol_rate_hz
        .map_or(LoraSpreadingFactor::All, |rate| {
            LoraSpreadingFactor::of_chips(signal.bandwidth_hz / rate)
        });
    p.iq = LoraIq::Both;
    0.0
}

pub(crate) fn lora_decoder() -> Native<LoraParams> {
    Native::new::<LoraChannel>(occupied_band)
        .filter(|p| Ok(channel_filter(p)))
        .input_rate(input_rate)
        .signatures(lora_signatures)
        .tune(lora_tune)
        .form(lora_form)
        .kinds(lora_kinds)
}

fn meshtastic_preset(label: &str, bandwidth: &str, spreading_factor: &str) -> PresetOption {
    PresetOption::new(
        label,
        &[
            ("bandwidth", json!(bandwidth)),
            ("spreading_factor", json!(spreading_factor)),
            ("protocol", json!("meshtastic")),
            ("iq", json!("normal")),
        ],
    )
    .title("Meshtastic modem preset")
}

fn meshcore_preset(label: &str, spreading_factor: &str) -> PresetOption {
    PresetOption::new(
        &format!("MeshCore {label}"),
        &[
            ("bandwidth", json!("khz62_5")),
            ("spreading_factor", json!(spreading_factor)),
            ("protocol", json!("meshcore")),
            ("iq", json!("normal")),
        ],
    )
    .title("MeshCore narrow preset")
}

fn lora_presets() -> Vec<PresetOption> {
    vec![
        meshtastic_preset("LongFast", "khz250", "sf11"),
        meshtastic_preset("LongModerate", "khz125", "sf11"),
        meshtastic_preset("LongSlow", "khz125", "sf12"),
        meshtastic_preset("MediumSlow", "khz250", "sf10"),
        meshtastic_preset("MediumFast", "khz250", "sf9"),
        meshtastic_preset("ShortSlow", "khz250", "sf8"),
        meshtastic_preset("ShortFast", "khz250", "sf7"),
        meshtastic_preset("ShortTurbo", "khz500", "sf7"),
        meshcore_preset("EU", "sf8"),
        meshcore_preset("US", "sf7"),
        PresetOption::new(
            "LoRaWAN",
            &[
                ("bandwidth", json!("khz125")),
                ("spreading_factor", json!("all")),
                ("protocol", json!("lorawan")),
                ("iq", json!("both")),
            ],
        )
        .title("Uplinks and downlinks on one 125 kHz channel"),
    ]
}

pub(crate) fn lora_form() -> Vec<SettingField> {
    vec![
        SettingField::new("preset", "Preset", Control::preset(lora_presets()))
            .title("Modem preset"),
        SettingField::new("bandwidth", "BW", Control::choice::<LoraBandwidth>())
            .title("LoRa bandwidth"),
        SettingField::new(
            "spreading_factor",
            "SF",
            Control::choice::<LoraSpreadingFactor>(),
        )
        .title("Spreading factor")
        .quiet(LoraSpreadingFactor::All),
        SettingField::new("iq", "IQ", Control::choice::<LoraIq>()).title("Chirp direction"),
        SettingField::new("protocol", "Protocol", Control::choice::<LoraProtocol>())
            .title("Payload protocol")
            .quiet(LoraProtocol::Auto),
        SettingField::new("keys", "Keys", Control::list(lora_key_form(), "built-in"))
            .title("Extra keys; Meshtastic defaults and MeshCore Public are built in")
            .quiet(Vec::<LoraKey>::new()),
        SettingField::new(
            "implicit_header.length",
            "Implicit",
            NumberSpec::new(1.0, 255.0, 1.0).unit("B").optional("off"),
        )
        .title("Implicit header payload length, empty for explicit headers"),
        SettingField::new(
            "implicit_header.coding_rate",
            "CR",
            Control::choice::<LoraCodingRate>(),
        )
        .title("Implicit header coding rate")
        .fallback(LoraCodingRate::default())
        .when("implicit_header.length", Vec::<u8>::new()),
        SettingField::new("implicit_header.crc", "CRC", Control::Toggle)
            .title("Implicit header payload carries a CRC")
            .fallback(true)
            .when("implicit_header.length", Vec::<u8>::new()),
    ]
}

fn lora_key_form() -> Vec<SettingField> {
    let text = |key: &str, placeholder: &str, kind: LoraKeyKind| {
        SettingField::new(key, placeholder, Control::text(placeholder)).when("kind", [kind])
    };
    vec![
        SettingField::new("kind", "Kind", Control::choice::<LoraKeyKind>())
            .fallback(LoraKeyKind::MeshtasticChannel),
        text("name", "Channel", LoraKeyKind::MeshtasticChannel),
        text("psk", "PSK base64", LoraKeyKind::MeshtasticChannel),
        text("name", "Channel or #tag", LoraKeyKind::MeshcoreChannel),
        text("secret", "Secret hex", LoraKeyKind::MeshcoreChannel),
        text("dev_addr", "DevAddr", LoraKeyKind::LorawanSession),
        text("nwk_s_key", "NwkSKey", LoraKeyKind::LorawanSession),
        text("app_s_key", "AppSKey", LoraKeyKind::LorawanSession),
        text("app_key", "AppKey", LoraKeyKind::LorawanAppKey),
    ]
}

fn lora_kinds() -> Vec<KindSpec> {
    vec![crate::lora::lora_kind()]
}

#[cfg(test)]
mod tests;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily, EventKind};

use self::phy::{Decoder, Frame, Layout};
use self::protocol::Keyring;
use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, NativeChannel, check_rate,
    datalink,
};

pub(crate) const OVERSAMPLING: f64 = 2.0;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| {
    let bandwidth = LoraParams::default().bandwidth.hz();
    ChannelDescriptor {
        type_id: "lora".to_owned(),
        name: "LoRa".to_owned(),
        summary: "LoRa, LoRaWAN, Meshtastic and MeshCore".to_owned(),
        family: DecoderFamily::UTILITY,
        bandwidth_hz: bandwidth,
        input_rate_hz: bandwidth * OVERSAMPLING,
        has_audio: false,
        emits: vec![EventKind::named("lora")],
        ..ChannelDescriptor::default()
    }
});

pub struct LoraChannel {
    params: LoraParams,
    decoder: Decoder,
    keys: Keyring,
    frames: Vec<Frame>,
}

fn params(settings: &ChannelSettings) -> Result<LoraParams, ChannelError> {
    crate::settings_of::<LoraParams>(settings)
}

pub(crate) fn input_rate(params: &LoraParams) -> f64 {
    params.bandwidth.hz() * OVERSAMPLING
}

pub(crate) fn occupied_band(params: &LoraParams) -> (f64, f64) {
    let half = params.bandwidth.hz() / 2.0;
    (-half, half)
}

/// None: the decoder band-limits both of its streams itself, a half-band for the search and a
/// windowed sinc for every symbol it reads, so a filter here would only cost time.
pub(crate) fn channel_filter(_params: &LoraParams) -> ChannelFilter {
    ChannelFilter::Passthrough
}

fn layout(params: &LoraParams) -> Layout {
    Layout {
        bandwidth_hz: params.bandwidth.hz(),
        spreading_factors: params.spreading_factor.factors(),
        iq: params.iq,
        implicit_header: params.implicit_header,
    }
}

fn event(frame: &Frame, params: &LoraParams, keys: &Keyring) -> DecoderEvent {
    DecoderEvent::of(&LoraFrame {
        spreading_factor: frame.spreading_factor,
        bandwidth_hz: params.bandwidth.hz(),
        coding_rate: frame.coding_rate,
        sync_word: frame.sync_word,
        implicit_header: frame.implicit_header,
        low_data_rate: frame.low_data_rate,
        inverted_iq: frame.inverted_iq,
        integrity: frame.integrity,
        fec_corrected: frame.fec_corrected,
        snr_db: frame.snr_db,
        frequency_error_hz: frame.frequency_error_hz,
        payload: datalink::hex(&frame.payload),
        decoded: frame
            .readable()
            .then(|| protocol::interpret(params.protocol, frame.sync_word, &frame.payload, keys))
            .flatten(),
    })
}

impl NativeChannel for LoraChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }
    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        let params = params(&settings)?.clone();
        params.validate().map_err(ChannelError::InvalidSettings)?;
        check_rate(ctx, &DESCRIPTOR, input_rate(&params))?;
        Ok(Self {
            decoder: Decoder::new(&layout(&params), settings.frequency_hz),
            keys: Keyring::new(&params.keys).map_err(ChannelError::InvalidSettings)?,
            frames: Vec::new(),
            params,
        })
    }
}

impl ChannelRx for LoraChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let next = params(&settings)?;
        next.validate().map_err(ChannelError::InvalidSettings)?;
        if layout(&next) != layout(&self.params) {
            return Err(ChannelError::InvalidSettings(
                "LoRa radio settings need a rebuilt channel".to_owned(),
            ));
        }
        self.keys = Keyring::new(&next.keys).map_err(ChannelError::InvalidSettings)?;
        self.decoder.retune(settings.frequency_hz);
        self.params = next.clone();
        Ok(())
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.frames.clear();
        self.decoder.process(iq, &mut self.frames);
        out.events.extend(
            self.frames
                .iter()
                .map(|frame| event(frame, &self.params, &self.keys)),
        );
    }
}
