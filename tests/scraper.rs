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
    models::{Track, TrackSource},
    scraper::{
        self, bp_search_artist, bp_search_tracks, create_client, shazam_similar_tracks,
        shazam_similar_tracks_for_track, yt_create_anon_playlist, yt_import_playlist,
        yt_search_simple,
    },
};

fn client() -> Arc<wreq::Client> {
    Arc::new(create_client().unwrap())
}

fn all_sources() -> Vec<TrackSource> {
    vec![
        TrackSource::Beatport,
        TrackSource::Soundcloud,
        TrackSource::Volumo,
        TrackSource::Traxsource,
    ]
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

#[tokio::test]
#[ignore = "hits real YouTube API"]
async fn test_yt_import_playlist() {
    let c = client();
    let playlist = yt_import_playlist(
        &c,
        "https://music.youtube.com/playlist?list=PLZ81rI6YPQ4kgyL6oqkdfbqDU2Zj3LFLN&si=ycq2PwZz1vbRl-CR",
    )
    .await
    .unwrap();
    assert_eq!(playlist.name, "Indie");
    // The playlist is live and shrinks as videos are taken down (34 tracks when this test was
    // written, 33 by 2026-10), so check for a plausible size rather than an exact count.
    assert!(
        playlist.tracks.len() > 25,
        "only {} tracks imported",
        playlist.tracks.len()
    );
    assert!(playlist
        .tracks
        .iter()
        .all(|t| !t.yt_id.is_empty() && !t.title.is_empty()));
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

// ── Shazam ──────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "hits real Shazam page"]
async fn test_shazam_similar_tracks() {
    let c = client();
    let tracks =
        shazam_similar_tracks(&c, "https://www.shazam.com/song/1243689211/fiore-and-i", 10)
            .await
            .unwrap();
    assert!(!tracks.is_empty(), "expected Shazam similar-track results");
    assert!(!tracks[0].title.is_empty());
    assert!(!tracks[0].artists.is_empty());
    assert!(tracks[0].url.starts_with("https://www.shazam.com/song/"));
}

#[tokio::test]
#[ignore = "hits Apple Music and Shazam pages"]
async fn test_shazam_similar_tracks_for_track() {
    let c = client();
    let track = Track {
        title: "Jamaican (Bam Bam)".to_string(),
        artists: vec!["HUGEL & SOLTO (FR)".to_string()],
        ..Default::default()
    };
    let tracks = shazam_similar_tracks_for_track(&c, &track, 5)
        .await
        .unwrap();
    assert!(!tracks.is_empty(), "expected Shazam similar-track results");
    assert!(!tracks[0].title.is_empty());
}

#[tokio::test]
#[ignore = "hits Apple Music, Shazam, YouTube, and metadata APIs"]
async fn test_search_shazam_similar() {
    let c = client();
    let track = Track {
        title: "Jamaican (Bam Bam)".to_string(),
        artists: vec!["HUGEL & SOLTO (FR)".to_string()],
        ..Default::default()
    };
    let (tx, mut rx) = mpsc::channel(20);
    tokio::spawn(scraper::search_shazam_similar(c, tx, track, 1, 1));

    let mut results = vec![];
    while let Some(message) = rx.recv().await {
        match message {
            MsgServer::SearchResult { track, source, .. } => {
                results.push((track, source));
            }
            MsgServer::SearchComplete { .. } => break,
            _ => {}
        }
    }

    assert!(!results.is_empty(), "expected a playable Shazam result");
    assert_eq!(results[0].1, TrackSource::Shazam);
    assert!(!results[0].0.yt_id.is_empty());
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
        all_sources(),
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
        all_sources(),
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
        all_sources(),
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
