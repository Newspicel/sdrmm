//! The real wire: `LiveKit` signalling and its str0m peers, driven on their own task.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use livekit_protocol::{
    JoinResponse, ParticipantUpdate, SessionDescription, SignalTarget, signal_request,
    signal_response,
};
use str0m::Candidate;
use str0m::change::{SdpAnswer, SdpOffer};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use url::Url;

use super::people::Me;
use super::wire::{Dial, Role, Wire, WireEvent};
use crate::matrix::focus::{LiveKitGrant, livekit_token};
use crate::matrix::livekit::peer::{OpusIn, OpusOut, Peer};
use crate::matrix::livekit::signal::{
    Roster, Signal, add_mic, candidate_of, leave, rtc_url, trickle_for,
};
use crate::matrix::membership::Focus;

/// Frames either way between the call and a connection, as on a link.
const QUEUE: usize = 64;
/// How long a leaving connection gets to tell the server.
const LEAVE_GRACE: Duration = Duration::from_secs(2);

fn lost() -> String {
    "Call lost".to_owned()
}

fn no_route() -> String {
    "No UDP route".to_owned()
}

/// Dials `LiveKit` through `lk-jwt-service` with a Matrix `OpenID` token.
pub(crate) struct LiveKit {
    pub(crate) client: matrix_sdk::Client,
    pub(crate) http: reqwest::Client,
    pub(crate) room: String,
    pub(crate) me: Me,
}

#[async_trait]
impl Dial for LiveKit {
    async fn grant(&self, focus: &Focus) -> Result<LiveKitGrant, String> {
        let openid = self
            .client
            .account()
            .request_openid_token()
            .await
            .map_err(|_| "Not signed in".to_owned())?;
        let Me { user, device } = &self.me;
        let member = self.me.member_id();
        livekit_token(
            &self.http, focus, &self.room, user, device, &member, &openid,
        )
        .await
    }

    async fn connect(&self, grant: &LiveKitGrant, role: Role) -> Result<Box<dyn Wire>, String> {
        let url = rtc_url(grant)?;
        let sfu = sfu_address(&url).await?;
        let (signal, join) = Signal::connect(&url).await?;
        let connection = Box::pin(Connection::open(signal, &join, sfu, role)).await?;
        Ok(Box::new(connection.spawn()))
    }
}

/// Where the SFU is, so a peer binds on the interface that routes there. IPv4 first, since
/// SFUs mostly offer IPv4 candidates and a peer binds one family.
pub(crate) async fn sfu_address(url: &Url) -> Result<SocketAddr, String> {
    let host = url.host_str().ok_or_else(no_route)?;
    let port = url.port_or_known_default().ok_or_else(no_route)?;
    let found: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| no_route())?
        .collect();
    found
        .iter()
        .find(|addr| addr.is_ipv4())
        .or_else(|| found.first())
        .copied()
        .ok_or_else(no_route)
}

fn description(kind: &str, sdp: String, id: u32) -> SessionDescription {
    SessionDescription {
        r#type: kind.to_owned(),
        sdp,
        id,
        ..Default::default()
    }
}

/// One signalling socket with its publisher and subscriber.
struct Connection {
    signal: Signal,
    roster: Roster,
    sfu: SocketAddr,
    publisher: Option<Peer>,
    subscriber: Option<Peer>,
    /// SFU candidates for the subscriber that came before its offer.
    early: Vec<Candidate>,
    mic: mpsc::Receiver<OpusOut>,
    mic_tx: mpsc::Sender<OpusOut>,
    /// The subscriber publishes nothing; this never yields.
    quiet: mpsc::Receiver<OpusOut>,
    _quiet: mpsc::Sender<OpusOut>,
    heard: mpsc::Receiver<OpusIn>,
    heard_tx: mpsc::Sender<OpusIn>,
    dropped: Arc<AtomicU64>,
    lost_here: u64,
}

/// The call's handle on a running connection.
pub(crate) struct LiveKitWire {
    mic: mpsc::Sender<OpusOut>,
    events: mpsc::Receiver<Result<WireEvent, String>>,
    leaving: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
    dropped: Arc<AtomicU64>,
}

#[async_trait]
impl Wire for LiveKitWire {
    fn publish(&mut self, frame: OpusOut) {
        if let Err(TrySendError::Full(_)) = self.mic.try_send(frame) {
            tracing::debug!("mic frame dropped");
        }
    }

