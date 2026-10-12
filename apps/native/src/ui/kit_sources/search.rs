use zgui::prelude::*;
use zgui_ui::prelude::*;

use super::popover_slot;

pub const SEARCHABLE_FROM: usize = 12;

#[must_use]
pub fn matching<'a>(options: &'a [(String, String)], query: &str) -> Vec<&'a (String, String)> {
    let wanted = query.trim().to_lowercase();
    options
        .iter()
        .filter(|(value, label)| {
            wanted.is_empty()
                || label.to_lowercase().contains(&wanted)
                || value.to_lowercase().contains(&wanted)
        })
        .collect()
}

fn shown_label(options: &[(String, String)], chosen: Option<&str>) -> String {
    options
        .iter()
        .find(|(value, _)| Some(value.as_str()) == chosen)
        .map_or_else(
            || chosen.unwrap_or_default().to_owned(),
            |(_, label)| label.clone(),
        )
}

pub fn searchable_pick(
    label: impl Into<String>,
    options: Vec<(String, String)>,
    chosen: Signal<Option<String>>,
    on_pick: impl Fn(String) + Clone + 'static,
) -> impl IntoView {
    let label: String = label.into();
    let (open, toggle, close) = popover_slot();
    let query = RwSignal::new_local(String::new());
    let menu = options.clone();
    let shown = move || chosen.with(|chosen| shown_label(&menu, chosen.as_deref()));
    let rows = move || {
        let query = query.get();
        matching(&options, &query)
            .into_iter()
            .map(|(value, text)| {
                let on_pick = on_pick.clone();
                let picked = value.clone();
                let marked = value.clone();
                let text = text.clone();
                AnyView::new(view! {
                    control(
                        class = "menu__row",
                        tabindex = Focus::Sequential,
                        a11y:role = Role::Button,
                        class:on = move || chosen.get().as_ref() == Some(&marked),
                        on:pointer_down:stop = move |_| {
                            close();
                            on_pick(picked.clone());
                        }
                    ) {
                        {text}
                    }
                })
            })
            .collect::<Vec<_>>()
    };
    let panel = move || {
        let label = label.clone();
        open.get().then(|| {
            AnyView::new(view! {
                column(class = "kit-pop kit-pop--chip kit-search", on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation()) {
                    row(class = "kit-field") {
                        Input(
                            value = query,
                            class = "native-input",
                            label = label,
                            placeholder = "Search",
                            on:key_down = move |ev: &mut EventCx<'_, events::KeyDown>| {
                                if matches!(ev.key, Key::Named(NamedKey::Escape)) {
                                    close();
                                    ev.prevent_default();
                                }
                            },
                        )
                    }
                    column(class = "kit-search__list") {{rows.clone()}}
                }
            })
        })
    };
    view! {
        box(class = "pick__wrap kit-pop-wrap") {
            control(
                class = "pick",
                a11y:role = Role::Button,
                class:on = open,
                tabindex = Focus::Sequential,
                on:pointer_down:stop = move |_| toggle()
            ) {
                text {{shown}}
                text(class = "pick__caret") {"v"}
            }
            {panel}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> Vec<(String, String)> {
        [
            ("us", "United States"),
            ("de", "Germany"),
            ("gb", "Great Britain"),
        ]
        .into_iter()
        .map(|(value, label)| (value.to_owned(), label.to_owned()))
        .collect()
    }

    #[test]
    fn a_search_matches_labels_and_values_without_case() {
        let options = options();
        let found = |query: &str| -> Vec<String> {
            matching(&options, query)
                .into_iter()
                .map(|(value, _)| value.clone())
                .collect()
        };
        assert_eq!(found(""), vec!["us", "de", "gb"]);
        assert_eq!(found("GER"), vec!["de"]);
        assert_eq!(found(" gb "), vec!["gb"]);
        assert_eq!(found("great"), vec!["gb"]);
        assert!(found("xx").is_empty());
    }

    #[test]
    fn the_picker_names_the_chosen_option_or_its_raw_value() {
        let options = options();
        assert_eq!(shown_label(&options, Some("de")), "Germany");
        assert_eq!(shown_label(&options, Some("fr")), "fr");
        assert_eq!(shown_label(&options, None), "");
    }
}
