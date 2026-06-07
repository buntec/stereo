/// Integration tests for the scraper module — equivalent to tests/test_lib.py.
///
/// These tests hit real external APIs (Beatport, YouTube, MusicBrainz) and are
/// marked `#[ignore]` so they don't run in CI or `cargo test` by default.
///
/// Run all of them with:
///     cargo test --manifest-path backend/Cargo.toml -- --ignored
///
/// Run a single test with:
///     cargo test --manifest-path backend/Cargo.toml test_yt_search -- --ignored
use std::sync::Arc;

use tokio::sync::mpsc;

use stereo_backend::{
    message::MsgServer,
    scraper::{
        self, bp_search_artist, bp_search_tracks, create_client, yt_create_anon_playlist,
        yt_search_simple,
    },
};

fn client() -> Arc<wreq::Client> {
    Arc::new(create_client().unwrap())
}

// ── YouTube ───────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "hits real YouTube API"]
async fn test_yt_search() {
    let c = client();
    let vids = yt_search_simple(&c, "walls mosoo deco", 3).await.unwrap();
    assert!(!vids.is_empty(), "expected at least one YouTube result");
    assert!(!vids[0].id.is_empty(), "video id should not be empty");
    assert!(!vids[0].title.is_empty(), "video title should not be empty");
}

#[tokio::test]
#[ignore = "hits real YouTube API"]
async fn test_yt_anon_playlist() {
    let c = client();
    let ids = vec![
        "nP60jajfMdw".to_string(),
        "0h6VHeysvh4".to_string(),
        "tDDsbq8PqBM".to_string(),
    ];
    let url = yt_create_anon_playlist(&c, &ids).await.unwrap();
    assert!(
        url.starts_with("https://music.youtube.com/watch"),
        "unexpected url: {url}"
    );
    assert!(url.contains("list="), "url missing list param: {url}");
}

// ── Beatport ─────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "hits real Beatport API (may be blocked without TLS impersonation)"]
async fn test_bp_search_tracks() {
    let c = client();
    let tracks = bp_search_tracks(&c, "kadosh").await.unwrap();
    assert!(!tracks.is_empty(), "expected Beatport track results");
    assert!(!tracks[0].track_name.is_empty());
    assert!(!tracks[0].artists.is_empty());
}

#[tokio::test]
#[ignore = "hits real Beatport API (may be blocked without TLS impersonation)"]
async fn test_bp_search_artist() {
    let c = client();
    let artists = bp_search_artist(&c, "kadosh", 10).await.unwrap();
    assert!(!artists.is_empty(), "expected Beatport artist results");
    assert!(!artists[0].display_name.is_empty());
}

// ── Integrated search (Beatport + YouTube) ───────────────────────────────────

/// Equivalent to Python's test_search_fuzzy
#[tokio::test]
#[ignore = "hits real Beatport + YouTube APIs"]
async fn test_search_fuzzy() {
    let c = client();
    let (tx, mut rx) = mpsc::channel(100);

    tokio::spawn(scraper::search_fuzzy(
        c,
        tx,
        "auguxt what that means".to_string(),
        1,
        3,
    ));

    let mut tracks = vec![];
    while let Some(msg) = rx.recv().await {
        match msg {
            MsgServer::SearchResult { track, .. } => tracks.push(track),
            MsgServer::SearchComplete { .. } => break,
            _ => {}
        }
    }

    assert!(!tracks.is_empty(), "expected fuzzy search results");
    assert!(!tracks[0].yt_id.is_empty());
}

/// Equivalent to Python's test_get_label_releases
#[tokio::test]
#[ignore = "hits real Beatport + YouTube APIs"]
async fn test_search_by_label() {
    let c = client();
    let (tx, mut rx) = mpsc::channel(100);

    tokio::spawn(scraper::search_by_label(
        c,
        tx,
        "Magnifik Music".to_string(),
        1,
        3,
    ));

    let mut tracks = vec![];
    while let Some(msg) = rx.recv().await {
        match msg {
            MsgServer::SearchResult { track, .. } => tracks.push(track),
            MsgServer::SearchComplete { .. } => break,
            _ => {}
        }
    }

    assert!(!tracks.is_empty(), "expected label release results");
    assert!(!tracks[0].yt_id.is_empty());
    assert_eq!(
        tracks[0].label.as_deref(),
        Some("Magnifik Music"),
        "label should match"
    );
}

/// Equivalent to Python's test_get_artist_releases
#[tokio::test]
#[ignore = "hits real Beatport + YouTube APIs"]
async fn test_search_by_artist() {
    let c = client();
    let (tx, mut rx) = mpsc::channel(100);

    tokio::spawn(scraper::search_by_artist(
        c,
        tx,
        "kadosh ofc".to_string(),
        1,
        3,
    ));

    let mut tracks = vec![];
    while let Some(msg) = rx.recv().await {
        match msg {
            MsgServer::SearchResult { track, .. } => tracks.push(track),
            MsgServer::SearchComplete { .. } => break,
            _ => {}
        }
    }

    assert!(!tracks.is_empty(), "expected artist release results");
    assert!(!tracks[0].yt_id.is_empty());
    assert!(!tracks[0].artists.is_empty());
}