    async fn next(&mut self) -> Result<WireEvent, String> {
        self.events.recv().await.unwrap_or_else(|| Err(lost()))
    }

    fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    async fn leave(&mut self) {
        if let Some(leaving) = self.leaving.take() {
            let _ = leaving.send(());
        }
        if let Some(task) = self.task.take()
            && tokio::time::timeout(LEAVE_GRACE, task).await.is_err()
        {
            tracing::debug!("LiveKit leave timed out");
        }
    }
}

async fn drive(
    peer: Option<&mut Peer>,
    out: &mut mpsc::Receiver<OpusOut>,
    heard: &mpsc::Sender<OpusIn>,
) -> Result<(), String> {
    match peer {
        Some(peer) => {
            peer.run(out, heard).await?;
            Err(lost())
        }
        None => std::future::pending().await,
    }
}

impl Connection {
    /// Seeds the roster from the join, then publishes our mic when we talk.
    async fn open(
        signal: Signal,
        join: &JoinResponse,
        sfu: SocketAddr,
        role: Role,
    ) -> Result<Self, String> {
        let mut roster = Roster::default();
        roster.apply(&ParticipantUpdate {
            participants: join.other_participants.clone(),
        });
        roster.joined();
        let (mic_tx, mic) = mpsc::channel(QUEUE);
        let (quiet_tx, quiet) = mpsc::channel(1);
        let (heard_tx, heard) = mpsc::channel(QUEUE);
        let mut connection = Self {
            signal,
            roster,
            sfu,
            publisher: None,
            subscriber: None,
            early: Vec::new(),
            mic,
            mic_tx,
            quiet,
            _quiet: quiet_tx,
            heard,
            heard_tx,
            dropped: Arc::new(AtomicU64::new(0)),
            lost_here: 0,
        };
        if let Role::Talk { encrypted } = role {
            connection.publish_mic(encrypted).await?;
        }
        Ok(connection)
    }

    async fn publish_mic(&mut self, encrypted: bool) -> Result<(), String> {
        self.signal
            .send(signal_request::Message::AddTrack(add_mic(encrypted)))
            .await?;
        let (peer, offer) = Peer::publisher(self.sfu).await?;
        let offer = description("offer", offer.to_sdp_string(), 0);
        self.signal
            .send(signal_request::Message::Offer(offer))
            .await?;
        self.trickle(&peer, SignalTarget::Publisher).await?;
        self.publisher = Some(peer);
        Ok(())
    }

    async fn trickle(&mut self, peer: &Peer, target: SignalTarget) -> Result<(), String> {
        let Some(mid) = peer.bundle() else {
            return Ok(());
        };
        for candidate in peer.local_candidates() {
            let trickle = trickle_for(&candidate, mid, target);
            self.signal
                .send(signal_request::Message::Trickle(trickle))
                .await?;
        }
        Ok(())
    }

    fn spawn(self) -> LiveKitWire {
        let (events_tx, events) = mpsc::channel(QUEUE);
        let (leaving, left) = oneshot::channel();
        let mic = self.mic_tx.clone();
        let dropped = Arc::clone(&self.dropped);
        let task = tokio::spawn(self.serve(events_tx, left));
        LiveKitWire {
            mic,
            events,
            leaving: Some(leaving),
            task: Some(task),
            dropped,
        }
    }

    /// Runs until told to leave, the call lets go, or the connection fails.
    async fn serve(
        mut self,
        events: mpsc::Sender<Result<WireEvent, String>>,
        mut left: oneshot::Receiver<()>,
    ) {
        match self.pump(&events, &mut left).await {
            Ok(()) => {
                let bye = signal_request::Message::Leave(leave());
                if let Err(reason) = self.signal.send(bye).await {
                    tracing::debug!(reason, "LiveKit leave not sent");
                }
            }
            Err(reason) => {
                let _ = events.send(Err(reason)).await;
            }
        }
    }

    async fn pump(
        &mut self,
        events: &mpsc::Sender<Result<WireEvent, String>>,
        left: &mut oneshot::Receiver<()>,
    ) -> Result<(), String> {
        loop {
            tokio::select! {
                message = self.signal.next() => self.on_signal(message?, events).await?,
                Some(frame) = self.heard.recv() => {
                    if !self.on_frame(frame, events) {
                        return Ok(());
                    }
                }
                ran = drive(self.publisher.as_mut(), &mut self.mic, &self.heard_tx) => ran?,
                ran = drive(self.subscriber.as_mut(), &mut self.quiet, &self.heard_tx) => ran?,
                _ = &mut *left => return Ok(()),
            }
            self.count_drops();
        }
    }

