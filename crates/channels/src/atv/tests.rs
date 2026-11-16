use sdrmm_wire::ChannelParams;

use super::*;
use crate::{
    AUDIO_RATE, VideoPicture,
    nfm::NfmParams,
    synth::atv::{AtvSource, BAR_LEVELS, COLOR_BARS, bars, color_bars_with_tone, picture},
    testutil::{THROUGHPUT_ATTEMPTS, dominant_tone, fastest_seconds, realtime_budget, settings},
};

fn channel(p: &AtvParams) -> AtvChannel {
    AtvChannel::new(
        ChannelCtx {
            input_rate: p.rate_hz,
        },
        settings(ChannelParams::of(p).expect("atv settings")),
    )
    .unwrap()
}

fn run(chan: &mut AtvChannel, iq: &[Complex<f32>]) -> Vec<VideoPicture> {
    run_media(chan, iq).0
}

fn run_media(chan: &mut AtvChannel, iq: &[Complex<f32>]) -> (Vec<VideoPicture>, Vec<f32>) {
    let mut out = ChannelOutputs::default();
    let mut pictures = Vec::new();
    let mut audio = Vec::new();
    let mut at = 0;
    for size in [4_096usize, 1_000, 65_536, 777].iter().copied().cycle() {
        if at >= iq.len() {
            break;
        }
        let end = (at + size).min(iq.len());
        out.reset();
        chan.process(&iq[at..end], &mut out);
        pictures.append(&mut out.video);
        audio.extend_from_slice(&out.audio_pcm);
        at = end;
    }
    (pictures, audio)
}

fn span_mean(picture: &VideoPicture, row: usize, part: usize, parts: usize) -> f64 {
    let w = usize::from(picture.width);
    let lo = w * part / parts;
    let hi = w * (part + 1) / parts;
    let inset = (hi - lo) / 4;
    let span = &picture.luma[row * w + lo + inset..row * w + hi - inset];
    span.iter().map(|&v| f64::from(v)).sum::<f64>() / span.len() as f64
}

fn bar_luma(picture: &VideoPicture, row: usize, bar: usize) -> f64 {
    span_mean(picture, row, bar, BAR_LEVELS.len())
}

fn bar_rgb(picture: &VideoPicture, row: usize, bar: usize) -> [f64; 3] {
    let width = usize::from(picture.width);
    let lo = width * bar / COLOR_BARS.len();
    let hi = width * (bar + 1) / COLOR_BARS.len();
    let inset = (hi - lo) / 4;
    let mut sum = [0.0; 3];
    let count = hi - lo - 2 * inset;
    for x in lo + inset..hi - inset {
        let at = (row * width + x) * 3;
        for (channel, total) in sum.iter_mut().enumerate() {
            *total += f64::from(picture.rgb[at + channel]);
        }
    }
    sum.map(|value| value / count as f64)
}

fn params_for(standard: AtvStandard, modulation: AtvModulation) -> AtvParams {
    AtvParams {
        modulation,
        standard,
        ..AtvParams::default()
    }
}

fn assert_bars_ascend(picture: &VideoPicture, what: &str) {
    let row = usize::from(picture.height) / 2;
    let luma: Vec<f64> = (0..BAR_LEVELS.len())
        .map(|bar| bar_luma(picture, row, bar))
        .collect();
    for pair in luma.windows(2) {
        assert!(
            pair[1] > pair[0] + 40.0,
            "{what}: bars not ascending: {luma:?}"
        );
    }
    assert!(luma[0] < 25.0, "{what}: black {luma:?}");
    assert!(luma[BAR_LEVELS.len() - 1] > 230.0, "{what}: white {luma:?}");
}

fn assert_colour_bars(picture: &VideoPicture, tolerance: f64) {
    let middle = usize::from(picture.height) / 3;
    for row in [middle, middle + 1] {
        for (bar, expected) in COLOR_BARS.iter().enumerate() {
            let got = bar_rgb(picture, row, bar);
            for channel in 0..3 {
                let want = f64::from(expected[channel]) * 255.0;
                assert!(
                    (got[channel] - want).abs() < tolerance,
                    "row {row} bar {bar} channel {channel}: got {got:?}, expected {expected:?}"
                );
            }
        }
    }
}

