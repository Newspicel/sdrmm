use std::rc::Rc;

use sdrmm_wire::rest::{CreatedRowId, SaveRadioRequest, SavedRadio};
use zgui::prelude::*;

use super::{actions::Busy, devices::network_radio_label};
use crate::{
    store::Store,
    ui::kit_sources::{Tone, titled_button},
};

const PATH: &str = "/api/saved-radios";

pub type Shelf = RwSignal<Vec<SavedRadio>>;

pub fn shelf(store: Store) -> Shelf {
    let shelf: Shelf = RwSignal::new(Vec::new());
    refresh(store, shelf);
    shelf
}

fn refresh(store: Store, shelf: Shelf) {
    zgui::task::spawn_local(async move {
        match store.api().get::<Vec<SavedRadio>>(PATH).await {
            Ok(radios) => {
                shelf.try_set(radios);
            }
            Err(error) => store.fail("Cannot read saved radios", &error),
        }
    });
}

pub fn save(store: Store, shelf: Shelf, device_id: String) {
    let request = SaveRadioRequest {
        label: network_radio_label(&device_id),
        device_id,
    };
    zgui::task::spawn_local(async move {
        let saved: anyhow::Result<CreatedRowId> = store.api().post(PATH, &request).await;
        if let Err(error) = saved {
            store.fail("Cannot save the radio", &error);
        }
        refresh(store, shelf);
    });
}

fn forget(store: Store, shelf: Shelf, id: i64) {
    zgui::task::spawn_local(async move {
        if let Err(error) = store.api().delete(&format!("{PATH}/{id}")).await {
            store.fail("Cannot forget the radio", &error);
        }
        refresh(store, shelf);
    });
}

pub fn saved_radios(
    store: Store,
    shelf: Shelf,
    busy: Busy,
    on_add: Rc<dyn Fn(String)>,
) -> impl IntoView {
    move || {
        let radios = shelf.get();
        if radios.is_empty() {
            return None;
        }
        let rows: Vec<AnyView> = radios
            .into_iter()
            .map(|radio| {
                let on_add = on_add.clone();
                let SavedRadio {
                    id,
                    device_id,
                    label,
                } = radio;
                let named = label.clone();
                AnyView::new(view! {
                    row(class = "kit-saved") {
                        {titled_button(move || label.clone(), Tone::Plain, busy.busy.into(), move || format!("Open {named}"), move || on_add(device_id.clone()))}
                        {titled_button(|| "Forget".to_owned(), Tone::Danger, Signal::stored(false), || "Forget this saved radio".to_owned(), move || forget(store, shelf, id))}
                    }
                })
            })
            .collect();
        Some(AnyView::new(view! {
            column(class = "kit-list-wrap") {
                text(class = "kit-legend") {"Saved"}
                {rows}
            }
        }))
    }
}
