use sdrmm_wire::{
    channel::ChannelParams,
    lora::{LoraBandwidth, LoraCodingRate, LoraIq, LoraKey, LoraProtocol, LoraSpreadingFactor},
};
use zgui::prelude::*;

use super::{
    controls::{Ctx, listed, number, select, titled, toggle},
    lora::{
        IMPLICIT_LENGTH, KEY_KINDS, KEYS_TITLE, KeyKind, LORA_BANDWIDTHS, LORA_CODING_RATES,
        LORA_IQ, LORA_PROTOCOLS, LORA_SPREADING_FACTORS, apply_preset, blank_key, key_complete,
        key_fields, key_text, keys_shown, kind_label, kind_of, preset_of, preset_options,
        with_implicit_length, with_key_at, with_key_text, without_key,
    },
    modes::rows,
    settings::NumberLimit,
};
use crate::ui::{
    kit_channel::{CLOSE_ICON, NumberSpec, Popover, button, icon, setting_row, text_field, tip},
    widgets::pick,
};

pub fn lora(ctx: Ctx) -> AnyView {
    let header = ctx.read(get!(Lora, p => p.implicit_header), None);
    let implicit = move || header.get().is_some().then(|| implicit_rows(ctx));
    rows(vec![
        preset(ctx),
        select(
            ctx,
            "BW",
            Some("LoRa bandwidth"),
            listed(LORA_BANDWIDTHS),
            ctx.read(get!(Lora, p => p.bandwidth), LoraBandwidth::Khz125),
            set!(Lora, p, v => p.bandwidth = v),
        ),
        select(
            ctx,
            "SF",
            Some("Spreading factor"),
            listed(LORA_SPREADING_FACTORS),
            ctx.read(
                get!(Lora, p => p.spreading_factor),
                LoraSpreadingFactor::All,
            ),
            set!(Lora, p, v => p.spreading_factor = v),
        ),
        titled(
            ctx,
            "IQ",
            Some("Chirp direction"),
            LORA_IQ,
            ctx.read(get!(Lora, p => p.iq), LoraIq::Normal),
            set!(Lora, p, v => p.iq = v),
        ),
        select(
            ctx,
            "Protocol",
            Some("Payload protocol"),
            listed(LORA_PROTOCOLS),
            ctx.read(get!(Lora, p => p.protocol), LoraProtocol::Auto),
            set!(Lora, p, v => p.protocol = v),
        ),
        keys(ctx),
        number(
            ctx,
            "Implicit",
            Some("Implicit header payload length, empty for explicit headers"),
            NumberSpec::new("Implicit header length")
                .limit(NumberLimit::new(IMPLICIT_LENGTH.0, IMPLICIT_LENGTH.1, 1.0))
                .unit("B")
                .optional("off"),
            ctx.read(
                get!(Lora, p => p.implicit_header.map(|held| f64::from(held.length))),
                None,
            ),
            set!(Lora, p, v => p.implicit_header = with_implicit_length(p.implicit_header, v)),
        ),
        AnyView::new(implicit),
    ])
}

fn preset(ctx: Ctx) -> AnyView {
    let chosen = Signal::derive(move || {
        Some(ctx.params.get().and_then(|params| match params {
            ChannelParams::Lora(lora) => preset_of(&lora),
            _ => None,
        }))
    });
    setting_row(
        "Preset",
        Some("Modem preset; frequency stays as tuned"),
        pick(preset_options(), chosen, move |next: Option<usize>| {
            if let Some(index) = next {
                ctx.write(move |params| {
                    if let ChannelParams::Lora(lora) = params {
                        apply_preset(lora, index);
                    }
                });
            }
        }),
    )
}

fn implicit_rows(ctx: Ctx) -> AnyView {
    rows(vec![
        select(
            ctx,
            "CR",
            Some("Implicit header coding rate"),
            listed(LORA_CODING_RATES),
            ctx.read(
                get!(Lora, p => p.implicit_header.map_or(LoraCodingRate::Cr45, |held| held.coding_rate)),
                LoraCodingRate::Cr45,
            ),
            set!(Lora, p, v => if let Some(held) = p.implicit_header.as_mut() { held.coding_rate = v }),
        ),
        toggle(
            ctx,
            "CRC",
            Some("Implicit header payload carries a CRC"),
            ctx.read(
                get!(Lora, p => p.implicit_header.is_none_or(|held| held.crc)),
                true,
            ),
            set!(Lora, p, v => if let Some(held) = p.implicit_header.as_mut() { held.crc = v }),
        ),
    ])
}

fn edit_keys(ctx: Ctx, change: impl FnOnce(&[LoraKey]) -> Vec<LoraKey>) {
    ctx.write(move |params| {
        if let ChannelParams::Lora(lora) = params {
            lora.keys = change(&lora.keys);
        }
    });
}

