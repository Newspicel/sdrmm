use zgui::prelude::*;

use crate::ui::kit_shell::{hover, small_icon};

const SHEET: &str = css!(
    r#"
.lib-row__head { align-items: center; gap: 6px; min-width: 0; }
.lib-row__badge { font-family: var(--mono); font-size: 10px; color: var(--ink-faint); flex: 0 0 auto; }
.lib-row__lead { flex: 0 0 auto; }
.lib-chip { padding: 0 5px; border: 1px solid var(--line); border-radius: 3px; font-family: var(--mono); font-size: 10px; color: var(--ink-dim); flex: 0 0 auto; }
"#
);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Badge {
    #[default]
    None,
    Chip(String),
    Legend(String),
}

#[derive(Default)]
pub struct LibRow {
    pub primary: String,
    pub badge: Badge,
    pub secondary: Option<String>,
    pub hint: Option<String>,
    pub lead: Option<AnyView>,
}

pub struct Pick {
    pub on_pick: Box<dyn Fn()>,
    pub enabled: Signal<bool, LocalStorage>,
}

pub fn install() {
    install_stylesheet("library-rows", SHEET);
}

fn badge_view(badge: Badge) -> Option<AnyView> {
    match badge {
        Badge::None => None,
        Badge::Chip(text) => Some(AnyView::new(view! { text(class = "lib-chip") {{text}} })),
        Badge::Legend(text) => Some(AnyView::new(
            view! { text(class = "lib-row__badge") {{text}} },
        )),
    }
}

fn main_view(primary: String, badge: Badge, secondary: Option<String>) -> impl IntoView {
    let secondary =
        secondary.map(|text| AnyView::new(view! { text(class = "sk-row__secondary") {{text}} }));
    view! {
        column(class = "sk-row__main") {
            row(class = "lib-row__head") {
                text(class = "sk-row__primary") {{primary}}
                {badge_view(badge)}
            }
            {secondary}
        }
    }
}

fn hinted(hint: Option<String>, body: AnyView) -> AnyView {
    match hint {
        Some(hint) => hover(hint, view! { row(class = "sk-row__main") {{body}} }),
        None => body,
    }
}

pub fn lib_row(row: LibRow, pick: Option<Pick>, actions: AnyView, below: AnyView) -> impl IntoView {
    install();
    let LibRow {
        primary,
        badge,
        secondary,
        hint,
        lead,
    } = row;
    let main = AnyView::new(main_view(primary, badge, secondary));
    let face = match pick {
        Some(Pick { on_pick, enabled }) => AnyView::new(view! {
            control(
                class = "sk-row__main sk-row__pick",
                tabindex = Focus::Sequential,
                a11y:role = Role::Button,
                state:disabled = move || !enabled.get(),
                on:click:stop = move |_| if enabled.get_untracked() { on_pick() }
            ) {
                {main}
            }
        }),
        None => main,
    };
    let lead = lead.map(|lead| AnyView::new(view! { box(class = "lib-row__lead") {{lead}} }));
    view! {
        column(class = "sk-row") {
            row(class = "sk-row__line") {
                {lead}
                {hinted(hint, face)}
                row(class = "sk-row__actions") {{actions}}
            }
            {below}
        }
    }
}

pub fn act(
    label: impl Into<String>,
    tone: &'static str,
    enabled: Signal<bool, LocalStorage>,
    tooltip: Option<String>,
    on_press: impl Fn() + 'static,
) -> AnyView {
    let label = label.into();
    let face = view! {
        control(
            class = "sk-btn",
            class:sm = tone == "sm",
            class:primary = tone == "primary",
            state:disabled = move || !enabled.get(),
            tabindex = Focus::Sequential,
            a11y:role = Role::Button,
            on:click:stop = move |_| if enabled.get_untracked() { on_press() }
        ) {
            {label}
        }
    };
    match tooltip {
        Some(tooltip) => hover(tooltip, face),
        None => AnyView::new(face),
    }
}

pub fn icon_act(
    body: &'static str,
    label: impl Into<String>,
    danger: bool,
    enabled: Signal<bool, LocalStorage>,
    on_press: impl Fn() + 'static,
) -> AnyView {
    let label = label.into();
    let tooltip = label.clone();
    hover(
        tooltip,
        view! {
            control(
                class = "sk-iconbtn sm",
                class:danger = danger,
                state:disabled = move || !enabled.get(),
                tabindex = Focus::Sequential,
                a11y:role = Role::Button,
                a11y:label = label,
                on:click:stop = move |_| if enabled.get_untracked() { on_press() }
            ) {
                {small_icon(body)}
            }
        },
    )
}

#[derive(Clone, Copy)]
pub struct Pending(RwSignal<bool>);

impl Pending {
    #[must_use]
    pub fn new() -> Self {
        Self(RwSignal::new(false))
    }

    #[must_use]
    pub fn busy(self) -> bool {
        self.0.get()
    }

    #[must_use]
    pub fn idle(self) -> Signal<bool, LocalStorage> {
        Signal::derive_local(move || !self.0.get())
    }

    pub fn run<F>(self, work: F)
    where
        F: Future<Output = ()> + 'static,
    {
        if self.0.get_untracked() {
            return;
        }
        self.0.set(true);
        zgui::task::spawn_local(async move {
            work.await;
            self.0.try_set(false);
        });
    }
}

impl Default for Pending {
    fn default() -> Self {
        Self::new()
    }
}
