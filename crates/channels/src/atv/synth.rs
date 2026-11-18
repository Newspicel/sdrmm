use sdrmm_modem::synth::{Render, Signal};
use std::f64::consts::TAU;

use crate::atv::AtvColor;
use crate::atv::AtvModulation;
use crate::atv::AtvParams;
use crate::atv::AtvStandard;
use num_complex::Complex;
use sdrmm_dsp::{FirC, blackman_harris};

const SYNC: f32 = 0.0;
const BLANKING: f32 = 0.3;
const NTSC_BLANKING: f32 = 40.0 / 140.0;
const NTSC_BLACK: f32 = 47.5 / 140.0;
const PAL_BURST: f32 = 0.15;
const NTSC_BURST: f32 = 20.0 / 140.0;

const AM_DEPTH: f64 = 0.875;

/// FM swing, sync tip to peak white, as a share of the sample rate.
const FM_SWING: f64 = 0.3;
const SOUND_DEVIATION_HZ: f64 = 50_000.0;
const SOUND_LEVEL: f64 = 0.12;
const EMBEDDED_SOUND_LEVEL: f64 = 0.01;
const PAL_SUBCARRIER_HZ: f64 = 4_433_618.75;
const NTSC_SUBCARRIER_HZ: f64 = 3_579_545.0;
const SECAM_DR_HZ: f64 = 4_406_250.0;
const SECAM_DB_HZ: f64 = 4_250_000.0;
const SECAM_DR_DEVIATION_HZ: f64 = 280_000.0;
const SECAM_DB_DEVIATION_HZ: f64 = 230_000.0;
const SECAM_LIMITS_HZ: (f64, f64) = (3_900_000.0, 4_756_000.0);
const SECAM_EMPHASIS_HZ: f64 = 85_000.0;
const SECAM_LEVEL: f32 = 0.115;
/// System B/G's vestige: the lower sideband runs full to 0.75 MHz and is gone by 1.25 MHz.
const VESTIGE_HZ: (f64, f64) = (750_000.0, 1_250_000.0);
const VESTIGE_TAPS: usize = 255;

pub const BAR_LEVELS: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
/// 75 % bars, so an AM carrier is never driven past zero.
pub const COLOR_BARS: [[f32; 3]; 8] = [
    [0.75, 0.75, 0.75],
    [0.75, 0.75, 0.0],
    [0.0, 0.75, 0.75],
    [0.0, 0.75, 0.0],
    [0.75, 0.0, 0.75],
    [0.75, 0.0, 0.0],
    [0.0, 0.0, 0.75],
    [0.0, 0.0, 0.0],
];

#[derive(Clone, Copy, Debug)]
struct Layout {
    line_s: f64,
    sync_s: f64,
    back_porch_s: f64,
    active_s: f64,
    pre_eq: u16,
    broad: u16,
    post_eq: u16,
    field_half_lines: u16,
    blank_after_group: u16,
    active_lines: u16,
    blanking: f32,
    black: f32,
    burst: (f64, f64),
}

fn layout(standard: AtvStandard) -> Layout {
    let (sync_us, back_us, active_us, pre_eq, broad, post_eq, blank_after_group, active_lines) =
        match standard {
            AtvStandard::Ccir625 => (4.7, 5.8, 52.0, 5, 5, 5, 17, 288),
            AtvStandard::Eia525 => (4.7, 4.7, 52.6, 6, 6, 6, 12, 240),
            AtvStandard::SystemA405 => (9.0, 5.8, 82.2, 0, 8, 0, 10, 188),
        };
    let (blanking, black, burst) = match standard {
        AtvStandard::Eia525 => (NTSC_BLANKING, NTSC_BLACK, (5.3e-6, 7.8e-6)),
        AtvStandard::Ccir625 => (BLANKING, BLANKING, (5.6e-6, 7.85e-6)),
        AtvStandard::SystemA405 => (BLANKING, BLANKING, (9.9e-6, 12.15e-6)),
    };
    let line_s = 1.0 / standard.line_rate_hz();
    Layout {
        line_s,
        sync_s: sync_us * 1e-6,
        back_porch_s: back_us * 1e-6,
        active_s: active_us * 1e-6,
        pre_eq,
        broad,
        post_eq,
        field_half_lines: standard.lines(),
        blank_after_group,
        active_lines,
        blanking,
        black,
        burst,
    }
}

