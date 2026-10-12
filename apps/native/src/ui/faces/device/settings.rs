use sdrmm_wire::{
    device::{Agc, AgcSetting, BandwidthSetting, Capabilities, DeviceSettings, StreamSettings},
    state::DeviceSet,
};
use zgui::prelude::*;

use super::{
    caps::{
        dc_block_on, filter_hz, filter_is_auto, has_dc_artifact, has_filter, snap_to_ranges,
        span_of,
    },
    extras::extra_chip,
    lanes::{rx_stream_count, stream_label},
    layout::{agc_mode_delta, radio_agc, rx_stages},
    radio::{LOOP_SETTING, Radio},
};
use crate::ui::{
    kit_sources::{
        NumberSpec,
        chips::{ChipFace, chip_field, chips, readout_chip, setting_chip, toggle_chip},
        dial::{is_tunable, tuning_range},
        number_field, tip, units,
    },
    widgets::{check, pick},
};

pub const CONVERTER_TIP: &str = "Local oscillator of a converter in front of the radio: positive for a downconverter, negative for an upconverter. Frequencies shown are what the antenna sees. Kept for this radio";

pub fn setting_chips(radio: Radio, set: &DeviceSet) -> impl IntoView {
    let caps = &set.capabilities;
    let mut items = vec![rate_chip(radio, caps)];
    if has_filter(caps) {
        items.push(filter_chip(radio, caps));
    }
    items.extend(antenna_chips(radio, caps));
    items.extend(agc_chip(radio, caps));
    items.extend(correction_chips(radio, caps));
    items.extend(switch_chips(radio, caps));
    let playing = set.playback.is_some();
    items.extend(
        caps.extra
            .iter()
            .filter(|setting| !playing || setting.name() != LOOP_SETTING)
            .map(|setting| extra_chip(radio, setting.clone())),
    );
    chips(items)
}

fn with_current(
    current: f64,
    mut options: Vec<(f64, String)>,
    show: fn(f64) -> String,
) -> Vec<(f64, String)> {
    if current > 0.0 && !options.iter().any(|(value, _)| *value == current) {
        options.insert(0, (current, show(current)));
    }
    options
}

fn rate_chip(radio: Radio, caps: &Capabilities) -> AnyView {
    let rate = radio.settings(|s| s.sample_rate.unwrap_or(0.0), 0.0);
    let shown = Signal::derive(move || units::sample_rate(rate.get()));
    if caps.sample_rates.len() == 1 && span_of(&caps.sample_rate_ranges).is_none() {
        return readout_chip("Rate", shown, "Sample rate, fixed by the radio");
    }
    let caps = caps.clone();
    setting_chip(ChipFace::new("Rate", shown, "Sample rate"), move || {
        AnyView::new(chip_field("Sample rate", rate_control(radio, &caps, rate)))
    })
}

fn rate_control(radio: Radio, caps: &Capabilities, rate: Signal<f64>) -> AnyView {
    let commit = move |sample_rate: f64| {
        radio.patch(DeviceSettings {
            sample_rate: Some(sample_rate),
            ..DeviceSettings::default()
        });
    };
    if !caps.sample_rates.is_empty() {
        let menu: Vec<(f64, String)> = caps
            .sample_rates
            .iter()
            .map(|r| (*r, units::sample_rate(*r)))
            .collect();
        let options =
            Memo::new(move |_| with_current(rate.get(), menu.clone(), units::sample_rate));
        let chosen = Signal::derive(move || Some(rate.get()));
        return AnyView::new(move || pick(options.get(), chosen, commit));
    }
    let span = span_of(&caps.sample_rate_ranges);
    let ranges = caps.sample_rate_ranges.clone();
    let mut spec =
        NumberSpec::unit("MS/s").step(span.and_then(|s| s.step).map_or(0.001, |step| step / 1e6));
    if let Some(span) = span {
        spec = spec.within(span.min / 1e6, span.max / 1e6);
    }
    let shown = Signal::derive(move || rate.get() / 1e6);
    AnyView::new(number_field(
        "Sample rate",
        shown,
        spec,
        Signal::stored(false),
        move |msps| commit(snap_to_ranges(&ranges, (msps * 1e6).round())),
    ))
}

