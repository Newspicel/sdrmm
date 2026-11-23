//! ICE over TCP (RFC 6544): we dial the SFU's passive candidates and frame every packet with
//! its length (RFC 4571). Networks that drop UDP still reach the SFU this way.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::mpsc::{self, error::TrySendError};
use tokio::time::timeout;

/// The SFU offers a passive candidate per address it has; a few cover them all.
const MAX_LINKS: usize = 8;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const QUEUE: usize = 64;

/// One packet received over TCP and who sent it.
pub(crate) type Packet = (SocketAddr, Vec<u8>);

/// The TCP connections of one peer, opened as ICE first sends to an address.
pub(crate) struct TcpLinks {
    links: HashMap<SocketAddr, mpsc::Sender<Vec<u8>>>,
    heard_tx: mpsc::Sender<Packet>,
    heard: mpsc::Receiver<Packet>,
}

impl TcpLinks {
    pub(crate) fn new() -> Self {
        let (heard_tx, heard) = mpsc::channel(QUEUE);
        Self {
            links: HashMap::new(),
            heard_tx,
            heard,
        }
    }

    /// Sends `packet` to `to`, dialling first. Like a datagram, a packet to an address that
    /// does not answer is lost.
    pub(crate) fn send(&mut self, to: SocketAddr, packet: &[u8]) {
        let Some(framed) = frame(packet) else {
            tracing::debug!(len = packet.len(), "packet too long for TCP");
            return;
        };
        let framed = match self.links.get(&to) {
            None => framed,
            Some(link) => match link.try_send(framed) {
                Ok(()) => return,
                Err(TrySendError::Full(_)) => return tracing::debug!(%to, "TCP packet dropped"),
                Err(TrySendError::Closed(framed)) => framed,
            },
        };
        if self.links.remove(&to).is_some() {
            tracing::debug!(%to, "TCP candidate dialled again");
        }
        self.dial(to, framed);
    }

    /// Opens a connection to `to` with `first` queued on it.
    fn dial(&mut self, to: SocketAddr, first: Vec<u8>) {
        self.links.retain(|_, link| !link.is_closed());
        if self.links.len() >= MAX_LINKS {
            return tracing::warn!(%to, "too many TCP candidates, packet dropped");
        }
        let (outgoing_tx, outgoing) = mpsc::channel(QUEUE);
        if outgoing_tx.try_send(first).is_err() {
            tracing::debug!(%to, "TCP packet dropped");
        }
        tokio::spawn(link(to, outgoing, self.heard_tx.clone()));
        self.links.insert(to, outgoing_tx);
    }

    /// The next packet any connection received. Pends while there is none.
    pub(crate) async fn recv(&mut self) -> Option<Packet> {
        self.heard.recv().await
    }
}

/// `packet` behind its 16-bit length.
fn frame(packet: &[u8]) -> Option<Vec<u8>> {
    let len = u16::try_from(packet.len()).ok()?;
    let mut framed = Vec::with_capacity(packet.len() + 2);
    framed.extend_from_slice(&len.to_be_bytes());
    framed.extend_from_slice(packet);
    Some(framed)
}

/// One connection, from dialling until either side closes or the peer lets go.
async fn link(to: SocketAddr, outgoing: mpsc::Receiver<Vec<u8>>, heard: mpsc::Sender<Packet>) {
    let stream = match timeout(CONNECT_TIMEOUT, TcpStream::connect(to)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(error)) => return tracing::debug!(%to, %error, "TCP candidate refused"),
        Err(_) => return tracing::debug!(%to, "TCP candidate timed out"),
    };
    if let Err(error) = stream.set_nodelay(true) {
        tracing::debug!(%error, "TCP delay kept");
    }
    tracing::debug!(%to, "TCP candidate connected");
    let (read, write) = stream.into_split();
    tokio::select! {
        () = read_packets(to, read, heard) => {}
        () = write_packets(write, outgoing) => {}
    }
}

async fn read_packets(from: SocketAddr, mut read: OwnedReadHalf, heard: mpsc::Sender<Packet>) {
    loop {
        let Ok(len) = read.read_u16().await else {
            return;
        };
        let mut packet = vec![0; usize::from(len)];
        if read.read_exact(&mut packet).await.is_err() || heard.send((from, packet)).await.is_err()
        {
            return;
        }
    }
}

async fn write_packets(mut write: OwnedWriteHalf, mut outgoing: mpsc::Receiver<Vec<u8>>) {
    while let Some(framed) = outgoing.recv().await {
        if let Err(error) = write.write_all(&framed).await {
            return tracing::debug!(%error, "TCP candidate lost");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn packets_go_behind_their_length() {
        assert_eq!(frame(&[7, 8, 9]), Some(vec![0, 3, 7, 8, 9]));
        assert_eq!(frame(&vec![0; 70_000]), None);
    }

    #[tokio::test]
    async fn packets_cross_a_dialled_connection_both_ways() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let sfu = listener.local_addr().unwrap();
        let mut links = TcpLinks::new();

        links.send(sfu, b"ping");
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut got = [0; 6];
        stream.read_exact(&mut got).await.unwrap();
        stream
            .write_all(&[0, 4, b'p', b'o', b'n', b'g'])
            .await
            .unwrap();

        assert_eq!(&got, b"\0\x04ping");
        assert_eq!(links.recv().await, Some((sfu, b"pong".to_vec())));
    }

    #[tokio::test]
    async fn an_address_that_refused_is_dialled_again() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let closed = listener.local_addr().unwrap();
        drop(listener);
        let mut links = TcpLinks::new();

        links.send(closed, b"ping");
        timeout(Duration::from_secs(2), async {
            while !links.links[&closed].is_closed() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        links.send(closed, b"again");

        assert!(!links.links[&closed].is_closed());
        let none = timeout(Duration::from_millis(200), links.recv()).await;
        assert!(none.is_err());
    }

    #[tokio::test]
    async fn links_beyond_the_cap_are_not_opened() {
        let mut listeners = Vec::new();
        let mut links = TcpLinks::new();
        for _ in 0..=MAX_LINKS {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            links.send(listener.local_addr().unwrap(), b"ping");
            listeners.push(listener);
        }

        assert_eq!(links.links.len(), MAX_LINKS);
    }
}