#[derive(Clone, Copy)]
enum Seg {
    Equalizing,
    Broad,
    HalfBlank,
    HalfLine,
    /// `row` counts picture lines down the field.
    Line {
        row: Option<u16>,
    },
}

pub struct AtvSource {
    rate: f64,
    layout: Layout,
    modulation: AtvModulation,
    invert: bool,
    interlace: bool,
    color: AtvColor,
    sound_subcarrier_hz: Option<f64>,
    swing_hz: f64,
    vestigial: bool,
}

impl AtvSource {
    #[must_use]
    pub fn new(params: &AtvParams, rate: f64) -> Self {
        Self {
            rate,
            layout: layout(params.standard),
            modulation: params.modulation,
            invert: params.invert,
            interlace: params.interlace,
            color: params.color,
            sound_subcarrier_hz: params.sound_subcarrier_hz,
            swing_hz: FM_SWING * rate,
            vestigial: false,
        }
    }

    /// AM with the lower sideband cut to a broadcast vestige.
    #[must_use]
    pub fn vestigial(mut self) -> Self {
        self.vestigial = true;
        self
    }

    /// FM with `hz` from sync tip to peak white.
    #[must_use]
    pub fn with_swing(mut self, hz: f64) -> Self {
        self.swing_hz = hz;
        self
    }

    fn field(&self, second: bool) -> Vec<Seg> {
        let l = self.layout;
        let group = usize::from(l.pre_eq + l.broad + l.post_eq);
        let lead = usize::from(second && l.field_half_lines % 2 == 1);
        let half_lines = if self.interlace {
            l.field_half_lines
        } else {
            l.field_half_lines & !1
        };
        let rest = usize::from(half_lines) - group - lead;
        let mut segs = Vec::new();
        segs.extend(std::iter::repeat_n(Seg::Equalizing, usize::from(l.pre_eq)));
        segs.extend(std::iter::repeat_n(Seg::Broad, usize::from(l.broad)));
        segs.extend(std::iter::repeat_n(Seg::Equalizing, usize::from(l.post_eq)));
        segs.extend(std::iter::repeat_n(Seg::HalfBlank, lead));
        for k in 0..rest / 2 {
            let row = k.checked_sub(usize::from(l.blank_after_group));
            segs.push(Seg::Line {
                row: row
                    .filter(|&r| r < usize::from(l.active_lines))
                    .map(|r| r as u16),
            });
        }
        if rest % 2 == 1 {
            segs.push(Seg::HalfLine);
        }
        segs
    }

    /// The composite level `u` seconds into `seg`, `t` seconds since the start, so the
    /// subcarrier runs on unbroken as it does on air.
    fn level(&self, seg: Seg, u: f64, t: f64, line: u64, pixel: &Pixel<'_>) -> f32 {
        let l = self.layout;
        let half = l.line_s / 2.0;
        match seg {
            Seg::Equalizing => {
                if u < l.sync_s / 2.0 {
                    SYNC
                } else {
                    l.blanking
                }
            }
            Seg::Broad => {
                if u < half - l.sync_s {
                    SYNC
                } else {
                    l.blanking
                }
            }
            Seg::HalfBlank => l.blanking,
            Seg::HalfLine => self.level(Seg::Line { row: None }, u, t, line, pixel),
            Seg::Line { row } => {
                if let Some(at) = self.at(row, u) {
                    self.picture(pixel(at.0, at.1), t, line)
                } else if u < l.sync_s {
                    SYNC
                } else if (l.burst.0..l.burst.1).contains(&u) {
                    l.blanking + self.burst(t, line)
                } else {
                    l.blanking
                }
            }
        }
    }

