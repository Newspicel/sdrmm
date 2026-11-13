use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpStream, UdpSocket},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use sdrmm_iqlink::{MAX_LINE_BYTES, Reply, Start};

use crate::{
    cpu,
    iio::{Buffer, IioError, Paths},
    loss::Loss,
    packets::Packetizer,
    ring::Ring,
    udp::Sender,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(200);
const SEND_CPU: usize = 1;
const LATE: &str = "no request in time";

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("control connection: {0}")]
    Control(#[from] io::Error),
    #[error(transparent)]
    Iio(#[from] IioError),
    #[error(transparent)]
    Protocol(#[from] sdrmm_iqlink::Error),
    #[error("{0}")]
    Refused(String),
    #[error("the radio stopped delivering: {0}")]
    Radio(io::Error),
    #[error("the receiver went away: {0}")]
    Network(io::Error),
    #[error("the sample reader panicked")]
    Panicked,
}

pub fn serve(control: TcpStream, paths: &Paths) -> Result<(), SessionError> {
    let peer = control.peer_addr()?;
    let mut reader = BufReader::new(control.try_clone()?);
    let start = match read_start(&mut reader, &control) {
        Ok(start) => start,
        Err(e) => return refuse(&control, e),
    };
    let (buffer, sender) = match open(paths, &start, peer) {
        Ok(opened) => opened,
        Err(e) => return refuse(&control, e),
    };
    eprintln!(
        "iqlinkd: streaming {} {:?} to {}:{}{}",
        start.device,
        start.elements,
        peer.ip(),
        start.udp_port,
        if sender.offloads() {
            ""
        } else {
            " without segmentation offload"
        }
    );
    reply(
        &control,
        &Reply::Streaming {
            frame_bytes: buffer.frame_bytes(),
        },
    )?;
    stream(&control, reader, buffer, sender)
}

fn refuse(control: &TcpStream, error: SessionError) -> Result<(), SessionError> {
    reply(control, &Reply::Refused(error.to_string()))?;
    Err(error)
}

fn reply(mut control: &TcpStream, reply: &Reply) -> io::Result<()> {
    control.write_all(reply.line().as_bytes())
}

fn read_start(
    reader: &mut BufReader<TcpStream>,
    control: &TcpStream,
) -> Result<Start, SessionError> {
    let line = read_request(reader, control, Instant::now() + REQUEST_TIMEOUT)?;
    control.set_read_timeout(None)?;
    Ok(Start::parse(&line)?)
}

/// Reads one line within `deadline` and `MAX_LINE_BYTES`, so a peer trickling bytes cannot hold
/// the radio's only session open.
fn read_request(
    reader: &mut BufReader<TcpStream>,
    control: &TcpStream,
    deadline: Instant,
) -> Result<String, SessionError> {
    let mut line = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(refused(LATE));
        }
        control.set_read_timeout(Some(left))?;
        let available = match reader.fill_buf() {
            Ok(available) => available,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Err(refused(LATE));
            }
            Err(e) => return Err(e.into()),
        };
        if available.is_empty() {
            return Err(refused("closed before a request"));
        }
        let end = available.iter().position(|&byte| byte == b'\n');
        let taken = end.map_or(available.len(), |at| at + 1);
        line.extend_from_slice(&available[..taken]);
        reader.consume(taken);
        if line.len() > MAX_LINE_BYTES {
            return Err(refused("request line too long"));
        }
        if end.is_some() {
            return String::from_utf8(line).map_err(|_| refused("request is not UTF-8"));
        }
    }
}

fn refused(reason: &str) -> SessionError {
    SessionError::Refused(reason.to_owned())
}

fn open(paths: &Paths, start: &Start, peer: SocketAddr) -> Result<(Buffer, Sender), SessionError> {
    let buffer = Buffer::open(paths, &start.device, &start.elements, start.buffer_frames)?;
    let payload_bytes = Packetizer::new(buffer.frame_bytes()).payload_bytes();
    if payload_bytes == 0 {
        return Err(SessionError::Refused(format!(
            "a frame of {} bytes does not fit a datagram",
            buffer.frame_bytes()
        )));
    }
    let local: SocketAddr = if peer.is_ipv4() {
        (Ipv4Addr::UNSPECIFIED, 0).into()
    } else {
        (Ipv6Addr::UNSPECIFIED, 0).into()
    };
    let socket = UdpSocket::bind(local)?;
    socket.connect(SocketAddr::new(peer.ip(), start.udp_port))?;
    Ok((buffer, Sender::new(socket, payload_bytes)))
}

