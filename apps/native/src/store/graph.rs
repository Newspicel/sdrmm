use std::{collections::HashSet, sync::Arc};

use sdrmm_wire::{
    patch::{PatchGraph, PatchNode},
    workspace::{PatchApplyReport, WorkspaceDetail},
};
use zgui::prelude::*;

use super::{Store, sync};
use crate::{
    shell::apply_toasts::{self, apply_toasts},
    workspace::{Folded, GraphPatch},
};

impl Store {
    pub fn edit_graph(self, edit: impl FnOnce(&mut PatchGraph)) {
        self.change_graph(edit, true);
    }

    pub fn edit_graph_quiet(self, edit: impl FnOnce(&mut PatchGraph)) {
        self.change_graph(edit, false);
    }

    fn change_graph(self, edit: impl FnOnce(&mut PatchGraph), apply: bool) {
        let mut graph = (*self.graph.get_untracked()).clone();
        edit(&mut graph);
        self.graph.set(Arc::new(graph));
        let save = self.commit();
        zgui::task::spawn_local(async move {
            match save.await {
                Ok(true) if apply => self.apply_soon(),
                Ok(_) => {}
                Err(error) => self.save_failed("Cannot save the patch", &error),
            }
        });
    }

    pub fn commit_layout(self) {
        let save = self.commit();
        zgui::task::spawn_local(async move {
            if let Err(error) = save.await {
                self.save_failed("Cannot save the layout", &error);
            }
        });
    }

    pub(crate) fn commit(self) -> impl Future<Output = anyhow::Result<bool>> + 'static {
        let graph = self.graph.get_untracked();
        let before = self
            .sent
            .try_update_value(|sent| std::mem::replace(sent, graph.clone()))
            .unwrap_or_default();
        let patch = GraphPatch::between(&before, &graph);
        let pending = if patch.is_empty() {
            None
        } else {
            self.unsaved.update_value(|count| *count += 1);
            Some(self.editor().map(|editor| editor.save(patch)))
        };
        async move {
            let Some(pending) = pending else {
                return Ok(false);
            };
            let Some(pending) = pending else {
                self.unsaved
                    .update_value(|count| *count = count.saturating_sub(1));
                return Err(anyhow::anyhow!("workspace is still loading"));
            };
            let saved = pending.await;
            self.unsaved
                .update_value(|count| *count = count.saturating_sub(1));
            self.take_saved(&saved?);
            Ok(true)
        }
    }

    fn save_failed(self, context: &str, error: &anyhow::Error) {
        if error.is::<Folded>() {
            return;
        }
        self.fail(context, error);
        self.resync();
    }

    pub(crate) fn take_saved(self, detail: &WorkspaceDetail) {
        if self.workspace.get_untracked() != Some(detail.info.id) {
            return;
        }
        self.read_detail(detail);
        if self.unsaved.get_value() > 0 {
            return;
        }
        let server = &detail.snapshot.graph;
        if *self.graph.get_untracked() != *server {
            self.graph.set(Arc::new(server.clone()));
        }
        self.sent.set_value(Arc::new(server.clone()));
    }

    pub(crate) fn adopt_graph(self, graph: PatchGraph) {
        let graph = Arc::new(graph);
        self.sent.set_value(graph.clone());
        self.graph.set(graph);
    }

    pub fn resync(self) {
        let Some(editor) = self.editor() else {
            return;
        };
        let pending = editor.refresh();
        zgui::task::spawn_local(async move {
            match pending.await {
                Ok(detail) => self.take_saved(&detail),
                Err(error) => tracing::debug!(%error, "cannot refresh the workspace"),
            }
        });
    }

    pub fn step_history(self, back: bool) {
        let Some(editor) = self.editor() else {
            return;
        };
        let pending = editor.step(back);
        zgui::task::spawn_local(async move {
            match pending.await {
                Ok(detail) => {
                    self.take_saved(&detail);
                    self.apply_soon();
                }
                Err(error) => self.fail("Cannot step the history", &error),
            }
        });
    }

    pub fn apply_soon(self) {
        let start = self
            .applying
            .try_update_value(sync::ApplyGate::ask)
            .unwrap_or(false);
        if !start {
            return;
        }
        zgui::task::spawn_local(async move {
            loop {
                self.apply().await;
                let again = self
                    .applying
                    .try_update_value(sync::ApplyGate::finished)
                    .unwrap_or(false);
                if !again {
                    break;
                }
            }
        });
    }

    pub(crate) fn announce(self, report: &PatchApplyReport, nodes: &[PatchNode]) {
        let now = apply_toasts(report, nodes);
        let fresh = self
            .said
            .try_update_value(|said| {
                let fresh = apply_toasts::fresh(said, now.clone());
                *said = now;
                fresh
            })
            .unwrap_or_default();
        for message in fresh {
            self.say(message);
        }
        self.refusals_from_report(report);
    }

    pub(crate) fn retry_surfaces(self) {
        for command in self.bus.get_value().retry_refused() {
            self.command(command);
        }
    }

    pub(crate) fn expect_switch(self, id: i64) {
        self.switches.update_value(|switches| switches.expect(id));
    }

    pub(crate) fn switched(self, id: i64, by: Option<String>, report: &PatchApplyReport) {
        let mine = self
            .switches
            .try_update_value(|switches| switches.arrived(id))
            .unwrap_or(false);
        self.retry_surfaces();
        if mine || self.workspace.get_untracked() == Some(id) {
            return;
        }
        let name = self
            .workspaces
            .get_untracked()
            .iter()
            .find(|found| found.id == id)
            .map(|found| found.name.clone());
        self.note(sync::switch_notice(by.as_deref(), name.as_deref()));
        let report = report.clone();
        zgui::task::spawn_local(async move { self.follow(id, report).await });
    }

    pub fn forget_gone_nodes(self) {
        let graph = self.graph.get();
        let present: HashSet<&str> = graph.nodes.iter().map(|node| node.id.as_str()).collect();
        let gone = self
            .arrays
            .with_untracked(|book| sync::gone_nodes(book.held_nodes(), &present));
        if !gone.is_empty() {
            self.arrays.update(|book| Arc::make_mut(book).forget(&gone));
        }
    }
}
