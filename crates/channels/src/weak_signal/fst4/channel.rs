use std::sync::LazyLock;

use sdrmm_decoder::ChannelError;
use sdrmm_decoder::registry::Native;
use sdrmm_wire::{
    ChannelDescriptor, ChannelParams, Control, DecoderEvent, DecoderSettings, EventKind, KindSpec,
    SettingField, Verification, labelled,
};
use serde::{Deserialize, Serialize};

use super::model::{Fst4Message, Fst4wSpot, fst4_kind, fst4w_kind};
use super::{Fst4Decoder, Fst4Found, Fst4Setup, SAMPLE_RATE, narrow, narrowing};
use crate::weak_signal::{
    Decoded, Fst4Channel, Fst4wChannel, Job, Mode, descriptor, search_band, search_filter,
    wsjt_form,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Fst4Period {
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
    #[serde(rename = "900")]
    S900,
    #[serde(rename = "1800")]
    S1800,
}

labelled! {
    Fst4Period {
        S15 => "15 s",
        S30 => "30 s",
        S60 => "60 s",
        S120 => "120 s",
        S300 => "300 s",
        S900 => "900 s",
        S1800 => "1800 s",
    }
}

impl Fst4Period {
    fn seconds(self) -> u32 {
        match self {
            Self::S15 => 15,
            Self::S30 => 30,
            Self::S60 => 60,
            Self::S120 => 120,
            Self::S300 => 300,
            Self::S900 => 900,
            Self::S1800 => 1_800,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Fst4wPeriod {
    #[default]
    #[serde(rename = "120")]
    S120,
    #[serde(rename = "300")]
    S300,
    #[serde(rename = "900")]
    S900,
    #[serde(rename = "1800")]
    S1800,
}

labelled! {
    Fst4wPeriod { S120 => "120 s", S300 => "300 s", S900 => "900 s", S1800 => "1800 s" }
}

impl Fst4wPeriod {
    fn seconds(self) -> u32 {
        match self {
            Self::S120 => 120,
            Self::S300 => 300,
            Self::S900 => 900,
            Self::S1800 => 1_800,
        }
    }
}

fn default_low_hz() -> f32 {
    200.0
}

fn default_high_hz() -> f32 {
    3_000.0
}

fn default_beacon_low_hz() -> f32 {
    1_400.0
}

fn default_beacon_high_hz() -> f32 {
    1_600.0
}

fn default_candidates() -> u16 {
    200
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fst4Params {
    #[serde(default)]
    pub period: Fst4Period,
    #[serde(default = "default_low_hz")]
    pub audio_low_hz: f32,
    #[serde(default = "default_high_hz")]
    pub audio_high_hz: f32,
    #[serde(default = "default_candidates")]
    pub max_candidates: u16,
}

impl Default for Fst4Params {
    fn default() -> Self {
        Self {
            period: Fst4Period::default(),
            audio_low_hz: default_low_hz(),
            audio_high_hz: default_high_hz(),
            max_candidates: default_candidates(),
        }
    }
}

impl DecoderSettings for Fst4Params {
    const TYPE_ID: &'static str = "fst4";
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fst4wParams {
    #[serde(default)]
    pub period: Fst4wPeriod,
    #[serde(default = "default_beacon_low_hz")]
    pub audio_low_hz: f32,
    #[serde(default = "default_beacon_high_hz")]
    pub audio_high_hz: f32,
    #[serde(default = "default_candidates")]
    pub max_candidates: u16,
}

impl Default for Fst4wParams {
    fn default() -> Self {
        Self {
            period: Fst4wPeriod::default(),
            audio_low_hz: default_beacon_low_hz(),
            audio_high_hz: default_beacon_high_hz(),
            max_candidates: default_candidates(),
        }
    }
}

impl DecoderSettings for Fst4wParams {
    const TYPE_ID: &'static str = "fst4w";
}

impl Fst4Params {
    pub(crate) fn setup(&self) -> Fst4Setup {
        Fst4Setup {
            period_s: self.period.seconds(),
            wspr: false,
        }
    }

    fn search(&self) -> (f32, f32, usize) {
        (
            self.audio_low_hz,
            self.audio_high_hz,
            usize::from(self.max_candidates),
        )
    }
}

impl Fst4wParams {
    pub(crate) fn setup(&self) -> Fst4Setup {
        Fst4Setup {
            period_s: self.period.seconds(),
            wspr: true,
        }
    }

    fn search(&self) -> (f32, f32, usize) {
        (
            self.audio_low_hz,
            self.audio_high_hz,
            usize::from(self.max_candidates),
        )
    }
}

impl Fst4Setup {
    /// The rate of the stream a period is decoded from.
    pub(crate) fn stream_rate(self) -> f64 {
        f64::from(SAMPLE_RATE) / narrowing(self.period_s) as f64
    }

    /// The window: the span WSJT-X analyses, in stream samples.
    pub(crate) fn slot_samples(self) -> usize {
        super::Timing::of(self.period_s)
            .and_then(|timing| timing.narrowed(narrowing(self.period_s)))
            .map_or(0, |timing| timing.fft_samples)
    }

    /// Windows a few seconds apart, never more than the slack a window leaves around a
    /// transmission.
    pub(crate) fn hop_samples(self) -> usize {
        let seconds = match self.period_s {
            15 => 3.0,
            30 => 5.0,
            60 => 6.0,
            120 => 8.0,
            300 | 900 => 10.0,
            _ => 6.0,
        };
        (seconds * self.stream_rate()) as usize
    }
}

/// The setting a channel's params select.
pub(crate) fn setup(params: &ChannelParams, wspr: bool) -> Result<Fst4Setup, ChannelError> {
    Ok(if wspr {
        params
            .parse::<Fst4wParams>()
            .map_err(ChannelError::InvalidSettings)?
            .setup()
    } else {
        params
            .parse::<Fst4Params>()
            .map_err(ChannelError::InvalidSettings)?
            .setup()
    })
}

/// The search a channel's params ask for, refused when it does not fit the period's stream.
pub(crate) fn search(
    params: &ChannelParams,
    setup: Fst4Setup,
) -> Result<(f32, f32, usize), ChannelError> {
    let search = if setup.wspr {
        params
            .parse::<Fst4wParams>()
            .map_err(ChannelError::InvalidSettings)?
            .search()
    } else {
        params
            .parse::<Fst4Params>()
            .map_err(ChannelError::InvalidSettings)?
            .search()
    };
    search_fits(setup, search.0, search.1)?;
    Ok(search)
}

/// Whether a search band fits the stream the period is decoded from.
pub(crate) fn search_fits(setup: Fst4Setup, low_hz: f32, high_hz: f32) -> Result<(), ChannelError> {
    if narrowing(setup.period_s) == 1 {
        return Ok(());
    }
    narrow::fits(setup.stream_rate() as f32, low_hz, high_hz).map_err(|(widest, center)| {
        ChannelError::InvalidSettings(format!(
            "at {} s the search spans at most {widest:.0} Hz, centred above {center:.0} Hz",
            setup.period_s
        ))
    })
}

/// The audio frequency at the stream's 0 Hz for a search from `low_hz` to `high_hz`.
pub(crate) fn shift_hz(setup: Fst4Setup, low_hz: f32, high_hz: f32) -> f32 {
    if narrowing(setup.period_s) == 1 {
        0.0
    } else {
        narrow::shift_hz(setup.stream_rate() as f32, f32::midpoint(low_hz, high_hz))
    }
}

pub(crate) static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| {
    descriptor(
        "fst4",
        "FST4",
        "FST4 LF and MF contacts",
        EventKind::named("fst4"),
        Verification::OnAir,
    )
});

pub(crate) static FST4W_DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| {
    descriptor(
        "fst4w",
        "FST4W",
        "FST4W LF and MF beacons",
        EventKind::named("fst4w"),
        Verification::OnAir,
    )
});

pub(crate) fn decoded(decoder: &mut Fst4Decoder, job: &Job, mode: Mode) -> Vec<Decoded> {
    decoder
        .decode(
            &job.samples,
            job.audio_low_hz - job.shift_hz,
            job.audio_high_hz - job.shift_hz,
            job.max_candidates,
        )
        .into_iter()
        .map(|found| event(job, mode, found))
        .collect()
}

fn event(job: &Job, mode: Mode, found: Fst4Found) -> Decoded {
    let (time_offset_s, slot) = mode.timing(job.window_time, found.start_s);
    let audio_hz = found.frequency_hz + job.shift_hz;
    let window_offset_s = found.start_s - mode.nominal_start_s() as f32;
    let event = match found.beacon {
        Some(beacon) => DecoderEvent::of(&Fst4wSpot {
            text: found.text.clone(),
            callsign: beacon.callsign,
            grid: beacon.grid,
            power_dbm: beacon.power_dbm,
            snr_db: found.snr_db,
            audio_hz,
            time_offset_s,
            hard_errors: found.hard_errors,
            slot,
        }),
        None => DecoderEvent::of(&Fst4Message {
            text: found.text.clone(),
            snr_db: found.snr_db,
            audio_hz,
            time_offset_s,
            hard_errors: found.hard_errors,
            cq_assumed: found.cq_assumed,
            slot,
        }),
    };
    Decoded {
        text: found.text,
        audio_hz,
        window_offset_s,
        event,
    }
}

fn fst4_form() -> Vec<SettingField> {
    let mut form = vec![
        SettingField::new("period", "Period", Control::choice::<Fst4Period>()).title("T/R period"),
    ];
    form.extend(wsjt_form());
    form
}

fn fst4w_form() -> Vec<SettingField> {
    let mut form = vec![
        SettingField::new("period", "Period", Control::choice::<Fst4wPeriod>()).title("T/R period"),
    ];
    form.extend(wsjt_form());
    form
}

fn fst4_kinds() -> Vec<KindSpec> {
    vec![fst4_kind()]
}

fn fst4w_kinds() -> Vec<KindSpec> {
    vec![fst4w_kind()]
}

pub(crate) fn fst4_decoder() -> Native<Fst4Params> {
    let decoder = Native::new::<Fst4Channel>(|params: &Fst4Params| search_band(params.search()))
        .filter(|params| search_filter(params.search()))
        .home(474_200.0)
        .form(fst4_form)
        .kinds(fst4_kinds);
    cfg_select! {
        any(test, feature = "synth") => decoder.synth(crate::weak_signal::synth::FST4_SIGNALS),
        _ => decoder,
    }
}

pub(crate) fn fst4w_decoder() -> Native<Fst4wParams> {
    Native::new::<Fst4wChannel>(|params: &Fst4wParams| search_band(params.search()))
        .filter(|params| search_filter(params.search()))
        .home(474_200.0)
        .form(fst4w_form)
        .kinds(fst4w_kinds)
}
