use sdrmm_wire::device::{DeviceSettings, ExtraSetting, ExtraValue, Range};
use zgui::prelude::*;

use super::{caps::fits_slider, radio::Radio};
use crate::ui::{
    kit_sources::{
        NumberSpec,
        chips::{ChipFace, chip_field, setting_chip, toggle_chip},
        debounced, number_field_in,
        search::{SEARCHABLE_FROM, searchable_pick},
        text_field, units,
    },
    widgets::{pick, slide},
};

fn extra_raw(radio: Radio, name: String) -> Signal<Option<serde_json::Value>> {
    Signal::derive(move || {
        radio
            .read(|set| {
                set.settings
                    .extra
                    .iter()
                    .find(|extra| extra.name == name)
                    .map(|extra| extra.value.clone())
            })
            .flatten()
    })
}

fn extra_patch(radio: Radio, name: String) -> impl Fn(serde_json::Value) + Clone + 'static {
    move |value| {
        radio.patch(DeviceSettings {
            extra: vec![ExtraValue {
                name: name.clone(),
                value,
            }],
            ..DeviceSettings::default()
        });
    }
}

#[must_use]
pub fn number_said(value: f64) -> String {
    format!("{value}")
}

#[must_use]
pub fn range_title(name: &str, range: &Range) -> String {
    format!(
        "{name}: {} to {}",
        number_said(range.min),
        number_said(range.max)
    )
}

#[must_use]
pub fn with_unit(value: &str, unit: &str) -> String {
    if unit.is_empty() {
        value.to_owned()
    } else {
        format!("{value} {unit}")
    }
}

pub fn extra_chip(radio: Radio, setting: ExtraSetting) -> AnyView {
    let name = setting
        .label()
        .map_or_else(|| units::setting_label(setting.name()), str::to_owned);
    let raw = extra_raw(radio, setting.name().to_owned());
    let commit = extra_patch(radio, setting.name().to_owned());
    match setting {
        ExtraSetting::Bool { default, .. } => {
            let on = Signal::derive(move || raw.get().and_then(|v| v.as_bool()).unwrap_or(default));
            toggle_chip(name.clone(), on, name, move |next| {
                commit(serde_json::Value::Bool(next));
            })
        }
        ExtraSetting::Enum {
            options, default, ..
        } => {
            let options: Vec<(String, String)> = options
                .into_iter()
                .map(|option| (option.value.clone(), option.label.unwrap_or(option.value)))
                .collect();
            let current = Signal::derive(move || {
                raw.get()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| default.clone())
            });
            enum_chip(name, options, current, commit)
        }
        ExtraSetting::Range { range, unit, .. } => {
            let value =
                Signal::derive(move || raw.get().and_then(|v| v.as_f64()).unwrap_or(range.min));
            let send = move |number: f64| {
                if let Some(number) = serde_json::Number::from_f64(number) {
                    commit(serde_json::Value::Number(number));
                }
            };
            range_chip(name, unit, range, value, send)
        }
        ExtraSetting::String { default, .. } => {
            let text = Signal::derive(move || {
                raw.get()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| default.clone())
            });
            let shown = Signal::derive(move || {
                let text = text.get();
                if text.is_empty() {
                    "none".to_owned()
                } else {
                    text
                }
            });
            let quiet = Signal::derive(move || text.with(String::is_empty));
            let field = name.clone();
            setting_chip(
                ChipFace::new(name.clone(), shown, name).quiet(quiet),
                move || {
                    let commit = commit.clone();
                    AnyView::new(chip_field(
                        field.clone(),
                        text_field(
                            field.clone(),
                            text,
                            "",
                            Signal::stored(false),
                            |_| true,
                            move |value| commit(serde_json::Value::String(value)),
                        ),
                    ))
                },
            )
        }
    }
}

fn enum_chip(
    name: String,
    options: Vec<(String, String)>,
    current: Signal<String>,
    commit: impl Fn(serde_json::Value) + Clone + 'static,
) -> AnyView {
    let named = options.clone();
    let shown = Signal::derive(move || {
        let value = current.get();
        named
            .iter()
            .find(|(option, _)| *option == value)
            .map_or(value, |(_, label)| label.clone())
    });
    let field = name.clone();
    setting_chip(ChipFace::new(name.clone(), shown, name), move || {
        let commit = commit.clone();
        let chosen = Signal::derive(move || Some(current.get()));
        let send = move |value: String| commit(serde_json::Value::String(value));
        let picker = if options.len() > SEARCHABLE_FROM {
            AnyView::new(searchable_pick(
                field.clone(),
                options.clone(),
                chosen,
                send,
            ))
        } else {
            AnyView::new(pick(options.clone(), chosen, send))
        };
        AnyView::new(chip_field(field.clone(), picker))
    })
}

fn range_chip(
    name: String,
    unit: String,
    range: Range,
    value: Signal<f64>,
    send: impl Fn(f64) + Clone + 'static,
) -> AnyView {
    let suffix = unit.clone();
    let shown = Signal::derive(move || with_unit(&number_said(value.get()), &suffix));
    let title = range_title(&name, &range);
    let field = name.clone();
    setting_chip(ChipFace::new(name, shown, title), move || {
        let send = send.clone();
        if fits_slider(&range) {
            return AnyView::new(chip_field(
                field.clone(),
                range_slider(unit.clone(), range, value, send),
            ));
        }
        let mut spec = NumberSpec::unit("").within(range.min, range.max);
        spec.step = range.step;
        AnyView::new(chip_field(
            field.clone(),
            number_field_in(
                field.clone(),
                unit.clone(),
                value,
                spec,
                Signal::stored(false),
                send,
            ),
        ))
    })
}

fn range_slider(
    unit: String,
    range: Range,
    value: Signal<f64>,
    commit: impl Fn(f64) + Clone + 'static,
) -> impl IntoView {
    let (pending, change) = debounced(commit);
    let shown = Signal::derive(move || pending.get().unwrap_or_else(|| value.get()));
    let digits = usize::from(range.step.is_some_and(|step| step < 1.0));
    let step = range.step.filter(|step| *step > 0.0).unwrap_or(1.0);
    let min = range.min;
    let read = move |v: f64| with_unit(&format!("{v:.digits$}"), &unit);
    slide(shown, range.min, range.max, read, move |v| {
        change(min + ((v - min) / step).round() * step);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_chip_names_its_span_and_unit() {
        let range = Range {
            min: 0.0,
            max: 49.5,
            step: Some(0.5),
        };
        assert_eq!(range_title("IF gain", &range), "IF gain: 0 to 49.5");
        assert_eq!(with_unit("3", "dB"), "3 dB");
        assert_eq!(with_unit("3", ""), "3");
        assert_eq!(number_said(2.0), "2");
    }
}
