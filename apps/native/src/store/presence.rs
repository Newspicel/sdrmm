use std::sync::Arc;

use sdrmm_wire::{
    patch::Position,
    presence::{DraggedNode, Pointer, peer_name},
    ws::{ClientCommand, ServerEvent},
};

use zgui::prelude::*;

use super::Store;
use crate::presence::{PointerGate, fitted};

#[derive(Clone, Debug, Default)]
pub struct Me {
    pub name: String,
    pub pointer: Pointer,
    pub gate: PointerGate,
}

fn now_ms() -> u64 {
    u64::try_from(jiff::Timestamp::now().as_millisecond()).unwrap_or_default()
}

impl Store {
    pub(crate) fn introduce(self) {
        let Some(author) = self.api().author().map(str::to_owned) else {
            return;
        };
        let name = self.me.with_value(|me| me.name.clone());
        self.me.update_value(|me| me.gate.forget());
        self.command(ClientCommand::Present { author, name });
    }

    pub fn present_as(self, wanted: &str) -> String {
        let name = peer_name(wanted).unwrap_or_default();
        let kept = name.clone();
        self.me.update_value(|me| me.name = kept);
        self.introduce();
        name
    }

    pub(crate) fn observe_peers(self, event: &ServerEvent) {
        let mut room = (*self.presence.get_untracked()).clone();
        if room.observe(event) {
            self.presence.set(Arc::new(room));
        }
    }

    pub fn point_at(self, at: Option<Position>) {
        self.point(|pointer| pointer.at = at);
    }

    pub fn point_selection(self, selected: Vec<String>, dragging: Vec<DraggedNode>) {
        self.point(|pointer| {
            pointer.selected = selected;
            pointer.dragging = dragging;
        });
    }

    fn point(self, change: impl FnOnce(&mut Pointer)) {
        let sent = self
            .me
            .try_update_value(|me| {
                let mut next = me.pointer.clone();
                change(&mut next);
                if next == me.pointer {
                    return None;
                }
                me.pointer = fitted(next);
                me.gate.offer(me.pointer.clone(), now_ms())
            })
            .flatten();
        if let Some(pointer) = sent {
            self.command(ClientCommand::Point(pointer));
        }
    }

    pub fn flush_pointer(self) {
        let sent = self
            .me
            .try_update_value(|me| me.gate.flush(now_ms()))
            .flatten();
        if let Some(pointer) = sent {
            self.command(ClientCommand::Point(pointer));
        }
    }
}
