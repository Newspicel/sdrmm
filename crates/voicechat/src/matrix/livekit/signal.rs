//! One `LiveKit` signalling socket: protobuf frames, keepalive, roster and trickle.

use std::collections::HashMap;
use std::mem;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use livekit_protocol::ParticipantUpdate;
use livekit_protocol::{
    AddTrackRequest, DisconnectReason, JoinResponse, LeaveRequest, Ping, SignalRequest,
    SignalResponse, SignalTarget, TrackSource, TrackType, TrickleRequest, encryption,
    leave_request, participant_info::State, signal_request, signal_response,
};
use prost::Message as _;
use serde::Deserialize;
use serde_json::json;
use str0m::Candidate;
use str0m::media::Mid;
use tokio::net::TcpStream;
use tokio::time::{Instant, sleep_until, timeout};
use tokio_tungstenite::tungstenite::{self, Message as Frame};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use url::Url;

use crate::matrix::focus::LiveKitGrant;

/// The signal protocol version `livekit-signaling` 0.3 sends.
const PROTOCOL_VERSION: u32 = 18;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_PING_INTERVAL: Duration = Duration::from_secs(5);
const DEFAULT_PING_TIMEOUT: Duration = Duration::from_secs(15);
/// Our mic track id, both in `AddTrackRequest.cid` and the publisher's msid.
pub(crate) const MIC_CID: &str = "mic";
const MIC_NAME: &str = "microphone";

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

fn bad_url() -> String {
    "Bad call server".to_owned()
}

fn lost() -> String {
    "Call lost".to_owned()
}

/// The signalling URL for a grant. It carries the token, so it is never logged.
pub(crate) fn rtc_url(grant: &LiveKitGrant) -> Result<Url, String> {
    let mut url = Url::parse(&grant.url).map_err(|_| bad_url())?;
    let scheme = match url.scheme() {
        "https" | "wss" => "wss",
        "http" | "ws" => "ws",
        _ => return Err(bad_url()),
    };
    url.set_scheme(scheme).map_err(|()| bad_url())?;
    url.path_segments_mut()
        .map_err(|()| bad_url())?
        .pop_if_empty()
        .push("rtc");
    url.query_pairs_mut()
        .clear()
        .append_pair("access_token", &grant.jwt)
        .append_pair("auto_subscribe", "1")
        .append_pair("sdk", "rust")
        .append_pair("protocol", &PROTOCOL_VERSION.to_string())
        .append_pair("adaptive_stream", "0");
    Ok(url)
}

/// When to ping and when a missing pong means the server is gone.
struct Pulse {
    every: Duration,
    patience: Duration,
    next_ping: Instant,
    deadline: Option<Instant>,
}

impl Pulse {
    fn new(interval_s: i32, timeout_s: i32) -> Self {
        let seconds = |value: i32, fallback| match u64::try_from(value) {
            Ok(secs) if secs > 0 => Duration::from_secs(secs),
            _ => fallback,
        };
        let every = seconds(interval_s, DEFAULT_PING_INTERVAL);
        Self {
            every,
            patience: seconds(timeout_s, DEFAULT_PING_TIMEOUT),
            next_ping: Instant::now() + every,
            deadline: None,
        }
    }

    fn wake(&self) -> Instant {
        self.deadline
            .map_or(self.next_ping, |d| d.min(self.next_ping))
    }

    fn answered(&mut self) {
        self.deadline = None;
    }

    fn expired(&self, now: Instant) -> bool {
        self.deadline.is_some_and(|deadline| now >= deadline)
    }

    fn ping_due(&mut self, now: Instant) -> bool {
        if now < self.next_ping {
            return false;
        }
        self.next_ping += self.every;
        self.deadline.get_or_insert(now + self.patience);
        true
    }
}

/// A `LiveKit` signalling socket.
pub(crate) struct Signal {
    socket: Socket,
    pulse: Pulse,
}

fn refused_or_missing(error: &tungstenite::Error) -> String {
    match error {
        tungstenite::Error::Http(response) if matches!(response.status().as_u16(), 401 | 403) => {
            "Call server refused".to_owned()
        }
        _ => "No call server".to_owned(),
    }
}

impl Signal {
    /// Opens the socket and waits for the join response.
    pub(crate) async fn connect(url: &Url) -> Result<(Self, JoinResponse), String> {
        crate::install_tls_provider();
        timeout(CONNECT_TIMEOUT, Self::open(url))
            .await
            .map_err(|_| "No call server".to_owned())?
    }

    async fn open(url: &Url) -> Result<(Self, JoinResponse), String> {
        let (socket, _) = tokio_tungstenite::connect_async(url.as_str())
            .await
            .map_err(|error| refused_or_missing(&error))?;
        let mut signal = Self {
            socket,
            pulse: Pulse::new(0, 0),
        };
        let join = signal.read_join().await?;
        signal.pulse = Pulse::new(join.ping_interval, join.ping_timeout);
        Ok((signal, join))
    }

    async fn read_join(&mut self) -> Result<JoinResponse, String> {
        loop {
            match self.next().await? {
                signal_response::Message::Join(join) => return Ok(join),
                signal_response::Message::Leave(_) => return Err("Call refused".to_owned()),
                _ => {}
            }
        }
    }

    pub(crate) async fn send(&mut self, request: signal_request::Message) -> Result<(), String> {
        let bytes = SignalRequest {
            message: Some(request),
        }
        .encode_to_vec();
        self.socket
            .send(Frame::Binary(bytes.into()))
            .await
            .map_err(|_| lost())
    }

