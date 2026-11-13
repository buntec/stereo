//! Manual smoke tests of the same network requests and parsers used by searches.
//! These never write to collections or remote playlists.

use std::{
    future::Future,
    pin::Pin,
    time::{Duration, Instant},
};

use anyhow::Context;
use serde::Serialize;
use tokio::task::JoinSet;
use wreq::Client;

use crate::{
    apple_music::{self, AMTrack},
    models::{BPTrack, SCTrack, ShazamTrack, TXTrack, Track, VLTrack, YTVideo},
    scraper,
    spotify::{self, SpTrack},
};

const CHECK_TIMEOUT: Duration = Duration::from_secs(45);
const SEARCH_QUERY: &str = "Solomun";
const SHAZAM_SONG_URL: &str = "https://www.shazam.com/song/1243689211/fiore-and-i";

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

/// A parsed search result, as far as a health check needs to judge it.
trait ProbeItem {
    /// Whether the parser filled in the fields a search result can't do without.
    fn is_usable(&self) -> bool;
    /// A short description, as an example in a passed check's message.
    fn describe(&self) -> String;
}

fn track_is_usable(title: &str, artists: &[String]) -> bool {
    !title.trim().is_empty() && artists.iter().any(|artist| !artist.trim().is_empty())
}

macro_rules! impl_probe_item_for_tracks {
    ($($ty:ty => $title:ident),* $(,)?) => {$(
        impl ProbeItem for $ty {
            fn is_usable(&self) -> bool {
                track_is_usable(&self.$title, &self.artists)
            }
            fn describe(&self) -> String {
                format!("{} – {}", self.artists.join(", "), self.$title)
            }
        }
    )*};
}

impl_probe_item_for_tracks!(
    BPTrack => track_name,
    SCTrack => track_name,
    VLTrack => track_name,
    TXTrack => track_name,
    SpTrack => track_name,
    AMTrack => track_name,
    ShazamTrack => title,
);

impl ProbeItem for YTVideo {
    fn is_usable(&self) -> bool {
        self.id.len() == 11 && !self.title.trim().is_empty()
    }
    fn describe(&self) -> String {
        self.title.clone()
    }
}

/// Chart entries come with their position.
impl<T: ProbeItem> ProbeItem for (usize, T) {
    fn is_usable(&self) -> bool {
        self.1.is_usable()
    }
    fn describe(&self) -> String {
        self.1.describe()
    }
}

// Empty responses can be a parser silently failing after an upstream markup change, so a 200
// response or a nonempty list isn't enough: most results must have their required fields.
// Searches skip the odd result without them, so one such result doesn't fail a check.
fn summarize<T: ProbeItem>(items: &[T]) -> anyhow::Result<String> {
    let total = items.len();
    anyhow::ensure!(
        total > 0,
        "No results for a known test query; the response format or catalog may have changed"
    );
    let mut usable = items.iter().filter(|item| item.is_usable());
    let example = usable
        .next()
        .context("No result had a title and artist; the response format may have changed")?
        .describe();
    let count = 1 + usable.count();
    anyhow::ensure!(
        count * 2 >= total,
        "Only {count} of {total} results had a title and artist; the response format may have changed"
    );
    Ok(if count == total {
        format!("{count} usable results; e.g. {example}")
    } else {
        format!("{count} of {total} results usable; e.g. {example}")
    })
}

type ProbeFuture = Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send>>;

struct Probe {
    name: &'static str,
    probe: String,
    run: ProbeFuture,
}

fn probe(
    name: &'static str,
    probe: impl Into<String>,
    run: impl Future<Output = anyhow::Result<String>> + Send + 'static,
) -> Probe {
    Probe {
        name,
        probe: probe.into(),
        run: Box::pin(run),
    }
}