#[test]
fn decodes_a_bar_pattern_from_a_ccir_625_transmission() {
    let p = params_for(AtvStandard::Ccir625, AtvModulation::Am);
    let iq = bars(&AtvSource::new(&p, p.rate_hz), 4);
    let pictures = run(&mut channel(&p), &iq);
    assert!(pictures.len() >= 6, "pictures {}", pictures.len());
    let picture = pictures.last().expect("at least one picture");
    assert_eq!((picture.width, picture.height), (922, 576));
    assert_eq!(
        picture.luma.len(),
        usize::from(picture.width) * usize::from(picture.height)
    );
    for row in [200usize, 201, 400] {
        for (bar, &level) in BAR_LEVELS.iter().enumerate() {
            let got = bar_luma(picture, row, bar);
            let want = f64::from(level) * 255.0;
            assert!(
                (got - want).abs() < 6.0,
                "row {row} bar {bar}: luma {got:.0}, expected {want:.0}"
            );
        }
    }
}

#[test]
fn decodes_pal_colour_to_the_standard_and_the_am_sound_carrier() {
    let p = AtvParams {
        color: AtvColor::Pal,
        sound_subcarrier_hz: Some(5_500_000.0),
        ..params_for(AtvStandard::Ccir625, AtvModulation::Am)
    };
    let iq = color_bars_with_tone(&AtvSource::new(&p, p.rate_hz), 3);
    let (pictures, audio) = run_media(&mut channel(&p), &iq);
    let picture = pictures.last().expect("a PAL picture");
    assert_eq!(
        picture.rgb.len(),
        usize::from(picture.width) * usize::from(picture.height) * 3
    );
    assert_colour_bars(picture, 14.0);
    let settled = &audio[audio.len() / 2..];
    let (tone, ratio) = dominant_tone(settled, f64::from(AUDIO_RATE));
    assert!((tone - 1_000.0).abs() < 20.0, "sound tone {tone} Hz");
    assert!(ratio > 20.0, "sound tone ratio {ratio}");
}

#[test]
fn decodes_ntsc_colour_to_the_standard() {
    let p = AtvParams {
        color: AtvColor::Ntsc,
        ..params_for(AtvStandard::Eia525, AtvModulation::Fm)
    };
    let iq = color_bars_with_tone(&AtvSource::new(&p, p.rate_hz), 3);
    let pictures = run(&mut channel(&p), &iq);
    assert_colour_bars(pictures.last().expect("an NTSC picture"), 14.0);
}

#[test]
fn pal_colour_holds_on_every_field_whichever_line_carries_minus_v() {
    let p = AtvParams {
        color: AtvColor::Pal,
        ..params_for(AtvStandard::Ccir625, AtvModulation::Fm)
    };
    let iq = color_bars_with_tone(&AtvSource::new(&p, p.rate_hz), 4);
    let pictures = run(&mut channel(&p), &iq);
    assert!(pictures.len() >= 6, "pictures {}", pictures.len());
    for picture in &pictures[pictures.len() - 4..] {
        assert_colour_bars(picture, 14.0);
    }
}

#[test]
fn every_standard_and_modulation_scans_the_bars_in_order() {
    for standard in [
        AtvStandard::Ccir625,
        AtvStandard::Eia525,
        AtvStandard::SystemA405,
    ] {
        for modulation in [AtvModulation::Am, AtvModulation::Fm] {
            let p = params_for(standard, modulation);
            let iq = bars(&AtvSource::new(&p, p.rate_hz), 3);
            let pictures = run(&mut channel(&p), &iq);
            let picture = pictures
                .last()
                .unwrap_or_else(|| panic!("{standard:?}/{modulation:?} produced no picture"));
            assert_eq!(picture.height, 2 * Geometry::new(standard, 1.0).field_lines);
            assert_bars_ascend(picture, &format!("{standard:?}/{modulation:?}"));
        }
    }
}

