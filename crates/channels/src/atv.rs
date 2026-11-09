use crate::registry::Native;
use sdrmm_wire::{ClickProfile, Control, KindSpec, NumberSpec, SettingField};
mod front;
mod model;
mod picture;
mod standard;
mod sync;
#[cfg(test)]
mod tests;
pub use model::*;
use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_wire::{ChannelDescriptor, ChannelSettings, DecoderFamily};

use crate::{
    ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, NativeChannel, check_rate,
};
use front::{Detector, Discriminator, Rate, SOUND_AUDIO_HZ, SOUND_DEVIATION_HZ, SoundDecoder};
use picture::Picture;
use standard::{CHROMA_BANDWIDTH_HZ, Geometry, subcarrier_hz, video_rate_hz};
use sync::{Fields, LineSync, Pulse, Samples};

const MIN_BANDWIDTH_HZ: f64 = 100_000.0;
const MAX_BANDWIDTH_HZ: f64 = 10_000_000.0;
const RATES_HZ: (f64, f64) = (1_000_000.0, 122_880_000.0);
/// What an AM picture keeps when the luma bandwidth follows the noise.
const AM_VIDEO_HZ: f64 = 5_000_000.0;
/// The chroma an AM vestigial-sideband channel carries above the subcarrier.
const AM_CHROMA_HZ: f64 = 600_000.0;
/// Share of the channel rate an AM picture fills each side of its carrier, leaving room to
/// tune off centre.
const AM_SPAN: f64 = 0.3;
/// Share of the channel rate an FM picture may swing over.
const FM_SPAN: f64 = 0.4;
const MAX_VIDEO_SHARE: f64 = 0.45;
const MONO_KEEP_HZ: f64 = 6_000_000.0;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "atv".to_owned(),
    name: "ATV".to_owned(),
    summary: "Analog TV picture and sound".to_owned(),
    family: DecoderFamily::VIDEO,
    bandwidth_hz: 1_500_000.0,
    input_rate_hz: AtvParams::default().rate_hz,
    has_audio: true,
    has_video: true,
    click: ClickProfile::Detector,
    ..ChannelDescriptor::default()
});

pub struct AtvChannel {
    params: AtvParams,
    input_rate: f64,
    detector: Detector,
    polarity: f32,
    sound: Option<SoundDecoder>,
    rate: Rate,
    video: Vec<f32>,
    resampled: Vec<f32>,
    samples: Samples,
    sync: LineSync,
    fields: Fields,
    picture: Picture,
}

fn params(settings: &ChannelSettings) -> Result<AtvParams, ChannelError> {
    crate::settings_of::<AtvParams>(settings)
}

fn check_params(p: &AtvParams) -> Result<(), ChannelError> {
    if !(p.rate_hz.is_finite() && (RATES_HZ.0..=RATES_HZ.1).contains(&p.rate_hz)) {
        return Err(ChannelError::InvalidSettings(format!(
            "atv rate must be in [{}, {}] Hz, got {}",
            RATES_HZ.0, RATES_HZ.1, p.rate_hz
        )));
    }
    match p.bandwidth_hz {
        Some(hz) if !(hz.is_finite() && (MIN_BANDWIDTH_HZ..=MAX_BANDWIDTH_HZ).contains(&hz)) => {
            Err(ChannelError::InvalidSettings(format!(
                "atv bandwidth must be in [{MIN_BANDWIDTH_HZ}, {MAX_BANDWIDTH_HZ}] Hz, got {hz}"
            )))
        }
        _ => Ok(()),
    }
}

pub(crate) fn channel_filter(p: &AtvParams) -> Result<ChannelFilter, ChannelError> {
    check_params(p)?;
    Ok(ChannelFilter::Passthrough)
}

pub(crate) fn input_rate(p: &AtvParams) -> f64 {
    p.rate_hz
}

fn sound_band_hz(p: &AtvParams) -> f64 {
    p.sound_subcarrier_hz
        .map_or(0.0, |carrier| carrier + SOUND_DEVIATION_HZ + SOUND_AUDIO_HZ)
}

