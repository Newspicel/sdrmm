use std::time::Duration;

use sdrmm_wire::patch::{NodeBody, PatchEdge, PatchNode, Position};
use zgui::prelude::*;

use super::{Screen, WAIT, dom::CssRect, gallery::fit};
use crate::store::Store;

const SPARE: &str = "speaker2";
const NUDGE: (f32, f32) = (96.0, 72.0);
const PAN_PX: f32 = 160.0;
const ZOOM_PX: f32 = -240.0;
const LASSO_PAD: f32 = 24.0;

fn position(store: Store, id: &str) -> Option<Position> {
    store
        .graph
        .get_untracked()
        .node(id)
        .map(|node| node.position)
}

fn has_edge(store: Store, from: (&str, &str), to: (&str, &str)) -> bool {
    store
        .graph
        .get_untracked()
        .edges
        .iter()
        .any(|edge: &PatchEdge| {
            edge.from.node == from.0
                && edge.from.port == from.1
                && edge.to.node == to.0
                && edge.to.port == to.1
        })
}

fn node_box(screen: &Screen, id: &str) -> CssRect {
    screen
        .css_box(&format!(".zf-node[data-id={id}]"))
        .unwrap_or_else(|| panic!("node {id} is not drawn"))
}

pub fn starter(screen: &mut Screen) {
    screen.settle_for("the starter patch", WAIT, |screen| {
        screen.store().graph.get_untracked().nodes.len() >= 5 && screen.find(".zf-node").len() >= 5
    });
    screen.shot("canvas/0-start");
}

pub fn move_node(screen: &mut Screen) {
    let store = screen.store();
    let before = position(store, "speaker");
    let from = screen.center_of(".zf-node[data-id=speaker] .node__title");
    screen.drag(from, (from.0 + NUDGE.0, from.1 + NUDGE.1), Modifiers::NONE);
    screen.settle_for("the moved speaker", WAIT, |screen| {
        position(screen.store(), "speaker") != before
    });
    screen.shot("canvas/1-moved");
}

pub fn add_spare(screen: &mut Screen) {
    let store = screen.store();
    let below = position(store, "speaker").map_or(Position { x: 0.0, y: 0.0 }, |at| Position {
        x: at.x,
        y: at.y + 320.0,
    });
    store.edit_graph(move |graph| {
        graph.nodes.push(PatchNode {
            id: SPARE.to_owned(),
            body: NodeBody::Speaker,
            position: below,
            size: None,
            label: None,
        });
    });
    screen.settle_for("the spare speaker", WAIT, |screen| {
        screen
            .css_box(&format!(".zf-node[data-id={SPARE}]"))
            .is_some()
    });
    screen.run_for(Duration::from_millis(300));
    fit(screen);
}

pub fn connect(screen: &mut Screen) {
    let from = screen.center_of(".zf-node[data-id=nfm] .port--out[text=audio] .zf-handle");
    let to = screen.center_of(&format!(".zf-node[data-id={SPARE}] .port--in .zf-handle"));
    screen.drag(from, to, Modifiers::NONE);
    screen.settle_for("the new wire", WAIT, |screen| {
        has_edge(screen.store(), ("nfm", "audio"), (SPARE, "audio"))
    });
    screen.shot("canvas/2-wired");
}

pub fn context_menus(screen: &mut Screen) {
    let spot = screen.blank_spot();
    screen.right_click(spot);
    assert!(screen.css_box(".canvas-menu").is_some(), "no pane menu");
    screen.shot("canvas/3-pane-menu");
    screen.click_on(".canvas-dismiss");
    let title = screen.center_of(".zf-node[data-id=scope] .node__title");
    screen.right_click(title);
    assert!(screen.css_box(".canvas-menu").is_some(), "no node menu");
    screen.shot("canvas/4-node-menu");
    screen.click_on(".canvas-dismiss");
    assert!(
        screen.css_box(".canvas-menu").is_none(),
        "the menu stays open"
    );
}

pub fn pan_and_zoom(screen: &mut Screen) {
    let spot = screen.blank_spot();
    let before = node_box(screen, "scope");
    screen.wheel(spot, (0.0, PAN_PX), Modifiers::NONE);
    screen.run_for(Duration::from_millis(200));
    let panned = node_box(screen, "scope");
    assert!(
        (panned.origin.y.0 - before.origin.y.0).abs() > 1.0,
        "the wheel did not pan"
    );
    screen.shot("canvas/5-panned");
    screen.wheel(spot, (0.0, ZOOM_PX), Modifiers::CONTROL);
    screen.run_for(Duration::from_millis(200));
    let zoomed = node_box(screen, "scope");
    assert!(
        (zoomed.size.width.0 - panned.size.width.0).abs() > 1.0,
        "ctrl-wheel did not zoom"
    );
    screen.shot("canvas/6-zoomed");
    fit(screen);
}

pub fn lasso_and_delete(screen: &mut Screen) {
    let target = node_box(screen, SPARE);
    let from = (target.origin.x.0 - LASSO_PAD, target.origin.y.0 - LASSO_PAD);
    let to = (
        target.origin.x.0 + target.size.width.0 + LASSO_PAD,
        target.origin.y.0 + target.size.height.0 + LASSO_PAD,
    );
    screen.drag(from, to, Modifiers::SHIFT);
    screen.settle_for("the lasso selection", WAIT, |screen| {
        screen
            .css_box(&format!(".zf-node[data-id={SPARE}][data-selected]"))
            .is_some()
    });
    screen.shot("canvas/7-selected");
    screen.key(NamedKey::Backspace, Modifiers::NONE);
    screen.settle_for("the deleted speaker", WAIT, |screen| {
        screen.store().graph.get_untracked().node(SPARE).is_none()
    });
    screen.shot("canvas/8-deleted");
}
