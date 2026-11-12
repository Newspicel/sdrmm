use std::{future::Future, io, pin::Pin, time::Duration};

use axum_server::accept::Accept;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

const TLS_HANDSHAKE: u8 = 0x16;
const HEAD_LIMIT: usize = 8 * 1024;
const PATIENCE: Duration = Duration::from_secs(5);
const HEAD_END: &[u8] = b"\r\n\r\n";
const BAD_REQUEST: &str =
    "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

/// Sends a browser that asked the HTTPS port in plain HTTP to the same address over HTTPS, so
/// an old `http://` bookmark still lands.
#[derive(Clone)]
pub(crate) struct RedirectPlain<A> {
    inner: A,
}

impl<A> RedirectPlain<A> {
    pub(crate) fn before(inner: A) -> Self {
        Self { inner }
    }
}

type Accepted<T, S> = Pin<Box<dyn Future<Output = io::Result<(T, S)>> + Send>>;

impl<S, A> Accept<TcpStream, S> for RedirectPlain<A>
where
    A: Accept<TcpStream, S> + Clone + Send + 'static,
    A::Future: Send,
    S: Send + 'static,
{
    type Stream = A::Stream;
    type Service = A::Service;
    type Future = Accepted<A::Stream, A::Service>;

    fn accept(&self, mut stream: TcpStream, service: S) -> Self::Future {
        let inner = self.inner.clone();
        Box::pin(async move {
            if speaks_tls(&stream).await? {
                return inner.accept(stream, service).await;
            }
            let answered = tokio::time::timeout(PATIENCE, redirect(&mut stream)).await;
            if !matches!(answered, Ok(Ok(()))) {
                tracing::debug!("a plain HTTP request on the HTTPS port got no answer");
            }
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "plain HTTP on the HTTPS port",
            ))
        })
    }
}

async fn speaks_tls(stream: &TcpStream) -> io::Result<bool> {
    let mut first = [0u8; 1];
    let read = tokio::time::timeout(PATIENCE, stream.peek(&mut first))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "the client sent nothing"))??;
    Ok(read == 1 && first[0] == TLS_HANDSHAKE)
}

async fn redirect(stream: &mut TcpStream) -> io::Result<()> {
    let mut head = Vec::new();
    let mut chunk = [0u8; 1024];
    while !head
        .windows(HEAD_END.len())
        .any(|window| window == HEAD_END)
        && head.len() < HEAD_LIMIT
    {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        head.extend_from_slice(&chunk[..read]);
    }
    stream
        .write_all(answer(&String::from_utf8_lossy(&head)).as_bytes())
        .await?;
    stream.shutdown().await
}

fn answer(head: &str) -> String {
    match https_address(head) {
        Some(address) => format!(
            "HTTP/1.1 308 Permanent Redirect\r\nLocation: {address}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        ),
        None => BAD_REQUEST.to_owned(),
    }
}

fn https_address(head: &str) -> Option<String> {
    let mut lines = head.split("\r\n");
    let target = lines.next()?.split(' ').nth(1)?;
    let host = lines
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("host"))
        .map(|(_, value)| value.trim())?;
    (target.starts_with('/') && printable(target) && names_a_host(host))
        .then(|| format!("https://{host}{target}"))
}

fn printable(target: &str) -> bool {
    target.bytes().all(|byte| byte.is_ascii_graphic())
}

fn names_a_host(host: &str) -> bool {
    !host.is_empty()
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".-:[]".contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_is_sent_to_the_same_address_over_https() {
        let head = "GET /flow?id=1 HTTP/1.1\r\nUser-Agent: x\r\nhost: radio.local:8080\r\n\r\n";
        assert_eq!(
            https_address(head).as_deref(),
            Some("https://radio.local:8080/flow?id=1")
        );
        assert!(answer(head).starts_with("HTTP/1.1 308 "));
        let ipv6 = "GET / HTTP/1.1\r\nHost: [::1]:8080\r\n\r\n";
        assert_eq!(https_address(ipv6).as_deref(), Some("https://[::1]:8080/"));
    }

    #[test]
    fn a_request_that_names_no_usable_address_is_refused() {
        for head in [
            "GET / HTTP/1.1\r\n\r\n",
            "GET http://elsewhere.example/ HTTP/1.1\r\nHost: radio.local\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: radio.local/@elsewhere.example\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: \r\n\r\n",
            "garbage",
            "",
        ] {
            assert_eq!(https_address(head), None, "{head:?}");
            assert_eq!(answer(head), BAD_REQUEST);
        }
    }

    #[test]
    fn a_header_after_the_head_is_not_read_as_the_host() {
        let head = "POST / HTTP/1.1\r\nAccept: */*\r\n\r\nHost: body.example\r\n";
        assert_eq!(https_address(head), None);
    }
}
