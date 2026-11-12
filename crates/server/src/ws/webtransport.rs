use std::{
    io,
    net::SocketAddr,
    sync::{RwLock, atomic},
    time::Duration,
};

use axum::{
    body::Body,
    extract::Request,
    http::header::{HOST, ORIGIN},
};
use sdrmm_wire::{WEBTRANSPORT_LENGTH_BYTES, WEBTRANSPORT_PATH, WebTransportInfo};
use wtransport::{
    Connection, Endpoint, Identity, RecvStream, ServerConfig, VarInt,
    endpoint::{IncomingSession, SessionRequest, endpoint_side::Server},
    tls::{Certificate, CertificateChain, PrivateKey},
};

use super::{Link, MAX_INCOMING_BYTES, MAX_SOCKETS, PathProbe, PathSample, outbox, serve_session};
use crate::{
    AppState,
    auth::{AuthGate, Identity as Caller, ListenerRole},
    tls,
};

mod lanes;

const KEEP_ALIVE: Duration = Duration::from_secs(5);
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const CONTROL_WAIT: Duration = Duration::from_secs(10);
const ROTATE_CHECK: Duration = Duration::from_secs(60 * 60);
const ROTATE_AFTER_DAYS: i64 = 7;
const CLOSE_NORMAL: u32 = 0;
const CLOSE_NO_CONTROL: u32 = 1;

/// What `/api/transport` tells a browser: where the sessions are taken and which certificate
/// to pin.
#[derive(Default)]
pub(crate) struct Advert(RwLock<Option<WebTransportInfo>>);

impl Advert {
    pub(crate) fn current(&self) -> Option<WebTransportInfo> {
        self.0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn publish(&self, info: WebTransportInfo) {
        *self
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(info);
    }
}

struct Minted {
    identity: Identity,
    sha256: String,
    at: jiff::Timestamp,
}

fn mint() -> io::Result<Minted> {
    let pinned = tls::pinned().map_err(io::Error::other)?;
    let certificate = Certificate::from_der(pinned.certificate).map_err(io::Error::other)?;
    Ok(Minted {
        identity: Identity::new(
            CertificateChain::single(certificate),
            PrivateKey::from_der_pkcs8(pinned.key),
        ),
        sha256: pinned.sha256,
        at: jiff::Timestamp::now(),
    })
}

fn config(bind: SocketAddr, identity: Identity) -> io::Result<ServerConfig> {
    Ok(ServerConfig::builder()
        .with_bind_address(bind)
        .with_identity(identity)
        .keep_alive_interval(Some(KEEP_ALIVE))
        .max_idle_timeout(Some(IDLE_TIMEOUT))
        .map_err(io::Error::other)?
        .build())
}

/// Takes sessions on the UDP port with the same number as the listener at `bind`.
pub(crate) fn start(state: &AppState, bind: SocketAddr) -> io::Result<tokio::task::JoinHandle<()>> {
    let minted = mint()?;
    let endpoint = Endpoint::server(config(bind, minted.identity)?)?;
    let port = endpoint.local_addr()?.port();
    state.webtransport.publish(WebTransportInfo {
        port,
        certificate_sha256: minted.sha256,
    });
    tracing::info!(port, "WebTransport listening");
    let gate = AuthGate::new(state, ListenerRole::Main, true);
    Ok(tokio::spawn(run(
        endpoint,
        bind,
        state.clone(),
        gate,
        minted.at,
    )))
}

async fn run(
    endpoint: Endpoint<Server>,
    bind: SocketAddr,
    state: AppState,
    gate: AuthGate,
    mut minted_at: jiff::Timestamp,
) {
    let mut check = tokio::time::interval(ROTATE_CHECK);
    check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            incoming = endpoint.accept() => {
                tokio::spawn(admit(incoming, state.clone(), gate.clone()));
            }
            _ = check.tick() => {
                if rotation_due(minted_at, jiff::Timestamp::now()) {
                    match rotate(&endpoint, bind, &state) {
                        Ok(at) => minted_at = at,
                        Err(error) => tracing::warn!(%error, "the WebTransport certificate was not renewed"),
                    }
                }
            }
        }
    }
}

