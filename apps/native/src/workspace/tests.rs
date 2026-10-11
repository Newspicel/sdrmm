use std::sync::Arc;

use sdrmm_engine::Engine;
use sdrmm_wire::patch::{NodeBody, PatchNode, Position, RackCell, RackSlot};

use sdrmm_wire::channel::{ChannelParams, ChannelSettings, NfmParams, NfmToneMode};

use super::*;

struct Server {
    api: Api,
    engine: Arc<Engine>,
    _handle: sdrmm_server::ServerHandle,
}

impl Server {
    async fn new() -> Self {
        let mut registry = sdrmm_device::DeviceRegistry::new();
        registry.register(0, Box::new(sdrmm_device_virtual::VirtualDriver::new()));
        let engine = Engine::with_registry(registry, None);
        let config = sdrmm_server::Config {
            bind: std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 0)),
            ..sdrmm_server::Config::default()
        };
        let handle = sdrmm_server::serve(config, engine.clone())
            .await
            .expect("server");
        Self {
            api: Api::new(format!("http://{}", handle.local_addr))
                .expect("client")
                .with_author(String::from("native-test")),
            engine,
            _handle: handle,
        }
    }

    async fn workspace(&self) -> WorkspaceDetail {
        let id = self
            .api
            .create_workspace("Native test")
            .await
            .expect("create workspace");
        let detail = self.api.workspace(id).await.expect("workspace");
        let mut snapshot = WorkspaceSnapshot::empty();
        snapshot.graph.nodes.push(node("scope"));
        snapshot.settings.band_region = Some("iaru-1".into());
        snapshot.settings.band_ruler = false;
        snapshot.rack.slots.push(RackSlot {
            node: "scope".into(),
            cell: RackCell {
                x: 0,
                y: 0,
                w: 4,
                h: 3,
            },
        });
        self.api
            .save_workspace(id, detail.info.revision, snapshot)
            .await
            .expect("seed");
        self.api.workspace(id).await.expect("seeded workspace")
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.engine.shutdown();
    }
}

fn node(id: &str) -> PatchNode {
    PatchNode {
        id: id.into(),
        body: NodeBody::Scope,
        position: Position { x: 0.0, y: 0.0 },
        size: None,
        label: None,
    }
}

#[tokio::test]
async fn queued_edits_preserve_settings_and_rack_and_undo_in_order() {
    let server = Server::new().await;
    let detail = server.workspace().await;
    let (session, worker) = Session::new(server.api.clone(), detail.clone());
    let task = tokio::spawn(worker.run());
    let mut graph = detail.snapshot.graph.clone();
    graph.nodes[0].position.x = 123.0;
    let first = session.save(GraphPatch::between(&detail.snapshot.graph, &graph));
    let moved = graph.clone();
    graph.nodes.push(node("scope2"));
    let second = session.save(GraphPatch::between(&moved, &graph));
    let undo = session.step(true);
    let redo = session.step(false);
    let (first, second, undo, redo) = tokio::join!(first, second, undo, redo);
    let second = second.expect("second save");
    let first = first.expect("first save");
    assert!(first.info.revision > detail.info.revision);
    let undo = undo.expect("undo");
    let redo = redo.expect("redo");
    assert!(second.info.revision > detail.info.revision);
    assert_eq!(second.snapshot.graph, graph);
    assert_eq!(redo.snapshot, second.snapshot);
    assert_ne!(undo.snapshot.graph, second.snapshot.graph);
    assert_eq!(redo.snapshot.settings, detail.snapshot.settings);
    assert_eq!(redo.snapshot.rack, detail.snapshot.rack);
    let before = graph.clone();
    graph.nodes.retain(|node| node.id != "scope");
    let removed = session
        .save(GraphPatch::between(&before, &graph))
        .await
        .expect("remove racked node");
    assert!(removed.snapshot.rack.slots.is_empty());
    assert_eq!(removed.snapshot.settings, detail.snapshot.settings);
    drop(session);
    task.await.expect("writer shutdown");
}

#[tokio::test]
async fn a_settings_edit_keeps_the_graph_and_rack() {
    let server = Server::new().await;
    let detail = server.workspace().await;
    let (session, worker) = Session::new(server.api.clone(), detail.clone());
    let task = tokio::spawn(worker.run());
    let mut settings = detail.snapshot.settings.clone();
    settings.band_ruler = true;
    let saved = session
        .settings(settings.clone())
        .await
        .expect("save settings");
    assert_eq!(saved.snapshot.settings, settings);
    assert_eq!(saved.snapshot.graph, detail.snapshot.graph);
    assert_eq!(saved.snapshot.rack, detail.snapshot.rack);
    drop(session);
    task.await.expect("writer shutdown");
}