fn probes(client: &Client) -> Vec<Probe> {
    let search_probe = format!("Search for “{SEARCH_QUERY}”");
    let c = || client.clone();
    vec![
        probe("Beatport", &search_probe, {
            let client = c();
            async move { summarize(&scraper::bp_search_tracks(&client, SEARCH_QUERY).await?) }
        }),
        probe("SoundCloud", &search_probe, {
            let client = c();
            async move { summarize(&scraper::sc_search_tracks(&client, SEARCH_QUERY, 3).await?) }
        }),
        probe("Volumo", &search_probe, {
            let client = c();
            async move { summarize(&scraper::vl_search_tracks(&client, SEARCH_QUERY, 3).await?) }
        }),
        probe("Traxsource", &search_probe, {
            let client = c();
            async move { summarize(&scraper::tx_search_tracks(&client, SEARCH_QUERY).await?) }
        }),
        probe("Spotify", &search_probe, {
            let client = c();
            async move { summarize(&spotify::search_tracks(&client, SEARCH_QUERY, 3).await?) }
        }),
        probe("Apple Music", &search_probe, {
            let client = c();
            async move {
                summarize(
                    &apple_music::search_tracks(
                        &client,
                        SEARCH_QUERY,
                        apple_music::SearchField::Any,
                        3,
                    )
                    .await?,
                )
            }
        }),
        probe("YouTube", "Search for “Daft Punk One More Time”", {
            let client = c();
            async move {
                summarize(&scraper::yt_search_simple(&client, "Daft Punk One More Time", 3).await?)
            }
        }),
        probe(
            "Shazam",
            "Similar songs for Fiore – And I, from its Shazam page",
            {
                let client = c();
                async move {
                    summarize(&scraper::shazam_similar_tracks(&client, SHAZAM_SONG_URL, 3).await?)
                }
            },
        ),
        // A collection track's similar songs first find it on Shazam through Apple Music's
        // search and Shazam's Apple-id mapping; that bridge is checked on its own, so its
        // failures aren't blamed on Shazam's song pages.
        probe(
            "Shazam lookup",
            "Find HUGEL & SOLTO (FR) – Jamaican (Bam Bam) on Apple Music, then on Shazam",
            {
                let client = c();
                async move {
                    let track = Track {
                        title: "Jamaican (Bam Bam)".to_string(),
                        artists: vec!["HUGEL & SOLTO (FR)".to_string()],
                        ..Default::default()
                    };
                    let id = scraper::shazam_id_for_track(&client, &track).await?;
                    Ok(format!("Shazam id {id}"))
                }
            },
        ),
        probe("Beatport charts", "Techno (Peak Time / Driving) top 100", {
            let client = c();
            async move { summarize(&scraper::bp_get_genre_top100(&client, 6).await?) }
        }),
        probe("Traxsource charts", "Top 100 tracks", {
            let client = c();
            async move { summarize(&scraper::tx_get_top100(&client).await?) }
        }),
    ]
}

/// Run independently of the user's enabled sources, search results and collection. Each probe
/// is its own task, so one probe's parsing doesn't hold up the others or count towards their
/// time; the `JoinSet` aborts them all when this future is dropped (the session's task aborted).
pub async fn run(client: &Client) -> Vec<CheckResult> {
    run_probes(probes(client), CHECK_TIMEOUT).await
}

async fn run_probes(probes: Vec<Probe>, timeout: Duration) -> Vec<CheckResult> {
    let mut results: Vec<CheckResult> = probes
        .iter()
        .map(|p| CheckResult {
            name: p.name.to_string(),
            probe: p.probe.clone(),
            status: CheckStatus::Failed,
            message: "The check crashed".to_string(),
            duration_ms: 0,
        })
        .collect();
    let mut tasks = JoinSet::new();
    for (index, p) in probes.into_iter().enumerate() {
        tasks.spawn(async move { (index, check(p.name, &p.probe, timeout, p.run).await) });
    }
    while let Some(joined) = tasks.join_next().await {
        // A panicked probe keeps its "crashed" placeholder.
        if let Ok((index, result)) = joined {
            results[index] = result;
        }
    }
    results
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

    #[tokio::test]
    async fn probes_keep_their_order_and_a_panic_fails_only_its_check() {
        let results = run_probes(
            vec![
                probe("slow", "a", async {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    Ok("slow done".into())
                }),
                probe("panics", "b", async { panic!("parser bug") }),
                probe("fast", "c", async { Ok("fast done".into()) }),
            ],
            CHECK_TIMEOUT,
        )
        .await;
        let summary: Vec<_> = results
            .iter()
            .map(|r| (r.name.as_str(), r.status, r.message.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                ("slow", CheckStatus::Passed, "slow done"),
                ("panics", CheckStatus::Failed, "The check crashed"),
                ("fast", CheckStatus::Passed, "fast done"),
            ]
        );
    }

    struct Item(&'static str, Vec<String>);

    impl ProbeItem for Item {
        fn is_usable(&self) -> bool {
            track_is_usable(self.0, &self.1)
        }
        fn describe(&self) -> String {
            format!("{} – {}", self.1.join(", "), self.0)
        }
    }

    fn item(title: &'static str, artist: &str) -> Item {
        Item(title, vec![artist.to_string()])
    }

    #[test]
    fn empty_or_mostly_malformed_results_do_not_pass() {
        assert!(summarize::<Item>(&[]).is_err());
        assert!(summarize(&[item("", "Artist")]).is_err());
        assert!(summarize(&[Item("Title", vec![])]).is_err());
        assert!(summarize(&[item("Title", "Artist"), item("", ""), item("", " ")]).is_err());
        assert_eq!(
            summarize(&[item("Title", "Artist")]).unwrap(),
            "1 usable results; e.g. Artist – Title"
        );
    }

    #[test]
    fn an_odd_malformed_result_does_not_fail_the_check() {
        assert_eq!(
            summarize(&[
                item("", "Artist"),
                item("Title", "Artist"),
                item("Other", "Artist")
            ])
            .unwrap(),
            "2 of 3 results usable; e.g. Artist – Title"
        );
    }
}