fn am_video_hz(p: &AtvParams) -> f64 {
    let chroma = subcarrier_hz(p.color).map_or(0.0, |carrier| carrier + AM_CHROMA_HZ);
    p.bandwidth_hz
        .unwrap_or(AM_VIDEO_HZ)
        .min(AM_SPAN * p.rate_hz)
        .max(chroma)
}

pub(crate) fn occupied_band(p: &AtvParams) -> (f64, f64) {
    match p.modulation {
        AtvModulation::Fm => (-FM_SPAN * p.rate_hz, FM_SPAN * p.rate_hz),
        AtvModulation::Am => {
            let video = am_video_hz(p);
            (-video, video.max(sound_band_hz(p)))
        }
    }
}

/// The video band the detected signal must keep: chroma for colour, the luma bandwidth's
/// skirt otherwise.
fn keep_hz(p: &AtvParams, video_rate: f64) -> f64 {
    let ceiling = MAX_VIDEO_SHARE * video_rate.min(p.rate_hz);
    subcarrier_hz(p.color).map_or_else(
        || {
            p.bandwidth_hz
                .map_or(MONO_KEEP_HZ, |hz| 2.0 * hz)
                .min(ceiling)
        },
        |carrier| carrier + CHROMA_BANDWIDTH_HZ,
    )
}

impl AtvChannel {
    fn build(input_rate: f64, p: &AtvParams) -> Result<Self, ChannelError> {
        check_params(p)?;
        let video_rate = video_rate_hz(p.standard, p.color, input_rate);
        let keep = keep_hz(p, video_rate);
        let embedded = if p.modulation == AtvModulation::Fm {
            sound_band_hz(p)
        } else {
            0.0
        };
        if keep.max(embedded) >= input_rate / 2.0 {
            return Err(ChannelError::InvalidSettings(format!(
                "atv colour or sound needs {:.0} Hz above the carrier, past the {:.0} Hz Nyquist limit",
                keep.max(embedded),
                input_rate / 2.0
            )));
        }
        let rate = Rate::new(input_rate, video_rate, keep)?;
        let g = Geometry::new(p.standard, video_rate);
        let detector = match p.modulation {
            AtvModulation::Fm => Detector::Discriminator(Discriminator::new()),
            AtvModulation::Am => Detector::am(am_video_hz(p) / input_rate),
        };
        let sound = p
            .sound_subcarrier_hz
            .map(|carrier| {
                let deemphasis = if p.color == AtvColor::Ntsc {
                    75.0
                } else {
                    50.0
                };
                SoundDecoder::new(input_rate, carrier, deemphasis)
            })
            .transpose()?;
        let flipped = matches!(p.modulation, AtvModulation::Am) != p.invert;
        Ok(Self {
            params: p.clone(),
            input_rate,
            detector,
            polarity: if flipped { -1.0 } else { 1.0 },
            sound,
            rate,
            video: Vec::new(),
            resampled: Vec::new(),
            samples: Samples::default(),
            sync: LineSync::new(g),
            fields: Fields::default(),
            picture: Picture::new(g, p.color, p.bandwidth_hz, p.sound_subcarrier_hz),
        })
    }

    fn rows(&self) -> u32 {
        u32::from(self.picture.height() / 2)
    }

    fn on_lines(&mut self, out: &mut ChannelOutputs) {
        let field_len = (f64::from(self.params.standard.lines()) / 2.0) as u32;
        while let Some(line) = self.sync.next(&self.samples) {
            let field = self.fields.on_line(&line, field_len);
            if field.new_field
                && let Some(picture) = self.picture.take()
            {
                out.video.push(picture);
            }
            if line.pulse != Pulse::Line {
                self.picture.coast();
                continue;
            }
            self.picture
                .observe(&self.samples, line.zero_h, &line.levels);
            let delay = u32::from(self.geometry_delay());
            let Some(row) = field.line.and_then(|l| l.checked_sub(delay)) else {
                continue;
            };
            if row >= self.rows() {
                continue;
            }
            let dest = if self.params.interlace {
                2 * row + u32::from(field.parity)
            } else {
                row
            };
            self.picture
                .draw(&self.samples, line.zero_h, &line.levels, dest as usize);
        }
        if !self.sync.locked() {
            self.fields.reset();
        }
        self.samples.forget_before(self.sync.keep_from());
    }

