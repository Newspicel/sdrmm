use sdrmm_wire::patch::{PortDirection, PortType};
use zgui::prelude::*;
use zgui_flow::prelude::{HandleKind, HandleProps, Side};

use crate::ui::{kit_shell::hover, node::Place};

const SHEET: &str = css!(
    r#"
.port { position: absolute; width: 0; height: 0; }
.port--in { left: 0; }
.port--out { right: 0; }
.port__label {
    position: absolute;
    top: -7px;
    padding: 0 4px;
    border-radius: 3px;
    background-color: color-mix(in oklab, var(--bg) 85%, transparent);
    font-family: var(--mono);
    font-size: 10px;
    color: var(--ink-faint);
    white-space: nowrap;
}
.port--in .port__label { right: 9px; }
.port--out .port__label { left: 9px; }
.port .zf-handle { width: 12px; height: 12px; border: none; background-color: transparent; color: var(--wire); }
.port__glyph { width: 12px; height: 12px; pointer-events: none; }
"#
);

#[must_use]
pub const fn handle_of(direction: PortDirection) -> (HandleKind, Side) {
    match direction {
        PortDirection::In => (HandleKind::Target, Side::Left),
        PortDirection::Out => (HandleKind::Source, Side::Right),
    }
}

#[must_use]
pub const fn glyph_of(port_type: PortType) -> &'static str {
    match port_type {
        PortType::Array => {
            r#"<circle cx="6" cy="6" r="4.5" fill="currentColor"/><circle cx="6" cy="6" r="2.2" fill="none" stroke="var(--panel)"/>"#
        }
        PortType::Iq | PortType::Position => {
            r#"<circle cx="6" cy="6" r="4.5" fill="currentColor"/>"#
        }
        PortType::Tx => r#"<circle cx="6" cy="6" r="4.5" fill="none" stroke="currentColor"/>"#,
        PortType::Baseband => r#"<path d="M10.5 6 A4.5 4.5 0 0 1 1.5 6 Z" fill="currentColor"/>"#,
        PortType::Audio => r#"<path d="M6 1 11 6 6 11 1 6Z" fill="currentColor"/>"#,
        PortType::Events => r#"<path d="M3 1h6l2 5-2 5H3L1 6Z" fill="currentColor"/>"#,
        PortType::Video => r#"<path d="M6 1 11 11H1Z" fill="currentColor"/>"#,
        PortType::Control => r#"<path d="M1 1 11 6 1 11Z" fill="currentColor"/>"#,
    }
}

#[must_use]
pub fn port_title(place: &Place) -> String {
    match &place.note {
        Some(note) => format!("{}: {note}", place.label),
        None => format!("{} ({})", place.label, place.port_type.as_str()),
    }
}

fn glyph(port_type: PortType) -> impl IntoView {
    let document = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" overflow="visible">{}</svg>"#,
        glyph_of(port_type)
    );
    zgui::elements::vector()
        .class("port__glyph")
        .document(&document)
}

fn port(place: Place) -> impl IntoView {
    let (kind, side) = handle_of(place.direction);
    let class = match place.direction {
        PortDirection::In => "port port--in",
        PortDirection::Out => "port port--out",
    };
    let id = place.name.clone();
    let title = port_title(&place);
    let label = AnyView::new(view! { text(class = "port__name") {{place.label.clone()}} });
    view! {
        box(class = class, style:top = Some(format!("{:.0}px", place.y))) {
            Handle(kind = kind, side = side, id = id, class = place.port_type.as_str()) {
                {glyph(place.port_type)}
            }
            box(class = "port__label") {
                {hover(title, label)}
            }
        }
    }
}

pub fn ports(places: Signal<Vec<Place>>) -> impl IntoView {
    install_stylesheet("patch-ports", SHEET);
    view! {
        for place in move || places.get(), key = |place: &Place| (place.name.clone(), place.direction == PortDirection::In, place.port_type.as_str(), place.y.to_bits()) {
            {port(place)}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(note: Option<&str>) -> Place {
        Place {
            name: "iq".to_owned(),
            label: "iq1".to_owned(),
            note: note.map(str::to_owned),
            port_type: PortType::Iq,
            direction: PortDirection::Out,
            x: 0.0,
            y: 0.0,
        }
    }

    #[test]
    fn a_port_is_titled_by_its_note_or_its_type() {
        assert_eq!(port_title(&place(None)), "iq1 (iq)");
        assert_eq!(port_title(&place(Some("The beam"))), "iq1: The beam");
    }

    #[test]
    fn each_port_type_draws_a_shape_of_its_own() {
        let all = [
            PortType::Baseband,
            PortType::Audio,
            PortType::Events,
            PortType::Video,
            PortType::Control,
            PortType::Tx,
            PortType::Array,
        ];
        for (at, port) in all.iter().enumerate() {
            for other in &all[at + 1..] {
                assert_ne!(glyph_of(*port), glyph_of(*other), "{port:?} {other:?}");
            }
        }
    }

    #[test]
    fn inputs_become_targets_on_the_left_and_outputs_sources_on_the_right() {
        assert_eq!(
            handle_of(PortDirection::In),
            (HandleKind::Target, Side::Left)
        );
        assert_eq!(
            handle_of(PortDirection::Out),
            (HandleKind::Source, Side::Right)
        );
    }
}
