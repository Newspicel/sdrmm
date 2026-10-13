use std::{collections::HashMap, sync::Arc};

use axum::extract::ws::{Message, Utf8Bytes};
use sdrmm_wire::ServerEvent;
use tokio::{
    sync::broadcast::{self, error::RecvError},
    task::JoinHandle,
};

use super::{Outbox, Session, text_event};
use crate::sinks::SinkHub;

const NOT_A_SINK: &str = "Not a sink";

#[derive(Default)]
pub(super) struct Sinks {
    watching: HashMap<String, JoinHandle<()>>,
}

impl Sinks {
    fn stop(&mut self, node: &str) {
        if let Some(task) = self.watching.remove(node) {
            task.abort();
        }
    }

    pub(super) fn abort(self) {
        for task in self.watching.into_values() {
            task.abort();
        }
    }
}

impl Session {
    pub(super) fn subscribe_sink(&mut self, node: String) {
        self.sinks.stop(&node);
        let task = spawn(node.clone(), self.out.clone(), self.state.sinks.clone());
        self.sinks.watching.insert(node, task);
    }

    pub(super) fn unsubscribe_sink(&mut self, node: &str) {
        self.sinks.stop(node);
    }
}

fn spawn(node: String, out: Outbox, hub: Arc<SinkHub>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let Some((snapshot, updates)) = hub.watch(&node).await else {
            let refused = ServerEvent::Error {
                message: NOT_A_SINK.to_owned(),
            };
            let _ = out.send(text_event(&refused)).await;
            return;
        };
        forward(&node, snapshot, updates, &out, &hub).await;
    })
}

async fn forward(
    node: &str,
    snapshot: Utf8Bytes,
    mut updates: broadcast::Receiver<Utf8Bytes>,
    out: &Outbox,
    hub: &SinkHub,
) {
    if out.send(Message::Text(snapshot)).await.is_err() {
        return;
    }
    loop {
        let text = match updates.recv().await {
            Ok(text) => text,
            Err(RecvError::Lagged(_)) => match hub.snapshot(node) {
                Some(fresh) => fresh,
                None => return,
            },
            Err(RecvError::Closed) => return,
        };
        if out.send(Message::Text(text)).await.is_err() {
            return;
        }
    }
}
