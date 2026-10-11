use std::{
    io::ErrorKind,
    net::{IpAddr, UdpSocket},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use sdrmm_device::{
    Block, BlockGap, BlockPool, ByteCoding, ByteConverter, CaptureRadio, CaptureStream,
    DeviceError, Next, StreamFailure, lock,
    net::{Endpoint, udp_receiver},
};

use crate::{
    caps::Remote,
    ethernet::{
        api::{Call, Request, Tuning, post},
        packet::{HEADER_BYTES, Header, PACKET_BYTES, PAYLOAD_BYTES, Placed, Tracker},
    },
    link::Stop,
};

const BLOCK_PACKETS: usize = 48;
const BLOCK_BYTES: usize = BLOCK_PACKETS * PAYLOAD_BYTES;
const CODING: ByteCoding = ByteCoding::TwosComplement { full_scale: 128.0 };

pub(crate) fn converter() -> ByteConverter {
    ByteConverter::new(CODING, BLOCK_BYTES / 2)
}

#[derive(Debug)]
pub(crate) struct NetRadio {
    pub(crate) endpoint: Endpoint,
    pub(crate) desired: Mutex<Remote>,
    streaming: AtomicBool,
    stop: Stop,
    pool: BlockPool,
}

impl NetRadio {
    pub(crate) fn new(endpoint: Endpoint, desired: Remote) -> Self {
        Self {
            endpoint,
            desired: Mutex::new(desired),
            streaming: AtomicBool::new(false),
            stop: Stop::default(),
            pool: BlockPool::default(),
        }
    }

    pub(crate) fn retune(&self, next: &Remote) -> Result<(), DeviceError> {
        if self.streaming.load(Ordering::Acquire) {
            post(&self.endpoint, &Request::configure(Tuning::of(next)))?;
        }
        Ok(())
    }
}

impl CaptureRadio for NetRadio {
    type Stream = NetStream;

    fn arm(&self) -> Result<NetStream, DeviceError> {
        self.stop.clear();
        let call = Call::open(&self.endpoint)?;
        let device = call.peer()?;
        let socket = udp_receiver(call.local()?)?;
        let port = socket
            .local_addr()
            .map_err(|e| DeviceError::Io(e.to_string()))?
            .port();
        let tuning = Tuning::of(&lock(&self.desired));
        call.send(&Request::start(tuning, port))?;
        self.streaming.store(true, Ordering::Release);
        tracing::debug!(endpoint = %self.endpoint, port, "ESP-SDR streams over Ethernet");
        Ok(NetStream::new(
            socket,
            device,
            self.pool.clone(),
            self.stop.clone(),
        ))
    }

    fn disarm(&self) {
        if !self.streaming.swap(false, Ordering::AcqRel) {
            return;
        }
        if let Err(error) = post(&self.endpoint, &Request::stop()) {
            tracing::warn!(endpoint = %self.endpoint, %error, "ESP-SDR did not stop streaming");
        }
    }
}

struct State {
    tracker: Tracker,
    datagram: Vec<u8>,
    held: bool,
    carried: Option<BlockGap>,
    returned: Option<BlockGap>,
}

pub(crate) struct NetStream {
    socket: UdpSocket,
    device: IpAddr,
    pool: BlockPool,
    stop: Stop,
    state: Mutex<State>,
    failure: Mutex<Option<StreamFailure>>,
}

fn merge(first: Option<BlockGap>, second: BlockGap) -> BlockGap {
    first.map_or(second, |first| BlockGap {
        exact: first.exact + second.exact,
        estimated: first.estimated + second.estimated,
    })
}

impl NetStream {
    fn new(socket: UdpSocket, device: IpAddr, pool: BlockPool, stop: Stop) -> Self {
        Self {
            socket,
            device,
            pool,
            stop,
            state: Mutex::new(State {
                tracker: Tracker::default(),
                datagram: vec![0; PACKET_BYTES + 1],
                held: false,
                carried: None,
                returned: None,
            }),
            failure: Mutex::new(None),
        }
    }

    fn receive(&self, state: &mut State) -> Result<Option<Placed>, DeviceError> {
        let (n, from) = match self.socket.recv_from(&mut state.datagram) {
            Ok(got) => got,
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                return Ok(None);
            }
            Err(e) => return Err(DeviceError::Io(e.to_string())),
        };
        if from.ip() != self.device {
            tracing::debug!(%from, "ignored a datagram from another host");
            return Ok(None);
        }
        let header = Header::parse(&state.datagram[..n])?;
        Ok(Some(state.tracker.place(header)))
    }

    fn gather(&self, state: &mut State, timeout: Duration) -> Result<Option<Block>, DeviceError> {
        let mut block = self.pool.take(BLOCK_BYTES);
        let mut filled = 0;
        let mut gap = state.carried.take();
        let deadline = Instant::now() + timeout;
        loop {
            if std::mem::take(&mut state.held) {
                append(&mut block, &mut filled, &state.datagram);
            }
            if filled == BLOCK_BYTES || Instant::now() >= deadline || self.stop.is_set() {
                break;
            }
            match self.receive(state)? {
                None | Some(Placed::Stale) => {}
                Some(Placed::After(found)) if filled > 0 => {
                    state.held = true;
                    state.carried = Some(found);
                    break;
                }
                Some(Placed::After(found)) => {
                    gap = Some(merge(gap, found));
                    state.held = true;
                }
                Some(Placed::Next) => state.held = true,
            }
        }
        if filled == 0 {
            state.carried = gap;
            return Ok(None);
        }
        block.truncate(filled);
        state.returned = gap;
        Ok(Some(block))
    }
}

