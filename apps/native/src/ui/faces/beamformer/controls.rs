use sdrmm_wire::{
    limits::{BEAMFORMER_LIMITS as LIMITS, Bounds as WireBounds},
    patch::NodeBody,
    processor::{
        DEFAULT_BAND_HZ,
        beamformer::{BeamMode, BeamformerParams, SteerSource},
    },
};
use zgui::{prelude::*, vocab::SharedString};

use super::logic::{
    ADAPTATIONS, BeamSetting, MODES, NOISE, STEER_UNWIRED, SteerKind, lane_options, null_label,
    options, reference_lanes, set_main, steer_kind, steer_pick, steer_title, title_of,
    toggled_reference, visible_settings, with_azimuth, with_null, without_null,
};
use crate::{
    store::Store,
    ui::{
        faces::processor::edit,
        kit_raster::{Bounds, button, number},
        kit_shell::{glyph, row_action},
        widgets::{check, pick, row_field, segments},
    },
};

const KHZ: f64 = 1_000.0;

pub fn body(body: &NodeBody) -> Option<&BeamformerParams> {
    match body {
        NodeBody::Beamformer(found) => Some(&found.settings),
        _ => None,
    }
}

fn body_mut(body: &mut NodeBody) -> Option<&mut BeamformerParams> {
    match body {
        NodeBody::Beamformer(found) => Some(&mut found.settings),
        _ => None,
    }
}

fn whole(bounds: WireBounds<u32>) -> Bounds {
    Bounds::whole(f64::from(bounds.min), f64::from(bounds.max))
}

fn real(bounds: WireBounds<f32>) -> Bounds {
    Bounds::new(f64::from(bounds.min), f64::from(bounds.max))
}

fn khz(bounds: WireBounds<f64>) -> Bounds {
    Bounds::new(bounds.min / KHZ, bounds.max / KHZ)
}

fn titled(title: impl Into<String>, body: impl IntoView + 'static) -> AnyView {
    let title = SharedString::from(title.into());
    AnyView::new(view! { box(a11y:description = title) {{body}} })
}

#[derive(Clone)]
pub struct Beam {
    pub store: Store,
    pub node: String,
    pub params: Memo<Option<BeamformerParams>>,
    pub lanes: Signal<usize>,
    pub wired: Memo<bool>,
}

impl Beam {
    fn set(&self, change: impl FnOnce(&mut BeamformerParams)) {
        edit(self.store, &self.node, body_mut, change);
    }

    fn read(&self, pick: fn(&BeamformerParams) -> f64) -> Signal<f64> {
        let params = self.params;
        Signal::derive(move || params.with(|params| params.as_ref().map(pick).unwrap_or_default()))
    }

    fn shown(&self) -> Memo<Vec<BeamSetting>> {
        let params = self.params;
        Memo::new(move |_| {
            params.with(|params| params.as_ref().map(visible_settings).unwrap_or_default())
        })
    }

    fn number(
        &self,
        name: &'static str,
        title: &'static str,
        pick: fn(&BeamformerParams) -> f64,
        bounds: Bounds,
        write: fn(&mut BeamformerParams, f64),
    ) -> AnyView {
        let beam = self.clone();
        titled(
            title,
            row_field(
                name,
                number(name, self.read(pick), bounds, move |value| {
                    beam.set(|params| write(params, value));
                }),
            ),
        )
    }
}

fn when(
    shown: Memo<Vec<BeamSetting>>,
    setting: BeamSetting,
    view: impl Fn() -> AnyView + 'static,
) -> impl IntoView {
    let on = Memo::new(move |_| shown.with(|shown| shown.contains(&setting)));
    move || on.get().then(&view)
}