fn filter_chip(radio: Radio, caps: &Capabilities) -> AnyView {
    let label = if caps.bandwidth_auto || !caps.bandwidths.is_empty() {
        "BW"
    } else {
        "Filter"
    };
    let auto = radio.settings(filter_is_auto, false);
    let value = Signal::derive(move || {
        if auto.get() {
            "auto".to_owned()
        } else {
            radio
                .read(|set| units::hz(filter_hz(&set.capabilities, &set.settings)))
                .unwrap_or_default()
        }
    });
    let caps = caps.clone();
    setting_chip(
        ChipFace::new(label, value, "Analog bandwidth before the ADC").quiet(auto),
        move || AnyView::new(chip_field("Analog bandwidth", filter_control(radio, &caps))),
    )
}

fn filter_control(radio: Radio, caps: &Capabilities) -> AnyView {
    let auto = radio.settings(filter_is_auto, false);
    let hz = Signal::derive(move || {
        radio
            .read(|set| filter_hz(&set.capabilities, &set.settings))
            .unwrap_or(0.0)
    });
    let commit = move |bandwidth: BandwidthSetting| {
        radio.patch(DeviceSettings {
            bandwidth: Some(bandwidth),
            ..DeviceSettings::default()
        });
    };
    let toggle = caps.bandwidth_auto.then(|| {
        tip(
            || "Let the radio match the filter to the rate".to_owned(),
            AnyView::new(view! {
                row(class = "kit-auto") {
                    {check(auto, move |on| commit(if on { BandwidthSetting::Auto } else { BandwidthSetting::Manual { hz: hz.get_untracked() } }))}
                    text(class = "kit-legend") {"Auto"}
                }
            }),
        )
    });
    let width = if caps.bandwidths.is_empty() {
        span_of(&caps.bandwidth_ranges).map(|span| {
            let ranges = caps.bandwidth_ranges.clone();
            let shown = Signal::derive(move || hz.get() / 1e6);
            let spec = NumberSpec::unit("MHz")
                .within(span.min / 1e6, span.max / 1e6)
                .step(0.01);
            AnyView::new(number_field(
                "Analog bandwidth",
                shown,
                spec,
                auto,
                move |mhz| {
                    commit(BandwidthSetting::Manual {
                        hz: snap_to_ranges(&ranges, (mhz * 1e6).round()),
                    });
                },
            ))
        })
    } else {
        let menu: Vec<(f64, String)> = caps
            .bandwidths
            .iter()
            .map(|w| (*w, units::hz(*w)))
            .collect();
        let options = Memo::new(move |_| with_current(hz.get(), menu.clone(), units::hz));
        let chosen = Signal::derive(move || Some(hz.get()));
        Some(AnyView::new(view! {
            box(class = "kit-slot", class:kit-muted = auto) {
                {move || pick(options.get(), chosen, move |hz| commit(BandwidthSetting::Manual { hz }))}
            }
        }))
    };
    AnyView::new(view! { row(class = "kit-chip-field__body") { {toggle} {width} } })
}

fn antenna_pick(radio: Radio, caps: &Capabilities, stream: Option<u32>) -> AnyView {
    let options: Vec<(String, String)> = caps
        .antennas
        .iter()
        .map(|a| (a.clone(), a.clone()))
        .collect();
    let chosen = antenna_of(radio, caps, stream);
    let chosen = Signal::derive(move || Some(chosen.get()));
    AnyView::new(pick(options, chosen, move |antenna| {
        radio.patch(match stream {
            Some(stream) => DeviceSettings {
                streams: vec![StreamSettings {
                    stream,
                    antenna: Some(antenna),
                    ..StreamSettings::default()
                }],
                ..DeviceSettings::default()
            },
            None => DeviceSettings {
                antenna: Some(antenna),
                ..DeviceSettings::default()
            },
        });
    }))
}

