use std::{collections::HashMap, sync::Arc, time::Duration};

use sdrmm_engine::Engine;
use sdrmm_wire::{DecodedRecord, EventOutputTarget, ServerEvent, event_output::FeedStatus};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    sync::{broadcast, watch},
    task::{JoinHandle, JoinSet},
};

use super::Binding;

const MAX_CLIENTS: usize = 16;
const QUEUE_CAPACITY: usize = 256;

struct Entry {
    target: EventOutputTarget,
    format: String,
    frames: broadcast::Sender<Arc<[u8]>>,
    status: watch::Sender<FeedStatus>,
    worker: Option<JoinHandle<()>>,
}

impl Drop for Entry {
    fn drop(&mut self) {
        if let Some(worker) = &self.worker {
            worker.abort();
        }
    }
}

#[derive(Default)]
pub(super) struct Outputs {
    entries: HashMap<String, Entry>,
}

impl Outputs {
    pub fn configure(&mut self, bindings: &[Binding]) {
        self.entries.retain(|node, entry| {
            bindings
                .iter()
                .any(|binding| binding.node == *node && binding.target == entry.target)
        });
        for binding in bindings {
            let EventOutputTarget::Feed {
                format,
                address,
                enabled: true,
            } = &binding.target
            else {
                continue;
            };
            let heartbeat = sdrmm_wire::catalog()
                .feed(format)
                .map(|feed| Arc::<[u8]>::from(feed.heartbeat.as_slice()));
            if self.entries.contains_key(&binding.node) {
                continue;
            }
            let (frames, _) = broadcast::channel(QUEUE_CAPACITY);
            let (status, _) = watch::channel(FeedStatus {
                node: binding.node.clone(),
                format: format.clone(),
                address: address.clone(),
                ..FeedStatus::default()
            });
            let listener = std::net::TcpListener::bind(address).and_then(|listener| {
                listener.set_nonblocking(true)?;
                TcpListener::from_std(listener)
            });
            let worker = match (listener, heartbeat) {
                (_, None) => {
                    report(&status, format!("No decoder offers the {format} feed"));
                    None
                }
                (Ok(listener), Some(heartbeat)) => {
                    status.send_modify(|status| status.listening = true);
                    Some(tokio::spawn(serve(
                        listener,
                        frames.clone(),
                        status.clone(),
                        heartbeat,
                    )))
                }
                (Err(error), Some(_)) => {
                    report(&status, format!("Cannot listen on {address}: {error}"));
                    None
                }
            };
            self.entries.insert(
                binding.node.clone(),
                Entry {
                    target: binding.target.clone(),
                    format: format.clone(),
                    frames,
                    status,
                    worker,
                },
            );
        }
    }

    pub fn push(&self, record: &DecodedRecord) {
        let catalog = sdrmm_wire::catalog();
        for node in &record.sinks {
            let Some(entry) = self.entries.get(node) else {
                continue;
            };
            let Some(feed) = catalog.feed(&entry.format) else {
                continue;
            };
            if entry.frames.receiver_count() == 0 || !feed.kinds.contains(&record.event.kind) {
                continue;
            }
            match feed.encoder.encode(&record.event) {
                Ok(frame) => {
                    let _ = entry.frames.send(frame.into());
                }
                Err(error) => report(&entry.status, error),
            }
        }
    }

    pub fn lost(&self, count: u64) {
        for entry in self.entries.values() {
            report(&entry.status, format!("Lost {count} decoder events"));
        }
    }

    pub fn publish_status(&self, engine: &Engine) {
        for entry in self.entries.values() {
            engine.emit_event(ServerEvent::FeedStatus(entry.status.borrow().clone()));
        }
    }
}

fn report(status: &watch::Sender<FeedStatus>, error: String) {
    tracing::error!(output = %status.borrow().node, %error, "feed export failed");
    status.send_modify(|status| status.error = Some(error));
}