pub fn view(beam: Beam) -> impl IntoView {
    let shown = beam.shown();
    view! {
        column(class = "params") {
            {mode(beam.clone())}
            {when(shown, BeamSetting::Steer, { let beam = beam.clone(); move || steer(beam.clone(), shown) })}
            {when(shown, BeamSetting::Nulls, { let beam = beam.clone(); move || nulls(beam.clone()) })}
            {when(shown, BeamSetting::Main, { let beam = beam.clone(); move || canceller(beam.clone()) })}
            {solver(beam.clone(), shown)}
            {when(shown, BeamSetting::Update, { let beam = beam.clone(); move || block(beam.clone(), shown) })}
            {band(beam)}
        }
    }
}

fn mode(beam: Beam) -> AnyView {
    let params = beam.params;
    let chosen =
        Signal::derive(move || params.with(|params| params.as_ref().map(|found| found.mode)));
    let title = move || {
        let current = chosen.get().map_or("", |mode| title_of(&MODES, mode));
        SharedString::from(format!("How lanes combine. {current}"))
    };
    AnyView::new(view! {
        box(a11y:description = title) {
            {row_field("Mode", pick(options(&MODES), chosen, move |mode: BeamMode| {
                beam.set(|params| params.mode = mode);
            }))}
        }
    })
}

fn steer(beam: Beam, shown: Memo<Vec<BeamSetting>>) -> AnyView {
    let params = beam.params;
    let wired = beam.wired;
    let steer = Memo::new(move |_| {
        params.with(|params| {
            params
                .as_ref()
                .map_or(SteerSource::Wired, |found| found.steer)
        })
    });
    let kind = Signal::derive(move || steer_kind(steer.get()));
    let picked = {
        let beam = beam.clone();
        move |chosen: SteerKind| {
            if chosen == SteerKind::Wired && !wired.get_untracked() {
                beam.store.say(STEER_UNWIRED.to_owned());
                return;
            }
            if let Some(next) = steer_pick(steer.get_untracked(), chosen) {
                beam.set(|params| params.steer = next);
            }
        }
    };
    let azimuth = {
        let beam = beam.clone();
        move || {
            let beam = beam.clone();
            (kind.get() == SteerKind::Fixed).then(|| {
                beam.number(
                    "Azimuth °",
                    "From array forward",
                    |params| match params.steer {
                        SteerSource::Fixed { azimuth_deg, .. } => azimuth_deg,
                        SteerSource::Wired => 0.0,
                    },
                    Bounds::new(0.0, 359.9),
                    |params, deg| params.steer = with_azimuth(params.steer, deg),
                )
            })
        }
    };
    let timeout = when(shown, BeamSetting::Timeout, move || {
        beam.number(
            "Timeout ms",
            "Hold the last bearing this long",
            |params| f64::from(params.steer_timeout_ms),
            whole(LIMITS.steer_timeout_ms),
            |params, ms| params.steer_timeout_ms = ms as u32,
        )
    });
    let title = move || SharedString::from(steer_title(wired.get()));
    AnyView::new(view! {
        column(class = "bf__group") {
            box(a11y:description = title) {
                {row_field("Steer", segments(vec![(SteerKind::Wired, "DF"), (SteerKind::Fixed, "Fixed")], kind, picked))}
            }
            {azimuth}
            {timeout}
        }
    })
}

