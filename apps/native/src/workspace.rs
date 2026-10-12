use anyhow::Context;
use sdrmm_wire::{
    channel::ChannelSettings,
    device::DeviceSettings,
    patch::{PatchGraph, RackLayout},
    workspace::{
        UpdateWorkspaceRequest, WorkspaceDetail, WorkspaceInfo, WorkspaceSettings,
        WorkspaceSnapshot,
    },
};
use tokio::sync::{mpsc, oneshot};

use crate::{api::Api, shell::rack_grid};

pub mod manage;
pub mod rebase;

pub use rebase::GraphPatch;

#[derive(Debug)]
pub struct Folded;

impl std::fmt::Display for Folded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("saved together with a later edit")
    }
}

impl std::error::Error for Folded {}

type Reply = oneshot::Sender<anyhow::Result<WorkspaceDetail>>;

enum Edit {
    Graph(GraphPatch),
    Refresh,
    Rack(RackLayout),
    Settings(WorkspaceSettings),
    Rename(String),
    History(bool),
    Channel(u32, u32, ChannelSettings),
    Device(u32, DeviceSettings),
    Saved(String, ChannelSettings),
}

#[derive(Clone)]
pub struct Session {
    edits: mpsc::UnboundedSender<(Edit, Reply)>,
}

pub struct Worker {
    api: Api,
    detail: WorkspaceDetail,
    edits: mpsc::UnboundedReceiver<(Edit, Reply)>,
    held: Option<(Edit, Reply)>,
}

type Pending = std::pin::Pin<Box<dyn Future<Output = anyhow::Result<WorkspaceDetail>>>>;

impl Session {
    pub fn new(api: Api, detail: WorkspaceDetail) -> (Self, Worker) {
        let (edits, receiver) = mpsc::unbounded_channel();
        (
            Self { edits },
            Worker {
                api,
                detail,
                edits: receiver,
                held: None,
            },
        )
    }

    pub fn save(
        &self,
        patch: GraphPatch,
    ) -> impl Future<Output = anyhow::Result<WorkspaceDetail>> + use<> {
        self.enqueue(Edit::Graph(patch))
    }

    pub fn refresh(&self) -> impl Future<Output = anyhow::Result<WorkspaceDetail>> + use<> {
        self.enqueue(Edit::Refresh)
    }

    pub fn rack(&self, rack: RackLayout) -> Pending {
        Box::pin(self.enqueue(Edit::Rack(rack)))
    }

    pub fn settings(&self, settings: WorkspaceSettings) -> Pending {
        Box::pin(self.enqueue(Edit::Settings(settings)))
    }

    pub fn rename(&self, name: String) -> Pending {
        Box::pin(self.enqueue(Edit::Rename(name)))
    }

    pub fn step(
        &self,
        back: bool,
    ) -> impl Future<Output = anyhow::Result<WorkspaceDetail>> + use<> {
        self.enqueue(Edit::History(back))
    }

    pub fn channel(
        &self,
        set: u32,
        channel: u32,
        settings: ChannelSettings,
    ) -> impl Future<Output = anyhow::Result<WorkspaceDetail>> + use<> {
        self.enqueue(Edit::Channel(set, channel, settings))
    }

    pub fn device(
        &self,
        set: u32,
        settings: DeviceSettings,
    ) -> impl Future<Output = anyhow::Result<WorkspaceDetail>> + use<> {
        self.enqueue(Edit::Device(set, settings))
    }

    pub fn saved_channel(
        &self,
        node: String,
        settings: ChannelSettings,
    ) -> impl Future<Output = anyhow::Result<WorkspaceDetail>> + use<> {
        self.enqueue(Edit::Saved(node, settings))
    }

    fn enqueue(&self, edit: Edit) -> impl Future<Output = anyhow::Result<WorkspaceDetail>> + use<> {
        let (reply, receive) = oneshot::channel();
        let sent = self.edits.send((edit, reply));
        async move {
            sent.map_err(|_| anyhow::anyhow!("workspace writer is closed"))?;
            receive
                .await
                .context("workspace writer stopped before saving")?
        }
    }
}

impl Worker {
    pub async fn run(mut self) {
        while let Some((edit, reply)) = self.next().await {
            match edit {
                Edit::Graph(patch) => self.save_batch(patch, reply).await,
                edit => {
                    let result = self.edit(edit).await;
                    deliver(reply, result);
                }
            }
        }
    }