fn stream(
    control: &TcpStream,
    reader: BufReader<TcpStream>,
    buffer: Buffer,
    sender: Sender,
) -> Result<(), SessionError> {
    let stop = Arc::new(AtomicBool::new(false));
    let watcher = watch(reader, stop.clone());
    let packets = Packetizer::new(buffer.frame_bytes());
    let loss = Loss::new(buffer.queued_chunks());
    if !buffer.mapped() {
        eprintln!("iqlinkd: reading samples with read(), expect a lower ceiling");
    }
    let ring = Ring::start(buffer, &stop)?;
    cpu::pin(SEND_CPU);
    let pumped = pump(ring, sender, packets, loss, &stop);
    stop.store(true, Ordering::Release);
    if let Err(e) = &pumped
        && let Err(said) = reply(control, &Reply::Refused(e.to_string()))
    {
        eprintln!("iqlinkd: could not tell the receiver why: {said}");
    }
    if let Err(e) = control.shutdown(Shutdown::Both) {
        eprintln!("iqlinkd: closing the control connection: {e}");
    }
    watcher.join().map_err(|_| SessionError::Panicked)?;
    pumped
}

fn watch(mut reader: BufReader<TcpStream>, stop: Arc<AtomicBool>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut sink = [0u8; 64];
        while matches!(reader.read(&mut sink), Ok(n) if n > 0) {}
        stop.store(true, Ordering::Release);
    })
}

fn pump(
    mut ring: Ring,
    mut sender: Sender,
    mut packets: Packetizer,
    mut loss: Loss,
    stop: &AtomicBool,
) -> Result<(), SessionError> {
    let pumped = pump_until_stopped(&mut ring, &mut sender, &mut packets, &mut loss, stop);
    stop.store(true, Ordering::Release);
    pumped.and(ring.finish())
}