#[test]
fn fm_levels_follow_the_sync_whatever_the_swing() {
    let p = params_for(AtvStandard::Ccir625, AtvModulation::Fm);
    for swing in [2_000_000.0, 10_000_000.0] {
        let iq = bars(&AtvSource::new(&p, p.rate_hz).with_swing(swing), 3);
        let picture = run(&mut channel(&p), &iq).pop().expect("a picture");
        assert_bars_ascend(&picture, &format!("swing {swing}"));
    }
}

#[test]
fn fm_holds_its_picture_three_megahertz_off_tune() {
    let p = params_for(AtvStandard::Ccir625, AtvModulation::Fm);
    for offset in [-3_000_000.0, 3_000_000.0] {
        let mut iq = bars(&AtvSource::new(&p, p.rate_hz).with_swing(6_000_000.0), 3);
        crate::synth::shift(&mut iq, offset, p.rate_hz);
        let picture = run(&mut channel(&p), &iq).pop().expect("a picture");
        assert_bars_ascend(&picture, &format!("offset {offset}"));
    }
}

#[test]
fn a_weak_fm_carrier_still_scans_a_clean_picture() {
    let p = params_for(AtvStandard::Ccir625, AtvModulation::Fm);
    let mut iq = bars(&AtvSource::new(&p, p.rate_hz).with_swing(6_000_000.0), 3);
    crate::synth::add_noise(&mut iq, 0xA7C3, 0.4);
    let picture = run(&mut channel(&p), &iq).pop().expect("a picture");
    let row = usize::from(picture.height) / 2;
    for (bar, &level) in BAR_LEVELS.iter().enumerate() {
        let got = bar_luma(&picture, row, bar);
        let want = f64::from(level) * 255.0;
        assert!(
            (got - want).abs() < 12.0,
            "bar {bar}: luma {got:.0}, expected {want:.0}"
        );
    }
}

#[test]
fn invert_undoes_a_reversed_transmission() {
    let mut p = params_for(AtvStandard::Ccir625, AtvModulation::Am);
    p.invert = true;
    let iq = bars(&AtvSource::new(&p, p.rate_hz), 3);
    let picture = run(&mut channel(&p), &iq).pop().expect("a picture");
    assert_bars_ascend(&picture, "inverted");
}

#[test]
fn progressive_sources_fill_every_row() {
    let mut p = params_for(AtvStandard::Ccir625, AtvModulation::Am);
    p.interlace = false;
    let iq = bars(&AtvSource::new(&p, p.rate_hz), 3);
    let picture = run(&mut channel(&p), &iq).pop().expect("a picture");
    let white = BAR_LEVELS.len() - 1;
    for row in [100usize, 101, 102, 103] {
        assert!(
            bar_luma(&picture, row, white) > 230.0,
            "row {row} of a progressive scan is blank"
        );
    }
}

#[test]
fn a_noisy_am_channel_still_scans_a_recognizable_picture() {
    let p = params_for(AtvStandard::Ccir625, AtvModulation::Am);
    let mut iq = bars(&AtvSource::new(&p, p.rate_hz), 3);
    crate::synth::add_noise(&mut iq, 0xA7C3, 0.08);
    let picture = run(&mut channel(&p), &iq).pop().expect("a picture");
    let row = usize::from(picture.height) / 2;
    let black = bar_luma(&picture, row, 0);
    let white = bar_luma(&picture, row, BAR_LEVELS.len() - 1);
    assert!(white - black > 200.0, "contrast {black:.0}..{white:.0}");
}

#[test]
fn a_narrow_channel_scans_a_narrow_picture() {
    let p = AtvParams {
        rate_hz: 2_400_000.0,
        ..params_for(AtvStandard::Ccir625, AtvModulation::Am)
    };
    let iq = bars(&AtvSource::new(&p, p.rate_hz), 3);
    let picture = run(&mut channel(&p), &iq).pop().expect("a picture");
    assert_eq!(picture.width, 125);
    assert_bars_ascend(&picture, "2.4 MHz");
}

#[test]
fn the_occupied_band_stays_inside_the_channel_rate() {
    for modulation in [AtvModulation::Am, AtvModulation::Fm] {
        let p = AtvParams {
            rate_hz: 2_400_000.0,
            ..params_for(AtvStandard::Ccir625, modulation)
        };
        let (low, high) = occupied_band(&p);
        assert!(
            -low < 1_000_000.0 && high < 1_000_000.0,
            "{modulation:?}: {low}..{high}"
        );
    }
}

