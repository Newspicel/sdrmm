use std::sync::atomic::{AtomicUsize, Ordering};

use axum::{Router, routing::get};

use super::*;

const UNREACHABLE: &str = "http://127.0.0.1:1/feed";

fn checked(body: &[u8]) -> Result<String, String> {
    let text = std::str::from_utf8(body).map_err(|_| "binary".to_owned())?;
    text.starts_with("ok")
        .then(|| text.to_owned())
        .ok_or_else(|| "no ok".to_owned())
}

fn feed(refresh: Duration) -> Feed<String> {
    Feed::new(
        Source {
            file: "feed.txt",
            url: UNREACHABLE,
            params: &[],
            refresh,
            max_bytes: 64,
        },
        checked,
    )
}

async fn serve(body: &'static str) -> (String, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = hits.clone();
    let app = Router::new().route(
        "/feed",
        get(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            async move { body }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move { axum::serve(listener, app).await });
    (format!("http://{address}/feed"), hits)
}

#[tokio::test]
async fn a_download_is_cached_on_disk_and_survives_a_restart() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (url, _) = serve("ok fresh").await;
    let first = feed(Duration::from_secs(3_600));
    let fetched = first
        .refresh_from(&url, Some(dir.path()))
        .await
        .expect("downloads");
    assert_eq!(fetched.value, "ok fresh");
    assert!(!dir.path().join("feed.txt.partial").exists());

    let restarted = feed(Duration::from_secs(3_600));
    assert_eq!(restarted.load(dir.path()).await, Ok(true));
    let loaded = restarted.current().expect("loaded");
    assert_eq!(loaded.value, "ok fresh");
    assert!(!restarted.due(SystemTime::now()));
}

#[tokio::test]
async fn a_fresh_copy_is_not_downloaded_again() {
    let (url, hits) = serve("ok").await;
    let feed = feed(Duration::from_secs(3_600));
    feed.refresh_from(&url, None).await.expect("first");
    feed.refresh_from(&url, None).await.expect("second");
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_bad_download_keeps_the_last_good_copy_and_backs_off() {
    let (good, _) = serve("ok old").await;
    let (bad, _) = serve("garbage").await;
    let feed = feed(Duration::ZERO);
    feed.refresh_from(&good, None).await.expect("good");
    let error = feed.refresh_from(&bad, None).await.err().expect("refused");
    assert!(error.contains("no ok"), "{error}");
    assert_eq!(feed.current().expect("kept").value, "ok old");
    assert!(!feed.due(SystemTime::now()));
    assert_eq!(feed.get(None).await.expect("kept").value, "ok old");
}

#[tokio::test]
async fn without_a_copy_a_failure_is_reported_without_asking_again() {
    let feed = feed(Duration::from_secs(3_600));
    let first = feed.refresh(None).await.err().expect("unreachable");
    assert_eq!(feed.get(None).await.err(), Some(first));
}

#[tokio::test]
async fn an_oversized_body_is_refused() {
    let (url, _) =
        serve("ok, followed by far more text than the sixty four byte limit of this feed allows")
            .await;
    let error = fetch(&url, &[], 64).await.expect_err("refused");
    assert!(error.contains("more than 64 bytes"), "{error}");
}

#[tokio::test]
async fn a_missing_cache_is_not_an_error_but_a_broken_one_is() {
    let dir = tempfile::tempdir().expect("tempdir");
    let feed = feed(Duration::from_secs(3_600));
    assert_eq!(feed.load(dir.path()).await, Ok(false));
    std::fs::write(dir.path().join("feed.txt"), "garbage").expect("write");
    assert!(feed.load(dir.path()).await.is_err());
    assert!(feed.current().is_none());
}