fn antenna_of(radio: Radio, caps: &Capabilities, stream: Option<u32>) -> Signal<String> {
    let first = caps.antennas.first().cloned().unwrap_or_default();
    Signal::derive(move || {
        radio
            .read(|set| match stream {
                Some(stream) => {
                    set.settings
                        .for_stream(stream, &set.capabilities.per_stream)
                        .antenna
                }
                None => set.settings.antenna.clone(),
            })
            .flatten()
            .unwrap_or_else(|| first.clone())
    })
}

fn antenna_chips(radio: Radio, caps: &Capabilities) -> Vec<AnyView> {
    if caps.antennas.len() <= 1 {
        return Vec::new();
    }
    if !caps.per_stream.antenna {
        let caps = caps.clone();
        let value = antenna_of(radio, &caps, None);
        return vec![setting_chip(
            ChipFace::new("Antenna", value, "Antenna port in use"),
            move || AnyView::new(chip_field("Antenna", antenna_pick(radio, &caps, None))),
        )];
    }
    let lanes = rx_stream_count(caps);
    (0..lanes)
        .map(|stream| {
            let port = stream_label("iq", stream, lanes);
            let caps = caps.clone();
            let value = antenna_of(radio, &caps, Some(stream));
            let field = format!("{port} antenna");
            setting_chip(
                ChipFace::new(
                    format!("Ant {port}"),
                    value,
                    format!("Antenna port feeding {port}"),
                ),
                move || {
                    AnyView::new(chip_field(
                        field.clone(),
                        antenna_pick(radio, &caps, Some(stream)),
                    ))
                },
            )
        })
        .collect()
}

fn agc_chip(radio: Radio, caps: &Capabilities) -> Option<AnyView> {
    if !caps.agc.offered() {
        return None;
    }
    let on_gain = !rx_stages(caps).is_empty();
    let state = Signal::derive(move || radio.read(radio_agc).unwrap_or_else(AgcSetting::off));
    let commit = move |agc: AgcSetting| {
        radio.patch(DeviceSettings {
            agc: Some(agc),
            ..DeviceSettings::default()
        });
    };
    let Agc::Modes { options } = &caps.agc else {
        let on = Signal::derive(move || state.get().on);
        return (!on_gain).then(|| {
            toggle_chip("AGC", on, "The radio sets its own gain", move |on| {
                commit(AgcSetting {
                    on,
                    ..state.get_untracked()
                });
            })
        });
    };
    let modes: Vec<(String, String)> = options
        .iter()
        .map(|option| {
            (
                option.value.clone(),
                option.label.clone().unwrap_or_else(|| option.value.clone()),
            )
        })
        .collect();
    let toggle = !on_gain;
    let named = modes.clone();
    let value = Signal::derive(move || {
        let state = state.get();
        if state.on || !toggle {
            let mode = state.mode.unwrap_or_default();
            named
                .iter()
                .find(|(value, _)| *value == mode)
                .map_or(mode, |(_, label)| label.clone())
        } else {
            "off".to_owned()
        }
    });
    let quiet = Signal::derive(move || toggle && !state.get().on);
    Some(setting_chip(
        ChipFace::new("AGC", value, "How the radio sets its own gain").quiet(quiet),
        move || {
            let modes = modes.clone();
            let on = Signal::derive(move || state.get().on);
            let switch = toggle.then(|| {
                AnyView::new(view! {
                    row(class = "kit-auto") {
                        {check(on, move |on| commit(AgcSetting { on, ..state.get_untracked() }))}
                        text(class = "kit-legend") {"Automatic gain"}
                    }
                })
            });
            let chosen = Signal::derive(move || state.get().mode);
            AnyView::new(chip_field(
                "Gain control",
                view! {
                    {switch}
                    box(class = "kit-slot", class:kit-muted = move || toggle && !on.get()) {
                        {pick(modes, chosen, move |mode: String| {
                            if let Some(delta) = radio.read_untracked(|set| agc_mode_delta(set, &mode)) {
                                radio.patch(delta);
                            }
                        })}
                    }
                },
            ))
        },
    ))
}