fn rotation_due(minted_at: jiff::Timestamp, now: jiff::Timestamp) -> bool {
    now.duration_since(minted_at) >= jiff::SignedDuration::from_hours(ROTATE_AFTER_DAYS * 24)
}

fn rotate(
    endpoint: &Endpoint<Server>,
    bind: SocketAddr,
    state: &AppState,
) -> io::Result<jiff::Timestamp> {
    let minted = mint()?;
    endpoint.reload_config(config(bind, minted.identity)?, false)?;
    state.webtransport.publish(WebTransportInfo {
        port: endpoint.local_addr()?.port(),
        certificate_sha256: minted.sha256,
    });
    Ok(minted.at)
}

async fn admit(incoming: IncomingSession, state: AppState, gate: AuthGate) {
    let request = match incoming.await {
        Ok(request) => request,
        Err(error) => {
            tracing::debug!(%error, "a WebTransport handshake failed");
            return;
        }
    };
    if request.path().split('?').next() != Some(WEBTRANSPORT_PATH) {
        request.not_found().await;
        return;
    }
    let Some(caller) = caller(&request, &gate) else {
        request.forbidden().await;
        return;
    };
    if state.clients.load(atomic::Ordering::Relaxed) >= MAX_SOCKETS {
        request.too_many_requests().await;
        return;
    }
    match request.accept().await {
        Ok(connection) => serve(connection, state, caller).await,
        Err(error) => tracing::debug!(%error, "a WebTransport session was not opened"),
    }
}

fn caller(request: &SessionRequest, gate: &AuthGate) -> Option<Caller> {
    let mut http = Request::builder()
        .uri(request.path())
        .header(HOST, request.authority());
    if let Some(origin) = request.origin() {
        http = http.header(ORIGIN, origin);
    }
    gate.admit_session(&http.body(Body::empty()).ok()?).ok()
}

async fn serve(connection: Connection, state: AppState, caller: Caller) {
    let Ok(Ok((control_tx, control_rx))) =
        tokio::time::timeout(CONTROL_WAIT, connection.accept_bi()).await
    else {
        connection.close(VarInt::from_u32(CLOSE_NO_CONTROL), b"no control stream");
        return;
    };
    let (out, output) = outbox::channel();
    let link = Link {
        out,
        writer: tokio::spawn(lanes::write(connection.clone(), control_tx, output)),
        commands: Box::pin(commands(control_rx)),
        path: Some(path_probe(&connection)),
    };
    serve_session(link, state, caller, String::new()).await;
    connection.close(VarInt::from_u32(CLOSE_NORMAL), b"");
}

fn path_probe(connection: &Connection) -> PathProbe {
    let connection = connection.clone();
    std::sync::Arc::new(move || {
        let path = connection.quic_connection().stats().path;
        PathSample {
            rtt: path.rtt,
            sent_packets: path.sent_packets,
            lost_packets: path.lost_packets,
        }
    })
}

fn commands(control: RecvStream) -> impl futures::Stream<Item = String> + Send {
    futures::stream::unfold(control, |mut control| async move {
        let command = read_command(&mut control).await?;
        Some((command, control))
    })
}

async fn read_command(control: &mut RecvStream) -> Option<String> {
    let mut length = [0u8; WEBTRANSPORT_LENGTH_BYTES];
    control.read_exact(&mut length).await.ok()?;
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_INCOMING_BYTES {
        tracing::warn!(
            length,
            "a WebTransport command is too long; closing the session"
        );
        return None;
    }
    let mut command = vec![0; length];
    control.read_exact(&mut command).await.ok()?;
    String::from_utf8(command).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_certificate_is_renewed_well_inside_its_validity() {
        let minted: jiff::Timestamp = "2026-01-01T00:00:00Z".parse().expect("timestamp");
        let early: jiff::Timestamp = "2026-01-07T23:00:00Z".parse().expect("timestamp");
        let due: jiff::Timestamp = "2026-01-08T00:00:00Z".parse().expect("timestamp");
        assert!(!rotation_due(minted, early));
        assert!(rotation_due(minted, due));
    }
}