fn append(block: &mut Block, filled: &mut usize, datagram: &[u8]) {
    block.bytes_mut()[*filled..*filled + PAYLOAD_BYTES]
        .copy_from_slice(&datagram[HEADER_BYTES..PACKET_BYTES]);
    *filled += PAYLOAD_BYTES;
}

impl CaptureStream for NetStream {
    type Block = Block;
    type Stop = Stop;

    fn stop_handle(&self) -> Stop {
        self.stop.clone()
    }

    fn next_block(&self, timeout: Duration) -> Next<Block> {
        if self.stop.is_set() {
            return Next::Ended;
        }
        let mut state = lock(&self.state);
        match self.gather(&mut state, timeout) {
            Ok(Some(block)) => Next::Block(block),
            Ok(None) => Next::Idle,
            Err(error) => {
                *lock(&self.failure) = Some(StreamFailure {
                    reason: error.to_string(),
                    gone: false,
                });
                Next::Ended
            }
        }
    }

    fn dropped(&self) -> u64 {
        0
    }

    fn block_gap(&self, _block: &Block, _bytes_per_sample: u64) -> Option<BlockGap> {
        lock(&self.state).returned.take()
    }

    fn failure(&self) -> StreamFailure {
        lock(&self.failure).clone().unwrap_or(StreamFailure {
            reason: "ESP-SDR stream ended".to_string(),
            gone: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::Write,
        net::{SocketAddr, TcpListener},
        sync::mpsc,
        thread,
    };

    use sdrmm_device::StopHandle;

    use super::*;
    use crate::{
        caps::tests::esp32,
        ethernet::api::{
            HTTP_PORT,
            tests::{STATUS, read_request, reply},
        },
    };

    const RATE: u32 = 16_000_000;

    fn packet(epoch: u32, sample: u64, fill: u8) -> Vec<u8> {
        let mut bytes = Header {
            epoch,
            rate: RATE,
            sample,
            time_us: sample / 16,
        }
        .encode();
        bytes[HEADER_BYTES..].fill(fill);
        bytes
    }

    struct Board {
        endpoint: Endpoint,
        requests: mpsc::Receiver<serde_json::Value>,
        server: thread::JoinHandle<()>,
    }

    fn board(packets: Vec<Vec<u8>>, calls: usize) -> Board {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let at: SocketAddr = listener.local_addr().expect("addr");
        let (tx, requests) = mpsc::channel();
        let server = thread::spawn(move || {
            let mut packets = Some(packets);
            for _ in 0..calls {
                let (mut tcp, peer) = listener.accept().expect("accept");
                let request = read_request(&tcp);
                tcp.write_all(reply(200, STATUS).as_bytes()).expect("reply");
                if let (Some(port), Some(packets)) = (request["port"].as_u64(), packets.take()) {
                    let udp = UdpSocket::bind("127.0.0.1:0").expect("udp");
                    let to = SocketAddr::new(peer.ip(), u16::try_from(port).expect("port"));
                    for packet in packets {
                        udp.send_to(&packet, to).expect("send");
                    }
                }
                tx.send(request).expect("record");
            }
        });
        Board {
            endpoint: Endpoint::parse(&at.to_string(), HTTP_PORT).expect("endpoint"),
            requests,
            server,
        }
    }

    fn block(stream: &NetStream) -> Block {
        for _ in 0..50 {
            if let Next::Block(block) = stream.next_block(Duration::from_millis(100)) {
                return block;
            }
        }
        panic!("no block arrived");
    }

    #[test]
    fn a_stream_starts_on_our_port_cuts_at_gaps_and_stops() {
        let board = board(
            vec![packet(1, 0, 1), packet(1, 672, 2), packet(1, 2_016, 3)],
            3,
        );
        let radio = NetRadio::new(board.endpoint.clone(), esp32().defaults());
        let stream = radio.arm().expect("armed");
        let start = board.requests.recv().expect("start");
        assert_eq!(start["op"], "start");
        assert_eq!(start["frequency"], 2_437_000_000u64);

        let first = block(&stream);
        assert_eq!(first.len(), 2 * PAYLOAD_BYTES);
        assert_eq!((first[0], first[PAYLOAD_BYTES]), (1, 2));
        assert_eq!(stream.block_gap(&first, 2), None);
        let second = block(&stream);
        assert_eq!((second.len(), second[0]), (PAYLOAD_BYTES, 3));
        assert_eq!(
            stream.block_gap(&second, 2),
            Some(BlockGap {
                exact: 672,
                estimated: 0
            })
        );

        let mut tuned = esp32().defaults();
        tuned.center_mhz = 2_450;
        radio.retune(&tuned).expect("retuned");
        assert_eq!(
            board.requests.recv().expect("configure")["frequency"],
            2_450_000_000u64
        );

        stream.stop_handle().stop();
        assert!(matches!(
            stream.next_block(Duration::from_millis(10)),
            Next::Ended
        ));
        radio.disarm();
        radio.disarm();
        assert_eq!(board.requests.recv().expect("stop")["op"], "stop");
        board.server.join().expect("board");
    }

    #[test]
    fn a_retune_before_the_stream_stays_local() {
        let radio = NetRadio::new(
            Endpoint::parse("127.0.0.1:1", HTTP_PORT).expect("endpoint"),
            esp32().defaults(),
        );
        assert!(radio.retune(&esp32().defaults()).is_ok());
        radio.disarm();
    }

    #[test]
    fn a_foreign_packet_ends_the_stream_with_its_reason() {
        let mut short = packet(1, 0, 0);
        short.truncate(100);
        let board = board(vec![short], 1);
        let radio = NetRadio::new(board.endpoint.clone(), esp32().defaults());
        let stream = radio.arm().expect("armed");
        board.server.join().expect("board");
        let mut ended = false;
        for _ in 0..50 {
            if matches!(stream.next_block(Duration::from_millis(100)), Next::Ended) {
                ended = true;
                break;
            }
        }
        assert!(ended);
        assert!(stream.failure().reason.contains("100 bytes"));
    }

    #[test]
    fn eight_bit_codes_span_full_scale() {
        assert_eq!(CODING.level(0x7f), 127.0 / 128.0);
        assert_eq!(CODING.level(0x80), -1.0);
    }
}
