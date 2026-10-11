use zgui::prelude::*;

use crate::{
    shell::notices::{NoticeLine, notice_line},
    store::Store,
    ui::kit_shell::{gated_button, hover},
};

const SHEET: &str = css!(
    r#"
.notices { flex: 0 0 auto; flex-direction: column; border-bottom: 1px solid var(--line); background-color: color-mix(in oklab, oklch(0.78 0.15 80) 10%, transparent); }
.notices__line { align-items: center; gap: 8px; padding: 2px 8px; }
.notices__text { flex: 1 1 auto; min-width: 0; font-size: 12px; color: var(--ink); overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
"#
);

pub fn strip(store: Store) -> impl IntoView {
    install_stylesheet("shell-notices", SHEET);
    let dismissing = RwSignal::new(None::<i64>);
    move || {
        let catalog = store.catalog.get();
        let lines: Vec<NoticeLine> = store
            .notices
            .get()
            .iter()
            .map(|notice| notice_line(notice, &catalog.nodes))
            .collect();
        (!lines.is_empty()).then(|| {
            let rows: Vec<AnyView> = lines
                .into_iter()
                .map(|line| AnyView::new(notice_row(store, dismissing, line)))
                .collect();
            AnyView::new(view! { column(class = "notices", a11y:role = Role::Status) {{rows}} })
        })
    }
}

fn notice_row(store: Store, dismissing: RwSignal<Option<i64>>, line: NoticeLine) -> impl IntoView {
    let id = line.id;
    let idle = Signal::derive_local(move || dismissing.get() != Some(id));
    view! {
        row(class = "notices__line") {
            {hover(line.title, view! { text(class = "notices__text") {{line.text}} })}
            {gated_button("Dismiss", "quiet", idle, move || dismiss(store, dismissing, id))}
        }
    }
}

fn dismiss(store: Store, dismissing: RwSignal<Option<i64>>, notice: i64) {
    let Some(workspace) = store.workspace.get_untracked() else {
        return;
    };
    dismissing.set(Some(notice));
    zgui::task::spawn_local(async move {
        let path = format!("/api/workspaces/{workspace}/notices/{notice}");
        if let Err(error) = store.api().delete(&path).await {
            store.fail("Cannot dismiss the notice", &error);
        }
        dismissing.set(None);
        store.resync();
    });
}
