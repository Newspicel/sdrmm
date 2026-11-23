//! One WebRTC peer connection to the `LiveKit` SFU, driven by str0m over UDP, or TCP where
//! UDP is shut.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Once;
use std::time::{Duration, Instant};

use str0m::change::{SdpAnswer, SdpOffer, SdpPendingOffer};
use str0m::channel::{ChannelConfig, Reliability};
use str0m::format::{Codec, PayloadParams};
use str0m::media::{Direction, Frequency, MediaData, MediaKind, MediaTime, Mid};
use str0m::net::{Protocol, Receive, TcpType};
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc, RtcError};
use tokio::net::UdpSocket;
use tokio::sync::mpsc::{self, error::TrySendError};
use tokio::time::sleep_until;

use super::signal::MIC_CID;
use super::tcp::TcpLinks;

const RELIABLE: &str = "_reliable";
const LOSSY: &str = "_lossy";
const DATAGRAM: usize = 2000;
/// How long ICE may stay unconnected before the call counts as lost. A disconnect may heal, and
/// pairs form only as candidates trickle in.
const ICE_GRACE: Duration = Duration::from_secs(15);
/// How often a peer reports what it sent and heard to the log.
const STATS_EVERY: Duration = Duration::from_secs(5);

/// One Opus frame to publish, with its RTP timestamp in 48 kHz units.
pub(crate) struct OpusOut {
    pub(crate) payload: Vec<u8>,
    pub(crate) timestamp: u32,
}

/// One Opus frame heard on a subscribed track.
pub(crate) struct OpusIn {
    pub(crate) mid: Mid,
    pub(crate) track: Option<String>,
    pub(crate) payload: Vec<u8>,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "calls play frames in arrival order; tests check the clock"
        )
    )]
    pub(crate) timestamp: u32,
}

/// The port an active TCP candidate names: it dials out, so it has none of its own.
const ACTIVE_TCP_PORT: u16 = 9;

/// A str0m connection and the sockets it owns.
pub(crate) struct Peer {
    rtc: Rtc,
    socket: UdpSocket,
    udp_local: SocketAddr,
    tcp: TcpLinks,
    tcp_local: SocketAddr,
    local: Vec<Candidate>,
    mic: Option<Mid>,
    pending: Option<SdpPendingOffer>,
    bundle: Option<Mid>,
    buf: Vec<u8>,
    dropped: u64,
    /// Since when ICE has not been connected; None while it is.
    down_since: Option<Instant>,
}

enum Turn {
    Wait(Instant),
    Closed,
}

fn no_route() -> String {
    "No UDP route".to_owned()
}

fn lost() -> String {
    "Call lost".to_owned()
}

fn failed(error: &RtcError) -> String {
    tracing::warn!(%error, "WebRTC failed");
    "Call failed".to_owned()
}

/// str0m built without default features has no crypto until one is installed.
fn install_crypto() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| str0m::crypto::from_feature_flags().install_process_default());
}

fn new_rtc() -> Rtc {
    install_crypto();
    Rtc::builder()
        .clear_codecs()
        .enable_opus(true, false)
        .set_stats_interval(Some(STATS_EVERY))
        .build(Instant::now())
}

/// The local address the OS routes toward `sfu`. Connecting a UDP socket sends nothing.
fn route_to(sfu: SocketAddr) -> Result<IpAddr, String> {
    let any = if sfu.is_ipv4() {
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    } else {
        IpAddr::V6(Ipv6Addr::UNSPECIFIED)
    };
    let probe = std::net::UdpSocket::bind((any, 0)).map_err(|_| no_route())?;
    probe.connect(sfu).map_err(|_| no_route())?;
    let local = probe.local_addr().map_err(|_| no_route())?.ip();
    if local.is_unspecified() {
        return Err(no_route());
    }
    Ok(local)
}

/// Logs how a peer comes up and what flows through it.
fn report(event: &Event) {
    match event {
        Event::Connected => tracing::debug!("WebRTC connected"),
        Event::ChannelOpen(_, label) => tracing::debug!(label, "data channel open"),
        Event::MediaAdded(added) => {
            tracing::debug!(mid = %added.mid, kind = ?added.kind, "media added");
        }
        Event::MediaEgressStats(stats) => tracing::debug!(
            packets = stats.packets,
            bytes = stats.bytes,
            loss = ?stats.loss,
            rtt = ?stats.rtt,
            "sent"
        ),
        Event::MediaIngressStats(stats) => tracing::debug!(
            packets = stats.packets,
            bytes = stats.bytes,
            loss = ?stats.loss,
            "received"
        ),
        _ => {}
    }
}

/// Hands one received packet to str0m.
fn feed(
    rtc: &mut Rtc,
    proto: Protocol,
    source: SocketAddr,
    destination: SocketAddr,
    packet: &[u8],
) -> Result<(), String> {
    let Ok(contents) = packet.try_into() else {
        tracing::debug!(%source, "unknown packet ignored");
        return Ok(());
    };
    let input = Input::Receive(
        Instant::now(),
        Receive {
            proto,
            source,
            destination,
            contents,
        },
    );
    rtc.handle_input(input).map_err(|error| failed(&error))
}