    async fn next(&mut self) -> Option<(Edit, Reply)> {
        match self.held.take() {
            Some(held) => Some(held),
            None => self.edits.recv().await,
        }
    }

    async fn save_batch(&mut self, mut patch: GraphPatch, reply: Reply) {
        let mut replies = vec![reply];
        while let Ok((edit, reply)) = self.edits.try_recv() {
            match edit {
                Edit::Graph(more) => {
                    patch.then(more);
                    replies.push(reply);
                }
                other => {
                    self.held = Some((other, reply));
                    break;
                }
            }
        }
        let result = self.save_graph(&patch).await;
        let last = replies.pop();
        for earlier in replies {
            deliver(earlier, folded(&result));
        }
        if let Some(last) = last {
            deliver(last, result);
        }
    }

    async fn save_graph(&mut self, patch: &GraphPatch) -> anyhow::Result<WorkspaceDetail> {
        let id = self.detail.info.id;
        let first = self.write(self.patched(patch)).await;
        if let Err(error) = first {
            if crate::api::error_code(&error).as_deref() != Some("conflict") {
                return Err(error);
            }
            self.detail = self.api.workspace(id).await?;
            self.write(self.patched(patch)).await?;
        }
        self.detail = self.api.workspace(id).await?;
        Ok(self.detail.clone())
    }

    fn patched(&self, patch: &GraphPatch) -> WorkspaceSnapshot {
        let graph = patch.onto(&self.detail.snapshot.graph);
        with_graph(self.detail.snapshot.clone(), graph)
    }

    async fn edit(&mut self, edit: Edit) -> anyhow::Result<WorkspaceDetail> {
        let id = self.detail.info.id;
        match edit {
            Edit::Graph(patch) => return self.save_graph(&patch).await,
            Edit::Refresh => {}
            Edit::Rack(rack) => {
                let snapshot = WorkspaceSnapshot {
                    rack: rack_grid::prune(&rack, &self.detail.snapshot.graph),
                    ..self.detail.snapshot.clone()
                };
                self.write(snapshot).await?;
            }
            Edit::Settings(settings) => {
                let snapshot = WorkspaceSnapshot {
                    settings,
                    ..self.detail.snapshot.clone()
                };
                self.write(snapshot).await?;
            }
            Edit::Rename(name) => {
                let body = UpdateWorkspaceRequest {
                    revision: self.detail.info.revision,
                    name: Some(name),
                    snapshot: None,
                };
                self.detail.info = self
                    .api
                    .put::<_, WorkspaceInfo>(&format!("/api/workspaces/{id}"), &body)
                    .await?;
            }
            Edit::History(back) => self.api.step_history(id, back).await?,
            Edit::Channel(set, channel, settings) => {
                self.api.patch_channel(set, channel, &settings).await?
            }
            Edit::Device(set, settings) => self.api.patch_device(set, &settings).await?,
            Edit::Saved(node, settings) => {
                self.api
                    .put::<_, serde::de::IgnoredAny>(
                        &format!("/api/workspaces/{id}/channels/{node}"),
                        &settings,
                    )
                    .await?;
            }
        }
        self.detail = self.api.workspace(id).await?;
        Ok(self.detail.clone())
    }

    async fn write(&mut self, snapshot: WorkspaceSnapshot) -> anyhow::Result<()> {
        snapshot.validate()?;
        let id = self.detail.info.id;
        self.detail.info = self
            .api
            .save_workspace(id, self.detail.info.revision, snapshot.clone())
            .await?;
        self.detail.snapshot = snapshot;
        Ok(())
    }
}

fn deliver(reply: Reply, result: anyhow::Result<WorkspaceDetail>) {
    if let Err(undelivered) = reply.send(result)
        && let Err(error) = undelivered
    {
        tracing::error!(%error, "workspace edit failed after its caller closed");
    }
}

fn folded(result: &anyhow::Result<WorkspaceDetail>) -> anyhow::Result<WorkspaceDetail> {
    match result {
        Ok(detail) => Ok(detail.clone()),
        Err(_) => Err(anyhow::Error::new(Folded)),
    }
}

fn with_graph(mut snapshot: WorkspaceSnapshot, graph: PatchGraph) -> WorkspaceSnapshot {
    snapshot.rack = rack_grid::prune(&snapshot.rack, &graph);
    snapshot.graph = graph;
    snapshot
}

#[cfg(test)]
mod tests;