fn nulls(beam: Beam) -> AnyView {
    let params = beam.params;
    let list = Memo::new(move |_| {
        params.with(|params| {
            params
                .as_ref()
                .map(|found| found.nulls_deg.clone())
                .unwrap_or_default()
        })
    });
    let auto = Signal::derive(move || {
        params.with(|params| params.as_ref().is_some_and(|found| found.auto_nulls))
    });
    let full = Signal::derive(move || list.with(|list| list.len() >= LIMITS.nulls));
    let draft = RwSignal::new(0.0);
    let chips = {
        let beam = beam.clone();
        move || {
            list.get()
                .into_iter()
                .map(|deg| {
                    let beam = beam.clone();
                    AnyView::new(view! {
                        row(class = "bf__null") {
                            text(class = "kit-mono") {{null_label(deg)}}
                            {row_action(glyph::CROSS, format!("Remove null {}", deg.round()), true, move || {
                                beam.set(|params| params.nulls_deg = without_null(&params.nulls_deg, deg));
                            })}
                        }
                    })
                })
                .collect::<Vec<_>>()
        }
    };
    let listed = move || {
        (!list.with(Vec::is_empty)).then(|| AnyView::new(row_field("Nulls", chips.clone())))
    };
    let add = {
        let beam = beam.clone();
        move || {
            beam.set(|params| {
                params.nulls_deg = with_null(&params.nulls_deg, draft.get_untracked())
            })
        }
    };
    let add_title = Signal::derive(move || {
        SharedString::from(if full.get() {
            format!("At most {}", LIMITS.nulls)
        } else {
            "Azimuths to silence".to_owned()
        })
    });
    AnyView::new(view! {
        column(class = "bf__group") {
            {listed}
            box(a11y:description = add_title) {
                {row_field("Add °", view! {
                    {number("New null", draft.into(), Bounds::new(0.0, 359.9), move |deg| draft.set(deg))}
                    {button("Add", full, add)}
                })}
            }
            {titled("Find and silence strong interferers", row_field("Auto", check(auto, move |on| {
                beam.set(|params| params.auto_nulls = on);
            })))}
        }
    })
}

fn canceller(beam: Beam) -> AnyView {
    let (params, lanes) = (beam.params, beam.lanes);
    let main =
        Signal::derive(move || params.with(|params| params.as_ref().map(|found| found.main_lane)));
    let pick_main = {
        let beam = beam.clone();
        move |lane: u32| beam.set(|params| set_main(params, lane))
    };
    let main_options = move || {
        let pick_main = pick_main.clone();
        titled(
            "Lane the others are cancelled from",
            row_field("Main", pick(lane_options(lanes.get()), main, pick_main)),
        )
    };
    let main_lane = Memo::new(move |_| {
        params.with(|params| params.as_ref().map_or(0, |found| found.main_lane))
    });
    let refs = {
        let beam = beam.clone();
        move || refs(beam.clone(), main_lane.get(), lanes.get())
    };
    AnyView::new(view! {
        column(class = "bf__group") {
            {main_options}
            {refs}
            {beam.number("Taps", "Delays per reference lane", |params| f64::from(params.taps), whole(LIMITS.taps), |params, taps| params.taps = taps as u32)}
        }
    })
}

fn refs(beam: Beam, main: u32, count: usize) -> AnyView {
    let (params, lanes) = (beam.params, beam.lanes);
    let boxes = lane_options(count)
        .into_iter()
        .filter(|(lane, _)| *lane != main)
        .map(|(lane, label)| {
            let beam = beam.clone();
            let on = Signal::derive(move || {
                params.with(|params| params.as_ref().is_some_and(|found| reference_lanes(found, lanes.get()).contains(&lane)))
            });
            AnyView::new(view! {
                row(class = "bf__ref") {
                    {check(on, move |_| beam.set(|params| params.reference_lanes = toggled_reference(params, lane, lanes.get_untracked())))}
                    text(class = "kit-mono") {{label}}
                }
            })
        })
        .collect::<Vec<_>>();
    titled(
        "Lanes that hear only the interference",
        row_field("Refs", boxes),
    )
}