#[tokio::test]
async fn an_external_edit_survives_a_save_from_an_older_copy() {
    let server = Server::new().await;
    let detail = server.workspace().await;
    let (session, worker) = Session::new(server.api.clone(), detail.clone());
    let task = tokio::spawn(worker.run());
    let mut external = detail.snapshot.clone();
    external.settings.band_ruler = true;
    server
        .api
        .save_workspace(detail.info.id, detail.info.revision, external.clone())
        .await
        .expect("external edit");
    let mut graph = detail.snapshot.graph.clone();
    graph.nodes[0].position.x = 321.0;
    let merged = session
        .save(GraphPatch::between(&detail.snapshot.graph, &graph))
        .await
        .expect("merged save");
    assert_eq!(merged.snapshot.graph, graph);
    assert_eq!(merged.snapshot.settings, external.settings);
    drop(session);
    task.await.expect("writer shutdown");
}

#[tokio::test]
async fn a_peer_graph_edit_survives_my_edit_and_reaches_me_on_refresh() {
    let server = Server::new().await;
    let detail = server.workspace().await;
    let (session, worker) = Session::new(server.api.clone(), detail.clone());
    let task = tokio::spawn(worker.run());
    let mut theirs = detail.snapshot.clone();
    theirs.graph.nodes.push(node("peer"));
    server
        .api
        .save_workspace(detail.info.id, detail.info.revision, theirs)
        .await
        .expect("peer edit");
    let refreshed = session.refresh().await.expect("refresh");
    assert!(refreshed.snapshot.graph.node("peer").is_some());
    let mut mine = detail.snapshot.graph.clone();
    mine.nodes.push(node("mine"));
    let saved = session
        .save(GraphPatch::between(&detail.snapshot.graph, &mine))
        .await
        .expect("my edit");
    let ids: Vec<&str> = saved
        .snapshot
        .graph
        .nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect();
    assert_eq!(ids, ["scope", "peer", "mine"]);
    drop(session);
    task.await.expect("writer shutdown");
}

#[tokio::test]
async fn a_burst_of_graph_edits_is_saved_as_one_revision() {
    let server = Server::new().await;
    let detail = server.workspace().await;
    let (session, worker) = Session::new(server.api.clone(), detail.clone());
    let mut before = detail.snapshot.graph.clone();
    let mut pending = Vec::new();
    for at in 0..3 {
        let mut after = before.clone();
        after.nodes.push(node(&format!("n{at}")));
        pending.push(session.save(GraphPatch::between(&before, &after)));
        before = after;
    }
    let task = tokio::spawn(worker.run());
    let results = futures::future::join_all(pending).await;
    let folded = results
        .iter()
        .filter(|result| result.as_ref().is_err_and(|error| error.is::<Folded>()))
        .count();
    assert_eq!(folded, 0);
    let last = results
        .into_iter()
        .last()
        .expect("three replies")
        .expect("saved");
    assert_eq!(last.snapshot.graph, before);
    assert_eq!(last.info.revision, detail.info.revision + 1);
    drop(session);
    task.await.expect("writer shutdown");
}

#[tokio::test]
async fn decoder_controls_change_the_virtual_radio_through_the_native_client() {
    let server = Server::new().await;
    let detail = server.workspace().await;
    let id = detail.info.id;
    let graph = crate::starter::graph(&server.api.devices().await.expect("virtual devices"));
    let detail_graph = detail.snapshot.graph.clone();
    let (session, worker) = Session::new(server.api.clone(), detail);
    let task = tokio::spawn(worker.run());
    session
        .save(GraphPatch::between(&detail_graph, &graph))
        .await
        .expect("save starter patch");
    server.api.activate_workspace(id).await.expect("activate");
    let report = server.api.apply_workspace(id).await.expect("apply");
    assert!(report.refused.is_empty(), "{:?}", report.refused);
    let state = server.api.state().await.expect("state");
    let set = state.device_sets.first().expect("device set");
    assert_eq!(set.device.driver, "virtual");
    let channel = set.channels.first().expect("channel");
    let mut settings = channel.settings.clone();
    let edit = |settings: &mut ChannelSettings, change: fn(&mut NfmParams)| {
        if let ChannelParams::Nfm(params) = &mut settings.params {
            change(params);
        }
    };
    edit(&mut settings, |params| params.bandwidth_hz = 25_000.0);
    let first = session.channel(set.id, channel.id, settings.clone());
    edit(&mut settings, |params| params.compander = true);
    let second = session.channel(set.id, channel.id, settings.clone());
    first.await.expect("first edit");
    second.await.expect("second edit");
    edit(&mut settings, |params| {
        params.tone_mode = NfmToneMode::Ctcss
    });
    session
        .channel(set.id, channel.id, settings.clone())
        .await
        .expect("enable tone squelch");
    let current = server.api.state().await.expect("updated state");
    assert_eq!(current.device_sets[0].channels[0].settings, settings);
    drop(session);
    task.await.expect("writer shutdown");
}
