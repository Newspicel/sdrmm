use std::time::Duration;

use sdrmm_wire::{
    DenoiseModel, DenoiseModelState, DenoiseModelStatus, DenoiseModelsResponse, DenoiseSettings,
};
use zgui::{prelude::*, reactive::RenderEffect, view::TimeoutHandle};

use crate::{
    store::Store,
    ui::kit_channel::{CLOSE_ICON, button, icon, tip, tip_text},
};

const POLL: Duration = Duration::from_millis(500);
const MODELS_PATH: &str = "/api/denoise-models";

#[must_use]
pub fn format_size(bytes: u64) -> String {
    let value = bytes as f64;
    let (scale, prefix) = [(1e9, "G"), (1e6, "M"), (1e3, "k")]
        .into_iter()
        .find(|(step, _)| value >= *step)
        .unwrap_or((1.0, ""));
    let text = format!("{:.1}", value / scale);
    format!("{} {prefix}B", text.trim_end_matches(".0"))
}

#[must_use]
pub fn download_percent(received: u64, bytes: u64) -> u64 {
    received.saturating_mul(100) / bytes.max(1)
}

#[must_use]
pub fn status_of(
    models: Option<&DenoiseModelsResponse>,
    model: DenoiseModel,
) -> Option<DenoiseModelStatus> {
    models?
        .models
        .iter()
        .find(|status| status.model == model)
        .cloned()
}

#[must_use]
pub fn model_missing(denoise: &DenoiseSettings, status: Option<&DenoiseModelStatus>) -> bool {
    denoise.enabled
        && denoise.mode == sdrmm_wire::DenoiseMode::Neural
        && status.is_some_and(|status| status.state != DenoiseModelState::Ready)
}

#[must_use]
pub fn downloading(models: Option<&DenoiseModelsResponse>) -> bool {
    models.is_some_and(|models| {
        models
            .models
            .iter()
            .any(|status| matches!(status.state, DenoiseModelState::Downloading { .. }))
    })
}

#[derive(Clone, Copy)]
pub struct Models {
    store: Store,
    pub held: RwSignal<Option<DenoiseModelsResponse>>,
}

impl Models {
    pub fn watch(store: Store, wanted: Signal<bool>) -> Self {
        let models = Self {
            store,
            held: RwSignal::new(None),
        };
        let fetch = RenderEffect::new(move |_| {
            if wanted.get() {
                models.refresh();
            }
        });
        let timer = StoredValue::new_local(None::<TimeoutHandle>);
        let poll = RenderEffect::new(move |_| {
            let busy = wanted.get() && models.held.with(|held| downloading(held.as_ref()));
            let next = busy
                .then(Timers::current)
                .flatten()
                .map(|clock| clock.set_timeout(POLL, move || models.refresh()));
            timer.set_value(next);
        });
        on_cleanup_local(move || {
            drop(fetch);
            drop(poll);
        });
        models
    }

    pub fn refresh(self) {
        let held = self.held;
        let store = self.store;
        zgui::task::spawn_local(async move {
            match store.api().get::<DenoiseModelsResponse>(MODELS_PATH).await {
                Ok(models) => {
                    let _unmounted = held.try_set(Some(models));
                }
                Err(error) => store.fail("Cannot list DPDFNet models", &error),
            }
        });
    }

    fn download(self, model: DenoiseModel) {
        let store = self.store;
        zgui::task::spawn_local(async move {
            let path = format!("{MODELS_PATH}/{}", model.name());
            if let Err(error) = store.api().post_empty::<()>(&path).await {
                store.fail("Cannot download the model", &error);
            }
            self.refresh();
        });
    }

    fn remove(self, model: DenoiseModel) {
        let store = self.store;
        zgui::task::spawn_local(async move {
            let path = format!("{MODELS_PATH}/{}", model.name());
            if let Err(error) = store.api().delete(&path).await {
                store.fail("Cannot remove the model", &error);
            }
            self.refresh();
        });
    }
}

pub fn model_state(models: Models, status: DenoiseModelStatus) -> AnyView {
    let model = status.model;
    let size = format_size(status.bytes);
    match status.state {
        DenoiseModelState::Ready => tip_text(
            move || format!("Remove model, frees {size}"),
            AnyView::new(view! {
                control(
                    class = "kc-icon kc-btn--danger",
                    tabindex = Focus::Sequential,
                    a11y:role = Role::Button,
                    a11y:label = "Remove model",
                    on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation(),
                    on:click:stop = move |_| models.remove(model)
                ) {
                    {icon(CLOSE_ICON, "kc-glyph kc-glyph--sm")}
                }
            }),
        ),
        DenoiseModelState::Downloading { received } => AnyView::new(view! {
            text(class = "kc-legend") {{format!("{}%", download_percent(received, status.bytes))}}
        }),
        DenoiseModelState::Failed { error } => tip_text(
            move || error.clone(),
            AnyView::new(button(
                "Retry",
                "kc-btn kc-btn--sm kc-danger",
                false,
                move || models.download(model),
            )),
        ),
        DenoiseModelState::Missing => tip(
            "Download model",
            AnyView::new(button(size, "kc-btn kc-btn--sm", false, move || {
                models.download(model)
            })),
        ),
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::DenoiseMode;

    use super::*;

    fn status(model: DenoiseModel, state: DenoiseModelState) -> DenoiseModelStatus {
        DenoiseModelStatus {
            model,
            bytes: 5_090_797,
            state,
        }
    }

    #[test]
    fn sizes_read_to_one_decimal() {
        assert_eq!(format_size(5_090_797), "5.1 MB");
        assert_eq!(format_size(7_282_645), "7.3 MB");
        assert_eq!(format_size(2_000), "2 kB");
        assert_eq!(format_size(12), "12 B");
    }

    #[test]
    fn progress_is_a_whole_percent_and_never_divides_by_zero() {
        assert_eq!(download_percent(2_545_398, 5_090_797), 49);
        assert_eq!(download_percent(5, 0), 500);
        assert_eq!(download_percent(0, 10), 0);
    }

    #[test]
    fn a_neural_denoiser_without_its_model_is_flagged() {
        let models = DenoiseModelsResponse {
            models: vec![
                status(DenoiseModel::Dpdfnet2, DenoiseModelState::Ready),
                status(
                    DenoiseModel::Dpdfnet8,
                    DenoiseModelState::Downloading { received: 10 },
                ),
            ],
        };
        let mut denoise = DenoiseSettings {
            enabled: true,
            mode: DenoiseMode::Neural,
            model: DenoiseModel::Dpdfnet8,
            ..DenoiseSettings::default()
        };
        let held = status_of(Some(&models), denoise.model);
        assert!(model_missing(&denoise, held.as_ref()));
        denoise.model = DenoiseModel::Dpdfnet2;
        let ready = status_of(Some(&models), denoise.model);
        assert!(!model_missing(&denoise, ready.as_ref()));
        denoise.mode = DenoiseMode::Spectral;
        assert!(!model_missing(&denoise, held.as_ref()));
        assert!(!model_missing(&denoise, None));
        assert!(downloading(Some(&models)));
        assert!(!downloading(None));
    }
}