#[test]
fn an_empty_channel_produces_no_picture() {
    let p = params_for(AtvStandard::Ccir625, AtvModulation::Am);
    let quiet = vec![Complex::new(0.0, 0.0); 400_000];
    assert!(run(&mut channel(&p), &quiet).is_empty());
}

#[test]
fn apply_changes_the_standard_and_the_geometry_with_it() {
    let mut chan = channel(&params_for(AtvStandard::Ccir625, AtvModulation::Am));
    assert_eq!((chan.picture.width(), chan.picture.height()), (922, 576));
    let p = params_for(AtvStandard::SystemA405, AtvModulation::Fm);
    chan.apply(settings(ChannelParams::of(&p).expect("atv settings")))
        .unwrap();
    assert_eq!(chan.picture.height(), 376);
    let iq = bars(&AtvSource::new(&p, p.rate_hz), 3);
    let picture = run(&mut chan, &iq).pop().expect("a picture");
    assert_eq!((picture.width, picture.height), (chan.picture.width(), 376));
}

#[test]
fn apply_refuses_a_rate_the_channel_is_not_fed() {
    let mut chan = channel(&AtvParams::default());
    let p = AtvParams {
        rate_hz: 10_000_000.0,
        ..AtvParams::default()
    };
    assert!(matches!(
        chan.apply(settings(ChannelParams::of(&p).expect("atv settings"))),
        Err(ChannelError::InvalidSettings(_))
    ));
}

#[test]
fn the_rate_setting_sets_the_input_rate() {
    let p = AtvParams {
        rate_hz: 12_000_000.0,
        ..AtvParams::default()
    };
    let params = ChannelParams::of(&p).expect("atv settings");
    assert_eq!(crate::input_rate(&params), 12_000_000.0);
}

#[test]
fn colour_needs_a_rate_that_carries_the_subcarrier() {
    let p = AtvParams {
        rate_hz: 8_000_000.0,
        color: AtvColor::Pal,
        ..AtvParams::default()
    };
    assert!(matches!(
        AtvChannel::new(
            ChannelCtx {
                input_rate: p.rate_hz
            },
            settings(ChannelParams::of(&p).expect("atv settings")),
        ),
        Err(ChannelError::InvalidSettings(_))
    ));
}

#[test]
fn mismatched_params_variant_is_rejected() {
    let mut chan = channel(&AtvParams::default());
    assert!(matches!(
        chan.apply(settings(
            ChannelParams::new("nfm", &NfmParams::default()).expect("nfm settings")
        )),
        Err(ChannelError::InvalidSettings(_))
    ));
}

#[test]
fn out_of_range_settings_are_rejected() {
    for p in [
        AtvParams {
            bandwidth_hz: Some(50_000.0),
            ..AtvParams::default()
        },
        AtvParams {
            bandwidth_hz: Some(f64::NAN),
            ..AtvParams::default()
        },
        AtvParams {
            rate_hz: 0.0,
            ..AtvParams::default()
        },
    ] {
        assert!(
            matches!(channel_filter(&p), Err(ChannelError::InvalidSettings(_))),
            "{p:?} must be rejected"
        );
    }
}

#[test]
fn rejects_a_mismatched_input_rate() {
    assert!(matches!(
        AtvChannel::new(
            ChannelCtx {
                input_rate: 48_000.0
            },
            settings(ChannelParams::of(&AtvParams::default()).expect("atv settings")),
        ),
        Err(ChannelError::InvalidSettings(_))
    ));
}

#[test]
fn repair_bridges_a_click_and_leaves_an_edge() {
    let mut luma: Vec<f32> = (0..64).map(|k| if k < 40 { 0.0 } else { 1.0 }).collect();
    for (k, v) in luma.iter_mut().enumerate().skip(14).take(5) {
        *v = 0.9 - 0.2 * (k as f32 - 16.0).abs();
    }
    let edge: Vec<f32> = luma[30..50].to_vec();
    luma::repair_clicks(&mut luma, 4, 0.3, &mut Vec::new());
    assert!(luma[10..22].iter().all(|v| v.abs() < 1e-6), "{luma:?}");
    assert_eq!(&luma[30..50], &edge[..]);
}