    fn count_drops(&self) {
        let peers = [&self.publisher, &self.subscriber]
            .into_iter()
            .flatten()
            .map(Peer::dropped)
            .sum::<u64>();
        self.dropped
            .store(peers + self.lost_here, Ordering::Relaxed);
    }

    /// Hands a frame to the call; false once the call is gone.
    fn on_frame(
        &mut self,
        frame: OpusIn,
        events: &mpsc::Sender<Result<WireEvent, String>>,
    ) -> bool {
        let identity = frame.track.as_deref().map_or_else(
            || format!("mid {}", frame.mid),
            |track| {
                self.roster
                    .identity_of_track(track)
                    .map_or_else(|| format!("track {track}"), str::to_owned)
            },
        );
        let heard = WireEvent::Heard {
            identity,
            payload: frame.payload,
        };
        match events.try_send(Ok(heard)) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.lost_here += 1;
                true
            }
            Err(TrySendError::Closed(_)) => false,
        }
    }

    async fn on_signal(
        &mut self,
        message: signal_response::Message,
        events: &mpsc::Sender<Result<WireEvent, String>>,
    ) -> Result<(), String> {
        match message {
            signal_response::Message::Answer(answer) => self.on_answer(&answer),
            signal_response::Message::Offer(offer) => self.on_offer(offer).await,
            signal_response::Message::Trickle(trickle) => {
                if let Some(candidate) = candidate_of(&trickle)? {
                    self.on_candidate(trickle.target, candidate);
                }
                Ok(())
            }
            signal_response::Message::Update(update) => {
                self.roster.apply(&update);
                self.roster.joined();
                for identity in self.roster.left() {
                    let _ = events.try_send(Ok(WireEvent::Left(identity)));
                }
                Ok(())
            }
            signal_response::Message::Leave(_) => Err("Call ended".to_owned()),
            _ => Ok(()),
        }
    }

    fn on_answer(&mut self, answer: &SessionDescription) -> Result<(), String> {
        let answer =
            SdpAnswer::from_sdp_string(&answer.sdp).map_err(|_| "Bad answer".to_owned())?;
        self.publisher
            .as_mut()
            .ok_or_else(lost)?
            .accept_answer(answer)
    }

    /// The SFU offers what we may hear: the first offer makes the subscriber, later ones
    /// renegotiate it.
    async fn on_offer(&mut self, offer: SessionDescription) -> Result<(), String> {
        let id = offer.id;
        let sdp = SdpOffer::from_sdp_string(&offer.sdp).map_err(|_| "Bad offer".to_owned())?;
        let answer = if let Some(subscriber) = self.subscriber.as_mut() {
            subscriber.accept_offer(sdp)?
        } else {
            let (mut subscriber, answer) = Peer::subscriber(self.sfu, sdp).await?;
            for candidate in self.early.drain(..) {
                subscriber.add_remote_candidate(candidate);
            }
            let reply = description("answer", answer.to_sdp_string(), id);
            self.signal
                .send(signal_request::Message::Answer(reply))
                .await?;
            self.trickle(&subscriber, SignalTarget::Subscriber).await?;
            self.subscriber = Some(subscriber);
            return Ok(());
        };
        let reply = description("answer", answer.to_sdp_string(), id);
        self.signal
            .send(signal_request::Message::Answer(reply))
            .await
    }

    fn on_candidate(&mut self, target: i32, candidate: Candidate) {
        let peer = if target == SignalTarget::Publisher as i32 {
            self.publisher.as_mut()
        } else {
            self.subscriber.as_mut()
        };
        match peer {
            Some(peer) => peer.add_remote_candidate(candidate),
            None if target == SignalTarget::Subscriber as i32 => self.early.push(candidate),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_sfu_is_where_the_signal_url_points() {
        let url = Url::parse("ws://127.0.0.1:7880/rtc").unwrap();
        assert_eq!(
            sfu_address(&url).await.unwrap(),
            "127.0.0.1:7880".parse::<SocketAddr>().unwrap()
        );
        let tls = Url::parse("wss://127.0.0.1/rtc").unwrap();
        assert_eq!(sfu_address(&tls).await.unwrap().port(), 443);
    }
}
