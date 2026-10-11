use std::{
    io::{ErrorKind, Read, Write},
    net::{IpAddr, TcpStream},
    time::Duration,
};

use sdrmm_device::{DeviceError, net::Endpoint};
use serde::{Deserialize, Serialize};

use crate::caps::Remote;

pub(crate) const HTTP_PORT: u16 = 80;
pub(crate) const PROTOCOL: u32 = 2;
const PATH: &str = "/api/rx";
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REPLY: usize = 16 << 10;
const MHZ: u64 = 1_000_000;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Op {
    Status,
    Configure,
    Start,
    Stop,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Tuning {
    pub(crate) frequency: u64,
    pub(crate) rate: u32,
    pub(crate) gain: u32,
    pub(crate) agc: u32,
    pub(crate) bandwidth: u64,
}

impl Tuning {
    pub(crate) fn of(remote: &Remote) -> Self {
        Self {
            frequency: u64::from(remote.center_mhz) * MHZ,
            rate: remote.rate,
            gain: remote.gain,
            agc: u32::from(remote.agc),
            bandwidth: u64::from(remote.bandwidth_mhz) * MHZ,
        }
    }

    pub(crate) fn mhz(hz: u64) -> u32 {
        u32::try_from(hz / MHZ).unwrap_or(u32::MAX)
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Request {
    op: Op,
    #[serde(flatten)]
    tuning: Option<Tuning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    port: Option<u16>,
}

impl Request {
    pub(crate) const fn status() -> Self {
        Self {
            op: Op::Status,
            tuning: None,
            port: None,
        }
    }

    pub(crate) const fn configure(tuning: Tuning) -> Self {
        Self {
            op: Op::Configure,
            tuning: Some(tuning),
            port: None,
        }
    }

    pub(crate) const fn start(tuning: Tuning, port: u16) -> Self {
        Self {
            op: Op::Start,
            tuning: Some(tuning),
            port: Some(port),
        }
    }

    pub(crate) const fn stop() -> Self {
        Self {
            op: Op::Stop,
            tuning: None,
            port: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Status {
    pub(crate) protocol: u32,
    pub(crate) gain_max: u32,
    #[serde(flatten)]
    pub(crate) tuning: Tuning,
}

#[derive(Deserialize)]
struct Refusal {
    error: String,
}

pub(crate) struct Call {
    tcp: TcpStream,
    endpoint: Endpoint,
}

impl Call {
    pub(crate) fn open(endpoint: &Endpoint) -> Result<Self, DeviceError> {
        let tcp = endpoint.connect()?;
        tcp.set_read_timeout(Some(REPLY_TIMEOUT)).map_err(io)?;
        tcp.set_write_timeout(Some(REPLY_TIMEOUT)).map_err(io)?;
        Ok(Self {
            tcp,
            endpoint: endpoint.clone(),
        })
    }

    pub(crate) fn local(&self) -> Result<IpAddr, DeviceError> {
        Ok(self.tcp.local_addr().map_err(io)?.ip())
    }

    pub(crate) fn peer(&self) -> Result<IpAddr, DeviceError> {
        Ok(self.tcp.peer_addr().map_err(io)?.ip())
    }

    pub(crate) fn send(mut self, request: &Request) -> Result<Status, DeviceError> {
        let body = serde_json::to_string(request).map_err(|e| DeviceError::Io(e.to_string()))?;
        let head = format!(
            "POST {PATH} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n",
            self.endpoint.authority(),
            body.len()
        );
        self.tcp
            .write_all(format!("{head}{body}").as_bytes())
            .map_err(|e| self.failed(&e))?;
        let (code, body) = self.receive()?;
        decode(&self.endpoint, code, &body)
    }

    fn receive(&mut self) -> Result<(u16, String), DeviceError> {
        let mut reply = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            if let Some(parsed) = parse_reply(&reply) {
                return parsed.map_err(|why| self.bad(&why));
            }
            if reply.len() > MAX_REPLY {
                return Err(self.bad("reply too long"));
            }
            match self.tcp.read(&mut chunk) {
                Ok(0) => return Err(self.bad("closed before a whole reply")),
                Ok(n) => reply.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(self.failed(&e)),
            }
        }
    }

    fn bad(&self, why: &str) -> DeviceError {
        DeviceError::Io(format!("{}: {why}", self.endpoint))
    }

    fn failed(&self, error: &std::io::Error) -> DeviceError {
        DeviceError::Io(format!("{}: {error}", self.endpoint))
    }
}

pub(crate) fn post(endpoint: &Endpoint, request: &Request) -> Result<Status, DeviceError> {
    Call::open(endpoint)?.send(request)
}

fn io(error: std::io::Error) -> DeviceError {
    DeviceError::Io(error.to_string())
}

fn decode(endpoint: &Endpoint, code: u16, body: &str) -> Result<Status, DeviceError> {
    if let Ok(Refusal { error }) = serde_json::from_str(body) {
        let message = format!("{endpoint}: {error}");
        return Err(if error.contains("owned") {
            DeviceError::InUse(message)
        } else {
            DeviceError::Unsupported(message)
        });
    }
    if code != 200 {
        return Err(DeviceError::Io(format!("{endpoint}: HTTP {code}")));
    }
    serde_json::from_str(body)
        .map_err(|e| DeviceError::Unsupported(format!("{endpoint}: not ESP-SDR RX: {e}")))
}

fn parse_reply(bytes: &[u8]) -> Option<Result<(u16, String), String>> {
    let end = bytes.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&bytes[..end]);
    let mut lines = head.split("\r\n");
    let code = lines
        .next()
        .and_then(|status| status.split(' ').nth(1))
        .and_then(|code| code.parse().ok());
    let Some(code) = code else {
        return Some(Err(format!("bad status line in {head:?}")));
    };
    let length = lines.find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())?
    });
    let Some(length) = length else {
        return Some(Err("reply has no Content-Length".to_string()));
    };
    let body = &bytes[end + 4..];
    (body.len() >= length).then(|| Ok((code, String::from_utf8_lossy(&body[..length]).into())))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{
        io::BufRead,
        net::{SocketAddr, TcpListener},
        thread,
    };

    use super::*;

    pub(crate) const STATUS: &str = r#"{"product":"ESP-SDR RX","protocol":2,"transport":"idle","epoch":3,"frequency":2442000000,"rate":16000000,"dc_correction":1,"gain":40,"agc":0,"gain_max":72,"bandwidth":0,"effective_bandwidth":16000000,"capture_drops":0,"transport_drops":0,"time_us":123,"usb_connected":false,"ip":"10.0.0.7","ethernet_mbps":1000}"#;

    pub(crate) fn reply(code: u16, body: &str) -> String {
        format!(
            "HTTP/1.1 {code} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
    }

    pub(crate) fn read_request(tcp: &TcpStream) -> serde_json::Value {
        let mut reader = std::io::BufReader::new(tcp);
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).expect("header");
            if line == "\r\n" {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length: ") {
                length = value.trim().parse().expect("length");
            }
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).expect("body");
        serde_json::from_slice(&body).expect("json")
    }

    fn answering(
        code: u16,
        body: &'static str,
    ) -> (Endpoint, thread::JoinHandle<serde_json::Value>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let at: SocketAddr = listener.local_addr().expect("addr");
        let server = thread::spawn(move || {
            let (mut tcp, _) = listener.accept().expect("accept");
            let request = read_request(&tcp);
            tcp.write_all(reply(code, body).as_bytes()).expect("reply");
            request
        });
        (
            Endpoint::parse(&at.to_string(), HTTP_PORT).expect("endpoint"),
            server,
        )
    }

    #[test]
    fn requests_carry_only_what_the_operation_needs() {
        let tuning = Tuning {
            frequency: 2_442_000_000,
            rate: 16_000_000,
            gain: 40,
            agc: 0,
            bandwidth: 0,
        };
        let json = |request: &Request| serde_json::to_string(request).expect("json");
        assert_eq!(json(&Request::status()), r#"{"op":"status"}"#);
        assert_eq!(json(&Request::stop()), r#"{"op":"stop"}"#);
        assert_eq!(
            json(&Request::start(tuning, 50_000)),
            r#"{"op":"start","frequency":2442000000,"rate":16000000,"gain":40,"agc":0,"bandwidth":0,"port":50000}"#
        );
    }

    #[test]
    fn a_status_reply_is_read_whole() {
        let (at, server) = answering(200, STATUS);
        let status = post(&at, &Request::status()).expect("status");
        assert_eq!(server.join().expect("server")["op"], "status");
        assert_eq!(status.protocol, PROTOCOL);
        assert_eq!(status.gain_max, 72);
        assert_eq!(status.tuning.frequency, 2_442_000_000);
    }

    #[test]
    fn a_refusal_names_its_reason() {
        let (at, server) = answering(
            400,
            r#"{"error":"Receiver is owned by the other transport"}"#,
        );
        let refused = post(&at, &Request::stop());
        server.join().expect("server");
        assert!(matches!(refused, Err(DeviceError::InUse(why)) if why.contains("other transport")));
        let (at, server) = answering(400, r#"{"error":"Unsupported receiver setting"}"#);
        assert!(matches!(
            post(&at, &Request::status()),
            Err(DeviceError::Unsupported(_))
        ));
        server.join().expect("server");
    }

    #[test]
    fn a_reply_waits_for_its_whole_body() {
        let whole = reply(200, "{}");
        assert!(parse_reply(&whole.as_bytes()[..whole.len() - 1]).is_none());
        assert_eq!(
            parse_reply(whole.as_bytes()),
            Some(Ok((200, "{}".to_string())))
        );
        assert!(matches!(
            parse_reply(b"HTTP/1.1 200 OK\r\n\r\n"),
            Some(Err(_))
        ));
    }

    #[test]
    fn whole_megahertz_survive_the_round_trip() {
        assert_eq!(Tuning::mhz(2_442_000_000), 2442);
        assert_eq!(Tuning::mhz(0), 0);
    }
}