fn correction_chips(radio: Radio, caps: &Capabilities) -> Vec<AnyView> {
    let mut items = Vec::new();
    if caps.ppm {
        let ppm = radio.settings(|s| s.ppm.unwrap_or(0.0), 0.0);
        let value = Signal::derive(move || NumberSpec::unit("").show(ppm.get()));
        let quiet = Signal::derive(move || ppm.get() == 0.0);
        items.push(setting_chip(
            ChipFace::new(
                "PPM",
                value,
                "Frequency correction in parts per million, kept for this radio",
            )
            .quiet(quiet),
            move || {
                AnyView::new(chip_field(
                    "Frequency correction",
                    number_field(
                        "Frequency correction",
                        ppm,
                        NumberSpec::unit("ppm").step(1.0),
                        Signal::stored(false),
                        move |ppm| {
                            radio.patch(DeviceSettings {
                                ppm: Some(ppm),
                                ..DeviceSettings::default()
                            });
                        },
                    ),
                ))
            },
        ));
    }
    if is_tunable(tuning_range(caps)) {
        let offset = radio.settings(|s| s.offset_hz.unwrap_or(0.0), 0.0);
        let value = Signal::derive(move || converter_said(offset.get()));
        let quiet = Signal::derive(move || offset.get() == 0.0);
        let mhz = Signal::derive(move || offset.get() / 1e6);
        items.push(setting_chip(
            ChipFace::new("Conv", value, CONVERTER_TIP).quiet(quiet),
            move || {
                AnyView::new(chip_field(
                    "Converter offset",
                    number_field(
                        "Converter offset",
                        mhz,
                        NumberSpec::unit("MHz").step(0.001),
                        Signal::stored(false),
                        move |mhz| {
                            radio.patch(DeviceSettings {
                                offset_hz: Some((mhz * 1e6).round()),
                                ..DeviceSettings::default()
                            });
                        },
                    ),
                ))
            },
        ));
    }
    items
}

#[must_use]
pub fn converter_said(offset_hz: f64) -> String {
    if offset_hz == 0.0 {
        "none".to_owned()
    } else {
        format!("{} MHz", offset_hz / 1e6)
    }
}

fn switch_chips(radio: Radio, caps: &Capabilities) -> Vec<AnyView> {
    let mut items = Vec::new();
    if has_dc_artifact(caps) {
        let dc = Signal::derive(move || {
            radio
                .read(|set| dc_block_on(&set.capabilities, &set.settings))
                .unwrap_or(false)
        });
        items.push(toggle_chip(
            "DC block",
            dc,
            "Remove the receiver's own DC spike by notching the centre bin",
            move |dc_block| {
                radio.patch(DeviceSettings {
                    dc_block: Some(dc_block),
                    ..DeviceSettings::default()
                });
            },
        ));
    }
    if caps.bias_tee {
        let on = radio.settings(|s| s.bias_tee.unwrap_or(false), false);
        items.push(toggle_chip(
            "Bias tee",
            on,
            "Powers an amplifier or active antenna over the coax",
            move |bias_tee| {
                radio.patch(DeviceSettings {
                    bias_tee: Some(bias_tee),
                    ..DeviceSettings::default()
                });
            },
        ));
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_converter_reads_its_offset_or_none() {
        assert_eq!(converter_said(0.0), "none");
        assert_eq!(converter_said(9.75e9), "9750 MHz");
        assert_eq!(converter_said(-125e6), "-125 MHz");
    }
}