    fn geometry_delay(&self) -> u16 {
        self.sync.geometry().picture_delay
    }
}

impl NativeChannel for AtvChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        let p = params(&settings)?;
        check_rate(ctx, &DESCRIPTOR, p.rate_hz)?;
        Self::build(ctx.input_rate, &p)
    }
}

impl ChannelRx for AtvChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let p = params(&settings)?;
        check_rate(
            ChannelCtx {
                input_rate: self.input_rate,
            },
            &DESCRIPTOR,
            p.rate_hz,
        )?;
        *self = Self::build(self.input_rate, &p)?;
        Ok(())
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        if self.params.modulation == AtvModulation::Am
            && let Some(sound) = &mut self.sound
        {
            sound.process_iq(iq, out);
        }
        self.detector.process(iq, &mut self.video);
        if self.params.modulation == AtvModulation::Fm
            && let Some(sound) = &mut self.sound
        {
            sound.process_composite(&self.video, out);
        }
        let polarity = self.polarity;
        for v in &mut self.video {
            *v *= polarity;
        }
        self.rate.process(&self.video, &mut self.resampled);
        self.samples.extend(&self.resampled);
        self.on_lines(out);
    }
}

pub(crate) fn atv_decoder() -> Native<AtvParams> {
    Native::new::<AtvChannel>(occupied_band)
        .filter(channel_filter)
        .input_rate(input_rate)
        .form(atv_form)
        .kinds(atv_kinds)
}

fn hertz_or_auto(hz: Option<f64>) -> String {
    hz.map_or_else(|| "Auto".to_owned(), sdrmm_wire::units::hertz)
}

pub(crate) fn atv_form() -> Vec<SettingField> {
    vec![
        SettingField::new("modulation", "Mod", Control::choice::<AtvModulation>())
            .title("Modulation"),
        SettingField::new("standard", "Lines", Control::choice::<AtvStandard>())
            .title("Scanning standard"),
        SettingField::new(
            "rate_hz",
            "Rate",
            Control::values(
                &[
                    2_400_000.0,
                    4_000_000.0,
                    8_000_000.0,
                    10_000_000.0,
                    12_000_000.0,
                    16_000_000.0,
                    20_000_000.0,
                    25_000_000.0,
                    32_000_000.0,
                    40_000_000.0,
                ],
                sdrmm_wire::units::sample_rate,
            ),
        )
        .title("Channel sample rate; FM needs its whole swing plus tuning error"),
        SettingField::new(
            "bandwidth_hz",
            "BW",
            Control::values(
                &[
                    None,
                    Some(1_000_000.0),
                    Some(2_000_000.0),
                    Some(3_000_000.0),
                    Some(4_000_000.0),
                    Some(5_000_000.0),
                ],
                hertz_or_auto,
            ),
        )
        .title("Luma bandwidth; Auto softens it as noise rises"),
        SettingField::new("color", "Colour", Control::choice::<AtvColor>())
            .title("Composite colour system"),
        SettingField::new(
            "sound_subcarrier_hz",
            "Sound",
            NumberSpec::mhz(500_000.0, 7_500_000.0, 500_000.0).optional("off"),
        )
        .title("FM sound subcarrier, empty for none"),
        SettingField::new("interlace", "Interlace", Control::Toggle)
            .title("Weave both fields into one frame"),
        SettingField::invert(),
    ]
}

fn atv_kinds() -> Vec<KindSpec> {
    vec![]
}
