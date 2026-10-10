use std::{
    io,
    net::{IpAddr, SocketAddr, UdpSocket},
    sync::Mutex,
    time::{Duration, Instant},
};

use sdrmm_device::{
    Block, BlockGap, BlockPool, DeviceError, Next, StreamFailure, lock,
    net::{Connection, Endpoint, Read},
};
use sdrmm_iqlink::{CONTROL_PORT, DATAGRAM_BYTES, HEADER_BYTES, Header, Reply, Start};
use socket2::{Domain, Protocol, Socket, Type};

use crate::iio::Stopper;

const PROBE_TIMEOUT: Duration = Duration::from_millis(300);
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);
const RECV_SLICE: Duration = Duration::from_millis(20);
const RECEIVE_BUFFERS: [usize; 4] = [8 << 20, 4 << 20, 2 << 20, 1 << 20];

pub(crate) struct Request<'a> {
    pub(crate) device: &'a str,
    pub(crate) elements: Vec<u32>,
    pub(crate) frames: usize,
    pub(crate) frame_bytes: usize,
    pub(crate) lanes: usize,
}

pub(crate) fn open(
    endpoint: &Endpoint,
    request: Request<'_>,
    pool: BlockPool,
) -> Result<Option<IqlinkStream>, DeviceError> {
    connect(
        &Endpoint::parse(endpoint.host(), CONTROL_PORT)?,
        request,
        pool,
    )
}

fn connect(
    control_at: &Endpoint,
    request: Request<'_>,
    pool: BlockPool,
) -> Result<Option<IqlinkStream>, DeviceError> {
    let Ok(tcp) = control_at.connect_within(PROBE_TIMEOUT) else {
        return Ok(None);
    };
    let local = tcp.local_addr().map_err(io_error)?.ip();
    let socket = receiver(local)?;
    let port = socket.local_addr().map_err(io_error)?.port();
    let control = Connection::new(tcp);
    let start = Start {
        device: request.device.to_string(),
        elements: request.elements,
        buffer_frames: request.frames,
        udp_port: port,
    };
    control.send(start.line().as_bytes())?;
    let reply = Reply::parse(&read_line(&control, REPLY_TIMEOUT)?)
        .map_err(|e| DeviceError::Io(e.to_string()))?;
    match reply {
        Reply::Streaming { frame_bytes } if frame_bytes == request.frame_bytes => {
            tracing::info!(at = %control_at, "ad936x samples arrive over iqlink");
            let shape = Shape {
                refill: start.buffer_frames * frame_bytes,
                frame_bytes,
                lanes: request.lanes.max(1) as u64,
            };
            Ok(Some(IqlinkStream::new(control, socket, pool, &shape)))
        }
        Reply::Streaming { frame_bytes } => Err(DeviceError::Io(format!(
            "iqlink sends {frame_bytes}-byte frames, expected {}",
            request.frame_bytes
        ))),
        Reply::Refused(reason) => {
            tracing::warn!(at = %control_at, %reason, "iqlink refused, falling back to iiod");
            Ok(None)
        }
    }
}

struct Shape {
    refill: usize,
    frame_bytes: usize,
    lanes: u64,
}

fn io_error(e: io::Error) -> DeviceError {
    DeviceError::Io(e.to_string())
}

fn receiver(local: IpAddr) -> Result<UdpSocket, DeviceError> {
    let domain = if local.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    };
    let socket = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP)).map_err(io_error)?;
    if !RECEIVE_BUFFERS
        .iter()
        .any(|size| socket.set_recv_buffer_size(*size).is_ok())
    {
        tracing::warn!("iqlink keeps the system's small receive buffer; expect drops");
    }
    socket
        .bind(&SocketAddr::new(local, 0).into())
        .map_err(io_error)?;
    let socket: UdpSocket = socket.into();
    socket
        .set_read_timeout(Some(RECV_SLICE))
        .map_err(io_error)?;
    Ok(socket)
}