async fn serve(
    listener: TcpListener,
    frames: broadcast::Sender<Arc<[u8]>>,
    status: watch::Sender<FeedStatus>,
    heartbeat: Arc<[u8]>,
) {
    let mut clients = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    if clients.len() == MAX_CLIENTS {
                        report(&status, "feed client limit reached".to_owned());
                        continue;
                    }
                    if let Err(error) = stream.set_nodelay(true) {
                        report(&status, format!("Feed client setup failed: {error}"));
                        continue;
                    }
                    clients.spawn(write_client(
                        stream,
                        frames.subscribe(),
                        status.clone(),
                        heartbeat.clone(),
                    ));
                    status.send_modify(|status| status.clients = clients.len() as u32);
                }
                Err(error) => {
                    report(&status, format!("Feed accept failed: {error}"));
                    status.send_modify(|status| { status.listening = false; status.clients = 0; });
                    return;
                }
            },
            result = clients.join_next(), if !clients.is_empty() => {
                if let Some(Err(error)) = result { report(&status, format!("Feed client worker failed: {error}")); }
                status.send_modify(|status| status.clients = clients.len() as u32);
            }
        }
    }
}

async fn write_client(
    mut stream: TcpStream,
    mut frames: broadcast::Receiver<Arc<[u8]>>,
    status: watch::Sender<FeedStatus>,
    heartbeat: Arc<[u8]>,
) {
    let mut beat = tokio::time::interval(Duration::from_secs(15));
    beat.reset();
    loop {
        let (bytes, frame) = tokio::select! {
            received = frames.recv() => match received {
                Ok(bytes) => (bytes, true),
                Err(broadcast::error::RecvError::Lagged(count)) => {
                    report(&status, format!("Slow feed client disconnected: lost {count} frames"));
                    return;
                }
                Err(broadcast::error::RecvError::Closed) => return,
            },
            _ = beat.tick(), if !heartbeat.is_empty() => (heartbeat.clone(), false),
        };
        match tokio::time::timeout(Duration::from_secs(2), stream.write_all(&bytes)).await {
            Ok(Ok(())) => {
                if frame {
                    status.send_modify(|status| status.frames += 1);
                }
            }
            Ok(Err(error)) => {
                report(&status, format!("Feed client disconnected: {error}"));
                return;
            }
            Err(_) => {
                report(
                    &status,
                    "Slow feed client disconnected: write timed out".to_owned(),
                );
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_channels::AdsbMessage;
    use sdrmm_wire::{DecoderEvent, EventKind};
    use tokio::io::AsyncReadExt;

    use super::*;

    const WAIT: Duration = Duration::from_secs(2);

    fn adsb() -> DecoderEvent {
        DecoderEvent::of(&AdsbMessage {
            raw: "8D40621D58C382D690C8AC2863A7".to_owned(),
            timestamp_12mhz: Some(0x0102_0304_0506),
            signal_level: Some(128),
            ..AdsbMessage::default()
        })
    }

    fn record(event: DecoderEvent, sink: &str) -> DecodedRecord {
        DecodedRecord {
            origin: None,
            sinks: vec![sink.to_owned()],
            callsign: None,
            device_set: 1,
            channel: 2,
            at: "2026-09-22T00:00:00Z".to_owned(),
            freq_hz: 1_090_000_000.0,
            event,
            view: None,
        }
    }

    fn beast(address: String) -> EventOutputTarget {
        EventOutputTarget::Feed {
            format: "beast".to_owned(),
            address,
            enabled: true,
        }
    }

    async fn served(outputs: &mut Outputs) -> (std::net::SocketAddr, watch::Receiver<FeedStatus>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (frames, _) = broadcast::channel(QUEUE_CAPACITY);
        let (status, observed) = watch::channel(FeedStatus::default());
        let worker = tokio::spawn(serve(
            listener,
            frames.clone(),
            status.clone(),
            Arc::from([]),
        ));
        outputs.entries.insert(
            "feed".to_owned(),
            Entry {
                target: beast(address.to_string()),
                format: "beast".to_owned(),
                frames,
                status,
                worker: Some(worker),
            },
        );
        (address, observed)
    }

    fn error(outputs: &Outputs, node: &str) -> String {
        outputs.entries[node]
            .status
            .borrow()
            .error
            .clone()
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn a_feed_frames_only_its_kinds_from_records_sent_to_it() {
        sdrmm_channels::registry::init();
        let mut outputs = Outputs::default();
        let (address, mut observed) = served(&mut outputs).await;
        let mut first = TcpStream::connect(address).await.unwrap();
        let mut second = TcpStream::connect(address).await.unwrap();
        tokio::time::timeout(WAIT, observed.wait_for(|status| status.clients == 2))
            .await
            .unwrap()
            .unwrap();

        outputs.push(&record(adsb(), "elsewhere"));
        let other = DecoderEvent::tagged(EventKind::named("rtty"), &"CQ");
        outputs.push(&record(other, "feed"));
        outputs.push(&record(adsb(), "feed"));

        let feed = sdrmm_wire::catalog()
            .feed("beast")
            .cloned()
            .expect("beast feed");
        let expected = feed.encoder.encode(&adsb()).unwrap();
        for client in [&mut first, &mut second] {
            let mut bytes = vec![0; expected.len()];
            tokio::time::timeout(WAIT, client.read_exact(&mut bytes))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(bytes, expected);
            assert!(
                tokio::time::timeout(Duration::from_millis(30), client.read_u8())
                    .await
                    .is_err(),
                "only the one ADS-B frame went out"
            );
        }
    }

    #[tokio::test]
    async fn removing_a_feed_closes_its_clients() {
        sdrmm_channels::registry::init();
        let mut outputs = Outputs::default();
        let (address, mut observed) = served(&mut outputs).await;
        let mut client = TcpStream::connect(address).await.unwrap();
        tokio::time::timeout(WAIT, observed.wait_for(|status| status.clients == 1))
            .await
            .unwrap()
            .unwrap();

        outputs.configure(&[]);

        let closed = tokio::time::timeout(WAIT, client.read_u8()).await.unwrap();
        assert_eq!(
            closed.unwrap_err().kind(),
            std::io::ErrorKind::UnexpectedEof
        );
    }

    #[tokio::test]
    async fn a_lagged_client_is_dropped_with_a_visible_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (server, _) = listener.accept().await.unwrap();
        let (frames, receiver) = broadcast::channel(1);
        let (status, _) = watch::channel(FeedStatus::default());
        frames.send(Arc::from([1u8])).unwrap();
        frames.send(Arc::from([2u8])).unwrap();

        tokio::time::timeout(
            WAIT,
            write_client(server, receiver, status.clone(), Arc::from([])),
        )
        .await
        .unwrap();

        let error = status.borrow().error.clone().unwrap_or_default();
        assert!(error.contains("lost 1 frames"), "{error}");
        drop(client);
    }

    #[tokio::test]
    async fn a_bind_failure_and_lost_events_are_visible() {
        sdrmm_channels::registry::init();
        let busy = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut outputs = Outputs::default();
        outputs.configure(&[Binding {
            node: "busy".to_owned(),
            target: beast(busy.local_addr().unwrap().to_string()),
        }]);

        assert!(!outputs.entries["busy"].status.borrow().listening);
        assert!(error(&outputs, "busy").contains("Cannot listen"));
        outputs.lost(3);
        assert!(error(&outputs, "busy").contains("Lost 3"));
    }

    #[tokio::test]
    async fn a_format_no_decoder_offers_is_visible() {
        sdrmm_channels::registry::init();
        let mut outputs = Outputs::default();
        outputs.configure(&[Binding {
            node: "odd".to_owned(),
            target: EventOutputTarget::Feed {
                format: "nope".to_owned(),
                address: "127.0.0.1:0".to_owned(),
                enabled: true,
            },
        }]);

        assert!(!outputs.entries["odd"].status.borrow().listening);
        assert!(error(&outputs, "odd").contains("No decoder offers"));
    }
}
