use std::time::Duration;

use zgui::prelude::*;

use crate::{
    decoded::now_ms,
    decoders::views::{Age, age_class, format_age},
    ui::kit_decoders::titled_text,
};

pub type Column = (&'static str, Option<&'static str>);

pub fn head(columns: &[Column]) -> impl IntoView + use<> {
    let cells: Vec<AnyView> = columns
        .iter()
        .map(|(name, title)| match title {
            Some(title) => titled_text((*name).to_owned(), (*title).to_owned(), "dk-td dk-th"),
            None => AnyView::new(view! { text(class = "dk-td dk-th") {{*name}} }),
        })
        .collect();
    view! { row(class = "dk-tr") {{cells}} }
}

pub fn plain(columns: &[&'static str]) -> impl IntoView + use<> {
    let named: Vec<Column> = columns.iter().map(|name| (*name, None)).collect();
    head(&named)
}

pub fn cells(cells: Vec<String>) -> impl IntoView {
    let cells: Vec<AnyView> = cells.into_iter().map(|text| cell(text, "")).collect();
    view! { row(class = "dk-tr") {{cells}} }
}

pub fn row(cells: Vec<AnyView>) -> impl IntoView {
    view! { row(class = "dk-tr") {{cells}} }
}

pub fn cell(text: String, tone: &'static str) -> AnyView {
    AnyView::new(view! { text(class = format!("dk-td {tone}")) {{text}} })
}

pub fn long_cell(text: String, tone: &'static str) -> AnyView {
    let class = match tone {
        "dim" => "dk-td dim",
        "fade" => "dk-td fade",
        _ => "dk-td",
    };
    titled_text(text.clone(), text, class)
}

#[must_use]
pub fn tone(age_ms: i64) -> &'static str {
    match age_class(age_ms) {
        Age::Fresh => "",
        Age::Stale => "dim",
        Age::Fading => "fade",
    }
}

#[must_use]
pub fn age(now: i64, at_ms: i64) -> (String, &'static str) {
    let age_ms = (now - at_ms).max(0);
    (format_age(age_ms), tone(age_ms))
}

pub fn ticking() -> RwSignal<i64> {
    let now = RwSignal::new(now_ms());
    let tick = set_interval(Duration::from_secs(1), move || now.set(now_ms()));
    on_cleanup_local(move || drop(tick));
    now
}

pub fn dash<T: ToString>(value: Option<T>) -> String {
    value.map_or_else(|| "-".to_owned(), |value| value.to_string())
}

pub fn empty(said: &'static str) -> AnyView {
    AnyView::new(view! { text(class = "hint") {{said}} })
}