    /// Where `u` seconds into a line with picture `row` falls in the picture, if it does.
    fn at(&self, row: Option<u16>, u: f64) -> Option<(f64, f64)> {
        let l = self.layout;
        let active_start = l.sync_s + l.back_porch_s;
        let row = row?;
        (active_start..active_start + l.active_s)
            .contains(&u)
            .then(|| {
                (
                    (u - active_start) / l.active_s,
                    f64::from(row) / f64::from(l.active_lines),
                )
            })
    }

    /// The SECAM difference `u` seconds into `seg` drives, and whether it is the red line:
    /// rest from the back porch on, the picture's colour over it, nothing through sync.
    fn secam_drive(&self, seg: Seg, u: f64, line: u64, pixel: &Pixel<'_>) -> Option<(bool, f32)> {
        let l = self.layout;
        let row = match seg {
            Seg::Line { row } => row,
            Seg::HalfLine => None,
            _ => return None,
        };
        if !(l.burst.0..l.sync_s + l.back_porch_s + l.active_s).contains(&u) {
            return None;
        }
        let red = line.is_multiple_of(2);
        let drive = self.at(row, u).map_or(0.0, |(x, y)| {
            let [r, g, b] = pixel(x, y);
            let luma = 0.299 * r + 0.587 * g + 0.114 * b;
            if red {
                -1.902 * (r - luma)
            } else {
                1.505 * (b - luma)
            }
        });
        Some((red, drive))
    }

    /// Luma plus `U sin + V cos` of the subcarrier, V flipping on alternate PAL lines.
    fn picture(&self, [r, g, b]: [f32; 3], t: f64, line: u64) -> f32 {
        let l = self.layout;
        let span = 1.0 - l.black;
        let y = 0.299 * r + 0.587 * g + 0.114 * b;
        let Some(carrier) = color_carrier(self.color) else {
            return l.black + span * y;
        };
        let u = 0.493 * (b - y);
        let v = 0.877 * (r - y) * self.v_sign(line);
        let (sin, cos) = (TAU * carrier * t).sin_cos();
        l.black + span * (y + u * sin as f32 + v * cos as f32)
    }

    /// The colour burst: on -U, swung 45 degrees toward the line's V sign for PAL.
    fn burst(&self, t: f64, line: u64) -> f32 {
        let Some(carrier) = color_carrier(self.color) else {
            return 0.0;
        };
        let (sin, cos) = (TAU * carrier * t).sin_cos();
        let (sin, cos) = (sin as f32, cos as f32);
        if self.color == AtvColor::Pal {
            PAL_BURST * std::f32::consts::FRAC_1_SQRT_2 * (-sin + self.v_sign(line) * cos)
        } else {
            -NTSC_BURST * sin
        }
    }

    fn v_sign(&self, line: u64) -> f32 {
        if self.color == AtvColor::Pal && line % 2 == 1 {
            -1.0
        } else {
            1.0
        }
    }

    fn seg_seconds(&self, seg: Seg) -> f64 {
        match seg {
            Seg::Line { .. } => self.layout.line_s,
            _ => self.layout.line_s / 2.0,
        }
    }

    fn video(&self, frames: u32, luma: &dyn Fn(f64) -> f32) -> Vec<f32> {
        self.composite(
            frames,
            &|x, _| {
                let y = luma(x);
                [y, y, y]
            },
            None,
        )
    }

    fn composite(&self, frames: u32, pixel: &Pixel<'_>, sound_hz: Option<f64>) -> Vec<f32> {
        let mut segs = Vec::new();
        for _ in 0..frames {
            segs.extend(self.field(false));
            segs.extend(self.field(self.interlace));
        }
        let mut out = Vec::new();
        let mut secam = Vec::new();
        let mut start = 0.0f64;
        let mut line = 0u64;
        for seg in segs {
            let seconds = self.seg_seconds(seg);
            let end = start + seconds * self.rate;
            let first = start.round() as usize;
            let count = end.round() as usize - first;
            for k in 0..count {
                let u = ((first + k) as f64 - start) / self.rate;
                let t = (first + k) as f64 / self.rate;
                out.push(self.level(seg, u, t, line, pixel));
                if self.color == AtvColor::Secam {
                    secam.push(self.secam_drive(seg, u, line, pixel));
                }
            }
            if matches!(seg, Seg::Line { .. }) {
                line += 1;
            }
            start = end;
        }
        add_secam(&mut out, &secam, self.rate, 1.0 - self.layout.black);
        if self.modulation == AtvModulation::Fm {
            add_sound_subcarrier(&mut out, self.rate, self.sound_subcarrier_hz.zip(sound_hz));
        }
        out
    }