fn keys(ctx: Ctx) -> AnyView {
    let held = ctx.read(get!(Lora, p => p.keys.clone()), Vec::new());
    let popover = Popover::new();
    let panel = move || popover.open().then(|| AnyView::new(keys_editor(ctx, held)));
    setting_row(
        "Keys",
        Some(KEYS_TITLE),
        view! {
            box(class = "kc-pop") {
                {move || button(keys_shown(held.get().len()), "kc-btn", false, move || popover.toggle())}
                {panel}
            }
        },
    )
}

fn keys_editor(ctx: Ctx, held: Signal<Vec<LoraKey>>) -> impl IntoView {
    let kinds = Memo::new(move |_| held.get().iter().map(kind_of).collect::<Vec<_>>());
    let draft = RwSignal::new(None::<LoraKey>);
    let rows = move || {
        kinds
            .get()
            .into_iter()
            .enumerate()
            .map(|(index, kind)| saved_key_row(ctx, held, index, kind))
            .collect::<Vec<_>>()
    };
    let draft_row = move || {
        let kind = draft.with(|draft| draft.as_ref().map(kind_of))?;
        Some(draft_key_row(ctx, draft, kind))
    };
    let adds = KEY_KINDS
        .iter()
        .map(|(kind, label, title)| {
            let kind = *kind;
            tip(
                title,
                AnyView::new(button(
                    format!("+ {label}"),
                    "kc-btn kc-btn--sm",
                    false,
                    move || draft.set(Some(blank_key(kind))),
                )),
            )
        })
        .collect::<Vec<_>>();
    view! {
        column(class = "kc-pop__panel kc-pop__panel--left kc-keys", on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation()) {
            {rows}
            {draft_row}
            row(class = "kc-chips") {{adds}}
        }
    }
}

fn saved_key_row(ctx: Ctx, held: Signal<Vec<LoraKey>>, index: usize, kind: KeyKind) -> AnyView {
    let fields = key_fields(kind)
        .iter()
        .enumerate()
        .map(|(field, placeholder)| {
            let placeholder: &'static str = placeholder;
            let value = Signal::derive(move || {
                held.with(|keys| keys.get(index).map(|key| key_text(key, field)))
                    .unwrap_or_default()
            });
            AnyView::new(text_field(
                value,
                placeholder,
                placeholder,
                "kc-num kc-key",
                move |text| {
                    edit_keys(ctx, move |keys| match keys.get(index) {
                        Some(key) => with_key_at(keys, index, with_key_text(key, field, &text)),
                        None => keys.to_vec(),
                    });
                    true
                },
            ))
        })
        .collect::<Vec<_>>();
    key_row(
        kind,
        fields,
        format!("Remove key {}", index + 1),
        move || {
            edit_keys(ctx, move |keys| without_key(keys, index));
        },
    )
}

fn draft_key_row(ctx: Ctx, draft: RwSignal<Option<LoraKey>>, kind: KeyKind) -> AnyView {
    let fields = key_fields(kind)
        .iter()
        .enumerate()
        .map(|(field, placeholder)| {
            let placeholder: &'static str = placeholder;
            let value = Signal::derive(move || {
                draft
                    .with(|draft| draft.as_ref().map(|key| key_text(key, field)))
                    .unwrap_or_default()
            });
            AnyView::new(text_field(
                value,
                placeholder,
                placeholder,
                "kc-num kc-key",
                move |text| {
                    let Some(next) = draft
                        .get_untracked()
                        .map(|key| with_key_text(&key, field, &text))
                    else {
                        return false;
                    };
                    if key_complete(&next) {
                        draft.set(None);
                        edit_keys(ctx, move |keys| {
                            let mut grown = keys.to_vec();
                            grown.push(next);
                            grown
                        });
                    } else {
                        draft.set(Some(next));
                    }
                    true
                },
            ))
        })
        .collect::<Vec<_>>();
    key_row(kind, fields, String::from("Remove new key"), move || {
        draft.set(None)
    })
}

fn key_row(
    kind: KeyKind,
    fields: Vec<AnyView>,
    remove_label: String,
    remove: impl Fn() + 'static,
) -> AnyView {
    let (label, title) = kind_label(kind);
    AnyView::new(view! {
        row(class = "kc-key-row") {
            {tip(title, AnyView::new(view! { text(class = "kc-name kc-name--tip kc-key-kind") {{label}} }))}
            row(class = "kc-cell") {{fields}}
            control(
                class = "kc-icon kc-btn--danger",
                tabindex = Focus::Sequential,
                a11y:role = Role::Button,
                a11y:label = remove_label,
                on:pointer_down = |ev: &mut EventCx<'_, events::PointerDown>| ev.stop_propagation(),
                on:click:stop = move |_| remove()
            ) {
                {icon(CLOSE_ICON, "kc-glyph kc-glyph--sm")}
            }
        }
    })
}