    /// The next server message, pinging on schedule. Safe to cancel.
    pub(crate) async fn next(&mut self) -> Result<signal_response::Message, String> {
        loop {
            let wake = self.pulse.wake();
            tokio::select! {
                frame = self.socket.next() => {
                    if let Some(message) = self.on_frame(frame)? {
                        return Ok(message);
                    }
                }
                () = sleep_until(wake) => self.tick().await?,
            }
        }
    }

    fn on_frame(
        &mut self,
        frame: Option<Result<Frame, tungstenite::Error>>,
    ) -> Result<Option<signal_response::Message>, String> {
        match frame.ok_or_else(lost)?.map_err(|_| lost())? {
            Frame::Binary(bytes) => Ok(self.on_response(&bytes)),
            Frame::Close(_) => Err("Call closed".to_owned()),
            _ => Ok(None),
        }
    }

    fn on_response(&mut self, bytes: &[u8]) -> Option<signal_response::Message> {
        let message = match SignalResponse::decode(bytes) {
            Ok(response) => response.message?,
            Err(error) => {
                tracing::warn!(%error, "undecodable signalling frame dropped");
                return None;
            }
        };
        if matches!(
            message,
            signal_response::Message::Pong(_) | signal_response::Message::PongResp(_)
        ) {
            self.pulse.answered();
            return None;
        }
        Some(message)
    }

    async fn tick(&mut self) -> Result<(), String> {
        let now = Instant::now();
        if self.pulse.expired(now) {
            return Err("Call timed out".to_owned());
        }
        if self.pulse.ping_due(now) {
            let ping = Ping {
                timestamp: unix_ms(),
                rtt: 0,
            };
            self.send(signal_request::Message::PingReq(ping)).await?;
        }
        Ok(())
    }
}

fn unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
        .unwrap_or_default()
}

/// The microphone track to publish.
pub(crate) fn add_mic(encrypted: bool) -> AddTrackRequest {
    let encryption = if encrypted {
        encryption::Type::Gcm
    } else {
        encryption::Type::None
    };
    AddTrackRequest {
        cid: MIC_CID.to_owned(),
        name: MIC_NAME.to_owned(),
        r#type: TrackType::Audio as i32,
        source: TrackSource::Microphone as i32,
        encryption: encryption as i32,
        ..Default::default()
    }
}

/// A client-initiated leave that asks for no reconnect.
pub(crate) fn leave() -> LeaveRequest {
    LeaveRequest {
        can_reconnect: false,
        reason: DisconnectReason::ClientInitiated as i32,
        action: leave_request::Action::Disconnect as i32,
        regions: None,
    }
}

/// Wraps a local ICE candidate for the server. `mid` is the first media section of our
/// description, the one index 0 names and every bundled section shares.
pub(crate) fn trickle_for(candidate: &Candidate, mid: Mid, target: SignalTarget) -> TrickleRequest {
    let init = json!({
        "candidate": candidate.to_sdp_string(),
        "sdpMid": mid.to_string(),
        "sdpMLineIndex": 0,
    });
    TrickleRequest {
        candidate_init: init.to_string(),
        target: target as i32,
        r#final: false,
    }
}

#[derive(Deserialize)]
struct CandidateInit {
    candidate: String,
}

/// The remote candidate in a trickle, or `None` for the end-of-candidates marker.
pub(crate) fn candidate_of(trickle: &TrickleRequest) -> Result<Option<Candidate>, String> {
    let init: CandidateInit =
        serde_json::from_str(&trickle.candidate_init).map_err(|_| "Bad candidate".to_owned())?;
    if init.candidate.is_empty() {
        return Ok(None);
    }
    Candidate::from_sdp_string(&init.candidate)
        .map(Some)
        .map_err(|_| "Bad candidate".to_owned())
}

/// Who is in the room, and which track belongs to whom. A device that reconnects
/// holds a new participant sid under the same identity until the old one is dropped.
#[derive(Default)]
pub(crate) struct Roster {
    identities: HashMap<String, String>,
    tracks: HashMap<String, String>,
    joined: Vec<String>,
    left: Vec<String>,
}

impl Roster {
    pub(crate) fn apply(&mut self, update: &ParticipantUpdate) {
        for participant in &update.participants {
            if participant.state == State::Disconnected as i32 {
                self.remove(&participant.sid);
            } else {
                self.upsert(participant);
            }
        }
    }

    fn remove(&mut self, sid: &str) {
        let Some(identity) = self.identities.remove(sid) else {
            return;
        };
        self.tracks.retain(|_, owner| owner != sid);
        if !self.identities.values().any(|other| *other == identity) {
            self.left.push(identity);
        }
    }

    fn upsert(&mut self, participant: &livekit_protocol::ParticipantInfo) {
        let identity = &participant.identity;
        let present = self.identities.values().any(|other| other == identity);
        self.identities
            .insert(participant.sid.clone(), identity.clone());
        if !present {
            self.joined.push(identity.clone());
        }
        self.tracks.retain(|_, owner| *owner != participant.sid);
        for track in &participant.tracks {
            self.tracks
                .insert(track.sid.clone(), participant.sid.clone());
        }
    }

    pub(crate) fn identity_of_track(&self, track_sid: &str) -> Option<&str> {
        let owner = self.tracks.get(track_sid)?;
        self.identities.get(owner).map(String::as_str)
    }

    /// Identities that left since the last call.
    pub(crate) fn left(&mut self) -> Vec<String> {
        mem::take(&mut self.left)
    }

    /// Identities that joined since the last call.
    pub(crate) fn joined(&mut self) -> Vec<String> {
        mem::take(&mut self.joined)
    }
}

#[cfg(test)]
mod tests;