    fn modulate(&self, video: &[f32], sound_hz: Option<f64>) -> Vec<Complex<f32>> {
        let mut phase = 0.0f64;
        let mut iq: Vec<Complex<f32>> = video
            .iter()
            .map(|&v| {
                let m = f64::from(if self.invert { 1.0 - v } else { v });
                match self.modulation {
                    AtvModulation::Am => Complex::new((1.0 - AM_DEPTH * m) as f32, 0.0),
                    AtvModulation::Fm => {
                        phase += TAU * self.swing_hz * (m - 0.5) / self.rate;
                        Complex::from_polar(1.0, phase as f32)
                    }
                }
            })
            .collect();
        if self.modulation == AtvModulation::Am {
            if self.vestigial {
                iq = vestige(&iq, self.rate);
            }
            self.add_am_sound(&mut iq, sound_hz);
        }
        iq
    }

    fn add_am_sound(&self, iq: &mut [Complex<f32>], sound_hz: Option<f64>) {
        let Some((carrier, tone)) = self.sound_subcarrier_hz.zip(sound_hz) else {
            return;
        };
        let mut phase = 0.0f64;
        for (i, sample) in iq.iter_mut().enumerate() {
            let audio = (TAU * tone * i as f64 / self.rate).sin();
            phase += TAU * (carrier + SOUND_DEVIATION_HZ * audio) / self.rate;
            *sample += Complex::from_polar(SOUND_LEVEL as f32, phase as f32);
        }
    }
}

type Pixel<'a> = dyn Fn(f64, f64) -> [f32; 3] + 'a;

/// A picture of `pixel(x, y)`, both from 0 to 1 across and down each field.
#[must_use]
pub fn picture(source: &AtvSource, frames: u32, pixel: &Pixel<'_>) -> Vec<Complex<f32>> {
    let video = source.composite(frames, pixel, None);
    source.modulate(&video, None)
}

#[must_use]
pub fn bars(source: &AtvSource, frames: u32) -> Vec<Complex<f32>> {
    let video = source.video(frames, &|x| {
        let bar = ((x * BAR_LEVELS.len() as f64) as usize).min(BAR_LEVELS.len() - 1);
        BAR_LEVELS[bar]
    });
    source.modulate(&video, None)
}

#[must_use]
pub fn color_bars_with_tone(source: &AtvSource, frames: u32) -> Vec<Complex<f32>> {
    let video = source.composite(
        frames,
        &|x, _| {
            let bar = ((x * COLOR_BARS.len() as f64) as usize).min(COLOR_BARS.len() - 1);
            COLOR_BARS[bar]
        },
        Some(1_000.0),
    );
    source.modulate(&video, Some(1_000.0))
}

fn color_carrier(color: AtvColor) -> Option<f64> {
    match color {
        AtvColor::Monochrome | AtvColor::Secam => None,
        AtvColor::Pal => Some(PAL_SUBCARRIER_HZ),
        AtvColor::Ntsc => Some(NTSC_SUBCARRIER_HZ),
    }
}