#[test]
fn colour_decode_keeps_well_ahead_of_the_channel_rate() {
    let p = AtvParams {
        color: AtvColor::Pal,
        ..params_for(AtvStandard::Ccir625, AtvModulation::Fm)
    };
    let iq = color_bars_with_tone(&AtvSource::new(&p, p.rate_hz), 3);
    let seconds = iq.len() as f64 / p.rate_hz;
    let elapsed = fastest_seconds(THROUGHPUT_ATTEMPTS, || {
        let mut chan = channel(&p);
        let mut out = ChannelOutputs::default();
        let started = std::time::Instant::now();
        for block in iq.chunks(16_384) {
            out.reset();
            chan.process(block, &mut out);
        }
        started.elapsed().as_secs_f64()
    });
    assert!(
        elapsed < realtime_budget(seconds),
        "{seconds:.2} s of colour video took {elapsed:.2} s"
    );
}

#[test]
fn the_sound_subcarrier_is_typed_in_mhz_and_kept_in_hz() {
    let form = atv_form();
    let sound = form
        .iter()
        .find(|field| field.key == "sound_subcarrier_hz")
        .expect("sound field");
    let Control::Number(spec) = &sound.control else {
        panic!("sound is a number: {:?}", sound.control);
    };
    assert_eq!(spec.unit.as_deref(), Some("MHz"));
    assert_eq!(spec.scale, Some(1e6));
    assert_eq!(spec.max, Some(7_500_000.0));
}

fn coloured(color: AtvColor, standard: AtvStandard, modulation: AtvModulation) -> AtvParams {
    AtvParams {
        color,
        ..params_for(standard, modulation)
    }
}

/// Mean RGB over the middle half of `row`.
fn row_rgb(picture: &VideoPicture, row: usize) -> [f64; 3] {
    let width = usize::from(picture.width);
    let mut sum = [0.0; 3];
    for x in width / 4..3 * width / 4 {
        for (channel, total) in sum.iter_mut().enumerate() {
            *total += f64::from(picture.rgb[(row * width + x) * 3 + channel]);
        }
    }
    sum.map(|value| value / (width / 2) as f64)
}

/// Mean false colour, `|R - G| + |B - G|`, over the middle half of `row`.
fn false_colour(picture: &VideoPicture, row: usize) -> f64 {
    let width = usize::from(picture.width);
    let pixels = width / 2;
    (width / 4..3 * width / 4)
        .map(|x| {
            let at = (row * width + x) * 3;
            let [r, g, b] = [0, 1, 2].map(|c| f64::from(picture.rgb[at + c]));
            (r - g).abs() + (b - g).abs()
        })
        .sum::<f64>()
        / pixels as f64
}

fn luma_swing(picture: &VideoPicture, row: usize) -> f64 {
    let width = usize::from(picture.width);
    let line = &picture.luma[row * width + width / 4..row * width + 3 * width / 4];
    let max = line.iter().copied().max().unwrap_or(0);
    let min = line.iter().copied().min().unwrap_or(0);
    f64::from(max - min)
}

/// A grey grating of `hz` across the picture: luma detail at the colour subcarrier.
fn grating(p: &AtvParams, hz: f64, frames: u32) -> Vec<Complex<f32>> {
    let active_s = match p.standard {
        AtvStandard::Eia525 => 52.6e-6,
        _ => 52.0e-6,
    };
    let source = AtvSource::new(p, p.rate_hz);
    picture(&source, frames, &|x, _| {
        let v = 0.5 + 0.3 * (std::f64::consts::TAU * hz * active_s * x).sin() as f32;
        [v, v, v]
    })
}

#[test]
fn vestigial_am_keeps_pal_colour_saturated() {
    let p = coloured(AtvColor::Pal, AtvStandard::Ccir625, AtvModulation::Am);
    let iq = color_bars_with_tone(&AtvSource::new(&p, p.rate_hz).vestigial(), 3);
    let pictures = run(&mut channel(&p), &iq);
    assert_colour_bars(pictures.last().expect("a PAL picture"), 14.0);
}