fn solver(beam: Beam, shown: Memo<Vec<BeamSetting>>) -> impl IntoView {
    let params = beam.params;
    let adapt = {
        let beam = beam.clone();
        when(shown, BeamSetting::Adaptation, move || {
            let beam = beam.clone();
            let chosen = Signal::derive(move || {
                params.with(|params| params.as_ref().map(|found| found.adaptation))
            });
            let title = move || {
                SharedString::from(format!(
                    "Weight solver. {}",
                    chosen
                        .get()
                        .map_or("", |found| title_of(&ADAPTATIONS, found))
                ))
            };
            AnyView::new(view! {
                box(a11y:description = title) {
                    {row_field("Adapt", pick(options(&ADAPTATIONS), chosen, move |found| beam.set(|params| params.adaptation = found)))}
                }
            })
        })
    };
    let step = {
        let beam = beam.clone();
        when(shown, BeamSetting::Step, move || {
            beam.number(
                "Step",
                "How fast weights follow",
                |params| f64::from(params.step),
                real(LIMITS.step),
                |params, step| params.step = step as f32,
            )
        })
    };
    let forget = {
        let beam = beam.clone();
        when(shown, BeamSetting::Forget, move || {
            beam.number(
                "Forget",
                "Memory of the RLS solver",
                |params| f64::from(params.forget),
                real(LIMITS.forget),
                |params, forget| params.forget = forget as f32,
            )
        })
    };
    let loading = {
        let beam = beam.clone();
        when(shown, BeamSetting::Loading, move || {
            beam.number(
                "Loading",
                "Diagonal loading, steadies the solve",
                |params| f64::from(params.loading),
                real(LIMITS.loading),
                |params, loading| params.loading = loading as f32,
            )
        })
    };
    let noise = when(shown, BeamSetting::Noise, move || {
        let beam = beam.clone();
        let chosen =
            Signal::derive(move || params.with(|params| params.as_ref().map(|found| found.noise)));
        let title = move || {
            SharedString::from(format!(
                "Noise model. {}",
                chosen.get().map_or("", |found| title_of(&NOISE, found))
            ))
        };
        AnyView::new(view! {
            box(a11y:description = title) {
                {row_field("Noise", pick(options(&NOISE), chosen, move |found| beam.set(|params| params.noise = found)))}
            }
        })
    });
    view! {
        column(class = "bf__group") {
            {adapt}
            {step}
            {forget}
            {loading}
            {noise}
        }
    }
}

fn block(beam: Beam, shown: Memo<Vec<BeamSetting>>) -> AnyView {
    let fade = {
        let beam = beam.clone();
        when(shown, BeamSetting::Crossfade, move || {
            beam.number(
                "Fade ms",
                "Blend old and new weights",
                |params| f64::from(params.crossfade_ms),
                whole(LIMITS.crossfade_ms),
                |params, ms| params.crossfade_ms = ms as u32,
            )
        })
    };
    let carry = {
        let beam = beam.clone();
        when(shown, BeamSetting::CarryOver, move || {
            beam.number(
                "Carry over",
                "Share of the last estimate kept",
                |params| f64::from(params.carry_over),
                real(LIMITS.carry_over),
                |params, kept| params.carry_over = kept as f32,
            )
        })
    };
    AnyView::new(view! {
        column(class = "bf__group") {
            {beam.number("Update ms", "New weights this often", |params| f64::from(params.update_ms), whole(LIMITS.update_ms), |params, ms| params.update_ms = ms as u32)}
            {fade}
            {carry}
        }
    })
}

fn band(beam: Beam) -> impl IntoView {
    let params = beam.params;
    let full = Signal::derive(move || {
        params.with(|params| {
            params
                .as_ref()
                .is_some_and(|found| found.bandwidth_hz.is_none())
        })
    });
    let width = {
        let beam = beam.clone();
        move || {
            (!full.get()).then(|| {
                beam.number(
                    "Width kHz",
                    "Band around the offset",
                    |params| params.bandwidth_hz.unwrap_or(DEFAULT_BAND_HZ) / KHZ,
                    khz(LIMITS.band.bandwidth_hz),
                    |params, value| params.bandwidth_hz = Some(value * KHZ),
                )
            })
        }
    };
    let offset = beam.number(
        "Offset kHz",
        "From the array centre",
        |params| params.offset_hz / KHZ,
        khz(LIMITS.band.offset_hz),
        |params, value| params.offset_hz = value * KHZ,
    );
    view! {
        column(class = "bf__group") {
            {offset}
            {titled("Band around the offset, or every lane sample", row_field("Full band", check(full, move |on| {
                beam.set(|params| params.bandwidth_hz = (!on).then_some(DEFAULT_BAND_HZ));
            })))}
            {width}
        }
    }
}