/// SECAM's FM chroma over `video`: each driven run pre-emphasized, then swung about its
/// line's rest frequency.
fn add_secam(video: &mut [f32], drive: &[Option<(bool, f32)>], rate: f64, span: f32) {
    let k = 2.0 * rate;
    let (low, high) = (TAU * SECAM_EMPHASIS_HZ, TAU * 3.0 * SECAM_EMPHASIS_HZ);
    let a0 = 1.0 + k / high;
    let (b0, b1, a1) = (
        (1.0 + k / low) / a0,
        (1.0 - k / low) / a0,
        (1.0 - k / high) / a0,
    );
    let (mut x1, mut y1) = (None, 0.0);
    let mut phase = 0.0f64;
    for (sample, drive) in video.iter_mut().zip(drive) {
        let Some((red, d)) = *drive else {
            x1 = None;
            continue;
        };
        let d = f64::from(d);
        let previous = x1.unwrap_or(d);
        let emphasized = if x1.is_some() {
            b0 * d + b1 * previous - a1 * y1
        } else {
            d
        };
        (x1, y1) = (Some(d), emphasized);
        let (rest, deviation) = if red {
            (SECAM_DR_HZ, SECAM_DR_DEVIATION_HZ)
        } else {
            (SECAM_DB_HZ, SECAM_DB_DEVIATION_HZ)
        };
        let hz = (rest + deviation * emphasized).clamp(SECAM_LIMITS_HZ.0, SECAM_LIMITS_HZ.1);
        phase = (phase + TAU * hz / rate).rem_euclid(TAU);
        *sample += SECAM_LEVEL * span * phase.cos() as f32;
    }
}

/// Cuts the lower sideband of AM vision to the broadcast vestige.
fn vestige(iq: &[Complex<f32>], rate: f64) -> Vec<Complex<f32>> {
    const GRID: usize = 4096;
    let (full, gone) = (VESTIGE_HZ.0 / rate, VESTIGE_HZ.1 / rate);
    let gain = |f: f64| ((f + gone) / (gone - full)).clamp(0.0, 1.0);
    let centre = (VESTIGE_TAPS - 1) as f64 / 2.0;
    let taps: Vec<Complex<f32>> = blackman_harris(VESTIGE_TAPS)
        .iter()
        .enumerate()
        .map(|(n, &w)| {
            let t = n as f64 - centre;
            let h: Complex<f64> = (0..GRID)
                .map(|k| {
                    let f = k as f64 / GRID as f64 - 0.5;
                    Complex::from_polar(gain(f), TAU * f * t)
                })
                .sum::<Complex<f64>>()
                / GRID as f64;
            Complex::new((h.re * f64::from(w)) as f32, (h.im * f64::from(w)) as f32)
        })
        .collect();
    let mut out = Vec::with_capacity(iq.len());
    FirC::new(&taps).process(iq, &mut out);
    out
}

fn add_sound_subcarrier(video: &mut [f32], rate: f64, sound: Option<(f64, f64)>) {
    let Some((carrier, tone)) = sound else {
        return;
    };
    let mut phase = 0.0;
    for (i, sample) in video.iter_mut().enumerate() {
        let audio = (TAU * tone * i as f64 / rate).sin();
        phase += TAU * (carrier + SOUND_DEVIATION_HZ * audio) / rate;
        *sample += EMBEDDED_SOUND_LEVEL as f32 * phase.cos() as f32;
    }
}

/// The signals a generator offers for this decoder.
pub(crate) static SIGNALS: &[Signal] = &[Signal {
    id: "atv",
    label: "ATV colour bars",
    rate_hz: 2_400_000.0,
    render: Render::Always(atv),
}];

fn atv() -> Vec<Complex<f32>> {
    let params = AtvParams {
        modulation: crate::atv::AtvModulation::Am,
        standard: crate::atv::AtvStandard::Ccir625,
        ..AtvParams::default()
    };
    let source = AtvSource::new(&params, 2_400_000.0);
    bars(&source, 8)
}

const FIXTURE_RATE: f64 = 2_400_000.0;

/// The reference captures the fixtures are written from.
pub(crate) static RECIPES: &[sdrmm_modem::synth::Recipe] = &[sdrmm_modem::synth::Recipe {
    stem: "atv_ccir625_2m4",
    rate: FIXTURE_RATE,
    note: "atv channel at +200 kHz -> 625/25 AM, five vertical bars black to white",
    iq: || {
        let source = AtvSource::new(&crate::atv::AtvParams::default(), FIXTURE_RATE);
        Ok(sdrmm_modem::synth::shifted(
            bars(&source, 2),
            200_000.0,
            FIXTURE_RATE,
        ))
    },
}];