#[test]
fn the_comb_keeps_fine_luma_and_leaves_it_grey() {
    for (color, standard, hz) in [
        (AtvColor::Pal, AtvStandard::Ccir625, 4_000_000.0),
        (AtvColor::Ntsc, AtvStandard::Eia525, 3_200_000.0),
    ] {
        let p = coloured(color, standard, AtvModulation::Fm);
        let pictures = run(&mut channel(&p), &grating(&p, hz, 3));
        let picture = pictures.last().expect("a picture");
        let row = usize::from(picture.height) / 2;
        let swing = luma_swing(picture, row);
        let hue = false_colour(picture, row);
        assert!(swing > 60.0, "{color:?}: grating swing {swing}");
        assert!(hue < 12.0, "{color:?}: false colour {hue}");
    }
}

#[test]
fn colour_stays_on_its_side_of_a_horizontal_edge() {
    let p = coloured(AtvColor::Pal, AtvStandard::Ccir625, AtvModulation::Fm);
    let source = AtvSource::new(&p, p.rate_hz);
    let (top, bottom) = ([0.75, 0.0, 0.0], [0.0, 0.0, 0.75]);
    let iq = picture(&source, 3, &|_, y| if y < 0.5 { top } else { bottom });
    let picture = run(&mut channel(&p), &iq).pop().expect("a picture");
    let edge = usize::from(picture.height) / 2;
    for (row, want) in [(edge - 16, top), (edge + 16, bottom)] {
        let got = row_rgb(&picture, row);
        for channel in 0..3 {
            let want = f64::from(want[channel]) * 255.0;
            assert!(
                (got[channel] - want).abs() < 20.0,
                "row {row}: got {got:?}, expected {want:?}"
            );
        }
    }
}

#[test]
fn decodes_secam_colour_on_am_and_fm() {
    for modulation in [AtvModulation::Am, AtvModulation::Fm] {
        let p = coloured(AtvColor::Secam, AtvStandard::Ccir625, modulation);
        let iq = color_bars_with_tone(&AtvSource::new(&p, p.rate_hz), 3);
        let pictures = run(&mut channel(&p), &iq);
        assert_colour_bars(pictures.last().expect("a SECAM picture"), 20.0);
    }
}

#[test]
fn secam_needs_625_lines() {
    let p = coloured(AtvColor::Secam, AtvStandard::Eia525, AtvModulation::Fm);
    let made = AtvChannel::new(
        ChannelCtx {
            input_rate: p.rate_hz,
        },
        settings(ChannelParams::of(&p).expect("atv settings")),
    );
    assert!(matches!(made, Err(ChannelError::InvalidSettings(_))));
}

#[test]
fn saturation_and_hue_apply_without_losing_lock() {
    let p = coloured(AtvColor::Pal, AtvStandard::Ccir625, AtvModulation::Fm);
    let iq = color_bars_with_tone(&AtvSource::new(&p, p.rate_hz), 3);
    let mut chan = channel(&p);
    run(&mut chan, &iq[..iq.len() / 2]);
    assert!(chan.sync.locked());
    let grey = AtvParams {
        saturation_pct: 0.0,
        ..p.clone()
    };
    chan.apply(settings(ChannelParams::of(&grey).expect("atv settings")))
        .expect("apply");
    assert!(
        chan.sync.locked(),
        "a saturation change restarted the decoder"
    );
    let picture = run(&mut chan, &iq[iq.len() / 2..])
        .pop()
        .expect("a picture");
    let row = usize::from(picture.height) / 3;
    assert!(
        false_colour(&picture, row) < 3.0,
        "saturation 0 left colour"
    );

    let turned = AtvParams {
        hue_deg: 180.0,
        ..p
    };
    chan.apply(settings(ChannelParams::of(&turned).expect("atv settings")))
        .expect("apply");
    let picture = run(&mut chan, &iq).pop().expect("a picture");
    let [r, g, b] = bar_rgb(&picture, usize::from(picture.height) / 3, 5);
    assert!(
        g > r + 40.0 && b > r + 40.0,
        "red turned half round: {r} {g} {b}"
    );
}

