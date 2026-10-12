use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use sdrmm_engine::Engine;
use sdrmm_wire::{
    ChannelDescriptor, NodeProblem, PatchGraph, ProblemSource, ResolveCtx, ResolvedPatch,
    ServerEvent, StateScope, StateSnapshot, patch::resolve,
};
use tokio::sync::{
    broadcast::{Receiver, error::TryRecvError},
    watch,
};

use crate::{AppState, Store, events::EventRoutes, gps::PositionRoutes, problems::ProblemBoard};

#[derive(Debug, Default)]
pub(crate) struct Resolved {
    pub patch: ResolvedPatch,
    pub graph: Arc<PatchGraph>,
    pub events: EventRoutes,
    pub positions: Arc<PositionRoutes>,
}

impl Resolved {
    pub(crate) fn of(graph: PatchGraph, live: &StateSnapshot) -> Self {
        let patch = crate::workspace::resolve(&graph, live);
        Self {
            events: EventRoutes::resolve(&patch, &graph),
            positions: Arc::new(PositionRoutes::resolve(&patch)),
            patch,
            graph: Arc::new(graph),
        }
    }
}

#[derive(Clone)]
struct Loaded {
    workspace: i64,
    revision: u64,
    graph: Arc<PatchGraph>,
}

pub(crate) struct ResolvedHub {
    tx: watch::Sender<Arc<Resolved>>,
    descriptors: Vec<ChannelDescriptor>,
    loaded: Mutex<Option<Loaded>>,
}

impl Default for ResolvedHub {
    fn default() -> Self {
        Self::new(sdrmm_engine::channel_types())
    }
}

impl ResolvedHub {
    pub(crate) fn new(descriptors: Vec<ChannelDescriptor>) -> Self {
        Self {
            tx: watch::channel(Arc::new(Resolved::default())).0,
            descriptors,
            loaded: Mutex::new(None),
        }
    }

