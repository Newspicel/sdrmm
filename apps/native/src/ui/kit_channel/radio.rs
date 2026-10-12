use zgui::prelude::*;

use super::{NumberSpec, button, format_mhz, number_field, tune_to};
use crate::ui::faces::channel::settings::{NumberLimit, clamp_offset_hz, offset_for_frequency_hz};

const DOWN_HZ: [f64; 2] = [-25_000.0, -5_000.0];
const UP_HZ: [f64; 2] = [5_000.0, 25_000.0];

#[must_use]
pub fn step_label(hz: f64) -> String {
    let sign = if hz > 0.0 { "+" } else { "−" };
    format!("{sign}{}k", super::format_number(hz.abs() / 1000.0, None))
}

pub fn offset_stepper(
    offset_hz: Signal<f64>,
    limit_hz: Signal<Option<f64>>,
    center_hz: Signal<Option<f64>>,
    on_offset: impl Fn(f64) + Clone + 'static,
) -> impl IntoView {
    let step_button = {
        let on_offset = on_offset.clone();
        move |hz: f64| {
            let on_offset = on_offset.clone();
            button(step_label(hz), "kc-btn kc-btn--mono", false, move || {
                on_offset(clamp_offset_hz(
                    offset_hz.get_untracked() + hz,
                    limit_hz.get_untracked(),
                ));
            })
        }
    };
    let field = {
        let on_offset = on_offset.clone();
        move || {
            let limit_khz = limit_hz.get().map(|limit| limit / 1000.0);
            let spec = NumberSpec::new("Offset")
                .limit(NumberLimit {
                    min: limit_khz.map(|limit| -limit),
                    max: limit_khz,
                    step: Some(0.5),
                })
                .unit("kHz")
                .size("kc-num kc-num--narrow");
            let on_offset = on_offset.clone();
            number_field(
                Signal::derive(move || Some(offset_hz.get() / 1000.0)),
                spec,
                move |khz| {
                    if let Some(khz) = khz {
                        on_offset(clamp_offset_hz(
                            (khz * 1000.0).round(),
                            limit_hz.get_untracked(),
                        ));
                    }
                },
            )
        }
    };
    let tune = move || {
        let center = center_hz.get().filter(|center| center.is_finite())?;
        let hz = Signal::derive(move || center_hz.get().unwrap_or(center) + offset_hz.get());
        let hint = Signal::derive(move || match limit_hz.get() {
            Some(limit) => format!(
                "Reaches {} to {}",
                format_mhz(center - limit),
                format_mhz(center + limit)
            ),
            None => format!("Center {}", format_mhz(center)),
        });
        Some(AnyView::new(tune_to(
            "Type a frequency to sit on",
            hz,
            hint,
            move |entered| offset_for_frequency_hz(entered, center, limit_hz.get_untracked()),
            Signal::stored(false),
            on_offset.clone(),
        )))
    };
    view! {
        row(class = "kc-cell") {
            {DOWN_HZ.into_iter().map(|hz| AnyView::new(step_button.clone()(hz))).collect::<Vec<_>>()}
            {field}
            {UP_HZ.into_iter().map(|hz| AnyView::new(step_button.clone()(hz))).collect::<Vec<_>>()}
            {tune}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_button_reads_in_kilohertz_with_its_sign() {
        assert_eq!(step_label(-25_000.0), "−25k");
        assert_eq!(step_label(5_000.0), "+5k");
    }
}
