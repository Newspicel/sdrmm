use std::sync::LazyLock;

use sdrmm_decoder::registry::Native;
use sdrmm_wire::{
    Capability, ChannelDescriptor, Control, DecoderSettings, EventKind, Hue, KindSpec,
    SettingField, Verification, labelled,
};
use serde::{Deserialize, Serialize};

use super::{Q65Decoder, Q65Setup};
use crate::weak_signal::{
    Decoded, Job, Mode, Q65Channel, WsjtMessage, descriptor, search_band, search_filter,
    wsjt_decoded, wsjt_form,
};

const RATE: usize = 12_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Q65Period {
    #[serde(rename = "15")]
    S15,
    #[serde(rename = "30")]
    S30,
    #[default]
    #[serde(rename = "60")]
    S60,
    #[serde(rename = "120")]
    S120,
    #[serde(rename = "300")]
    S300,
}

labelled! {
    Q65Period { S15 => "15 s", S30 => "30 s", S60 => "60 s", S120 => "120 s", S300 => "300 s" }
}

impl Q65Period {
    fn seconds(self) -> u32 {
        match self {
            Self::S15 => 15,
            Self::S30 => 30,
            Self::S60 => 60,
            Self::S120 => 120,
            Self::S300 => 300,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Q65Submode {
    #[default]
    A,
    B,
    C,
    D,
    E,
}

labelled! {
    Q65Submode {
        A => "A": "Tones one baud apart",
        B => "B": "Tones two baud apart",
        C => "C": "Tones four baud apart",
        D => "D": "Tones eight baud apart",
        E => "E": "Tones sixteen baud apart",
    }
}

fn default_low_hz() -> f32 {
    200.0
}

fn default_high_hz() -> f32 {
    3_000.0
}

fn default_candidates() -> u16 {
    50
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Q65Params {
    #[serde(default)]
    pub period: Q65Period,
    #[serde(default)]
    pub submode: Q65Submode,
    #[serde(default = "default_low_hz")]
    pub audio_low_hz: f32,
    #[serde(default = "default_high_hz")]
    pub audio_high_hz: f32,
    #[serde(default = "default_candidates")]
    pub max_candidates: u16,
}

impl Default for Q65Params {
    fn default() -> Self {
        Self {
            period: Q65Period::default(),
            submode: Q65Submode::default(),
            audio_low_hz: default_low_hz(),
            audio_high_hz: default_high_hz(),
            max_candidates: default_candidates(),
        }
    }
}

impl DecoderSettings for Q65Params {
    const TYPE_ID: &'static str = "q65";
}

impl Q65Params {
    pub(crate) fn setup(&self) -> Q65Setup {
        Q65Setup {
            period_s: self.period.seconds(),
            submode: self.submode as u8,
        }
    }
}

impl Q65Setup {
    /// Windows a few seconds apart, never more than a start the decoder searches can absorb.
    pub(crate) fn hop_samples(self) -> usize {
        let seconds = match self.period_s {
            0..=15 => 2,
            16..=30 => 3,
            31..=60 => 4,
            _ => 5,
        };
        seconds * RATE
    }
}

pub(crate) static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| {
    descriptor(
        "q65",
        "Q65",
        "Q65 EME and scatter contacts",
        EventKind::named("q65"),
        Verification::OnAir,
    )
});

pub(crate) fn search(params: &Q65Params) -> (f32, f32, usize) {
    (
        params.audio_low_hz,
        params.audio_high_hz,
        usize::from(params.max_candidates),
    )
}

pub(crate) fn decoded(decoder: &mut Q65Decoder, job: &Job, mode: Mode) -> Vec<Decoded> {
    decoder
        .decode(
            &job.samples,
            job.audio_low_hz,
            job.audio_high_hz,
            job.max_candidates,
        )
        .into_iter()
        .map(|found| {
            let unclocked = found.start_s - mode.nominal_start_s() as f32;
            wsjt_decoded(job, mode, found, unclocked)
        })
        .collect()
}

fn form() -> Vec<SettingField> {
    let mut form = vec![
        SettingField::new("period", "Period", Control::choice::<Q65Period>()).title("T/R period"),
        SettingField::new("submode", "Submode", Control::choice::<Q65Submode>())
            .title("Tone spacing"),
    ];
    form.extend(wsjt_form());
    form
}

pub(crate) fn kind() -> KindSpec {
    KindSpec::tagged::<WsjtMessage>(EventKind::named("q65"), "Q65", "Stations", Hue::Muted)
        .caps(&[Capability::Log, Capability::Station, Capability::Spot])
        .sighting()
        .build()
}

fn kinds() -> Vec<KindSpec> {
    vec![kind()]
}

fn band(params: &Q65Params) -> (f64, f64) {
    search_band(search(params))
}

pub(crate) fn q65_decoder() -> Native<Q65Params> {
    let decoder = Native::new::<Q65Channel>(band)
        .filter(|params| search_filter(search(params)))
        .home(50_275_000.0)
        .form(form)
        .kinds(kinds);
    cfg_select! {
        any(test, feature = "synth") => decoder.synth(crate::weak_signal::synth::Q65_SIGNALS),
        _ => decoder,
    }
}