    pub(crate) fn current(&self) -> Arc<Resolved> {
        self.tx.borrow().clone()
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<Arc<Resolved>> {
        self.tx.subscribe()
    }

    pub(crate) fn reload(&self, store: &Store, engine: &Engine, problems: &ProblemBoard) -> bool {
        let mut loaded = self.lock();
        match store.active_workspace() {
            Ok(active) => {
                *loaded = active.map(|detail| Loaded {
                    workspace: detail.info.id,
                    revision: detail.info.revision,
                    graph: Arc::new(detail.snapshot.graph),
                });
            }
            Err(error) => {
                tracing::error!(%error, "could not read the active workspace to resolve it");
                return false;
            }
        }
        self.publish(loaded.as_ref(), engine, problems)
    }

    pub(crate) fn rebind(&self, engine: &Engine, problems: &ProblemBoard) -> bool {
        let loaded = self.lock();
        self.publish(loaded.as_ref(), engine, problems)
    }

    fn lock(&self) -> MutexGuard<'_, Option<Loaded>> {
        self.loaded.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn resolve(&self, loaded: &Loaded, live: &StateSnapshot) -> (ResolvedPatch, Vec<NodeProblem>) {
        let ctx = ResolveCtx {
            descriptors: Some(&self.descriptors),
            live: Some(live),
        };
        let (mut patch, refused) = resolve::compile_draft(&loaded.graph, &ctx);
        for wire in &refused {
            tracing::warn!(
                from = %wire.edge.from.node,
                to = %wire.edge.to.node,
                reason = %wire.reason,
                workspace = loaded.workspace,
                "the active workspace holds a refused wire"
            );
        }
        let problems = resolve::bind(&mut patch, &loaded.graph, live);
        patch.workspace = Some(loaded.workspace);
        patch.revision = loaded.revision;
        (patch, problems)
    }

    fn publish(&self, loaded: Option<&Loaded>, engine: &Engine, problems: &ProblemBoard) -> bool {
        let live = engine.snapshot();
        let (mut patch, found, graph) = match loaded {
            Some(loaded) => {
                let (patch, found) = self.resolve(loaded, &live);
                (patch, found, loaded.graph.clone())
            }
            None => (ResolvedPatch::default(), Vec::new(), Arc::default()),
        };
        problems.replace(engine, ProblemSource::Bind, &found);
        problems.retain(engine, |node, _| patch.node(node).is_some());
        let events = EventRoutes::resolve(&patch, &graph);
        let current = self.current();
        let patch_changed = !current.patch.same_binding(&patch);
        if !patch_changed && current.graph.same_topology(&graph) && current.events == events {
            return false;
        }
        patch.epoch = current.patch.epoch + u64::from(patch_changed);
        let next = Arc::new(Resolved {
            positions: Arc::new(PositionRoutes::resolve(&patch)),
            patch,
            graph,
            events,
        });
        self.tx.send_replace(next.clone());
        if patch_changed {
            engine.emit_event(ServerEvent::PatchResolved(Box::new(next.patch.clone())));
        }
        patch_changed
    }
}

#[derive(Clone, Copy, Default)]
struct Wake {
    reload: bool,
    rebind: bool,
}

impl Wake {
    fn note(&mut self, event: Result<ServerEvent, TryRecvError>) -> bool {
        match event {
            Ok(ServerEvent::StateChanged {
                scope: StateScope::All | StateScope::Workspaces | StateScope::Workspace(_),
            })
            | Err(TryRecvError::Lagged(_)) => self.reload = true,
            Ok(ServerEvent::StateChanged {
                scope: StateScope::Devices | StateScope::DeviceSet(_) | StateScope::Arrays,
            }) => self.rebind = true,
            Ok(_) => {}
            Err(TryRecvError::Empty | TryRecvError::Closed) => return false,
        }
        true
    }
}

pub(crate) async fn run(state: AppState) {
    let mut events = state.engine.subscribe_events();
    refresh(
        &state,
        Wake {
            reload: true,
            rebind: false,
        },
    )
    .await;
    loop {
        let first = match events.recv().await {
            Ok(event) => Ok(event),
            Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                Err(TryRecvError::Lagged(count))
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        };
        let mut wake = Wake::default();
        wake.note(first);
        drain(&mut events, &mut wake);
        if wake.reload || wake.rebind {
            refresh(&state, wake).await;
        }
    }
}

fn drain(events: &mut Receiver<ServerEvent>, wake: &mut Wake) {
    while wake.note(events.try_recv()) {}
}

async fn refresh(state: &AppState, wake: Wake) {
    let state = state.clone();
    let done = tokio::task::spawn_blocking(move || {
        if wake.reload {
            state
                .resolved
                .reload(&state.store, &state.engine, &state.problems)
        } else {
            state.resolved.rebind(&state.engine, &state.problems)
        }
    })
    .await;
    if let Err(error) = done {
        tracing::error!(%error, "resolving the patch stopped");
    }
}

pub(crate) fn follow<F>(state: &AppState, name: &'static str, apply: F)
where
    F: Fn(&AppState) + Send + Sync + 'static,
{
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        tracing::warn!(
            name,
            "no runtime in context: this part will not follow the patch"
        );
        return;
    };
    let mut changes = state.resolved.subscribe();
    let state = state.clone();
    let apply = Arc::new(apply);
    handle.spawn(async move {
        loop {
            changes.borrow_and_update();
            let (app, step) = (state.clone(), apply.clone());
            if let Err(error) = tokio::task::spawn_blocking(move || step(&app)).await {
                tracing::error!(%error, name, "following the patch stopped");
                return;
            }
            if changes.changed().await.is_err() {
                return;
            }
        }
    });
}

impl AppState {
    pub(crate) fn resolve_now(&self) -> bool {
        self.resolved
            .reload(&self.store, &self.engine, &self.problems)
    }
}

#[cfg(test)]
mod tests;
