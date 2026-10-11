mod chrome;
mod dom;
mod driver;
mod gallery;
mod gestures;
mod gpu;
mod input;
mod picture;
mod select;

use std::time::Duration;

use zgui::prelude::*;

use crate::store::Phase;

pub use driver::Screen;

const WAIT: Duration = Duration::from_secs(20);
const LIVE: Duration = Duration::from_secs(2);

fn starter_screen() -> Option<Screen> {
    let mut screen = Screen::ready()?;
    if screen.store().phase.get_untracked() == Phase::NoWorkspace {
        screen.shot("first-launch");
        screen.click_on(".start .sk-btn.primary");
    }
    screen.settle_for("the starter patch", WAIT, |screen| {
        let store = screen.store();
        store.phase.get_untracked() == Phase::Ready
            && store.graph.get_untracked().nodes.len() >= 5
            && !store.state.get_untracked().device_sets.is_empty()
    });
    screen.run_for(LIVE);
    Some(screen)
}

#[test]
#[ignore = "renders the app on a graphics device; run with --ignored"]
fn starter() {
    let Some(mut screen) = starter_screen() else {
        return;
    };
    screen.shot("starter");
}

#[test]
#[ignore = "renders the app on a graphics device; run with --ignored"]
fn every_face() {
    let Some(mut screen) = Screen::empty() else {
        return;
    };
    let store = screen.store();
    let built = gallery::build(
        &store.devices.get_untracked(),
        &store.channel_types.get_untracked(),
    );
    assert!(
        built.refused.is_empty(),
        "refused nodes: {:?}",
        built.refused
    );
    let ids: Vec<String> = built
        .graph
        .nodes
        .iter()
        .map(|node| node.id.clone())
        .collect();
    let graph = built.graph;
    store.edit_graph(move |held| *held = graph);
    screen.settle_for("every node drawn", WAIT, |screen| {
        screen.find(".zf-node").len() == ids.len()
    });
    gallery::fit(&mut screen);
    screen.run_for(LIVE);
    screen.shot("faces/overview");
    gallery::faces(&mut screen, &ids);
    gallery::modes(&mut screen);
}

#[test]
#[ignore = "renders the app on a graphics device; run with --ignored"]
fn canvas_interactions() {
    let Some(mut screen) = starter_screen() else {
        return;
    };
    gestures::starter(&mut screen);
    gestures::move_node(&mut screen);
    gestures::add_spare(&mut screen);
    gestures::connect(&mut screen);
    gestures::context_menus(&mut screen);
    gestures::pan_and_zoom(&mut screen);
    gestures::lasso_and_delete(&mut screen);
}

#[test]
#[ignore = "renders the app on a graphics device; run with --ignored"]
fn shell() {
    let Some(mut screen) = starter_screen() else {
        return;
    };
    chrome::library(&mut screen);
    chrome::tools(&mut screen);
    chrome::workspaces(&mut screen);
    chrome::palette(&mut screen);
    chrome::dialogs(&mut screen);
    chrome::themes(&mut screen);
    chrome::rack_and_full(&mut screen);
}
