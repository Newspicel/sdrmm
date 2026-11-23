use std::{
    io::ErrorKind,
    net::{ToSocketAddrs, UdpSocket},
};

use axum::body::Bytes;
use sdrmm_modem::dvb::PACKET;
use sdrmm_wire::{StreamOutNode, StreamTransport};
use tokio::sync::broadcast;

use super::encode::bytes_of;

/// Transport packets in one datagram: the most that fit an Ethernet frame.
const PACKETS_PER_DATAGRAM: usize = 7;
const RTP_HEADER: usize = 12;
const RTP_VERSION: u8 = 0x80;
/// The static RTP payload type of an MPEG transport stream.
const RTP_MP2T: u8 = 33;

/// Where encoded packets go.
pub(super) enum Outlet {
    Serve(broadcast::Sender<Bytes>),
    Send { socket: UdpSocket, rtp: Option<Rtp> },
}

impl Outlet {
    pub(super) fn open(
        settings: &StreamOutNode,
        viewers: broadcast::Sender<Bytes>,
    ) -> Result<Self, String> {
        let rtp = match settings.transport {
            StreamTransport::Http => return Ok(Self::Serve(viewers)),
            StreamTransport::Udp => None,
            StreamTransport::Rtp => Some(Rtp::new(&settings.address)),
        };
        let target = settings
            .address
            .to_socket_addrs()
            .map_err(|error| format!("{}: {error}", settings.address))?
            .next()
            .ok_or_else(|| format!("{} has no address", settings.address))?;
        let local = if target.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };
        let socket = UdpSocket::bind(local).map_err(|error| error.to_string())?;
        socket
            .connect(target)
            .map_err(|error| format!("{target}: {error}"))?;
        Ok(Self::Send { socket, rtp })
    }

    /// Whether anything takes the stream now: always for a sent one, a viewer for a served one.
    pub(super) fn wanted(&self) -> bool {
        match self {
            Self::Serve(viewers) => viewers.receiver_count() > 0,
            Self::Send { .. } => true,
        }
    }

    /// Hands `packets` on, `now` the stream clock; the bytes that left.
    pub(super) fn deliver(&mut self, packets: &[[u8; PACKET]], now: u64) -> Result<usize, String> {
        if packets.is_empty() {
            return Ok(0);
        }
        match self {
            Self::Serve(viewers) => {
                let bytes = bytes_of(packets);
                let length = bytes.len();
                let _ = viewers.send(Bytes::from(bytes));
                Ok(length)
            }
            Self::Send { socket, rtp } => {
                let mut sent = 0;
                for group in packets.chunks(PACKETS_PER_DATAGRAM) {
                    let datagram = datagram(rtp.as_mut(), group, now);
                    match socket.send(&datagram) {
                        Ok(length) => sent += length,
                        Err(error) if error.kind() == ErrorKind::ConnectionRefused => {}
                        Err(error) => return Err(error.to_string()),
                    }
                }
                Ok(sent)
            }
        }
    }
}

fn datagram(rtp: Option<&mut Rtp>, packets: &[[u8; PACKET]], now: u64) -> Vec<u8> {
    let mut datagram = Vec::with_capacity(RTP_HEADER + packets.len() * PACKET);
    if let Some(rtp) = rtp {
        datagram.extend_from_slice(&rtp.header(now));
    }
    for packet in packets {
        datagram.extend_from_slice(packet);
    }
    datagram
}

/// One RTP session's numbering.
pub(super) struct Rtp {
    sequence: u16,
    source: u32,
}

impl Rtp {
    fn new(address: &str) -> Self {
        let source = address.bytes().fold(0x811C_9DC5u32, |hash, byte| {
            (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
        });
        Self {
            sequence: 0,
            source,
        }
    }

    /// The next header, stamped with `now` on the 90 kHz clock.
    fn header(&mut self, now: u64) -> [u8; RTP_HEADER] {
        let mut header = [0; RTP_HEADER];
        header[0] = RTP_VERSION;
        header[1] = RTP_MP2T;
        header[2..4].copy_from_slice(&self.sequence.to_be_bytes());
        header[4..8].copy_from_slice(&(now as u32).to_be_bytes());
        header[8..12].copy_from_slice(&self.source.to_be_bytes());
        self.sequence = self.sequence.wrapping_add(1);
        header
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packets(count: usize) -> Vec<[u8; PACKET]> {
        (0..count).map(|at| [at as u8; PACKET]).collect()
    }

    #[test]
    fn udp_sends_seven_packets_a_datagram() {
        let receiver = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let settings = StreamOutNode {
            transport: StreamTransport::Udp,
            address: receiver.local_addr().expect("address").to_string(),
        };
        let mut outlet = Outlet::open(&settings, broadcast::channel(1).0).expect("open");
        assert_eq!(outlet.deliver(&packets(9), 0), Ok(9 * PACKET));
        let mut datagram = [0; 2048];
        assert_eq!(receiver.recv(&mut datagram).expect("first"), 7 * PACKET);
        assert_eq!(receiver.recv(&mut datagram).expect("second"), 2 * PACKET);
        assert_eq!(datagram[0], 7);
    }

    #[test]
    fn rtp_numbers_each_datagram_and_stamps_the_clock() {
        let receiver = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let settings = StreamOutNode {
            transport: StreamTransport::Rtp,
            address: receiver.local_addr().expect("address").to_string(),
        };
        let mut outlet = Outlet::open(&settings, broadcast::channel(1).0).expect("open");
        outlet.deliver(&packets(14), 90_000).expect("deliver");
        let mut datagram = [0; 2048];
        for sequence in 0..2u16 {
            let length = receiver.recv(&mut datagram).expect("datagram");
            assert_eq!(length, RTP_HEADER + 7 * PACKET);
            assert_eq!(datagram[..2], [RTP_VERSION, RTP_MP2T]);
            assert_eq!(datagram[2..4], sequence.to_be_bytes());
            assert_eq!(datagram[4..8], 90_000u32.to_be_bytes());
        }
    }

    #[test]
    fn a_served_stream_is_wanted_only_while_watched() {
        let viewers = broadcast::channel(4).0;
        let mut outlet = Outlet::open(&StreamOutNode::default(), viewers.clone()).expect("open");
        assert!(!outlet.wanted());
        let mut viewer = viewers.subscribe();
        assert!(outlet.wanted());
        assert_eq!(outlet.deliver(&packets(2), 0), Ok(2 * PACKET));
        assert_eq!(viewer.try_recv().expect("chunk").len(), 2 * PACKET);
    }

    #[test]
    fn an_address_that_does_not_resolve_is_reported() {
        let settings = StreamOutNode {
            transport: StreamTransport::Udp,
            address: "not an address".to_owned(),
        };
        assert!(Outlet::open(&settings, broadcast::channel(1).0).is_err());
    }
}