fn read_line(control: &Connection, timeout: Duration) -> Result<String, DeviceError> {
    let deadline = Instant::now() + timeout;
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    while Instant::now() < deadline {
        match control.read(&mut byte, deadline - Instant::now()) {
            Read::Got(_) if byte[0] == b'\n' => {
                return Ok(String::from_utf8_lossy(&line).into_owned());
            }
            Read::Got(_) => line.push(byte[0]),
            Read::Idle => {}
            Read::Ended => return Err(DeviceError::Io("iqlink closed before answering".into())),
        }
    }
    Err(DeviceError::Io(format!(
        "iqlink did not answer within {timeout:?}"
    )))
}

#[derive(Debug, PartialEq, Eq)]
enum Placed {
    Next,
    After(BlockGap),
    Stale,
}

#[derive(Debug)]
struct Sequence {
    frame_bytes: usize,
    lanes: u64,
    expected: Option<u64>,
    board_lost: u64,
}

impl Sequence {
    fn place(&mut self, header: &Header, payload: usize) -> Result<Placed, DeviceError> {
        if usize::from(header.frame_bytes) != self.frame_bytes
            || !payload.is_multiple_of(self.frame_bytes)
        {
            return Err(DeviceError::Io(format!(
                "iqlink sent a {payload}-byte payload of {}-byte frames, expected {}-byte frames",
                header.frame_bytes, self.frame_bytes
            )));
        }
        let expected = *self.expected.get_or_insert(header.index);
        if header.index < expected {
            return Ok(Placed::Stale);
        }
        self.expected = Some(header.index + (payload / self.frame_bytes) as u64);
        let board = header.board_lost.saturating_sub(self.board_lost);
        self.board_lost = self.board_lost.max(header.board_lost);
        let gap = header.index - expected;
        if gap == 0 {
            return Ok(Placed::Next);
        }
        let estimated = board.min(gap);
        Ok(Placed::After(BlockGap {
            exact: (gap - estimated) * self.lanes,
            estimated: estimated * self.lanes,
        }))
    }
}

fn merge(first: Option<BlockGap>, second: BlockGap) -> BlockGap {
    first.map_or(second, |first| BlockGap {
        exact: first.exact + second.exact,
        estimated: first.estimated + second.estimated,
    })
}

struct State {
    sequence: Sequence,
    datagram: Vec<u8>,
    held: usize,
    carried: Option<BlockGap>,
    returned: Option<BlockGap>,
    stale: u64,
}

pub(crate) struct IqlinkStream {
    control: Connection,
    socket: UdpSocket,
    pool: BlockPool,
    refill: usize,
    stopper: Stopper,
    state: Mutex<State>,
}

impl std::fmt::Debug for IqlinkStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IqlinkStream")
            .field("refill", &self.refill)
            .finish_non_exhaustive()
    }
}

enum Fill {
    Room,
    Full,
}

impl IqlinkStream {
    fn new(control: Connection, socket: UdpSocket, pool: BlockPool, shape: &Shape) -> Self {
        let stopper = Stopper::socket(control.stop_handle());
        let payload = DATAGRAM_BYTES - HEADER_BYTES;
        Self {
            control,
            socket,
            pool,
            refill: shape.refill.max(payload),
            stopper,
            state: Mutex::new(State {
                sequence: Sequence {
                    frame_bytes: shape.frame_bytes.max(1),
                    lanes: shape.lanes,
                    expected: None,
                    board_lost: 0,
                },
                datagram: vec![0; DATAGRAM_BYTES],
                held: 0,
                carried: None,
                returned: None,
                stale: 0,
            }),
        }
    }

    pub(crate) fn stopper(&self) -> Stopper {
        self.stopper.clone()
    }

    pub(crate) fn next_block(&self, timeout: Duration) -> Next<Block> {
        if self.stopper.is_stopped() {
            return Next::Ended;
        }
        let mut state = lock(&self.state);
        match self.gather(&mut state, timeout) {
            Ok(Some(block)) => Next::Block(block),
            Ok(None) => self.idle(),
            Err(e) => {
                self.control.fail(e.to_string());
                Next::Ended
            }
        }
    }

