use sdrmm_wire::ChannelParams;

use super::*;
use crate::{
    AUDIO_RATE, VideoPicture,
    nfm::NfmParams,
    synth::atv::{AtvSource, BAR_LEVELS, COLOR_BARS, bars, color_bars_with_tone},
    testutil::{dominant_tone, realtime_budget, settings},
};

fn channel(p: AtvParams) -> AtvChannel {
    AtvChannel::new(
        ChannelCtx {
            input_rate: p.rate_hz,
        },
        settings(ChannelParams::of(&p).expect("atv settings")),
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
    let pictures = run(&mut channel(p), &iq);
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
    let (pictures, audio) = run_media(&mut channel(p), &iq);
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
    let pictures = run(&mut channel(p), &iq);
    assert_colour_bars(pictures.last().expect("an NTSC picture"), 14.0);
}

#[test]
fn pal_colour_holds_on_every_field_whichever_line_carries_minus_v() {
    let p = AtvParams {
        color: AtvColor::Pal,
        ..params_for(AtvStandard::Ccir625, AtvModulation::Fm)
    };
    let iq = color_bars_with_tone(&AtvSource::new(&p, p.rate_hz), 4);
    let pictures = run(&mut channel(p), &iq);
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
            let pictures = run(&mut channel(p), &iq);
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
        let picture = run(&mut channel(p.clone()), &iq).pop().expect("a picture");
        assert_bars_ascend(&picture, &format!("swing {swing}"));
    }
}

#[test]
fn fm_holds_its_picture_three_megahertz_off_tune() {
    let p = params_for(AtvStandard::Ccir625, AtvModulation::Fm);
    for offset in [-3_000_000.0, 3_000_000.0] {
        let mut iq = bars(&AtvSource::new(&p, p.rate_hz).with_swing(6_000_000.0), 3);
        crate::synth::shift(&mut iq, offset, p.rate_hz);
        let picture = run(&mut channel(p.clone()), &iq).pop().expect("a picture");
        assert_bars_ascend(&picture, &format!("offset {offset}"));
    }
}

#[test]
fn a_weak_fm_carrier_still_scans_a_clean_picture() {
    let p = params_for(AtvStandard::Ccir625, AtvModulation::Fm);
    let mut iq = bars(&AtvSource::new(&p, p.rate_hz).with_swing(6_000_000.0), 3);
    crate::synth::add_noise(&mut iq, 0xA7C3, 0.4);
    let picture = run(&mut channel(p), &iq).pop().expect("a picture");
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
    let picture = run(&mut channel(p), &iq).pop().expect("a picture");
    assert_bars_ascend(&picture, "inverted");
}

#[test]
fn progressive_sources_fill_every_row() {
    let mut p = params_for(AtvStandard::Ccir625, AtvModulation::Am);
    p.interlace = false;
    let iq = bars(&AtvSource::new(&p, p.rate_hz), 3);
    let picture = run(&mut channel(p), &iq).pop().expect("a picture");
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
    let picture = run(&mut channel(p), &iq).pop().expect("a picture");
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
    let picture = run(&mut channel(p), &iq).pop().expect("a picture");
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
    assert!(run(&mut channel(p), &quiet).is_empty());
}

#[test]
fn apply_changes_the_standard_and_the_geometry_with_it() {
    let mut chan = channel(params_for(AtvStandard::Ccir625, AtvModulation::Am));
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
    let mut chan = channel(AtvParams::default());
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
    let mut chan = channel(AtvParams::default());
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
    picture::repair_clicks(&mut luma, 4, 0.3, &mut Vec::new());
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
    let mut chan = channel(p);
    let mut out = ChannelOutputs::default();
    let started = std::time::Instant::now();
    for block in iq.chunks(16_384) {
        out.reset();
        chan.process(block, &mut out);
    }
    let elapsed = started.elapsed().as_secs_f64();
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
