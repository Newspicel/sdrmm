use std::time::Duration;

use zgui::prelude::*;

use super::Screen;

const LIBRARY_TABS: [&str; 6] = [
    "Templates",
    "Presets",
    "Bookmarks",
    "Bands",
    "Occupancy",
    "Recordings",
];
const THEMES: [&str; 3] = ["dark", "light", "system"];
const SETTLE: Duration = Duration::from_millis(500);
const THEME_BUTTON: usize = 2;
const HELP_BUTTON: usize = 3;

fn slug(text: &str) -> String {
    text.chars()
        .filter(|letter| letter.is_ascii_alphanumeric() || *letter == ' ')
        .collect::<String>()
        .trim()
        .to_lowercase()
        .replace(' ', "-")
}

fn press(screen: &mut Screen, selector: &str) {
    screen.click_on(selector);
    screen.run_for(SETTLE);
}

fn escape(screen: &mut Screen) {
    screen.key(NamedKey::Escape, Modifiers::NONE);
    screen.run_for(SETTLE);
}

pub fn library(screen: &mut Screen) {
    press(screen, ".hb__quiet[text=Library]");
    for tab in LIBRARY_TABS {
        press(screen, &format!(".lib__tab[text={tab}]"));
        screen.shot(&format!("shell/library-{}", slug(tab)));
    }
}

pub fn tools(screen: &mut Screen) {
    press(screen, ".lib__tab[text=Tools]");
    screen.settle_for("the tool list", Duration::from_secs(10), |screen| {
        !screen.find(".tools__row").is_empty()
    });
    screen.shot("shell/tools");
    let names = screen.texts(".tools__row .tool-ink");
    for (index, name) in names.iter().enumerate() {
        let at = screen.nth(".tools__row", index);
        screen.click(at);
        screen.run_for(SETTLE);
        screen.shot(&format!("shell/tool-{}", slug(name)));
        press(screen, ".tools__foot [text=All tools]");
    }
    press(screen, ".tools__foot [text=Close]");
    assert!(screen.css_box(".tools").is_none(), "the tools stay open");
}

pub fn workspaces(screen: &mut Screen) {
    press(screen, ".hb__menu");
    assert!(screen.css_box(".wm").is_some(), "no workspace menu");
    screen.shot("shell/workspaces");
    escape(screen);
}

pub fn palette(screen: &mut Screen) {
    let spot = screen.blank_spot();
    screen.double_click(spot);
    screen.run_for(SETTLE);
    assert!(screen.store().palette.get_untracked(), "no palette");
    screen.shot("shell/palette");
    press(screen, ".pal__search");
    screen.type_text("scope");
    screen.run_for(SETTLE);
    screen.shot("shell/palette-search");
    escape(screen);
    if screen.store().palette.get_untracked() {
        escape(screen);
    }
}

pub fn dialogs(screen: &mut Screen) {
    for (button, name) in [("Licenses", "about"), ("Report a problem", "report")] {
        let help = screen.nth(".hb .sk-iconbtn", HELP_BUTTON);
        screen.click(help);
        screen.run_for(SETTLE);
        screen.shot("shell/help");
        press(screen, &format!(".help__row[text={button}]"));
        screen.shot(&format!("shell/{name}"));
        escape(screen);
    }
}

pub fn themes(screen: &mut Screen) {
    for theme in THEMES {
        let at = screen.nth(".hb .sk-iconbtn", THEME_BUTTON);
        screen.click(at);
        screen.run_for(SETTLE);
        screen.shot(&format!("shell/theme-{theme}"));
    }
}

pub fn rack_and_full(screen: &mut Screen) {
    press(screen, ".hb__tab[text=Rack]");
    screen.shot("shell/rack");
    press(screen, ".hb__tab[text=Patch]");
    let expand = screen.nth(".zf-node[data-id=scope] .node__btn", 0);
    screen.click(expand);
    screen.run_for(SETTLE);
    screen.shot("shell/full-scope");
    escape(screen);
}
