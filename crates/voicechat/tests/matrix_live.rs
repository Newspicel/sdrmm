//! Joins a real Matrix call and plays a tone. Needs a homeserver, a bot account and a room:
//! `SDRMM_MATRIX_HOMESERVER`, `SDRMM_MATRIX_USER`, `SDRMM_MATRIX_PASSWORD`, `SDRMM_MATRIX_ROOM`
//! and `SDRMM_MATRIX_STORE`, a folder kept between runs so they share one device.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::match_wild_err_arm)]

use std::collections::HashSet;
use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sdrmm_voicechat::{
    FRAME, Link, LinkState, MatrixAccount, MatrixLogin, MatrixStore, RATE_HZ, Service, Session,
};
use tokio::sync::watch;
use tokio::time::{Instant, MissedTickBehavior, timeout_at};

const JOIN_WITHIN: Duration = Duration::from_secs(30);
const TONE_HZ: f32 = 1000.0;
const TONE_DBFS: f32 = -12.0;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} not set"))
}

fn trace() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "warn,sdrmm_voicechat=debug".into());
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_test_writer()
        .try_init();
}

fn account(store: &Path) -> MatrixAccount {
    MatrixAccount {
        homeserver: env("SDRMM_MATRIX_HOMESERVER"),
        user: env("SDRMM_MATRIX_USER"),
        login: MatrixLogin::Password(env("SDRMM_MATRIX_PASSWORD")),
        nickname: "sdrmm".into(),
        recovery_key: std::env::var("SDRMM_MATRIX_RECOVERY_KEY").ok(),
        store: MatrixStore {
            dir: Some(store.join("sdk")),
            passphrase: "sdrmm-live-test".into(),
        },
        session: std::fs::read_to_string(store.join("session.json")).ok(),
    }
}

/// Writes every session the account hands out, so the next run restores the same device.
fn keep_saved(mut saved: watch::Receiver<Option<String>>, path: PathBuf) {
    tokio::spawn(async move {
        loop {
            if let Some(json) = saved.borrow_and_update().clone() {
                std::fs::write(&path, json).unwrap();
            }
            if saved.changed().await.is_err() {
                return;
            }
        }
    });
}

async fn joined(state: &mut watch::Receiver<LinkState>) {
    let deadline = Instant::now() + JOIN_WITHIN;
    let mut failed = None;
    loop {
        let now = state.borrow_and_update().clone();
        eprintln!("link: {now:?}");
        match now {
            LinkState::Joined => return,
            LinkState::Failed(reason) => failed = Some(reason),
            LinkState::Connecting => {}
        }
        match timeout_at(deadline, state.changed()).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => panic!("link gone: {failed:?}"),
            Err(_) => panic!("not joined within {JOIN_WITHIN:?}: {failed:?}"),
        }
    }
}

fn listen(link: &mut Link) -> tokio::task::JoinHandle<usize> {
    let (_, mut heard) = tokio::sync::mpsc::channel(1);
    std::mem::swap(&mut heard, &mut link.heard);
    tokio::spawn(async move {
        let mut talkers = HashSet::new();
        let mut frames = 0;
        while let Some(heard) = heard.recv().await {
            frames += 1;
            if talkers.insert(heard.talker.clone()) {
                eprintln!("heard: {:?}", heard.talker);
            }
        }
        frames
    })
}

async fn play_tone(link: &Link, seconds: u64) {
    let amplitude = 10f32.powf(TONE_DBFS / 20.0);
    let step = TAU * TONE_HZ / RATE_HZ as f32;
    let mut phase = 0f32;
    let mut tick = tokio::time::interval(Duration::from_millis(20));
    tick.set_missed_tick_behavior(MissedTickBehavior::Burst);
    for _ in 0..seconds * 50 {
        tick.tick().await;
        let frame = (0..FRAME)
            .map(|_| {
                phase = (phase + step) % TAU;
                amplitude * phase.sin()
            })
            .collect();
        link.send.send(frame).await.unwrap();
    }
}

#[ignore = "needs a live Matrix homeserver"]
#[tokio::test(flavor = "multi_thread")]
async fn joins_a_call_and_plays_a_tone() {
    trace();
    let store = PathBuf::from(env("SDRMM_MATRIX_STORE"));
    std::fs::create_dir_all(&store).unwrap();
    let seconds = std::env::var("SDRMM_MATRIX_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    let session = Session::open(Service::Matrix(account(&store)));
    keep_saved(session.saved().unwrap(), store.join("session.json"));
    let mut link = session.join(&env("SDRMM_MATRIX_ROOM")).unwrap();
    joined(&mut link.state).await;
    let heard = listen(&mut link);
    play_tone(&link, seconds).await;
    let state = link.state.borrow().clone();
    eprintln!("after tone: {state:?}, drops {}", link.dropped());
    assert_eq!(state, LinkState::Joined);
    drop(link);
    tokio::time::sleep(Duration::from_secs(3)).await;
    eprintln!("heard frames: {}", heard.await.unwrap());
    drop(session);
}