/// The first `a=mid` of a description: the bundle tag.
fn first_mid(sdp: &str) -> Option<Mid> {
    sdp.lines()
        .find_map(|line| line.strip_prefix("a=mid:"))
        .map(|mid| Mid::from(mid.trim()))
}

/// `LiveKit` packs `participant|track` into the msid stream id; Firefox keeps the track sid
/// there, other offers carry it as the msid track id. Matches `extractTrackSid` in client-sdk-js.
fn track_sid<'a>(stream_id: &'a str, track_id: &'a str) -> Option<&'a str> {
    let packed = stream_id.split_once('|').map(|(_, track)| track);
    packed
        .into_iter()
        .chain([track_id])
        .find(|sid| sid.starts_with("TR"))
}

impl Peer {
    async fn bind(sfu: SocketAddr) -> Result<Self, String> {
        let socket = UdpSocket::bind((route_to(sfu)?, 0))
            .await
            .map_err(|_| no_route())?;
        let udp_local = socket.local_addr().map_err(|_| no_route())?;
        let tcp_local = SocketAddr::new(udp_local.ip(), ACTIVE_TCP_PORT);
        let udp = Candidate::host(udp_local, "udp").map_err(|_| no_route())?;
        let tcp = Candidate::builder()
            .tcp()
            .host(tcp_local)
            .tcptype(TcpType::Active)
            .build()
            .map_err(|_| no_route())?;
        let mut rtc = new_rtc();
        let local = [udp, tcp]
            .into_iter()
            .map(|candidate| rtc.add_local_candidate(candidate).cloned())
            .collect::<Option<Vec<_>>>()
            .ok_or_else(no_route)?;
        Ok(Self {
            rtc,
            socket,
            udp_local,
            tcp: TcpLinks::new(),
            tcp_local,
            local,
            mic: None,
            pending: None,
            bundle: None,
            buf: vec![0; DATAGRAM],
            dropped: 0,
            down_since: Some(Instant::now()),
        })
    }

    /// The publishing connection: we offer one send-only Opus track and the two data
    /// channels `LiveKit` clients open.
    pub(crate) async fn publisher(sfu: SocketAddr) -> Result<(Self, SdpOffer), String> {
        let mut peer = Self::bind(sfu).await?;
        let mut api = peer.rtc.sdp_api();
        let mic = api.add_media(
            MediaKind::Audio,
            Direction::SendOnly,
            None,
            Some(MIC_CID.to_owned()),
            None,
        );
        api.add_channel_with_config(ChannelConfig {
            label: RELIABLE.to_owned(),
            ..ChannelConfig::default()
        });
        api.add_channel_with_config(ChannelConfig {
            label: LOSSY.to_owned(),
            ordered: false,
            reliability: Reliability::MaxRetransmits { retransmits: 0 },
            ..ChannelConfig::default()
        });
        let (offer, pending) = api.apply().ok_or_else(|| "Call failed".to_owned())?;
        peer.mic = Some(mic);
        peer.pending = Some(pending);
        peer.bundle = first_mid(&offer.to_sdp_string());
        Ok((peer, offer))
    }

    /// The subscribing connection: the SFU offers, we answer.
    pub(crate) async fn subscriber(
        sfu: SocketAddr,
        offer: SdpOffer,
    ) -> Result<(Self, SdpAnswer), String> {
        let mut peer = Self::bind(sfu).await?;
        let answer = peer.accept_offer(offer)?;
        Ok((peer, answer))
    }

    /// Answers an offer, also each renegotiation as the SFU adds tracks.
    pub(crate) fn accept_offer(&mut self, offer: SdpOffer) -> Result<SdpAnswer, String> {
        let answer = self
            .rtc
            .sdp_api()
            .accept_offer(offer)
            .map_err(|error| failed(&error))?;
        self.bundle = first_mid(&answer.to_sdp_string()).or(self.bundle);
        Ok(answer)
    }

    /// The mid our trickled candidates name; `None` until the first offer or answer.
    pub(crate) fn bundle(&self) -> Option<Mid> {
        self.bundle
    }

    pub(crate) fn accept_answer(&mut self, answer: SdpAnswer) -> Result<(), String> {
        let pending = self
            .pending
            .take()
            .ok_or_else(|| "Call failed".to_owned())?;
        self.rtc
            .sdp_api()
            .accept_answer(pending, answer)
            .map_err(|error| failed(&error))
    }

    /// Takes a trickled SFU candidate: UDP, or a passive TCP one we can dial.
    pub(crate) fn add_remote_candidate(&mut self, candidate: Candidate) {
        let reachable = match candidate.proto() {
            Protocol::Udp => true,
            Protocol::Tcp => candidate.tcptype() == Some(TcpType::Passive),
            _ => false,
        };
        if reachable {
            self.rtc.add_remote_candidate(candidate);
        }
    }

    pub(crate) fn local_candidates(&self) -> Vec<Candidate> {
        self.local.clone()
    }

    /// Heard frames thrown away because the reader fell behind.
    pub(crate) fn dropped(&self) -> u64 {
        self.dropped
    }