#[cfg(test)]
mod tests {
    use crate::atv::AtvParams;

    use super::*;

    const RATE: f64 = 2_000_000.0;

    fn source(standard: AtvStandard) -> AtvSource {
        AtvSource::new(
            &AtvParams {
                standard,
                ..AtvParams::default()
            },
            RATE,
        )
    }

    #[test]
    fn a_frame_lasts_exactly_one_frame_period() {
        for standard in [
            AtvStandard::Ccir625,
            AtvStandard::Eia525,
            AtvStandard::SystemA405,
        ] {
            let src = source(standard);
            let video = src.video(2, &|_| 1.0);
            let want = 2.0 * RATE / standard.frame_rate_hz();
            let got = video.len() as f64;
            assert!(
                (got - want).abs() <= 2.0,
                "{standard:?}: {got} samples for two frames, expected {want}"
            );
        }
    }

    #[test]
    fn the_second_field_lands_its_lines_half_a_line_later() {
        for standard in [
            AtvStandard::Ccir625,
            AtvStandard::Eia525,
            AtvStandard::SystemA405,
        ] {
            let src = source(standard);
            let seconds =
                |segs: Vec<Seg>| segs.into_iter().map(|s| src.seg_seconds(s)).sum::<f64>();
            let length = |second: bool| seconds(src.field(second));
            assert!((length(false) - length(true)).abs() < 1e-12, "{standard:?}");
            let until = |second: bool, stop: fn(&Seg) -> bool| {
                seconds(
                    src.field(second)
                        .into_iter()
                        .take_while(|s| !stop(s))
                        .collect(),
                )
            };
            let is_line = |s: &Seg| matches!(s, Seg::Line { .. });
            let is_broad = |s: &Seg| matches!(s, Seg::Broad);
            let line_s = src.layout.line_s;
            let grid = (length(false) + until(true, is_line) - until(false, is_line)) / line_s;
            assert!(
                (grid - grid.round()).abs() < 1e-9,
                "{standard:?}: the second field's lines sit {grid} lines after the first's"
            );
            let shift = (length(false) + until(true, is_broad) - until(false, is_broad)) / line_s;
            assert!(
                (shift.fract() - 0.5).abs() < 1e-9,
                "{standard:?}: broad pulses moved by {shift} lines, expected a half-line offset"
            );
        }
    }

    #[test]
    fn levels_sit_where_the_standards_put_them() {
        let src = source(AtvStandard::Ccir625);
        let video = src.video(1, &|_| 1.0);
        let lo = video.iter().copied().fold(f32::MAX, f32::min);
        let hi = video.iter().copied().fold(f32::MIN, f32::max);
        assert_eq!(lo, SYNC);
        assert_eq!(hi, 1.0);
        let l = src.layout;
        let at = |seconds: f64| video[(seconds * RATE) as usize];
        let group = f64::from(l.pre_eq + l.broad + l.post_eq) * l.line_s / 2.0;
        let line_start = group + 20.0 * l.line_s;
        assert_eq!(at(line_start + l.sync_s + 0.5e-6), BLANKING);
        assert_eq!(at(line_start + l.sync_s / 2.0), SYNC);
        assert_eq!(at(line_start + l.line_s - 0.5e-6), BLANKING);
    }

    #[test]
    fn am_keys_sync_to_peak_carrier() {
        let src = source(AtvStandard::Ccir625);
        let iq = bars(&src, 1);
        let peak = iq.iter().map(|s| s.norm()).fold(f32::MIN, f32::max);
        let trough = iq.iter().map(|s| s.norm()).fold(f32::MAX, f32::min);
        assert!((peak - 1.0).abs() < 1e-6, "peak {peak}");
        assert!(
            (trough - (1.0 - AM_DEPTH as f32)).abs() < 1e-6,
            "trough {trough}"
        );
    }
}
