use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{body::Bytes, extract::ws::Message};
use sdrmm_wire::FrameKind;
use tokio::sync::Notify;
use wtransport::{Connection, SendStream, VarInt};

use super::super::outbox::{Output, WRITE_TIMEOUT};

const LANE_IDLE: Duration = Duration::from_secs(5);
const CONTROL_PRIORITY: i32 = 2;
const AUDIO_PRIORITY: i32 = 1;
const MEDIA_PRIORITY: i32 = 0;
const CLOSE_STALLED: u32 = 2;
const KEY_BYTES: usize = 4;

/// Drains a session's outbox onto the connection: events in order on the control stream, audio
/// as datagrams, and every other stream of frames on a lane of its own, so a lost packet holds
/// back only the stream it belonged to.
pub(super) async fn write(connection: Connection, mut control: SendStream, mut output: Output) {
    control.set_priority(CONTROL_PRIORITY);
    let mut lanes = Lanes::new(connection.clone());
    while let Some(message) = output.recv().await {
        match message {
            Message::Text(text) => {
                if !framed(&mut control, text.as_bytes()).await {
                    return;
                }
            }
            Message::Binary(frame) => output.dropped(lanes.carry(frame)),
            Message::Close(close) => {
                let (code, reason) = close.map_or((0, String::new()), |close| {
                    (u32::from(close.code), close.reason.to_string())
                });
                connection.close(VarInt::from_u32(code), reason.as_bytes());
                return;
            }
            Message::Ping(_) | Message::Pong(_) => {}
        }
    }
    let _ = control.finish().await;
}

async fn framed(stream: &mut SendStream, message: &[u8]) -> bool {
    let Ok(length) = u32::try_from(message.len()) else {
        return false;
    };
    let written = tokio::time::timeout(WRITE_TIMEOUT, async {
        stream.write_all(&length.to_le_bytes()).await?;
        stream.write_all(message).await
    })
    .await;
    matches!(written, Ok(Ok(())))
}

struct Lanes {
    connection: Connection,
    open: HashMap<(u8, u16), Lane>,
}

impl Lanes {
    fn new(connection: Connection) -> Self {
        Self {
            connection,
            open: HashMap::new(),
        }
    }

    /// Returns how many frames this one displaced or could not be routed.
    fn carry(&mut self, frame: Bytes) -> u64 {
        let Some(key) = key(&frame) else {
            return 1;
        };
        let audio = key.0 == FrameKind::AudioOpus as u8;
        if audio && self.connection.send_datagram(&frame).is_ok() {
            return 0;
        }
        let priority = if audio {
            AUDIO_PRIORITY
        } else {
            MEDIA_PRIORITY
        };
        if !self.open.contains_key(&key) {
            self.open.retain(|_, lane| !lane.task.is_finished());
        }
        let lane = self
            .open
            .entry(key)
            .or_insert_with(|| Lane::open(&self.connection, priority));
        match lane.slot.put(frame) {
            Ok(displaced) => u64::from(displaced),
            Err(frame) => {
                *lane = Lane::open(&self.connection, priority);
                u64::from(lane.slot.put(frame).is_err())
            }
        }
    }
}

fn key(frame: &[u8]) -> Option<(u8, u16)> {
    let header = frame.get(..KEY_BYTES)?;
    Some((header[1], u16::from_le_bytes([header[2], header[3]])))
}

struct Lane {
    slot: Arc<Slot>,
    task: tokio::task::JoinHandle<()>,
}

impl Lane {
    fn open(connection: &Connection, priority: i32) -> Self {
        let slot = Arc::new(Slot::default());
        Self {
            task: tokio::spawn(run(connection.clone(), slot.clone(), priority)),
            slot,
        }
    }
}

impl Drop for Lane {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn run(connection: Connection, slot: Arc<Slot>, priority: i32) {
    let Some(mut stream) = open(&connection).await else {
        slot.close();
        return;
    };
    stream.set_priority(priority);
    loop {
        let frame = match tokio::time::timeout(LANE_IDLE, slot.take()).await {
            Ok(frame) => frame,
            Err(_) if slot.close_if_empty() => break,
            Err(_) => continue,
        };
        if !framed(&mut stream, &frame).await {
            slot.close();
            connection.close(VarInt::from_u32(CLOSE_STALLED), b"stalled");
            return;
        }
    }
    let _ = stream.finish().await;
}

async fn open(connection: &Connection) -> Option<SendStream> {
    connection.open_uni().await.ok()?.await.ok()
}

/// The one frame a lane has yet to send. A newer frame displaces it, so a lane that cannot keep
/// up skips ahead instead of falling behind.
#[derive(Default)]
struct Slot {
    held: Mutex<Held>,
    ready: Notify,
}

#[derive(Default)]
struct Held {
    frame: Option<Bytes>,
    closed: bool,
}

impl Slot {
    fn held(&self) -> std::sync::MutexGuard<'_, Held> {
        self.held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Says whether an unsent frame was displaced, or hands the frame back from a closed lane.
    fn put(&self, frame: Bytes) -> Result<bool, Bytes> {
        let mut held = self.held();
        if held.closed {
            return Err(frame);
        }
        let displaced = held.frame.replace(frame).is_some();
        drop(held);
        self.ready.notify_one();
        Ok(displaced)
    }

    async fn take(&self) -> Bytes {
        loop {
            let ready = self.ready.notified();
            if let Some(frame) = self.held().frame.take() {
                return frame;
            }
            ready.await;
        }
    }

    fn close(&self) {
        self.held().closed = true;
    }

    fn close_if_empty(&self) -> bool {
        let mut held = self.held();
        held.closed = held.frame.is_none();
        held.closed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(value: u8) -> Bytes {
        Bytes::from(vec![1, FrameKind::Spectrum as u8, 7, 0, value])
    }

    #[tokio::test]
    async fn a_newer_frame_displaces_the_one_not_yet_sent() {
        let slot = Slot::default();
        assert_eq!(slot.put(frame(1)), Ok(false));
        assert_eq!(slot.put(frame(2)), Ok(true));
        assert_eq!(slot.take().await, frame(2));
    }

    #[tokio::test]
    async fn an_idle_lane_closes_only_while_it_holds_nothing() {
        let slot = Slot::default();
        slot.put(frame(1)).expect("open");
        assert!(!slot.close_if_empty());
        assert_eq!(slot.take().await, frame(1));
        assert!(slot.close_if_empty());
        assert_eq!(slot.put(frame(2)), Err(frame(2)));
    }

    #[test]
    fn a_frame_names_its_kind_and_stream() {
        assert_eq!(key(&frame(0)), Some((FrameKind::Spectrum as u8, 7)));
        assert_eq!(key(&[1, 2]), None);
    }
}