    /// The `LiveKit` track sid of a received media, from its msid.
    pub(crate) fn track_of(&self, mid: Mid) -> Option<String> {
        let media = self.rtc.media(mid)?;
        track_sid(media.stream_id(), media.track_id()).map(str::to_owned)
    }

    /// Drives the connection until `outgoing` or `incoming` closes. Safe to cancel, so
    /// signalling can be fed between calls; a cancel loses at most one datagram.
    pub(crate) async fn run(
        &mut self,
        outgoing: &mut mpsc::Receiver<OpusOut>,
        incoming: &mpsc::Sender<OpusIn>,
    ) -> Result<(), String> {
        self.tick()?;
        loop {
            let wake = match self.drain(incoming).await? {
                Turn::Wait(wake) => wake,
                Turn::Closed => return Ok(()),
            };
            tokio::select! {
                received = self.socket.recv_from(&mut self.buf) => {
                    let (len, source) = received.map_err(|_| lost())?;
                    self.receive(len, source)?;
                }
                Some((source, packet)) = self.tcp.recv() => {
                    feed(&mut self.rtc, Protocol::Tcp, source, self.tcp_local, &packet)?;
                }
                frame = outgoing.recv() => match frame {
                    Some(frame) => self.send(&frame)?,
                    None => return Ok(()),
                },
                () = sleep_until(wake.into()) => self.tick()?,
            }
        }
    }

    async fn drain(&mut self, incoming: &mpsc::Sender<OpusIn>) -> Result<Turn, String> {
        loop {
            match self.rtc.poll_output().map_err(|error| failed(&error))? {
                Output::Timeout(wake) => return Ok(Turn::Wait(self.give_up_at().min(wake))),
                Output::Transmit(transmit) if transmit.proto == Protocol::Tcp => {
                    self.tcp.send(transmit.destination, &transmit.contents);
                }
                Output::Transmit(transmit) => {
                    if let Err(error) = self
                        .socket
                        .send_to(&transmit.contents, transmit.destination)
                        .await
                    {
                        tracing::debug!(%error, "datagram not sent");
                    }
                }
                Output::Event(event) => {
                    if let Some(turn) = self.on_event(event, incoming) {
                        return Ok(turn);
                    }
                }
            }
        }
    }

    fn on_event(&mut self, event: Event, incoming: &mpsc::Sender<OpusIn>) -> Option<Turn> {
        match event {
            Event::MediaData(data) => self.hear(&data, incoming),
            Event::IceConnectionStateChange(state) => {
                tracing::debug!(?state, "ICE");
                self.down_since = match state {
                    IceConnectionState::Connected | IceConnectionState::Completed => None,
                    _ => self.down_since.or(Some(Instant::now())),
                };
                None
            }
            other => {
                report(&other);
                None
            }
        }
    }

    fn hear(&mut self, data: &MediaData, incoming: &mpsc::Sender<OpusIn>) -> Option<Turn> {
        let frame = OpusIn {
            mid: data.mid,
            track: self.track_of(data.mid),
            payload: data.data.to_vec(),
            timestamp: data.time.numer() as u32,
        };
        match incoming.try_send(frame) {
            Ok(()) => None,
            Err(TrySendError::Full(_)) => {
                self.dropped += 1;
                None
            }
            Err(TrySendError::Closed(_)) => Some(Turn::Closed),
        }
    }

    fn receive(&mut self, len: usize, source: SocketAddr) -> Result<(), String> {
        let Some(datagram) = self.buf.get(..len) else {
            return Ok(());
        };
        feed(
            &mut self.rtc,
            Protocol::Udp,
            source,
            self.udp_local,
            datagram,
        )
    }

    /// When ICE has been down too long, or far off while it is up.
    fn give_up_at(&self) -> Instant {
        self.down_since
            .map_or(Instant::now() + ICE_GRACE, |since| since + ICE_GRACE)
    }

    fn tick(&mut self) -> Result<(), String> {
        if self.down_since.is_some() && Instant::now() >= self.give_up_at() {
            tracing::warn!("ICE did not connect");
            return Err(lost());
        }
        self.rtc
            .handle_input(Input::Timeout(Instant::now()))
            .map_err(|error| failed(&error))
    }

    /// Frames before the connection is up have nowhere to go; str0m would drop them too.
    fn send(&mut self, frame: &OpusOut) -> Result<(), String> {
        let Some(mid) = self.mic else {
            return Ok(());
        };
        if !self.rtc.is_connected() {
            return Ok(());
        }
        let Some(writer) = self.rtc.writer(mid) else {
            return Ok(());
        };
        let pt = writer
            .payload_params()
            .find(|params| params.spec().codec == Codec::Opus)
            .map(PayloadParams::pt)
            .ok_or_else(|| "No Opus".to_owned())?;
        let time = MediaTime::new(u64::from(frame.timestamp), Frequency::FORTY_EIGHT_KHZ);
        writer
            .write(pt, Instant::now(), time, frame.payload.as_slice())
            .map_err(|error| failed(&error))
    }
}

#[cfg(test)]
mod tests;