#[test]
fn picture_adjustments_out_of_range_are_rejected() {
    for p in [
        AtvParams {
            saturation_pct: 250.0,
            ..AtvParams::default()
        },
        AtvParams {
            hue_deg: f64::NAN,
            ..AtvParams::default()
        },
    ] {
        let made = AtvChannel::new(
            ChannelCtx {
                input_rate: p.rate_hz,
            },
            settings(ChannelParams::of(&p).expect("atv settings")),
        );
        assert!(matches!(made, Err(ChannelError::InvalidSettings(_))));
    }
}

#[test]
fn pal_keeps_fine_grey_detail_that_changes_every_few_lines() {
    let p = coloured(AtvColor::Pal, AtvStandard::Ccir625, AtvModulation::Fm);
    let source = AtvSource::new(&p, p.rate_hz);
    let bands = 288.0 / 6.0;
    let iq = picture(&source, 3, &|x, y| {
        let on = ((y * bands) as u32).is_multiple_of(2);
        let detail = if on {
            0.3 * (std::f64::consts::TAU * 4_000_000.0 * 52.0e-6 * x).sin() as f32
        } else {
            0.0
        };
        let v = 0.5 + detail;
        [v, v, v]
    });
    let picture = run(&mut channel(&p), &iq).pop().expect("a picture");
    let rows: Vec<usize> = (100..400).collect();
    let swing = rows
        .iter()
        .map(|&r| luma_swing(&picture, r))
        .fold(0.0, f64::max);
    let hue = rows.iter().map(|&r| false_colour(&picture, r)).sum::<f64>() / rows.len() as f64;
    assert!(swing > 60.0, "detail swing {swing}");
    assert!(hue < 8.0, "false colour {hue}");
}

#[test]
fn pal_3d_decodes_colour_and_keeps_fine_grey_detail() {
    let p = AtvParams {
        pal_split: AtvPalSplit::Frames,
        ..coloured(AtvColor::Pal, AtvStandard::Ccir625, AtvModulation::Fm)
    };
    let iq = color_bars_with_tone(&AtvSource::new(&p, p.rate_hz), 8);
    let pictures = run(&mut channel(&p), &iq);
    assert!(
        pictures.len() >= 2 * (8 - 4),
        "3D keeps a picture a field: {}",
        pictures.len()
    );
    assert_colour_bars(pictures.last().expect("a 3D PAL picture"), 14.0);
    let pictures = run(&mut channel(&p), &grating(&p, 4_000_000.0, 8));
    let picture = pictures.last().expect("a 3D PAL picture");
    let row = usize::from(picture.height) / 2;
    assert!(luma_swing(picture, row) > 60.0, "3D grating swing");
    assert!(false_colour(picture, row) < 12.0, "3D false colour");
}

#[test]
fn a_very_weak_am_carrier_still_locks_and_noise_alone_does_not() {
    let p = coloured(AtvColor::Pal, AtvStandard::Ccir625, AtvModulation::Am);
    let source = AtvSource::new(&p, p.rate_hz).vestigial();
    let mut iq = color_bars_with_tone(&source, 6);
    crate::synth::add_noise(&mut iq, 7, 1.6);
    let pictures = run(&mut channel(&p), &iq);
    assert!(
        (8..=12).contains(&pictures.len()),
        "-2 dB CNR: {} pictures of 12 fields",
        pictures.len()
    );
    let mut noise = vec![Complex::new(0.0, 0.0); iq.len()];
    crate::synth::add_noise(&mut noise, 11, 1.0);
    assert!(
        run(&mut channel(&p), &noise).len() <= 1,
        "noise alone drew pictures"
    );
}

#[test]
fn fm_finds_its_own_polarity() {
    let mut sent = params_for(AtvStandard::Ccir625, AtvModulation::Fm);
    sent.invert = true;
    let iq = bars(
        &AtvSource::new(&sent, sent.rate_hz).with_swing(6_000_000.0),
        4,
    );
    let p = params_for(AtvStandard::Ccir625, AtvModulation::Fm);
    let picture = run(&mut channel(&p), &iq).pop().expect("a picture");
    assert_bars_ascend(&picture, "inverted FM, invert off");
}