    fn gather(&self, state: &mut State, timeout: Duration) -> Result<Option<Block>, DeviceError> {
        let mut block = self.pool.take(self.refill);
        let mut filled = 0;
        let mut gap = state.carried.take();
        if state.held > 0 {
            let held = std::mem::take(&mut state.held);
            let payload = &state.datagram[HEADER_BYTES..held];
            block.bytes_mut()[..payload.len()].copy_from_slice(payload);
            filled = payload.len();
        }
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline && !self.stopper.is_stopped() {
            if let Fill::Full = self.receive(state, &mut block, &mut filled, &mut gap)? {
                break;
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

    fn receive(
        &self,
        state: &mut State,
        block: &mut Block,
        filled: &mut usize,
        gap: &mut Option<BlockGap>,
    ) -> Result<Fill, DeviceError> {
        let n = match self.socket.recv(&mut state.datagram) {
            Ok(n) => n,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Ok(Fill::Room);
            }
            Err(e) => return Err(io_error(e)),
        };
        let header = Header::decode(&state.datagram[..n])
            .map_err(|e| DeviceError::Io(format!("iqlink: {e}")))?;
        let payload = n - HEADER_BYTES;
        match state.sequence.place(&header, payload)? {
            Placed::Stale => {
                state.stale += 1;
                tracing::debug!(stale = state.stale, "iqlink datagram arrived out of order");
                return Ok(Fill::Room);
            }
            Placed::After(found) if *filled > 0 => {
                state.held = n;
                state.carried = Some(found);
                return Ok(Fill::Full);
            }
            Placed::After(found) => *gap = Some(merge(gap.take(), found)),
            Placed::Next => {}
        }
        if *filled + payload > self.refill {
            state.held = n;
            return Ok(Fill::Full);
        }
        block.bytes_mut()[*filled..*filled + payload]
            .copy_from_slice(&state.datagram[HEADER_BYTES..n]);
        *filled += payload;
        Ok(if *filled + payload > self.refill {
            Fill::Full
        } else {
            Fill::Room
        })
    }

    fn idle(&self) -> Next<Block> {
        let mut line = [0u8; 256];
        match self.control.read(&mut line, Duration::ZERO) {
            Read::Idle => Next::Idle,
            Read::Got(n) => {
                let said = String::from_utf8_lossy(&line[..n]);
                let reason = match Reply::parse(&said) {
                    Ok(Reply::Refused(reason)) => reason,
                    _ => said.trim().to_string(),
                };
                self.control.fail(format!("iqlink: {reason}"));
                Next::Ended
            }
            Read::Ended => Next::Ended,
        }
    }

    pub(crate) fn block_gap(&self) -> Option<BlockGap> {
        lock(&self.state).returned.take()
    }

    pub(crate) fn failure(&self) -> StreamFailure {
        self.control.failure()
    }
}

impl Drop for IqlinkStream {
    fn drop(&mut self) {
        self.control.close();
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        thread,
    };

    use super::*;

    fn sequence() -> Sequence {
        Sequence {
            frame_bytes: 4,
            lanes: 2,
            expected: None,
            board_lost: 0,
        }
    }

    fn header(index: u64, board_lost: u64) -> Header {
        Header {
            frame_bytes: 4,
            index,
            board_lost,
        }
    }

    fn placed(seq: &mut Sequence, index: u64, board_lost: u64) -> Placed {
        seq.place(&header(index, board_lost), 40).expect("placed")
    }

    #[test]
    fn datagrams_in_order_carry_no_gap() {
        let mut seq = sequence();
        assert_eq!(placed(&mut seq, 500, 0), Placed::Next);
        assert_eq!(placed(&mut seq, 510, 0), Placed::Next);
    }

    #[test]
    fn a_lost_datagram_is_an_exact_gap_in_samples_of_every_lane() {
        let mut seq = sequence();
        placed(&mut seq, 0, 0);
        assert_eq!(
            placed(&mut seq, 15, 0),
            Placed::After(BlockGap {
                exact: 10,
                estimated: 0
            })
        );
    }

    #[test]
    fn a_jump_the_radio_estimated_is_reported_as_estimated() {
        let mut seq = sequence();
        placed(&mut seq, 0, 0);
        assert_eq!(
            placed(&mut seq, 1_010, 1_000),
            Placed::After(BlockGap {
                exact: 0,
                estimated: 2_000
            })
        );
        assert_eq!(
            placed(&mut seq, 2_520, 2_000),
            Placed::After(BlockGap {
                exact: 1_000,
                estimated: 2_000
            })
        );
    }

    #[test]
    fn a_late_datagram_is_stale_and_a_foreign_shape_is_refused() {
        let mut seq = sequence();
        placed(&mut seq, 100, 0);
        assert_eq!(placed(&mut seq, 50, 0), Placed::Stale);
        assert!(seq.place(&header(110, 0), 41).is_err());
        let mut wide = header(110, 0);
        wide.frame_bytes = 8;
        assert!(seq.place(&wide, 40).is_err());
    }

    fn daemon(datagrams: Vec<(u64, u64, usize)>) -> (Endpoint, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let handle = thread::spawn(move || {
            let (mut control, peer) = listener.accept().expect("accept");
            let mut line = String::new();
            BufReader::new(control.try_clone().expect("clone"))
                .read_line(&mut line)
                .expect("start");
            let start = Start::parse(&line).expect("start line");
            control
                .write_all(Reply::Streaming { frame_bytes: 4 }.line().as_bytes())
                .expect("reply");
            let udp = UdpSocket::bind("127.0.0.1:0").expect("udp");
            udp.connect(SocketAddr::new(peer.ip(), start.udp_port))
                .expect("connect");
            for (index, board_lost, frames) in datagrams {
                let mut datagram = header(index, board_lost).encode().to_vec();
                datagram.extend((0..frames * 4).map(|i| (index as usize * 4 + i) as u8));
                udp.send(&datagram).expect("datagram");
            }
            let mut rest = [0u8; 16];
            while matches!(std::io::Read::read(&mut control, &mut rest), Ok(n) if n > 0) {}
        });
        let at = Endpoint::parse(&format!("127.0.0.1:{port}"), 1).expect("endpoint");
        (at, handle)
    }

    fn request() -> Request<'static> {
        Request {
            device: "cf-ad9361-lpc",
            elements: vec![0, 1],
            frames: 1_000,
            frame_bytes: 4,
            lanes: 1,
        }
    }

    fn next(stream: &IqlinkStream) -> Block {
        for _ in 0..50 {
            if let Next::Block(block) = stream.next_block(Duration::from_millis(100)) {
                return block;
            }
        }
        panic!("no block arrived");
    }

    #[test]
    fn a_stream_assembles_datagrams_and_cuts_blocks_at_gaps() {
        let (at, daemon) = daemon(vec![(0, 0, 10), (10, 0, 10), (30, 0, 10), (40, 0, 5)]);
        let stream = connect(&at, request(), BlockPool::default())
            .expect("connect")
            .expect("iqlink answered");
        let first = next(&stream);
        assert_eq!(first.len(), 80);
        assert_eq!(first[0], 0);
        assert_eq!(stream.block_gap(), None);
        let second = next(&stream);
        assert_eq!(second.len(), 60);
        assert_eq!(second[0], 120);
        assert_eq!(
            stream.block_gap(),
            Some(BlockGap {
                exact: 10,
                estimated: 0
            })
        );
        sdrmm_device::StopHandle::stop(&stream.stopper());
        assert!(matches!(
            stream.next_block(Duration::from_millis(10)),
            Next::Ended
        ));
        drop(stream);
        daemon.join().expect("daemon");
    }

    #[test]
    fn nothing_listening_means_no_iqlink() {
        let at = Endpoint::parse("127.0.0.1:1", 1).expect("endpoint");
        assert!(
            connect(&at, request(), BlockPool::default())
                .expect("probe")
                .is_none()
        );
    }

    #[test]
    fn a_refusal_falls_back_quietly() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let refuser = thread::spawn(move || {
            let (mut control, _) = listener.accept().expect("accept");
            let mut line = String::new();
            BufReader::new(control.try_clone().expect("clone"))
                .read_line(&mut line)
                .expect("start");
            control
                .write_all(Reply::Refused("busy".into()).line().as_bytes())
                .expect("reply");
        });
        let at = Endpoint::parse(&format!("127.0.0.1:{port}"), 1).expect("endpoint");
        assert!(
            connect(&at, request(), BlockPool::default())
                .expect("probe")
                .is_none()
        );
        refuser.join().expect("refuser");
    }
}
