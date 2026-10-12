use sdrmm_wire::{decode::DecoderEvent, rest::EventAudio};
use zgui::prelude::*;

use crate::{
    decoders::{detail::event_detail, log::LogRow},
    store::Store,
    ui::kit_decoders,
};

use super::player::clip_player;

pub fn panel(store: Store, row: &LogRow) -> AnyView {
    AnyView::new(view! {
        column(
            class = "dlog__detail",
            on:click:stop = |_| {},
            on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation()
        ) {
            {row_detail(store, row)}
        }
    })
}

fn audio_of(event: &DecoderEvent) -> Option<&EventAudio> {
    match event {
        DecoderEvent::Call(call) => call.audio.as_ref(),
        DecoderEvent::Transmission(t) => t.audio.as_ref(),
        _ => None,
    }
}

fn audio_view(store: Store, row: &LogRow, audio: &EventAudio) -> AnyView {
    let url = audio.url.clone();
    let saved = audio.url.clone();
    let extension = audio.media_type.rsplit('/').next().unwrap_or("bin");
    let name = format!("{}-{}.{extension}", row.kind, row.at.replace(':', "-"));
    AnyView::new(view! {
        row(class = "dlog__player") {
            {clip_player(store, url)}
            control(
                class = "btn",
                on:click:stop = move |_| kit_decoders::save_download(store, saved.clone(), name.clone())
            ) {"Save audio"}
        }
    })
}

fn row_detail(store: Store, row: &LogRow) -> impl IntoView + use<> {
    let event = row.source.event();
    let detail = event_detail(event);
    let audio = audio_of(event).map(|audio| audio_view(store, row, audio));
    let object = match event {
        DecoderEvent::BroadcastData(data) => {
            Some(AnyView::new(kit_decoders::broadcast_data_view(store, data)))
        }
        _ => None,
    };
    let empty = (detail.fields.is_empty() && detail.body.is_none())
        .then(|| AnyView::new(view! { text(class = "dk-dim") {"Nothing beyond the summary."} }));
    let fields =
        (!detail.fields.is_empty()).then(|| AnyView::new(kit_decoders::fields_view(detail.fields)));
    let body = detail
        .body
        .map(|body| AnyView::new(view! { text(class = "dk-pre") {{body}} }));
    view! {
        {audio}
        {object}
        {fields}
        {body}
        {empty}
    }
}