fn pump_until_stopped(
    ring: &mut Ring,
    sender: &mut Sender,
    packets: &mut Packetizer,
    loss: &mut Loss,
    stop: &AtomicBool,
) -> Result<(), SessionError> {
    while !stop.load(Ordering::Acquire) {
        let Some(lease) = ring.acquire(POLL)? else {
            continue;
        };
        let lost = loss.book(&lease.chunk);
        if lost > 0 {
            eprintln!("iqlinkd: about {lost} frames lost on the radio");
            packets.skip(lost, loss.lost());
        }
        sender
            .send(packets.datagrams(&lease.bytes))
            .map_err(SessionError::Network)?;
        ring.release(lease);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead, BufReader, Write},
        net::{TcpListener, TcpStream, UdpSocket},
        thread,
        time::Duration,
    };

    use sdrmm_iqlink::{HEADER_BYTES, Header, Reply, Start};

    use super::*;
    use crate::iio::fake;

    fn server(paths: Paths) -> (SocketAddr, thread::JoinHandle<Result<(), SessionError>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = thread::spawn(move || {
            let (control, _) = listener.accept().expect("accept");
            serve(control, &paths)
        });
        (addr, handle)
    }

    fn ask(addr: SocketAddr, start: &Start) -> (TcpStream, Reply) {
        let mut control = TcpStream::connect(addr).expect("connect");
        control.write_all(start.line().as_bytes()).expect("start");
        let mut line = String::new();
        BufReader::new(control.try_clone().expect("clone"))
            .read_line(&mut line)
            .expect("reply");
        (control, Reply::parse(&line).expect("reply line"))
    }

    fn start(device: &str, udp_port: u16) -> Start {
        Start {
            device: device.to_string(),
            elements: vec![0, 1],
            buffer_frames: 100,
            udp_port,
        }
    }

    #[test]
    fn a_session_streams_the_radio_in_order_until_it_runs_dry() {
        let samples: Vec<u8> = (0..1_200u32).map(|i| (i % 251) as u8).collect();
        let root = tempfile::tempdir().expect("tempdir");
        let (addr, handle) = server(fake::radio(root.path(), &samples));
        let udp = UdpSocket::bind("127.0.0.1:0").expect("udp");
        udp.set_read_timeout(Some(Duration::from_secs(2)))
            .expect("timeout");
        let port = udp.local_addr().expect("addr").port();
        let (_control, reply) = ask(addr, &start(fake::NAME, port));
        assert_eq!(reply, Reply::Streaming { frame_bytes: 4 });
        let mut got = Vec::new();
        let mut buf = [0u8; 2048];
        while got.len() < samples.len() {
            let n = udp.recv(&mut buf).expect("datagram");
            let header = Header::decode(&buf[..n]).expect("header");
            assert_eq!(header.index, got.len() as u64 / 4);
            assert_eq!(header.board_lost, 0);
            got.extend_from_slice(&buf[HEADER_BYTES..n]);
        }
        assert_eq!(got, samples);
        let ended = handle.join().expect("session thread");
        assert!(matches!(ended, Err(SessionError::Radio(_))), "{ended:?}");
        assert_eq!(fake::read(&paths_of(root.path()), "buffer/enable"), "0");
    }

    fn paths_of(root: &std::path::Path) -> Paths {
        Paths {
            devices: root.join("devices"),
            dev: root.join("dev"),
        }
    }

    #[test]
    fn an_unknown_device_is_refused_with_a_reason() {
        let root = tempfile::tempdir().expect("tempdir");
        let (addr, handle) = server(fake::radio(root.path(), &[]));
        let (_control, reply) = ask(addr, &start("missing", 9));
        let Reply::Refused(reason) = reply else {
            panic!("expected a refusal, got {reply:?}");
        };
        assert!(reason.contains("missing"), "{reason}");
        assert!(handle.join().expect("session thread").is_err());
    }

    #[test]
    fn an_endless_request_line_is_refused() {
        let root = tempfile::tempdir().expect("tempdir");
        let (addr, handle) = server(fake::radio(root.path(), &[]));
        let mut control = TcpStream::connect(addr).expect("connect");
        control
            .write_all(&vec![b'x'; MAX_LINE_BYTES + 1])
            .expect("flood");
        let mut line = String::new();
        BufReader::new(control.try_clone().expect("clone"))
            .read_line(&mut line)
            .expect("reply");
        assert_eq!(
            Reply::parse(&line).expect("reply line"),
            Reply::Refused("request line too long".to_owned())
        );
        assert!(handle.join().expect("session thread").is_err());
    }

    #[test]
    fn a_trickled_request_runs_out_of_time() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let mut client = TcpStream::connect(listener.local_addr().expect("addr")).expect("connect");
        let (control, _) = listener.accept().expect("accept");
        let mut reader = BufReader::new(control.try_clone().expect("clone"));
        client.write_all(b"START").expect("partial");
        let deadline = Instant::now() + Duration::from_millis(100);
        let read = read_request(&mut reader, &control, deadline);
        assert!(
            matches!(&read, Err(SessionError::Refused(reason)) if reason == LATE),
            "{read:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn closing_the_control_connection_ends_the_session() {
        let root = tempfile::tempdir().expect("tempdir");
        let paths = fake::radio(root.path(), &[]);
        let node = paths.dev.join(fake::ID);
        std::fs::remove_file(&node).expect("remove");
        std::os::unix::fs::symlink("/dev/zero", &node).expect("endless radio");
        let (addr, handle) = server(paths);
        let udp = UdpSocket::bind("127.0.0.1:0").expect("udp");
        let port = udp.local_addr().expect("addr").port();
        let (control, reply) = ask(addr, &start(fake::NAME, port));
        assert_eq!(reply, Reply::Streaming { frame_bytes: 4 });
        control.shutdown(Shutdown::Both).expect("close");
        let ended = handle.join().expect("session thread");
        assert!(ended.is_ok(), "{ended:?}");
    }
}
