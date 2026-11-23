//! One WebRTC peer connection to the `LiveKit` SFU, driven by str0m over one UDP socket.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Once;
use std::time::Instant;

use str0m::change::{SdpAnswer, SdpOffer, SdpPendingOffer};
use str0m::channel::{ChannelConfig, Reliability};
use str0m::format::{Codec, PayloadParams};
use str0m::media::{Direction, Frequency, MediaData, MediaKind, MediaTime, Mid};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc, RtcError};
use tokio::net::UdpSocket;
use tokio::sync::mpsc::{self, error::TrySendError};
use tokio::time::sleep_until;

use super::signal::MIC_CID;

const RELIABLE: &str = "_reliable";
const LOSSY: &str = "_lossy";
const DATAGRAM: usize = 2000;

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
    pub(crate) timestamp: u32,
}

/// A str0m connection and the socket it owns.
pub(crate) struct Peer {
    rtc: Rtc,
    socket: UdpSocket,
    local: Vec<Candidate>,
    mic: Option<Mid>,
    pending: Option<SdpPendingOffer>,
    bundle: Option<Mid>,
    buf: Vec<u8>,
    dropped: u64,
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
        let addr = socket.local_addr().map_err(|_| no_route())?;
        let candidate = Candidate::host(addr, "udp").map_err(|_| no_route())?;
        let mut rtc = new_rtc();
        let local = rtc
            .add_local_candidate(candidate)
            .cloned()
            .ok_or_else(no_route)?;
        Ok(Self {
            rtc,
            socket,
            local: vec![local],
            mic: None,
            pending: None,
            bundle: None,
            buf: vec![0; DATAGRAM],
            dropped: 0,
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

    /// Takes a trickled SFU candidate. Only UDP ones can reach our socket.
    pub(crate) fn add_remote_candidate(&mut self, candidate: Candidate) {
        if candidate.proto() == Protocol::Udp {
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
                Output::Timeout(wake) => return Ok(Turn::Wait(wake)),
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
                    if let Some(turn) = self.on_event(event, incoming)? {
                        return Ok(turn);
                    }
                }
            }
        }
    }

    fn on_event(
        &mut self,
        event: Event,
        incoming: &mpsc::Sender<OpusIn>,
    ) -> Result<Option<Turn>, String> {
        match event {
            Event::MediaData(data) => Ok(self.hear(&data, incoming)),
            Event::IceConnectionStateChange(IceConnectionState::Disconnected) => Err(lost()),
            _ => Ok(None),
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
        let destination = self.socket.local_addr().map_err(|_| lost())?;
        let Some(datagram) = self.buf.get(..len) else {
            return Ok(());
        };
        let Ok(contents) = datagram.try_into() else {
            tracing::debug!(%source, "unknown datagram ignored");
            return Ok(());
        };
        let input = Input::Receive(
            Instant::now(),
            Receive {
                proto: Protocol::Udp,
                source,
                destination,
                contents,
            },
        );
        self.rtc.handle_input(input).map_err(|error| failed(&error))
    }

    fn tick(&mut self) -> Result<(), String> {
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
