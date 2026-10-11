use std::{
    collections::HashMap,
    sync::{Arc, Mutex, mpsc},
    thread::JoinHandle,
};

use sdrmm_wire::{
    NodeBody, PatchGraph, RotatorAxes, RotatorLink, RotatorNode, RotatorStatus, ServerEvent,
};

use crate::AppState;

mod driver;
mod gs232;
mod link;
mod plan;
mod rotctld;

use driver::Driver;

#[derive(Clone, Debug, PartialEq)]
struct Follow {
    satellite: Option<String>,
    settings: RotatorNode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Connection {
    link: RotatorLink,
    axes: RotatorAxes,
}

#[derive(Clone, Debug, PartialEq)]
struct Wanted {
    node: String,
    connection: Connection,
    follow: Follow,
}

struct Worker {
    connection: Connection,
    follow: Arc<Mutex<Follow>>,
    wake: mpsc::Sender<()>,
    handle: JoinHandle<()>,
}

#[derive(Default)]
pub(crate) struct RotatorHub {
    workers: Mutex<HashMap<String, Worker>>,
    statuses: Arc<Mutex<HashMap<String, RotatorStatus>>>,
}

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl RotatorHub {
    pub(crate) fn snapshot(&self) -> Vec<ServerEvent> {
        locked(&self.statuses)
            .values()
            .map(|status| ServerEvent::RotatorUpdate {
                status: Box::new(status.clone()),
            })
            .collect()
    }

    pub(crate) fn reconcile(&self, state: &AppState) {
        let graph = match state.store.active_workspace() {
            Ok(active) => active
                .map(|active| active.snapshot.graph)
                .unwrap_or_default(),
            Err(error) => {
                tracing::warn!(%error, "could not reconcile rotators");
                return;
            }
        };
        let wanted = wanted(&graph);
        let mut workers = locked(&self.workers);
        workers.retain(|node, worker| {
            let keep = wanted
                .iter()
                .any(|want| want.node == *node && want.connection == worker.connection);
            if !keep {
                locked(&self.statuses).remove(node);
            }
            keep
        });
        for want in wanted {
            if let Some(worker) = workers.get(&want.node) {
                *locked(&worker.follow) = want.follow;
                let _ = worker.wake.send(());
                continue;
            }
            if let Some(worker) = self.spawn(state, want.clone()) {
                workers.insert(want.node, worker);
            }
        }
    }

    fn spawn(&self, state: &AppState, want: Wanted) -> Option<Worker> {
        let (wake, woken) = mpsc::channel();
        let follow = Arc::new(Mutex::new(want.follow));
        let driver = Driver::new(
            want.node.clone(),
            want.connection.clone(),
            follow.clone(),
            self.statuses.clone(),
            state,
        );
        match std::thread::Builder::new()
            .name("sdrmm-rotator".to_owned())
            .spawn(move || driver.run(&woken))
        {
            Ok(handle) => Some(Worker {
                connection: want.connection,
                follow,
                wake,
                handle,
            }),
            Err(error) => {
                tracing::error!(%error, node = want.node, "could not start a rotator");
                None
            }
        }
    }
}

impl Drop for RotatorHub {
    fn drop(&mut self) {
        let workers: Vec<Worker> = locked(&self.workers).drain().map(|(_, w)| w).collect();
        for worker in workers {
            drop(worker.wake);
            if worker.handle.join().is_err() {
                tracing::error!("rotator driver panicked");
            }
        }
    }
}

fn wanted(graph: &PatchGraph) -> Vec<Wanted> {
    graph
        .nodes
        .iter()
        .filter_map(|node| match &node.body {
            NodeBody::Rotator(settings) => Some(Wanted {
                node: node.id.clone(),
                connection: Connection {
                    link: settings.link.clone()?,
                    axes: settings.axes,
                },
                follow: Follow {
                    satellite: graph
                        .sources_of(&node.id, "control")
                        .find(|source| {
                            graph
                                .node(source)
                                .is_some_and(|node| matches!(node.body, NodeBody::Satellite(_)))
                        })
                        .map(str::to_owned),
                    settings: settings.clone(),
                },
            }),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests;
