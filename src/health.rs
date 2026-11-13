//! Manual smoke tests of the same network requests and parsers used by searches.
//! These never write to collections or remote playlists.

use std::{
    future::Future,
    time::{Duration, Instant},
};

use serde::Serialize;
use wreq::Client;

use crate::{models::Track, scraper, spotify};

const CHECK_TIMEOUT: Duration = Duration::from_secs(45);
const SEARCH_QUERY: &str = "Solomun";

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CheckStatus {
    Passed,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub name: String,
    pub probe: String,
    pub status: CheckStatus,
    pub message: String,
    pub duration_ms: u64,
}

async fn check(
    name: &str,
    probe: &str,
    timeout: Duration,
    run: impl Future<Output = anyhow::Result<String>>,
) -> CheckResult {
    let start = Instant::now();
    let outcome = tokio::time::timeout(timeout, run).await;
    let (status, message) = match outcome {
        Ok(Ok(message)) => (CheckStatus::Passed, message),
        Ok(Err(error)) => (CheckStatus::Failed, format!("{error:#}")),
        Err(_) => (
            CheckStatus::Failed,
            format!("Timed out after {} seconds", timeout.as_secs()),
        ),
    };
    CheckResult {
        name: name.to_string(),
        probe: probe.to_string(),
        status,
        message,
        duration_ms: start.elapsed().as_millis() as u64,
    }
}

// Empty responses can be a parser silently failing after an upstream markup change. Check
// required fields too, rather than treating a 200 response or a nonempty array as success.
fn summarize_tracks<'a>(
    tracks: impl IntoIterator<Item = (&'a str, &'a [String])>,
) -> anyhow::Result<String> {
    let mut count = 0;
    let mut example = String::new();
    for (title, artists) in tracks {
        anyhow::ensure!(
            !title.trim().is_empty() && artists.iter().any(|artist| !artist.trim().is_empty()),
            "Parsed a result without a title or artist; the response format may have changed"
        );
        if count == 0 {
            example = format!("{} – {title}", artists.join(", "));
        }
        count += 1;
    }
    anyhow::ensure!(
        count > 0,
        "No results for a known test query; the response format or catalog may have changed"
    );
    Ok(format!("{count} usable results; e.g. {example}"))
}

/// Run independently of the user's enabled sources, search results and collection. Futures
/// are joined here (not detached), so aborting the session's task cancels every pending probe.
pub async fn run(client: &Client) -> Vec<CheckResult> {
    let search_probe = format!("Search for “{SEARCH_QUERY}”");
    let (bp, sc, vl, tx, sp, yt, shazam, bp_chart, tx_chart) = tokio::join!(
        check("Beatport", &search_probe, CHECK_TIMEOUT, async {
            let tracks = scraper::bp_search_tracks(client, SEARCH_QUERY).await?;
            summarize_tracks(
                tracks
                    .iter()
                    .map(|t| (t.track_name.as_str(), t.artists.as_slice())),
            )
        }),
        check("SoundCloud", &search_probe, CHECK_TIMEOUT, async {
            let tracks = scraper::sc_search_tracks(client, SEARCH_QUERY, 3).await?;
            summarize_tracks(
                tracks
                    .iter()
                    .map(|t| (t.track_name.as_str(), t.artists.as_slice())),
            )
        }),
        check("Volumo", &search_probe, CHECK_TIMEOUT, async {
            let tracks = scraper::vl_search_tracks(client, SEARCH_QUERY, 3).await?;
            summarize_tracks(
                tracks
                    .iter()
                    .map(|t| (t.track_name.as_str(), t.artists.as_slice())),
            )
        }),
        check("Traxsource", &search_probe, CHECK_TIMEOUT, async {
            let tracks = scraper::tx_search_tracks(client, SEARCH_QUERY).await?;
            summarize_tracks(
                tracks
                    .iter()
                    .map(|t| (t.track_name.as_str(), t.artists.as_slice())),
            )
        }),
        check("Spotify", &search_probe, CHECK_TIMEOUT, async {
            let tracks = spotify::search_tracks(client, SEARCH_QUERY, 3).await?;
            summarize_tracks(
                tracks
                    .iter()
                    .map(|t| (t.track_name.as_str(), t.artists.as_slice())),
            )
        }),
        check(
            "YouTube",
            "Search for “Daft Punk One More Time”",
            CHECK_TIMEOUT,
            async {
                let videos =
                    scraper::yt_search_simple(client, "Daft Punk One More Time", 3).await?;
                anyhow::ensure!(
                    !videos.is_empty(),
                    "No videos for a known test query; YouTube's search response may have changed"
                );
                anyhow::ensure!(
                    videos
                        .iter()
                        .all(|v| v.id.len() == 11 && !v.title.trim().is_empty()),
                    "Parsed a video without a valid id or title"
                );
                Ok(format!(
                    "{} usable videos; e.g. {}",
                    videos.len(),
                    videos[0].title
                ))
            }
        ),
        check(
            "Shazam",
            "Similar songs for HUGEL & SOLTO (FR) – Jamaican (Bam Bam), via Apple Music",
            CHECK_TIMEOUT,
            async {
                let track = Track {
                    title: "Jamaican (Bam Bam)".to_string(),
                    artists: vec!["HUGEL & SOLTO (FR)".to_string()],
                    ..Default::default()
                };
                let tracks = scraper::shazam_similar_tracks_for_track(client, &track, 3).await?;
                summarize_tracks(
                    tracks
                        .iter()
                        .map(|t| (t.title.as_str(), t.artists.as_slice())),
                )
            }
        ),
        check(
            "Beatport charts",
            "Techno (Peak Time / Driving) top 100",
            CHECK_TIMEOUT,
            async {
                let tracks = scraper::bp_get_genre_top100(client, 6).await?;
                summarize_tracks(
                    tracks
                        .iter()
                        .map(|(_, t)| (t.track_name.as_str(), t.artists.as_slice())),
                )
            }
        ),
        check(
            "Traxsource charts",
            "Top 100 tracks",
            CHECK_TIMEOUT,
            async {
                let tracks = scraper::tx_get_top100(client).await?;
                summarize_tracks(
                    tracks
                        .iter()
                        .map(|(_, t)| (t.track_name.as_str(), t.artists.as_slice())),
                )
            }
        ),
    );
    vec![bp, sc, vl, tx, sp, yt, shazam, bp_chart, tx_chart]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reports_success_failure_chain_and_timeout() {
        let passed = check("test", "query", CHECK_TIMEOUT, async {
            Ok("parsed results".into())
        })
        .await;
        assert_eq!(passed.status, CheckStatus::Passed);
        let failed = check("test", "query", CHECK_TIMEOUT, async {
            Err(anyhow::anyhow!("HTTP 403").context("search failed"))
        })
        .await;
        assert_eq!(failed.status, CheckStatus::Failed);
        assert_eq!(failed.message, "search failed: HTTP 403");
        let timeout = check(
            "test",
            "query",
            Duration::from_millis(1),
            std::future::pending(),
        )
        .await;
        assert_eq!(timeout.status, CheckStatus::Failed);
        assert!(timeout.message.starts_with("Timed out"));
    }

    #[test]
    fn empty_or_malformed_results_do_not_pass() {
        assert!(summarize_tracks([]).is_err());
        let artists = vec!["Artist".to_string()];
        assert!(summarize_tracks([("", artists.as_slice())]).is_err());
        assert!(summarize_tracks([("Title", [].as_slice())]).is_err());
        assert_eq!(
            summarize_tracks([("Title", artists.as_slice())]).unwrap(),
            "1 usable results; e.g. Artist – Title"
        );
    }
}
