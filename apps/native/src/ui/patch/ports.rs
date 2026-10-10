use sdrmm_wire::patch::PortDirection;
use zgui::prelude::*;
use zgui_flow::prelude::{HandleKind, HandleProps, Side};

use crate::ui::node::Place;

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
    pointer-events: none;
    white-space: nowrap;
}
.port--in .port__label { right: 9px; }
.port--out .port__label { left: 9px; }
"#
);

#[must_use]
pub const fn handle_of(direction: PortDirection) -> (HandleKind, Side) {
    match direction {
        PortDirection::In => (HandleKind::Target, Side::Left),
        PortDirection::Out => (HandleKind::Source, Side::Right),
    }
}

fn port(place: Place) -> impl IntoView {
    let (kind, side) = handle_of(place.direction);
    let class = match place.direction {
        PortDirection::In => "port port--in",
        PortDirection::Out => "port port--out",
    };
    let id = place.name.clone();
    view! {
        box(class = class, style:top = Some(format!("{:.0}px", place.y))) {
            Handle(kind = kind, side = side, id = id, class = place.port_type.as_str())
            text(class = "port__label") {{place.name.clone()}}
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
