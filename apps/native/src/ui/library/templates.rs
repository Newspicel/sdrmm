use sdrmm_wire::{
    rest::{ApplyTemplateRequest, TemplateInfo, TemplatesResponse},
    units,
};
use zgui::prelude::*;

use crate::{
    shell::templates::{apply_title, supports, template_size, templates_hint},
    store::Store,
    ui::{
        kit_shell::{glyph, hint, list, row_action},
        library::{
            active_set, load,
            rows::{Badge, LibRow, Pending, act, lib_row},
        },
    },
};

pub fn panel(store: Store) -> impl IntoView {
    let templates = load::<TemplatesResponse>(store, String::from("/api/templates"));
    let active = active_set(store);
    let applied = RwSignal::new(None::<TemplateInfo>);
    let explained = RwSignal::new(None::<String>);
    let applying = Pending::new();
    let listed = move || {
        templates
            .get()
            .and_then(Result::ok)
            .map(|found| found.templates.clone())
            .unwrap_or_default()
    };
    let top_hint = move || templates_hint(&listed(), active.get().as_ref()).map(hint);
    let banner = move || {
        applied.get().map(|template| {
            AnyView::new(view! {
                column(class = "sk-list") {
                    column(class = "sk-row") {
                        text(class = "sk-row__primary") {{template.name.clone()}}
                        text(class = "sk-text") {{template.explainer.clone()}}
                    }
                }
            })
        })
    };
    let rows = move || {
        listed()
            .into_iter()
            .map(|template| {
                AnyView::new(template_row(store, template, applied, explained, applying))
            })
            .collect::<Vec<_>>()
    };
    let failed = move || {
        matches!(templates.get(), Some(Err(_))).then(|| hint("Could not load the templates."))
    };
    view! {
        column(class = "sk-panel") {
            {top_hint}
            {failed}
            {banner}
            {list(None, rows)}
        }
    }
}

async fn apply(
    store: Store,
    device_set: u32,
    template: TemplateInfo,
    applied: RwSignal<Option<TemplateInfo>>,
) {
    let body = ApplyTemplateRequest { device_set };
    let path = format!("/api/templates/{}/apply", template.id);
    match store
        .api()
        .post::<_, serde::de::IgnoredAny>(&path, &body)
        .await
    {
        Ok(_) => {
            applied.set(Some(template));
            store.apply().await;
        }
        Err(error) => store.fail("Cannot apply the template", &error),
    }
    store.refresh_state();
}

fn apply_button(
    store: Store,
    template: TemplateInfo,
    applied: RwSignal<Option<TemplateInfo>>,
    applying: Pending,
) -> impl IntoView {
    let active = active_set(store);
    move || {
        let set = active.get();
        let fits = supports(&template, set.as_ref());
        let enabled = Signal::derive_local(move || fits && !applying.busy());
        let chosen = template.clone();
        let device_set = set.as_ref().map(|set| set.id);
        act(
            "Apply",
            "sm",
            enabled,
            apply_title(&template, set.as_ref()),
            move || {
                if let Some(device_set) = device_set {
                    applying.run(apply(store, device_set, chosen.clone(), applied));
                }
            },
        )
    }
}

fn template_row(
    store: Store,
    template: TemplateInfo,
    applied: RwSignal<Option<TemplateInfo>>,
    explained: RwSignal<Option<String>>,
    applying: Pending,
) -> impl IntoView {
    let badge = format!(
        "{} \u{b7} {} \u{b7} {}",
        units::hertz(template.center_hz),
        units::sample_rate(template.sample_rate),
        template_size(&template)
    );
    let id = template.id.clone();
    let open_id = template.id.clone();
    let explainer = template.explainer.clone();
    let below = move || {
        (explained.get().as_deref() == Some(open_id.as_str()))
            .then(|| AnyView::new(view! { text(class = "sk-text") {{explainer.clone()}} }))
    };
    let actions = view! {
        {row_action(glyph::INFO, format!("About {}", template.name), false, move || {
            explained.update(|open| {
                *open = if open.as_deref() == Some(id.as_str()) { None } else { Some(id.clone()) };
            });
        })}
        {apply_button(store, template.clone(), applied, applying)}
    };
    lib_row(
        LibRow {
            primary: template.name.clone(),
            badge: Badge::Legend(badge),
            secondary: Some(template.description.clone()),
            ..LibRow::default()
        },
        None,
        AnyView::new(actions),
        AnyView::new(below),
    )
}
