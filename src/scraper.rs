use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, LazyLock},
};

use anyhow::Context;
use chrono::NaiveDate;
use futures::{future::join_all, stream, FutureExt, Stream, StreamExt};
use scraper::{Html, Selector};
use tokio::sync::{mpsc, RwLock};
use wreq::Client;
use wreq_util::Emulation;

use crate::{
    apple_music::{self, AMTrack},
    message::MsgServer,
    models::{
        BPArtist, BPLabel, BPTrack, SCTrack, SCUser, ShazamTrack, TXTrack, Track, TrackSource,
        VLTrack, YTVideo,
    },
    spotify::{self, SpTrack},
    utils::is_original_mix,
};

pub fn create_client() -> anyhow::Result<Client> {
    Ok(Client::builder()
        .emulation(Emulation::Chrome124)
        .redirect(wreq::redirect::Policy::limited(10))
        // Without these a stalled upstream hangs a search, fixup or export indefinitely.
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30))
        .build()?)
}

// ── Beatport ─────────────────────────────────────────────────────────────────

fn parse_release_date(s: &str) -> Option<NaiveDate> {
    // Beatport sometimes returns full datetime strings; take just the date portion.
    let date_part = s.get(..10).filter(|p| p.len() == 10).unwrap_or(s);
    NaiveDate::parse_from_str(date_part, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
        .or_else(|_| NaiveDate::parse_from_str(&format!("{s}-01"), "%Y-%m-%d"))
        .or_else(|_| NaiveDate::parse_from_str(&format!("{s}-01-01"), "%Y-%m-%d"))
        .ok()
}

fn to_url_slug(s: &str) -> String {
    let ascii: String = s
        .chars()
        .map(|c| match c {
            'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => {
                'a'
            }
            'È' | 'É' | 'Ê' | 'Ë' | 'è' | 'é' | 'ê' | 'ë' => 'e',
            'Ì' | 'Í' | 'Î' | 'Ï' | 'ì' | 'í' | 'î' | 'ï' => 'i',
            'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
            'Ù' | 'Ú' | 'Û' | 'Ü' | 'ù' | 'ú' | 'û' | 'ü' => 'u',
            'Ñ' | 'ñ' => 'n',
            'Ç' | 'ç' => 'c',
            'Ý' | 'ý' | 'ÿ' => 'y',
            'Ð' | 'ð' => 'd',
            'Þ' | 'þ' => 't',
            'ß' => 's',
            'Æ' | 'æ' => 'a',
            'Œ' | 'œ' => 'o',
            _ => c,
        })
        .collect();
    let slug: String = ascii
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    slug.split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Reads a response body, failing on a non-2xx status so that a blocked or failing source is
/// reported to the client instead of silently yielding no results. Cloudflare's bot check is
/// called out specifically, since it is the usual reason a scraped source suddenly goes dark.
async fn response_text(resp: wreq::Response) -> anyhow::Result<String> {
    let status = resp.status();
    let challenged = resp
        .headers()
        .get("cf-mitigated")
        .is_some_and(|v| v == "challenge");
    let text = resp.text().await?;
    if challenged || (status.as_u16() == 403 && text.contains("challenge-platform")) {
        return Err(BotCheckBlocked(status).into());
    }
    anyhow::ensure!(status.is_success(), "request failed with HTTP {status}");
    Ok(text)
}

/// A request was answered with Cloudflare's bot check instead of the page.
#[derive(Debug)]
struct BotCheckBlocked(wreq::StatusCode);

impl std::fmt::Display for BotCheckBlocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "request blocked by Cloudflare's bot check (HTTP {})",
            self.0
        )
    }
}

impl std::error::Error for BotCheckBlocked {}

async fn response_json(resp: wreq::Response) -> anyhow::Result<serde_json::Value> {
    let text = response_text(resp).await?;
    serde_json::from_str(&text).context("response is not valid JSON")
}

/// Fetches a Beatport page's embedded `__NEXT_DATA__` JSON, sending the browser clearance from
/// [`crate::beatport`] when connected. A page without that JSON means Beatport changed its markup
/// or served something other than the requested page, which is an error rather than "no
/// results". A bot-check block says how to get past it, since only the user can.
async fn bp_page<Q: serde::Serialize + ?Sized>(
    client: &Client,
    url: &str,
    params: &Q,
) -> anyhow::Result<serde_json::Value> {
    let req = crate::beatport::authorize(client.get(url).query(params)).await;
    let html = match response_text(req.send().await?).await {
        Ok(html) => html,
        Err(e) if e.is::<BotCheckBlocked>() => {
            anyhow::bail!(match crate::beatport::connected_browser().await {
                Some(browser) => format!(
                    "{e}. The clearance imported from {browser} was rejected, probably because \
                     it expired: run a search on beatport.com in {browser}, then reconnect \
                     Beatport in Settings."
                ),
                None => format!(
                    "{e}. Connect Beatport in Settings to search with your browser's clearance."
                ),
            })
        }
        Err(e) => return Err(e),
    };
    get_next_data(&html).context("page has no __NEXT_DATA__ (did Beatport change its markup?)")
}

/// Whether Beatport's search lets a request with this `Cookie` header and User-Agent past its bot
/// check. Fails when that can't be told, e.g. because Beatport is unreachable.
pub async fn bp_clearance_works(
    client: &Client,
    cookie: &str,
    user_agent: &str,
) -> anyhow::Result<bool> {
    let req = client
        .get(crate::beatport::SEARCH_URL)
        .query(&[("q", "stereo"), ("per_page", "1")])
        .header("Cookie", cookie)
        .header("User-Agent", user_agent);
    match response_text(req.send().await?).await {
        Ok(html) => {
            anyhow::ensure!(
                get_next_data(&html).is_some(),
                "Beatport's search page has no __NEXT_DATA__ (did Beatport change its markup?)"
            );
            Ok(true)
        }
        Err(e) if e.is::<BotCheckBlocked>() => Ok(false),
        Err(e) => Err(e.context("checking the clearance with a Beatport search failed")),
    }
}

fn get_next_data(html: &str) -> Option<serde_json::Value> {
    let doc = Html::parse_document(html);
    let sel = Selector::parse("script#__NEXT_DATA__").ok()?;
    let el = doc.select(&sel).next()?;
    let text = el.text().collect::<String>();
    serde_json::from_str(&text).ok()
}

// ── Shazam ──────────────────────────────────────────────────────────────────

/// Parses a Shazam song link and returns its numeric id plus an absolute URL.
///
/// Shazam currently emits `/song/{id}/{slug}` links for this section, but accepting
/// `/track/{id}/{slug}` as well makes the parser tolerant of older page variants.
fn parse_shazam_song_link(href: &str) -> Option<(i64, String)> {
    let url = url::Url::parse(href)
        .or_else(|_| url::Url::parse(&format!("https://www.shazam.com{href}")))
        .ok()?;
    let host = url.host_str()?;
    if !matches!(host, "www.shazam.com" | "shazam.com") {
        return None;
    }

    let mut segments = url.path_segments()?;
    let kind = segments.next()?;
    if kind != "song" && kind != "track" {
        return None;
    }
    let id = segments.next()?.parse().ok()?;
    Some((id, url.to_string()))
}

/// Extracts the server-rendered `Similar Songs` cards from a Shazam song page.
///
/// The title and artist test ids are scoped to the similar-songs section, rather than the
/// generic `songItem` test id, so this does not accidentally include top tracks, playlists, or
/// other recommendation sections on the page. Shazam renders an empty link before the visible
/// title link for its play-button overlay; the first non-empty title is therefore intentional.
pub fn parse_shazam_similar_tracks(html: &str, limit: usize) -> Vec<ShazamTrack> {
    if limit == 0 {
        return vec![];
    }

    let doc = Html::parse_document(html);
    let item_sel = Selector::parse("div[data-test-id=\"songItem\"]").unwrap();
    let title_sel =
        Selector::parse("a[data-test-id=\"track_userevent_similar-songs_songTitle\"]").unwrap();
    let artist_sel =
        Selector::parse("a[data-test-id=\"track_userevent_similar-songs_artistName\"]").unwrap();
    let mut seen = HashSet::new();
    let mut tracks = Vec::new();

    for item in doc.select(&item_sel) {
        let Some(title_link) = item
            .select(&title_sel)
            .find(|link| !link.text().collect::<String>().trim().is_empty())
        else {
            continue;
        };
        let Some(href) = title_link.value().attr("href") else {
            continue;
        };
        let Some((id, url)) = parse_shazam_song_link(href) else {
            continue;
        };

        let title = title_link.text().collect::<String>().trim().to_string();
        let Some(artist_link) = item.select(&artist_sel).next() else {
            continue;
        };
        let artists = artist_link
            .value()
            .attr("aria-label")
            .map(str::to_string)
            .unwrap_or_else(|| artist_link.text().collect::<String>().trim().to_string());
        let artists = artists.trim().to_string();
        if title.is_empty() || artists.is_empty() || !seen.insert(id) {
            continue;
        }

        tracks.push(ShazamTrack {
            id,
            title,
            artists: vec![artists],
            url,
        });
        if tracks.len() >= limit {
            break;
        }
    }

    tracks
}

/// Fetches a Shazam song page and returns its similar-song recommendations.
///
/// This intentionally scrapes the page instead of depending on Shazam's private similarities
/// endpoint. The endpoint is not consistently available for older ids, while the public page
/// follows old links to the current canonical song and contains the rendered recommendation
/// cards. `limit` only caps the locally parsed page results; Shazam currently renders the first
/// recommendation page in the HTML response.
pub async fn shazam_similar_tracks(
    client: &Client,
    song_url: &str,
    limit: usize,
) -> anyhow::Result<Vec<ShazamTrack>> {
    let parsed = url::Url::parse(song_url).context("invalid Shazam song URL")?;
    anyhow::ensure!(
        parsed.scheme() == "https"
            && matches!(
                parsed.host_str(),
                Some("www.shazam.com") | Some("shazam.com")
            ),
        "expected an https://www.shazam.com/song/... URL"
    );

    let resp = client
        .get(song_url)
        .header("Accept-Language", "en-US,en;q=0.9")
        .send()
        .await?;
    anyhow::ensure!(
        resp.status().is_success(),
        "Shazam song page returned {}",
        resp.status()
    );
    let html = resp.text().await?;
    let tracks = parse_shazam_similar_tracks(&html, limit);
    tracing::info!(
        "Shazam returned {} similar tracks for {}",
        tracks.len(),
        song_url
    );
    Ok(tracks)
}

/// Finds the Shazam id for a normal collection track: finds the track on Apple Music, then asks
/// Shazam's Apple-id mapping endpoint for it. Collection tracks do not store a Shazam id, so this
/// is the bridge used by the row-level "similar songs" action.
pub async fn shazam_id_for_track(client: &Client, track: &Track) -> anyhow::Result<i64> {
    let apple_id = apple_music_id_for_track(client, track)
        .await?
        .context("Apple Music search found no matching song")?;
    let mapping_url = format!("https://www.shazam.com/services/sd/s/a2st/US/en-US/{apple_id}");
    let mapping = response_json(client.get(mapping_url).send().await?).await?;
    mapping
        .as_object()
        .and_then(|items| items.values().find_map(|value| value.as_str()))
        .and_then(|id| id.parse().ok())
        .context("Shazam did not map the Apple Music song")
}

/// The Apple Music song id of `track`, if Apple Music has it. Searched for with its version first,
/// so an extended mix maps to the extended mix, then without, since Apple often doesn't carry the
/// version in the title at all; the best-scoring song wins, as in `fixup_track`.
async fn apple_music_id_for_track(client: &Client, track: &Track) -> anyhow::Result<Option<i64>> {
    let title = crate::utils::strip_feat(&track.title);
    let artists = crate::utils::strip_artist_disambiguators(&track.artists.join(" "));
    let base_query = format!("{artists} {title}").trim().to_string();
    anyhow::ensure!(!base_query.is_empty(), "the track has no title or artist");
    let mix_name = track.mix_name.as_deref().filter(|m| !is_original_mix(m));
    let queries = mix_name
        .map(|mix| format!("{base_query} {mix}"))
        .into_iter()
        .chain([base_query]);
    for query in queries {
        let candidates =
            apple_music::search_tracks(client, &query, apple_music::SearchField::Any, 10).await?;
        let best = candidates
            .into_iter()
            .map(RawTrack::Am)
            .map(|raw| {
                let score = score_raw_match(&raw, &track.title, &track.artists, mix_name);
                (score, raw)
            })
            .filter(|(score, _)| *score > 0.5)
            .max_by(|(a, _), (b, _)| a.total_cmp(b));
        if let Some((_, RawTrack::Am(song))) = best {
            return Ok(Some(song.id));
        }
    }
    Ok(None)
}

/// Resolves a collection track to its Shazam page and returns the page's similar songs.
pub async fn shazam_similar_tracks_for_track(
    client: &Client,
    track: &Track,
    limit: usize,
) -> anyhow::Result<Vec<ShazamTrack>> {
    let shazam_id = shazam_id_for_track(client, track).await?;
    shazam_similar_tracks(
        client,
        &format!("https://www.shazam.com/track/{shazam_id}"),
        limit,
    )
    .await
}

pub async fn bp_search_tracks(client: &Client, q: &str) -> anyhow::Result<Vec<BPTrack>> {
    tracing::info!("Beatport track search: {q}");
    let json = bp_page(
        client,
        "https://www.beatport.com/search/tracks",
        &[("q", q), ("per_page", "1000"), ("page", "1")],
    )
    .await?;

    let mut tracks = vec![];
    if let Some(queries) = json["props"]["pageProps"]["dehydratedState"]["queries"].as_array() {
        for query in queries {
            if let Some(items) = query["state"]["data"]["data"].as_array() {
                for item in items {
                    if let Some(t) = parse_bp_track_from_search(item) {
                        tracks.push(t);
                    }
                }
            }
        }
    }
    tracing::info!("Beatport returned {} tracks", tracks.len());
    Ok(tracks)
}

fn parse_bp_track_from_search(item: &serde_json::Value) -> Option<BPTrack> {
    let artists: Vec<String> = item["artists"]
        .as_array()?
        .iter()
        .filter_map(|a| a["artist_name"].as_str().map(String::from))
        .collect();
    let track_name = item["track_name"].as_str()?.trim().to_string();
    let mix_name = item["mix_name"].as_str()?.trim().to_string();
    let release_date = parse_release_date(item["release_date"].as_str().unwrap_or(""))?;
    let track_id = item["track_id"].as_i64()?;
    let label = item["label"]["label_name"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let bpm = item["bpm"].as_i64();
    let key = item["key_name"].as_str().map(String::from);
    let isrc = item["isrc"].as_str().map(String::from);
    let genre: Vec<String> = item["genre"]
        .as_array()
        .map(|g| {
            g.iter()
                .filter_map(|x| x["genre_name"].as_str().map(|s| s.trim().to_string()))
                .collect()
        })
        .unwrap_or_default();

    Some(BPTrack {
        artists,
        bpm,
        key,
        isrc,
        label,
        release_date,
        track_id,
        track_name,
        mix_name,
        genre,
    })
}

fn parse_bp_track_from_releases(item: &serde_json::Value) -> Option<BPTrack> {
    let artists: Vec<String> = item["artists"]
        .as_array()?
        .iter()
        .filter_map(|a| a["name"].as_str().map(|s| s.trim().to_string()))
        .collect();
    let track_name = item["name"].as_str()?.trim().to_string();
    let mix_name = item["mix_name"].as_str()?.trim().to_string();
    let release_date = parse_release_date(item["new_release_date"].as_str().unwrap_or(""))?;
    let track_id = item["id"].as_i64()?;
    let label = item["release"]["label"]["name"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    let bpm = item["bpm"].as_i64();
    let key = item["key"]["name"].as_str().map(|s| s.trim().to_string());
    let isrc = item["isrc"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.trim().to_string());
    let genre = item["genre"]["name"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    Some(BPTrack {
        artists,
        bpm,
        key,
        isrc,
        label,
        release_date,
        track_id,
        track_name,
        mix_name,
        genre: vec![genre],
    })
}

pub async fn bp_search_labels(
    client: &Client,
    name: &str,
    limit: usize,
) -> anyhow::Result<Vec<BPLabel>> {
    let json = bp_page(
        client,
        "https://www.beatport.com/search/labels",
        &[("q", name)],
    )
    .await?;

    let mut labels = vec![];
    if let Some(queries) = json["props"]["pageProps"]["dehydratedState"]["queries"].as_array() {
        for query in queries {
            if let Some(items) = query["state"]["data"]["data"].as_array() {
                for item in items {
                    let display_name = item["label_name"].as_str().unwrap_or("").trim().to_string();
                    let slug = item["slug"]
                        .as_str()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| to_url_slug(&display_name));
                    let id = item["label_id"].as_i64().unwrap_or(0);
                    tracing::debug!("Label result: display={display_name:?} slug={slug:?} id={id}");
                    labels.push(BPLabel {
                        display_name,
                        name: slug,
                        id,
                    });
                    if labels.len() >= limit {
                        return Ok(labels);
                    }
                }
            }
        }
    }
    Ok(labels)
}

pub async fn bp_search_artist(
    client: &Client,
    name: &str,
    limit: usize,
) -> anyhow::Result<Vec<BPArtist>> {
    let json = bp_page(
        client,
        "https://www.beatport.com/search/artists",
        &[("q", name)],
    )
    .await?;

    let mut artists = vec![];
    if let Some(queries) = json["props"]["pageProps"]["dehydratedState"]["queries"].as_array() {
        for query in queries {
            if let Some(items) = query["state"]["data"]["data"].as_array() {
                for item in items {
                    let display_name = item["artist_name"]
                        .as_str()
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    let slug = item["slug"]
                        .as_str()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| to_url_slug(&display_name));
                    let id = item["artist_id"].as_i64().unwrap_or(0);
                    let image_uri = item["artist_image_uri"].as_str().unwrap_or("").to_string();
                    tracing::debug!(
                        "Artist result: display={display_name:?} slug={slug:?} id={id}"
                    );
                    artists.push(BPArtist {
                        display_name,
                        name: slug,
                        id,
                        image_uri,
                    });
                    if artists.len() >= limit {
                        return Ok(artists);
                    }
                }
            }
        }
    }
    Ok(artists)
}

/// Beatport track ids from the user's collection with the artists and label stored for them.
/// Beatport's track pages (unlike its search) aren't behind its bot check and list the Beatport
/// ids of a track's artists and label, so these let artist/label searches find names already in
/// the collection without Beatport's search.
#[derive(Clone, Default)]
pub struct KnownBpTracks(Arc<Vec<BpTrackCredits>>);

/// A collection track's `(bp_id, artists, label)`.
pub type BpTrackCredits = (i64, Vec<String>, Option<String>);

/// How many of a name's collection tracks to try before falling back to Beatport's search; one
/// normally suffices, more only help if Beatport has since removed a track.
const KNOWN_BP_TRACKS_TRIED: usize = 3;

impl KnownBpTracks {
    pub fn new(tracks: Vec<BpTrackCredits>) -> Self {
        Self(Arc::new(tracks))
    }

    fn by_artist(&self, name: &str) -> Vec<i64> {
        self.0
            .iter()
            .filter(|(_, artists, _)| artists.iter().any(|a| is_same_name(a, name)))
            .map(|(id, _, _)| *id)
            .take(KNOWN_BP_TRACKS_TRIED)
            .collect()
    }

    fn on_label(&self, name: &str) -> Vec<i64> {
        self.0
            .iter()
            .filter(|(_, _, label)| label.as_deref().is_some_and(|l| is_same_name(l, name)))
            .map(|(id, _, _)| *id)
            .take(KNOWN_BP_TRACKS_TRIED)
            .collect()
    }
}

/// Beatport artists and labels seen on track pages, keyed by normalized name, so each name costs
/// at most one track-page fetch per process.
static BP_ARTISTS: LazyLock<RwLock<HashMap<String, BPArtist>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));
static BP_LABELS: LazyLock<RwLock<HashMap<String, BPLabel>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Reads a track page's artist and label credits and caches them in `BP_ARTISTS`/`BP_LABELS`.
async fn bp_cache_track_credits(client: &Client, track_id: i64) -> anyhow::Result<()> {
    let url = format!("https://www.beatport.com/track/track/{track_id}");
    let json = bp_page(client, &url, &[] as &[(&str, &str)]).await?;
    let key = format!("track-details-{track_id}");
    let track = json["props"]["pageProps"]["dehydratedState"]["queries"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|q| q["queryKey"][0].as_str() == Some(key.as_str()))
        .map(|q| &q["state"]["data"])
        .with_context(|| format!("no details on the page of Beatport track {track_id}"))?;
    let (artists, label) = parse_bp_track_credits(track);
    let mut cache = BP_ARTISTS.write().await;
    for artist in artists {
        cache.insert(
            crate::utils::normalize_for_match(&artist.display_name),
            artist,
        );
    }
    drop(cache);
    if let Some(label) = label {
        BP_LABELS.write().await.insert(
            crate::utils::normalize_for_match(&label.display_name),
            label,
        );
    }
    Ok(())
}

/// The artists and label of a track page's `track-details` query.
fn parse_bp_track_credits(track: &serde_json::Value) -> (Vec<BPArtist>, Option<BPLabel>) {
    let credit = |v: &serde_json::Value| {
        Some((
            v["id"].as_i64()?,
            v["name"].as_str()?.trim().to_string(),
            v["slug"].as_str().map(|s| s.trim().to_string()),
        ))
    };
    let artists = track["artists"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(credit)
        .map(|(id, display_name, slug)| BPArtist {
            name: slug.unwrap_or_else(|| to_url_slug(&display_name)),
            display_name,
            id,
            image_uri: String::new(),
        })
        .collect();
    let label = credit(&track["label"]).map(|(id, display_name, slug)| BPLabel {
        name: slug.unwrap_or_else(|| to_url_slug(&display_name)),
        display_name,
        id,
    });
    (artists, label)
}

/// A Beatport artist or label.
trait BpCredit: Clone {
    fn display_name(&self) -> &str;
}

impl BpCredit for BPArtist {
    fn display_name(&self) -> &str {
        &self.display_name
    }
}

impl BpCredit for BPLabel {
    fn display_name(&self) -> &str {
        &self.display_name
    }
}

/// Looks up `name` in a credits cache, matching names the way [`is_same_name`] does.
async fn bp_cached<T: BpCredit>(cache: &RwLock<HashMap<String, T>>, name: &str) -> Option<T> {
    cache
        .read()
        .await
        .values()
        .find(|v| is_same_name(v.display_name(), name))
        .cloned()
}

/// Resolves `name` to a Beatport artist or label: first via the collection's tracks by that
/// name (see [`KnownBpTracks`]), then via Beatport's search.
async fn bp_find_credit<T: BpCredit>(
    client: &Client,
    name: &str,
    known_tracks: Vec<i64>,
    cache: &RwLock<HashMap<String, T>>,
) -> Option<T> {
    if let Some(found) = bp_cached(cache, name).await {
        return Some(found);
    }
    for track_id in known_tracks {
        if let Err(e) = bp_cache_track_credits(client, track_id).await {
            tracing::warn!("Reading credits of Beatport track {track_id} failed: {e}");
            continue;
        }
        if let Some(found) = bp_cached(cache, name).await {
            return Some(found);
        }
    }
    None
}

/// Finds the Beatport artist named `name`, without Beatport's search if the collection has a
/// Beatport track by them. Otherwise takes the most relevant search result whose name actually
/// matches, rather than the top hit, which may only be a similar name.
async fn bp_find_artist(
    client: &Client,
    name: &str,
    known: &KnownBpTracks,
) -> anyhow::Result<Option<BPArtist>> {
    if let Some(artist) = bp_find_credit(client, name, known.by_artist(name), &BP_ARTISTS).await {
        return Ok(Some(artist));
    }
    Ok(bp_search_artist(client, name, 10)
        .await?
        .into_iter()
        .find(|a| is_same_name(&a.display_name, name)))
}

/// Finds the Beatport label named `name`, like [`bp_find_artist`]. Search results fall back to
/// the top hit when none matches exactly, since label names vary more ("Defected" vs.
/// "Defected Records").
async fn bp_find_label(
    client: &Client,
    name: &str,
    known: &KnownBpTracks,
) -> anyhow::Result<Option<BPLabel>> {
    if let Some(label) = bp_find_credit(client, name, known.on_label(name), &BP_LABELS).await {
        return Ok(Some(label));
    }
    let labels = bp_search_labels(client, name, 10).await?;
    let exact = labels
        .iter()
        .position(|l| is_same_name(&l.display_name, name));
    Ok(labels.into_iter().nth(exact.unwrap_or(0)))
}

/// The tracks by the Beatport artist named `name`, newest first, or none if Beatport has no
/// such artist.
pub async fn bp_get_artist_tracks(
    client: &Client,
    name: &str,
    known: &KnownBpTracks,
) -> anyhow::Result<Vec<BPTrack>> {
    match bp_find_artist(client, name, known).await? {
        Some(artist) => bp_get_artist_releases(client, &artist, None).await,
        None => Ok(vec![]),
    }
}

/// The tracks on the Beatport label named `name`, newest first, or none if Beatport has no
/// such label.
pub async fn bp_get_label_tracks(
    client: &Client,
    name: &str,
    known: &KnownBpTracks,
) -> anyhow::Result<Vec<BPTrack>> {
    match bp_find_label(client, name, known).await? {
        Some(label) => bp_get_label_releases(client, &label, None).await,
        None => Ok(vec![]),
    }
}

/// Beatport serves at most this many tracks a page, whatever `per_page` asks for.
const BP_PAGE_SIZE: usize = 100;

/// Page `page` (from 1) of the Beatport track list at `url`, newest first and back to
/// `start_date`, and whether the page was full, so the list may go on.
async fn bp_get_releases(
    client: &Client,
    url: &str,
    page: usize,
    start_date: Option<NaiveDate>,
) -> anyhow::Result<(Vec<BPTrack>, bool)> {
    let mut params: Vec<(&str, String)> = vec![
        ("page", page.to_string()),
        ("per_page", BP_PAGE_SIZE.to_string()),
    ];
    if let Some(start) = start_date {
        let end = chrono::Local::now().date_naive();
        params.push(("publish_date", format!("{}:{}", start, end)));
    }
    let json = bp_page(client, url, &params).await?;

    let mut rows = 0;
    let mut tracks = vec![];
    if let Some(queries) = json["props"]["pageProps"]["dehydratedState"]["queries"].as_array() {
        for query in queries {
            let has_tracks = query["queryKey"]
                .as_array()
                .map(|k| k.iter().any(|v| v.as_str() == Some("tracks")))
                .unwrap_or(false);
            if !has_tracks {
                continue;
            }
            if let Some(results) = query["state"]["data"]["results"].as_array() {
                rows += results.len();
                tracks.extend(results.iter().filter_map(parse_bp_track_from_releases));
            }
        }
    }
    Ok((tracks, rows >= BP_PAGE_SIZE))
}

/// A Beatport URL with these path segments, percent-encoded. Slugs from track pages keep
/// non-ASCII letters (RBØR's is `rbør`), which the HTTP client would send raw and Beatport
/// rejects with HTTP 400.
fn bp_url(segments: &[&str]) -> String {
    let mut url = url::Url::parse("https://www.beatport.com").expect("valid base URL");
    url.path_segments_mut()
        .expect("base URL has a path")
        .extend(segments);
    url.into()
}

fn bp_label_tracks_url(label: &BPLabel) -> String {
    bp_url(&["label", &label.name, &label.id.to_string(), "tracks"])
}

fn bp_artist_tracks_url(artist: &BPArtist) -> String {
    bp_url(&["artist", &artist.name, &artist.id.to_string(), "tracks"])
}

/// The newest page of the label's tracks, back to `start_date`.
pub async fn bp_get_label_releases(
    client: &Client,
    label: &BPLabel,
    start_date: Option<NaiveDate>,
) -> anyhow::Result<Vec<BPTrack>> {
    let url = bp_label_tracks_url(label);
    Ok(bp_get_releases(client, &url, 1, start_date).await?.0)
}

/// The newest page of the artist's tracks, back to `start_date`.
pub async fn bp_get_artist_releases(
    client: &Client,
    artist: &BPArtist,
    start_date: Option<NaiveDate>,
) -> anyhow::Result<Vec<BPTrack>> {
    let url = bp_artist_tracks_url(artist);
    tracing::info!("Fetching artist releases: {url}");
    Ok(bp_get_releases(client, &url, 1, start_date).await?.0)
}

/// Extracts the chart from a genre's top-100 page `__NEXT_DATA__` as (1-based chart position,
/// track) pairs, in chart order. The position is taken before unparseable entries are dropped,
/// so a skipped entry leaves a gap instead of shifting everything below it. The chart's query
/// key looks like `genre-6-top-100-tracks-per_page=100&hype=false`.
fn parse_bp_top100(json: &serde_json::Value) -> Option<Vec<(usize, BPTrack)>> {
    let queries = json["props"]["pageProps"]["dehydratedState"]["queries"].as_array()?;
    let chart = queries.iter().find(|q| {
        q["queryKey"]
            .as_array()
            .and_then(|k| k.first())
            .and_then(|k| k.as_str())
            .is_some_and(|k| k.contains("top-100-tracks"))
    })?;
    let results = chart["state"]["data"]["results"].as_array()?;
    Some(
        results
            .iter()
            .enumerate()
            .filter_map(|(i, item)| Some((i + 1, parse_bp_track_from_releases(item)?)))
            .collect(),
    )
}

/// Fetches the Beatport top-100 chart for a genre. Only the id identifies the genre — Beatport
/// ignores the slug in the URL, so a placeholder is used.
pub async fn bp_get_genre_top100(
    client: &Client,
    genre_id: i64,
) -> anyhow::Result<Vec<(usize, BPTrack)>> {
    let url = format!("https://www.beatport.com/genre/genre/{genre_id}/top-100");
    tracing::info!("Fetching Beatport top 100: {url}");
    let json = bp_page(client, &url, &[] as &[(&str, &str)]).await?;
    parse_bp_top100(&json).with_context(|| format!("no top-100 chart found for genre {genre_id}"))
}

// ── SoundCloud ───────────────────────────────────────────────────────────────

// SoundCloud's public API requires an unofficial `client_id` that's embedded in one of
// its web app's JS bundles and rotates periodically — cache it and refetch on 401.
static SC_CLIENT_ID: LazyLock<RwLock<Option<String>>> = LazyLock::new(|| RwLock::new(None));

fn extract_client_id(js: &str) -> Option<String> {
    let idx = js.find("client_id:\"")?;
    let rest = &js[idx + "client_id:\"".len()..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

async fn fetch_sc_client_id(client: &Client) -> anyhow::Result<String> {
    let html = response_text(client.get("https://soundcloud.com/").send().await?).await?;
    let script_urls: Vec<String> = {
        let doc = Html::parse_document(&html);
        let sel = Selector::parse("script[src]").unwrap();
        doc.select(&sel)
            .filter_map(|el| el.value().attr("src"))
            .filter(|src| src.contains("sndcdn.com/assets"))
            .map(String::from)
            .collect()
    };

    let results = join_all(script_urls.iter().map(|url| async move {
        let js = client.get(url).send().await.ok()?.text().await.ok()?;
        extract_client_id(&js)
    }))
    .await;

    results
        .into_iter()
        .flatten()
        .next()
        .context("could not locate SoundCloud client_id")
}

async fn get_sc_client_id(client: &Client) -> anyhow::Result<String> {
    if let Some(id) = SC_CLIENT_ID.read().await.clone() {
        return Ok(id);
    }
    let id = fetch_sc_client_id(client).await?;
    *SC_CLIENT_ID.write().await = Some(id.clone());
    Ok(id)
}

async fn sc_get(
    client: &Client,
    path: &str,
    params: &[(&str, String)],
) -> anyhow::Result<serde_json::Value> {
    for attempt in 0..2 {
        let client_id = get_sc_client_id(client).await?;
        let mut pairs: Vec<(String, String)> = params
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        pairs.push(("client_id".to_string(), client_id));

        let resp = client
            .get(format!("https://api-v2.soundcloud.com{path}"))
            .query(&pairs)
            .send()
            .await?;
        if resp.status().as_u16() == 401 && attempt == 0 {
            *SC_CLIENT_ID.write().await = None;
            continue;
        }
        return response_json(resp).await;
    }
    anyhow::bail!("SoundCloud request failed after client_id refresh")
}

fn parse_sc_track(item: &serde_json::Value) -> Option<SCTrack> {
    let track_name = item["title"].as_str()?.trim().to_string();
    let username = item["user"]["username"].as_str().unwrap_or("").to_string();
    let artists = item["publisher_metadata"]["artist"]
        .as_str()
        .map(|s| {
            s.split(',')
                .map(|a| a.trim().to_string())
                .filter(|a| !a.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| vec![username]);
    let release_date = item["release_date"]
        .as_str()
        .or_else(|| item["display_date"].as_str())
        .or_else(|| item["created_at"].as_str())
        .and_then(parse_release_date)?;
    let label = item["label_name"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    let genre = item["genre"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| vec![s.to_string()])
        .unwrap_or_default();

    Some(SCTrack {
        artists,
        label,
        release_date,
        track_name,
        genre,
    })
}

pub async fn sc_search_tracks(
    client: &Client,
    q: &str,
    limit: usize,
) -> anyhow::Result<Vec<SCTrack>> {
    tracing::info!("SoundCloud track search: {q}");
    let json = sc_get(
        client,
        "/search/tracks",
        &[("q", q.to_string()), ("limit", limit.to_string())],
    )
    .await?;
    Ok(json["collection"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(parse_sc_track)
        .collect())
}

pub async fn sc_search_users(
    client: &Client,
    q: &str,
    limit: usize,
) -> anyhow::Result<Vec<SCUser>> {
    let json = sc_get(
        client,
        "/search/users",
        &[("q", q.to_string()), ("limit", limit.to_string())],
    )
    .await?;
    Ok(json["collection"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let id = item["id"].as_i64()?;
            let username = item["username"].as_str()?.to_string();
            Some(SCUser { id, username })
        })
        .collect())
}

/// Up to `limit` of the user's tracks, newest first, from `cursor` (the start if none), and the
/// cursor of the tracks after them, if any. The cursor is the `offset` of the response's
/// `next_href`, which isn't a count: SoundCloud ignores a numeric one.
pub async fn sc_get_user_tracks(
    client: &Client,
    user_id: i64,
    limit: usize,
    cursor: Option<&str>,
) -> anyhow::Result<(Vec<SCTrack>, Option<String>)> {
    let mut params = vec![("limit", limit.to_string())];
    params.extend(cursor.map(|c| ("offset", c.to_string())));
    let json = sc_get(client, &format!("/users/{user_id}/tracks"), &params).await?;
    let items = json["collection"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let next = json["next_href"]
        .as_str()
        .filter(|_| !items.is_empty())
        .and_then(|href| url::Url::parse(href).ok())
        .and_then(|url| {
            url.query_pairs()
                .find(|(k, _)| k == "offset")
                .map(|(_, v)| v.into_owned())
        });
    Ok((items.iter().filter_map(parse_sc_track).collect(), next))
}

/// Up to `limit` tracks of the SoundCloud account of the artist named `name`.
pub async fn sc_get_artist_tracks(
    client: &Client,
    name: &str,
    limit: usize,
) -> anyhow::Result<Vec<SCTrack>> {
    sc_tracks_by_person(client, name, limit, false).await
}

/// Up to `limit` tracks of the SoundCloud account of the label named `name`, labeled with the
/// account's name.
pub async fn sc_get_label_tracks(
    client: &Client,
    name: &str,
    limit: usize,
) -> anyhow::Result<Vec<SCTrack>> {
    sc_tracks_by_person(client, name, limit, true).await
}

/// The SoundCloud account of the artist or label named `name` (an artist or label both being
/// SoundCloud "people").
async fn sc_find_person(
    client: &Client,
    name: &str,
    is_label: bool,
) -> anyhow::Result<Option<SCUser>> {
    // For artists, take the first account actually named `name` rather than the top hit, which
    // may just be a similar name (searching "Luch" also returns "LUCH!Q"). Label accounts are
    // often named differently from the label itself ("Defected" for "Defected Records"), so
    // label lookups keep the top hit.
    let users = sc_search_users(client, name, if is_label { 1 } else { 10 }).await?;
    Ok(if is_label {
        users.into_iter().next()
    } else {
        users.into_iter().find(|u| is_same_name(&u.username, name))
    })
}

/// For a label, the account *is* the label regardless of what a track's own `label_name`
/// says, so every track is labeled with the account's username.
fn sc_label_tracks(tracks: &mut [SCTrack], user: &SCUser) {
    for track in tracks {
        track.label = Some(user.username.clone());
    }
}

/// The newest tracks of the account matching `name` (see [`sc_find_person`]).
async fn sc_tracks_by_person(
    client: &Client,
    name: &str,
    limit: usize,
    is_label: bool,
) -> anyhow::Result<Vec<SCTrack>> {
    let Some(user) = sc_find_person(client, name, is_label).await? else {
        return Ok(vec![]);
    };
    let (mut tracks, _) = sc_get_user_tracks(client, user.id, limit, None).await?;
    if is_label {
        sc_label_tracks(&mut tracks, &user);
    }
    Ok(tracks)
}

// ── Volumo ───────────────────────────────────────────────────────────────────

// Volumo's genre list is small (~30 entries) and effectively static — cache it for the
// process lifetime rather than refetching on every search.
static VL_GENRES: LazyLock<RwLock<Option<HashMap<i64, String>>>> =
    LazyLock::new(|| RwLock::new(None));

async fn fetch_vl_genres(client: &Client) -> anyhow::Result<HashMap<i64, String>> {
    let json = response_json(
        client
            .get("https://volumo.com/api/v1/genres")
            .header("Accept", "application/json")
            .send()
            .await?,
    )
    .await?;
    Ok(json
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| {
            let id = g["id"].as_i64()?;
            let name = g["name"].as_str()?.to_string();
            Some((id, name))
        })
        .collect())
}

async fn get_vl_genres(client: &Client) -> anyhow::Result<HashMap<i64, String>> {
    if let Some(genres) = VL_GENRES.read().await.clone() {
        return Ok(genres);
    }
    let genres = fetch_vl_genres(client).await?;
    *VL_GENRES.write().await = Some(genres.clone());
    Ok(genres)
}

fn parse_vl_track(item: &serde_json::Value, genres: &HashMap<i64, String>) -> Option<VLTrack> {
    let artists: Vec<String> = item["artists"]
        .as_array()?
        .iter()
        .filter_map(|a| a["name"].as_str().map(String::from))
        .collect();
    let track_name = item["title"].as_str()?.trim().to_string();
    let mix_name = item["version"].as_str().unwrap_or("").trim().to_string();
    let release_date = parse_release_date(item["release_start_at"].as_str().unwrap_or(""))?;
    let track_id = item["id"].as_i64()?;
    let label = item["recordlabel"]["name"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    let isrc = item["isrc"].as_str().map(String::from);
    let bpm = item["bpm"].as_i64();
    let key = item["keysign"].as_str().map(|s| s.trim().to_string());
    let genre = item["genre_id"]
        .as_i64()
        .and_then(|id| genres.get(&id).cloned());

    Some(VLTrack {
        artists,
        label,
        release_date,
        track_id,
        track_name,
        mix_name,
        isrc,
        bpm,
        key,
        genre,
    })
}

pub async fn vl_search_tracks(
    client: &Client,
    q: &str,
    limit: usize,
) -> anyhow::Result<Vec<VLTrack>> {
    tracing::info!("Volumo track search: {q}");
    let filter = serde_json::json!({ "search": q, "curation": "all" }).to_string();
    let limit_str = limit.to_string();
    let (resp, genres) = tokio::join!(
        client
            .get("https://volumo.com/api/v1/tracks")
            .header("Accept", "application/json")
            .query(&[
                ("filter", filter.as_str()),
                ("sort", "relevance"),
                ("limit", limit_str.as_str()),
            ])
            .send(),
        get_vl_genres(client),
    );
    let json = response_json(resp?).await?;
    let genres = genres.unwrap_or_else(|e| {
        tracing::warn!("Volumo genre list fetch failed: {e}");
        HashMap::new()
    });
    let tracks = json["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| parse_vl_track(item, &genres))
        .collect();
    Ok(tracks)
}

/// Whose catalog a label or artist search fetches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Credit {
    Label,
    Artist,
}

impl Credit {
    fn noun(self) -> &'static str {
        match self {
            Credit::Label => "label",
            Credit::Artist => "artist",
        }
    }

    /// Whether a source's `candidate` name is the label or artist the user searched for.
    pub fn matches(self, candidate: &str, wanted: &str) -> bool {
        match self {
            Credit::Label => is_same_label(candidate, wanted),
            Credit::Artist => is_same_name(candidate, wanted),
        }
    }

    /// The names to search a source for: `name`, then for a label its name without a generic
    /// suffix ("Defected Records" → "Defected"). A source may list the label only under its
    /// short name, and its search won't find that from the full one.
    fn queries(self, name: &str) -> Vec<String> {
        let mut queries = vec![name.to_string()];
        if self == Credit::Label {
            queries.extend(strip_label_suffixes(name));
        }
        queries
    }
}

/// Picks the catalog for `wanted` among a source's `candidates`, given in the source's
/// relevance order. The best kind of match wins, then the source's ranking: exactly the same
/// name, then the same name with a disambiguator ("Lucho (AR)" for "Lucho"), then a label
/// alias that [`Credit::matches`] accepts ("Solid Grooves Records" for "Solid Grooves"). So
/// "Lucho" isn't taken for "Lucho (VE)", nor "Solid Grooves" for "Solid Grooves Records",
/// when the source lists them first. Several equally good matches, such as two aliases, are
/// usually one label listed twice, so the source's ranking decides between them.
fn pick_credit<T>(
    credit: Credit,
    wanted: &str,
    candidates: impl IntoIterator<Item = (String, T)>,
) -> Option<T> {
    let wanted_key = crate::utils::normalize_for_match(wanted);
    let tier = |name: &str| {
        if crate::utils::normalize_for_match(name) == wanted_key {
            Some(0)
        } else if is_same_name(name, wanted) {
            Some(1)
        } else if credit.matches(name, wanted) {
            Some(2)
        } else {
            None
        }
    };
    let mut ranked: Vec<(u8, String, T)> = candidates
        .into_iter()
        .filter_map(|(name, id)| Some((tier(&name)?, name, id)))
        .collect();
    // Stable, so the source's order decides within a tier.
    ranked.sort_by_key(|(tier, _, _)| *tier);
    let best = ranked.first().map(|(tier, _, _)| *tier)?;
    let ties: Vec<&str> = ranked
        .iter()
        .take_while(|(tier, _, _)| *tier == best)
        .map(|(_, name, _)| name.as_str())
        .collect();
    if best > 0 && ties.len() > 1 {
        tracing::info!(
            "Several {}s match {wanted:?}: {ties:?}; taking the first",
            credit.noun()
        );
    }
    ranked.into_iter().next().map(|(_, _, id)| id)
}

/// Fetches up to `limit` tracks released on the Volumo label named `name`, newest first and
/// back to `since`, or none if Volumo has no label by that name.
pub async fn vl_get_label_tracks(
    client: &Client,
    name: &str,
    limit: usize,
    since: Option<NaiveDate>,
) -> anyhow::Result<Vec<VLTrack>> {
    vl_get_credit_tracks(client, Credit::Label, name, limit, since).await
}

/// Fetches up to `limit` tracks by the Volumo artist named `name`, including remixes, newest
/// first and back to `since`, or none if Volumo has no artist by that name.
pub async fn vl_get_artist_tracks(
    client: &Client,
    name: &str,
    limit: usize,
    since: Option<NaiveDate>,
) -> anyhow::Result<Vec<VLTrack>> {
    vl_get_credit_tracks(client, Credit::Artist, name, limit, since).await
}

/// The `items` of a Volumo list response, which must be there even when empty, so a changed
/// response isn't mistaken for a search without matches.
fn vl_items(json: &serde_json::Value) -> anyhow::Result<&Vec<serde_json::Value>> {
    json["items"]
        .as_array()
        .context("Volumo's response has no items list; its API may have changed")
}

/// The Volumo id of the label or artist named `name` (see [`pick_credit`]).
async fn vl_find_credit(
    client: &Client,
    credit: Credit,
    name: &str,
) -> anyhow::Result<Option<i64>> {
    let contributor_type = match credit {
        Credit::Label => "recordlabel",
        Credit::Artist => "artist",
    };
    for query in credit.queries(name) {
        let filter = serde_json::json!({ "search": query }).to_string();
        let resp = client
            .get(format!(
                "https://volumo.com/api/v1/contributors/{contributor_type}"
            ))
            .header("Accept", "application/json")
            .query(&[
                ("filter", filter.as_str()),
                ("sort", "relevance"),
                ("limit", "10"),
            ])
            .send()
            .await?;
        let json = response_json(resp).await?;
        let candidates = vl_items(&json)?
            .iter()
            .filter_map(|item| Some((item["name"].as_str()?.to_string(), item["id"].as_i64()?)));
        if let Some(id) = pick_credit(credit, name, candidates) {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// The track list filter for the Volumo label or artist named `name`, released back to
/// `since`, or none if Volumo has no label or artist by that name. A track search filters by
/// one contributor id only. Volumo credits collaborators separately, so a joint entry such as
/// "Solomun & Stimming" has no tracks of its own to miss.
async fn vl_credit_filter(
    client: &Client,
    credit: Credit,
    name: &str,
    since: Option<NaiveDate>,
) -> anyhow::Result<Option<String>> {
    let Some(id) = vl_find_credit(client, credit, name).await? else {
        tracing::info!("Volumo has no {} named {name:?}", credit.noun());
        return Ok(None);
    };
    tracing::info!("Volumo {} tracks: {name} ({id})", credit.noun());
    let id_key = match credit {
        Credit::Label => "recordlabel_id",
        Credit::Artist => "artist_id",
    };
    // Pre-orders end at today's release date.
    let today = chrono::Local::now().date_naive();
    let mut filter = serde_json::json!({
        id_key: id,
        "curation": "all",
        "release_start_to": today.to_string(),
    });
    if let Some(since) = since {
        filter["release_start_from"] = since.to_string().into();
    }
    Ok(Some(filter.to_string()))
}

/// Up to `limit` tracks matching `filter` from `offset` on, newest first, and how many the
/// response listed, released or not.
async fn vl_tracks_page(
    client: &Client,
    filter: &str,
    offset: usize,
    limit: usize,
) -> anyhow::Result<(usize, Vec<VLTrack>)> {
    let (offset, limit) = (offset.to_string(), limit.to_string());
    let (resp, genres) = tokio::join!(
        client
            .get("https://volumo.com/api/v1/tracks")
            .header("Accept", "application/json")
            .query(&[
                ("filter", filter),
                // Newest first.
                ("sort", "date"),
                ("limit", limit.as_str()),
                ("offset", offset.as_str()),
            ])
            .send(),
        get_vl_genres(client),
    );
    let json = response_json(resp?).await?;
    let genres = genres.unwrap_or_else(|e| {
        tracing::warn!("Volumo genre list fetch failed: {e}");
        HashMap::new()
    });
    let items = vl_items(&json)?;
    let today = chrono::Local::now().date_naive();
    let tracks = items
        .iter()
        .filter_map(|item| parse_vl_track(item, &genres))
        .filter(|t| t.release_date <= today)
        .collect();
    Ok((items.len(), tracks))
}

async fn vl_get_credit_tracks(
    client: &Client,
    credit: Credit,
    name: &str,
    limit: usize,
    since: Option<NaiveDate>,
) -> anyhow::Result<Vec<VLTrack>> {
    match vl_credit_filter(client, credit, name, since).await? {
        Some(filter) => Ok(vl_tracks_page(client, &filter, 0, limit).await?.1),
        None => Ok(vec![]),
    }
}

// ── Traxsource ───────────────────────────────────────────────────────────────

// Traxsource has no JSON API; its combined `/search` page server-renders a "Tracks" preview
// (capped at ~10 results, regardless of the total match count shown in the page's tab
// counter) that we scrape directly. The dedicated `/search/tracks` (and `/search/titles`)
// pages behind the "see all" links are Cloudflare-challenge-protected, so we don't use them.
fn parse_tx_track(row: &scraper::ElementRef) -> Option<TXTrack> {
    let title_sel = Selector::parse("div.trk-cell.title > a").ok()?;
    let version_sel = Selector::parse("div.trk-cell.title span.version").ok()?;
    let artist_sel = Selector::parse("div.trk-cell.artists a.com-artists").ok()?;
    let label_sel = Selector::parse("div.trk-cell.label a").ok()?;
    let key_bpm_sel = Selector::parse("div.trk-cell.key-bpm").ok()?;
    let genre_sel = Selector::parse("div.trk-cell.genre a").ok()?;
    let date_sel = Selector::parse("div.trk-cell.r-date").ok()?;

    let track_id = row.value().attr("data-trid")?.parse::<i64>().ok()?;
    let track_name = row
        .select(&title_sel)
        .next()?
        .text()
        .collect::<String>()
        .trim()
        .to_string();
    let mix_name = row
        .select(&version_sel)
        .next()
        .and_then(|el| el.text().next())
        .unwrap_or("")
        .trim()
        .to_string();
    let artists: Vec<String> = row
        .select(&artist_sel)
        .map(|el| el.text().collect::<String>().trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let label = row
        .select(&label_sel)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .unwrap_or_default();
    let genre = row
        .select(&genre_sel)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .filter(|s| !s.is_empty());
    let release_date = row
        .select(&date_sel)
        .next()
        .and_then(|el| parse_release_date(el.text().collect::<String>().trim()))?;

    let key_bpm_parts: Vec<String> = row
        .select(&key_bpm_sel)
        .next()
        .map(|el| {
            el.text()
                .map(|t| t.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let mut bpm = None;
    let mut key = None;
    for part in key_bpm_parts {
        if let Ok(b) = part.parse::<i64>() {
            bpm = Some(b);
        } else {
            key = Some(part);
        }
    }

    Some(TXTrack {
        artists,
        label,
        release_date,
        track_id,
        track_name,
        mix_name,
        bpm,
        key,
        genre,
    })
}

// The search page fails with a 502 unless the request carries a `PHPSESSID` cookie. Any value
// is accepted, including one the server never issued, so send a random one made once per
// process instead of fetching a session first. It's kept off the shared client's other
// requests, which have no cookie store.
static TX_SESSION_COOKIE: LazyLock<String> =
    LazyLock::new(|| format!("PHPSESSID={}", uuid::Uuid::new_v4().simple()));

async fn tx_search_page(client: &Client, q: &str) -> anyhow::Result<Html> {
    let resp = client
        .get("https://www.traxsource.com/search")
        .query(&[("term", q)])
        .header(wreq::header::COOKIE, TX_SESSION_COOKIE.as_str())
        .send()
        .await?;
    Ok(Html::parse_document(&response_text(resp).await?))
}

/// A page's number of track rows and the released tracks among them. Pre-orders show
/// "Pre-order for <date>" in place of the release date and are skipped, since they can't be
/// played yet.
fn parse_tx_page(doc: &Html) -> (usize, Vec<TXTrack>) {
    let row_sel = Selector::parse("div.trk-row[data-trid]").unwrap();
    let pre_order_sel = Selector::parse("div.trk-cell.r-date .pre-ord-txt").unwrap();
    let today = chrono::Local::now().date_naive();
    let rows: Vec<_> = doc.select(&row_sel).collect();
    let tracks = rows
        .iter()
        .filter(|row| row.select(&pre_order_sel).next().is_none())
        .filter_map(parse_tx_track)
        .filter(|t| t.release_date <= today)
        .collect();
    (rows.len(), tracks)
}

pub async fn tx_search_tracks(client: &Client, q: &str) -> anyhow::Result<Vec<TXTrack>> {
    tracing::info!("Traxsource track search: {q}");
    let (_, tracks) = parse_tx_page(&tx_search_page(client, q).await?);
    tracing::info!("Traxsource returned {} tracks", tracks.len());
    Ok(tracks)
}

/// The labels or artists a search page links to, as (name, `/label/<id>/<slug>` or
/// `/artist/<id>/<slug>` path) in page order: its label or artist results, then the credits of
/// its track and release results.
fn tx_credit_candidates(doc: &Html, credit: Credit) -> Vec<(String, String)> {
    let link_sel = Selector::parse(match credit {
        Credit::Label => "div.label-grid a.title, div.trk-cell.label a, a.com-label",
        Credit::Artist => "div.artist-grid a.title, div.trk-cell.artists a.com-artists",
    })
    .unwrap();
    let prefix = format!("/{}/", credit.noun());
    doc.select(&link_sel)
        .filter_map(|a| {
            let href = a.value().attr("href")?;
            let path = href.split(['?', '#']).next().unwrap_or(href);
            let name = a.text().collect::<String>().trim().to_string();
            path.starts_with(&prefix).then(|| (name, path.to_string()))
        })
        .collect()
}

/// The path of the Traxsource label or artist named `name` (see [`pick_credit`]).
async fn tx_find_credit_path(
    client: &Client,
    credit: Credit,
    name: &str,
) -> anyhow::Result<Option<String>> {
    for query in credit.queries(name) {
        let candidates = tx_credit_candidates(&tx_search_page(client, &query).await?, credit);
        if let Some(path) = pick_credit(credit, name, candidates) {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

/// Fetches up to `limit` tracks released on the Traxsource label named `name`, newest first
/// and back to about `since`, or none if a search finds no label by that name.
pub async fn tx_get_label_tracks(
    client: &Client,
    name: &str,
    limit: usize,
    since: Option<NaiveDate>,
) -> anyhow::Result<Vec<TXTrack>> {
    tx_get_credit_tracks(client, Credit::Label, name, limit, since).await
}

/// Fetches up to `limit` tracks by the Traxsource artist named `name`, including remixes,
/// newest first and back to about `since`, or none if a search finds no artist by that name.
pub async fn tx_get_artist_tracks(
    client: &Client,
    name: &str,
    limit: usize,
    since: Option<NaiveDate>,
) -> anyhow::Result<Vec<TXTrack>> {
    tx_get_credit_tracks(client, Credit::Artist, name, limit, since).await
}

const TX_PAGE_SIZE: usize = 100;
/// Bounds the requests for one catalog, whatever the limit.
const TX_MAX_PAGES: usize = 10;

/// The URL of the Traxsource label's or artist's track list (see [`tx_credit_page`]), or none
/// if a search finds no label or artist named `name`.
async fn tx_credit_url(
    client: &Client,
    credit: Credit,
    name: &str,
) -> anyhow::Result<Option<String>> {
    let Some(path) = tx_find_credit_path(client, credit, name).await? else {
        tracing::info!("Traxsource has no {} named {name:?}", credit.noun());
        return Ok(None);
    };
    let url = format!("https://www.traxsource.com/scripts/builder.php{path}");
    tracing::info!("Traxsource {} tracks: {url}", credit.noun());
    Ok(Some(url))
}

/// Page `page` (from 1) of a label's or artist's tracks, newest first: its number of rows and
/// the released tracks among them (see [`parse_tx_page`]). Label and artist pages are
/// Cloudflare-challenge-protected, but the same page loaded through `builder.php`, which the
/// site uses for in-page navigation, isn't.
async fn tx_credit_page(
    client: &Client,
    url: &str,
    page: usize,
) -> anyhow::Result<(usize, Vec<TXTrack>)> {
    let resp = client
        .get(url)
        .query(&[
            ("cn", "tracks"),
            ("ipp", TX_PAGE_SIZE.to_string().as_str()),
            ("ob", "r_date"),
            ("so", "desc"),
            ("page", page.to_string().as_str()),
        ])
        .header(wreq::header::COOKIE, TX_SESSION_COOKIE.as_str())
        .send()
        .await?;
    Ok(parse_tx_page(&Html::parse_document(
        &response_text(resp).await?,
    )))
}

/// Pre-orders come first and can fill whole pages, so this pages on until it has `limit`
/// released tracks, reaches `since` (the last page may reach further back) or the catalog ends.
async fn tx_get_credit_tracks(
    client: &Client,
    credit: Credit,
    name: &str,
    limit: usize,
    since: Option<NaiveDate>,
) -> anyhow::Result<Vec<TXTrack>> {
    let Some(url) = tx_credit_url(client, credit, name).await? else {
        return Ok(vec![]);
    };
    let mut tracks = vec![];
    for page in 1..=TX_MAX_PAGES {
        let (rows, page_tracks) = tx_credit_page(client, &url, page).await?;
        let reached_since = since.is_some_and(|since| {
            page_tracks
                .last()
                .is_some_and(|t: &TXTrack| t.release_date < since)
        });
        tracks.extend(page_tracks);
        if rows < TX_PAGE_SIZE || tracks.len() >= limit || reached_since {
            break;
        }
    }
    tracks.truncate(limit);
    tracing::info!(
        "Traxsource returned {} {} tracks",
        tracks.len(),
        credit.noun()
    );
    Ok(tracks)
}

/// Extracts the cross-genre top-100 chart from Traxsource's `/top/tracks` page as (1-based chart
/// position, track) pairs, in chart order. The rows are the same `div.trk-row` markup as the search
/// page, plus a `div.tnum` position cell (and no key/BPM cell). The page's own position is used,
/// falling back to the row's index, so an unparseable row leaves a gap instead of shifting
/// everything below it.
fn parse_tx_top100(html: &str) -> Vec<(usize, TXTrack)> {
    let doc = Html::parse_document(html);
    let row_sel = Selector::parse("div.trk-row[data-trid]").unwrap();
    let pos_sel = Selector::parse("div.tnum").unwrap();
    doc.select(&row_sel)
        .enumerate()
        .filter_map(|(i, row)| {
            let position = row
                .select(&pos_sel)
                .next()
                .and_then(|el| el.text().collect::<String>().trim().parse().ok())
                .unwrap_or(i + 1);
            Some((position, parse_tx_track(&row)?))
        })
        .collect()
}

/// Fetches Traxsource's top-100 chart across all genres.
pub async fn tx_get_top100(client: &Client) -> anyhow::Result<Vec<(usize, TXTrack)>> {
    let url = "https://www.traxsource.com/top/tracks";
    tracing::info!("Fetching Traxsource top 100: {url}");
    let html = response_text(client.get(url).send().await?).await?;
    let chart = parse_tx_top100(&html);
    if chart.is_empty() {
        anyhow::bail!("no top-100 chart found on {url}");
    }
    Ok(chart)
}

// ── YouTube ───────────────────────────────────────────────────────────────────

const YT_CLIENTS: &[(&str, &str)] = &[
    ("WEB", "2.20240201.00.00"),
    ("MWEB", "2.20240201.00.00"),
    ("ANDROID", "19.09.37"),
];

/// Extracts a playlist id from a public YouTube or YouTube Music playlist URL.
pub fn parse_youtube_playlist_id(input: &str) -> Option<String> {
    let parsed = url::Url::parse(input.trim()).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    if !(host == "youtube.com" || host.ends_with(".youtube.com")) {
        return None;
    }

    let id = parsed
        .query_pairs()
        .find(|(key, _)| key == "list")
        .map(|(_, value)| value.into_owned())?;
    (!id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
    .then_some(id)
}

/// Extracts a video id from a YouTube or YouTube Music video link (`watch?v=`, `youtu.be/`,
/// `/shorts/`, `/embed/`, `/live/`).
pub fn parse_youtube_video_id(input: &str) -> Option<String> {
    let parsed = url::Url::parse(input.trim()).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    let mut segments = parsed.path_segments()?;
    let id = if host == "youtu.be" {
        segments.next().map(str::to_string)
    } else if host == "youtube.com" || host.ends_with(".youtube.com") {
        match segments.next() {
            Some("watch") => parsed
                .query_pairs()
                .find(|(key, _)| key == "v")
                .map(|(_, value)| value.into_owned()),
            Some("shorts" | "embed" | "live" | "v") => segments.next().map(str::to_string),
            _ => None,
        }
    } else {
        None
    }?;
    (id.len() == 11
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
    .then_some(id)
}

fn extract_json_assignment(html: &str, marker: &str) -> Option<serde_json::Value> {
    let start = html.find(marker)? + marker.len();
    let json_start = html[start..].find('{')? + start;
    let bytes = html.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (offset, byte) in bytes[json_start..].iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }

        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return serde_json::from_slice(&bytes[json_start..=json_start + offset]).ok();
                }
            }
            _ => {}
        }
    }
    None
}

fn text_content(value: &serde_json::Value) -> String {
    value["content"]
        .as_str()
        .or_else(|| value["simpleText"].as_str())
        .map(str::to_string)
        .or_else(|| {
            value["runs"].as_array().map(|runs| {
                runs.iter()
                    .filter_map(|run| run["text"].as_str())
                    .collect::<String>()
            })
        })
        .unwrap_or_default()
}

fn first_video_id(value: &serde_json::Value) -> Option<String> {
    if let Some(id) = value["videoId"].as_str() {
        return Some(id.to_string());
    }
    if let Some(sources) = value["sources"].as_array() {
        for source in sources {
            if let Some(url) = source["url"].as_str() {
                if let Some(rest) = url.split("/vi/").nth(1) {
                    if let Some(id) = rest.split('/').next().filter(|id| !id.is_empty()) {
                        return Some(id.to_string());
                    }
                }
            }
        }
    }
    value.as_object()?.values().find_map(first_video_id)
}

fn topic_artist(value: String) -> String {
    value
        .strip_suffix(" - Topic")
        .or_else(|| value.strip_suffix(" – Topic"))
        .unwrap_or(&value)
        .trim()
        .to_string()
}

fn parse_lockup_track(lockup: &serde_json::Value) -> Option<Track> {
    let metadata = &lockup["metadata"]["lockupMetadataViewModel"];
    let title = text_content(&metadata["title"]);
    if title.is_empty() || lockup["contentImage"]["thumbnailViewModel"].is_null() {
        return None;
    }

    let yt_id = first_video_id(lockup)?;
    let artist = metadata["metadata"]["contentMetadataViewModel"]["metadataRows"]
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row["metadataParts"].as_array())
        .and_then(|parts| parts.first())
        .map(|part| topic_artist(text_content(&part["text"])))
        .filter(|artist| !artist.is_empty());

    Some(Track {
        yt_id,
        title,
        artists: artist.into_iter().collect(),
        ..Default::default()
    })
}

fn parse_legacy_playlist_video(video: &serde_json::Value) -> Option<Track> {
    let yt_id = video["videoId"].as_str()?.to_string();
    let title = text_content(&video["title"]);
    if title.is_empty() {
        return None;
    }
    let artist = text_content(&video["shortBylineText"]);
    Some(Track {
        yt_id,
        title,
        artists: (!artist.is_empty())
            .then(|| topic_artist(artist))
            .into_iter()
            .collect(),
        ..Default::default()
    })
}

fn collect_playlist_tracks(value: &serde_json::Value, tracks: &mut Vec<Track>) {
    if let Some(video) = value.get("playlistVideoRenderer") {
        if let Some(track) = parse_legacy_playlist_video(video) {
            tracks.push(track);
        }
        return;
    }
    if let Some(lockup) = value.get("lockupViewModel") {
        if let Some(track) = parse_lockup_track(lockup) {
            tracks.push(track);
        }
        return;
    }
    if let Some(object) = value.as_object() {
        for child in object.values() {
            collect_playlist_tracks(child, tracks);
        }
    } else if let Some(array) = value.as_array() {
        for child in array {
            collect_playlist_tracks(child, tracks);
        }
    }
}

fn html_config_value(html: &str, key: &str) -> Option<String> {
    let marker = format!("\"{key}\":\"");
    let start = html.find(&marker)? + marker.len();
    let end = html[start..].find('"')? + start;
    Some(html[start..end].to_string())
}

fn continuation_token(value: &serde_json::Value) -> Option<String> {
    value["continuationCommand"]["token"]
        .as_str()
        .map(String::from)
        .or_else(|| value.as_object()?.values().find_map(continuation_token))
        .or_else(|| value.as_array()?.iter().find_map(continuation_token))
}

fn playlist_name(data: &serde_json::Value) -> Option<String> {
    data["metadata"]["playlistMetadataRenderer"]["title"]
        .as_str()
        .map(String::from)
        .or_else(|| {
            data["sidebar"]["playlistSidebarRenderer"]["items"]
                .as_array()?
                .iter()
                .find_map(|item| {
                    let title = &item["playlistSidebarPrimaryInfoRenderer"]["title"];
                    let value = text_content(title);
                    (!value.is_empty()).then_some(value)
                })
        })
}

fn unique_tracks(tracks: impl IntoIterator<Item = Track>) -> Vec<Track> {
    let mut seen = std::collections::HashSet::new();
    tracks
        .into_iter()
        .filter(|track| seen.insert(track.yt_id.clone()))
        .collect()
}

pub struct YouTubePlaylist {
    pub name: String,
    pub tracks: Vec<Track>,
}

/// Loads a public YouTube playlist page and converts its videos to collection tracks. This uses
/// the public page's embedded data rather than the authenticated YouTube Data API, so importing
/// does not require a Google account or API key.
pub async fn yt_import_playlist(client: &Client, input: &str) -> anyhow::Result<YouTubePlaylist> {
    let playlist_id =
        parse_youtube_playlist_id(input).context("not a valid YouTube playlist link")?;
    let page_url = format!("https://www.youtube.com/playlist?list={playlist_id}");
    let response = client
        .get(&page_url)
        .header("User-Agent", "Mozilla/5.0")
        .header("Accept-Language", "en-US,en;q=0.9")
        .send()
        .await?;
    anyhow::ensure!(
        response.status().is_success(),
        "YouTube returned {}",
        response.status()
    );
    let html = response.text().await?;
    let data = extract_json_assignment(&html, "var ytInitialData = ")
        .context("could not read playlist data from YouTube")?;

    let name = playlist_name(&data).unwrap_or_else(|| playlist_id.clone());
    let mut tracks = Vec::new();
    collect_playlist_tracks(&data, &mut tracks);
    let mut token = continuation_token(&data);

    // The first page normally contains the complete playlist for smaller lists. Continue paging
    // for larger lists, with a bound so a changed YouTube response cannot loop forever.
    let api_key = html_config_value(&html, "INNERTUBE_API_KEY");
    let client_version = html_config_value(&html, "INNERTUBE_CLIENT_VERSION");
    let visitor_data = data["responseContext"]["visitorData"].as_str();

    for _ in 0..20 {
        let Some(current_token) = token.take() else {
            break;
        };
        let body = serde_json::json!({
            "context": {
                "client": {
                    "clientName": "WEB",
                    "clientVersion": client_version.as_deref().unwrap_or("2.20240201.00.00"),
                    "visitorData": visitor_data,
                }
            },
            "continuation": url::form_urlencoded::parse(
                format!("token={current_token}").as_bytes(),
            )
                .next()
                .map(|(_, value)| value.into_owned())
                .unwrap_or(current_token),
        });
        let endpoint = if let Some(key) = api_key.as_deref() {
            format!("https://www.youtube.com/youtubei/v1/browse?key={key}")
        } else {
            "https://www.youtube.com/youtubei/v1/browse".to_string()
        };
        let response = client
            .post(endpoint)
            .header("User-Agent", "Mozilla/5.0")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;
        if !response.status().is_success() {
            break;
        }
        let page: serde_json::Value = response.json().await?;
        let before = tracks.len();
        collect_playlist_tracks(&page, &mut tracks);
        token = continuation_token(&page);
        if tracks.len() == before && token.is_none() {
            break;
        }
    }

    let tracks = unique_tracks(tracks);
    anyhow::ensure!(!tracks.is_empty(), "playlist contains no public videos");
    Ok(YouTubePlaylist { name, tracks })
}

/// Videos longer than this are excluded from search results — likely DJ sets/mixes
/// rather than the single track being matched.
const MAX_VIDEO_LENGTH_SECS: u32 = 15 * 60;

fn get_text_runs(v: &serde_json::Value) -> String {
    v.as_array()
        .and_then(|a| a.first())
        .and_then(|f| f["text"].as_str())
        .map(String::from)
        .unwrap_or_default()
}

/// Parses a YouTube duration string like "3:45" or "1:02:34" into seconds.
fn parse_duration_secs(text: &str) -> Option<u32> {
    let mut secs = 0u32;
    for part in text.split(':') {
        secs = secs * 60 + part.parse::<u32>().ok()?;
    }
    Some(secs)
}

/// Searches YouTube with each of `YT_CLIENTS` in turn until one finds videos. Finding none is
/// `Ok` once a client has answered with search results; only when none did (network errors,
/// error statuses, responses without results) is it an error, the last client's.
pub async fn yt_search_simple(
    client: &Client,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<YTVideo>> {
    let mut answered = false;
    let mut last_error = None;
    for (name, version) in YT_CLIENTS {
        let body = serde_json::json!({
            "context": {"client": {"clientName": name, "clientVersion": version}},
            "query": query,
        });

        let resp = client
            .post("https://www.youtube.com/youtubei/v1/search")
            .header("User-Agent", "Mozilla/5.0")
            .header("Accept-Language", "en-US,en;q=0.9")
            .query(&[("key", "AIzaSyDummyKey")])
            .json(&body)
            .send()
            .await;

        let data = match resp {
            Ok(r) => response_json(r).await,
            Err(error) => Err(error.into()),
        };
        let data = match data {
            Ok(data) => data,
            Err(error) => {
                last_error = Some(error.context(format!("{name} client")));
                continue;
            }
        };

        let mut results = vec![];
        let sections = &data["contents"]["twoColumnSearchResultsRenderer"]["primaryContents"]
            ["sectionListRenderer"]["contents"];
        if let Some(sections) = sections.as_array() {
            answered = true;
            'outer: for section in sections {
                if let Some(items) = section["itemSectionRenderer"]["contents"].as_array() {
                    for item in items {
                        if let Some(video) = item.get("videoRenderer") {
                            let length_secs = video["lengthText"]["simpleText"]
                                .as_str()
                                .and_then(parse_duration_secs);
                            if !matches!(length_secs, Some(secs) if secs <= MAX_VIDEO_LENGTH_SECS) {
                                continue;
                            }
                            let id = match video["videoId"].as_str() {
                                Some(id) => id.to_string(),
                                None => continue,
                            };
                            let title = get_text_runs(&video["title"]["runs"]);
                            let channel = {
                                let o = get_text_runs(&video["ownerText"]["runs"]);
                                if o.is_empty() {
                                    get_text_runs(&video["longBylineText"]["runs"])
                                } else {
                                    o
                                }
                            };
                            results.push(YTVideo {
                                title,
                                id,
                                channel,
                                duration_secs: length_secs,
                            });
                            if results.len() >= limit {
                                break 'outer;
                            }
                        }
                    }
                }
            }
        }

        if !results.is_empty() {
            return Ok(results);
        }
        if !answered {
            last_error = Some(anyhow::anyhow!(
                "{name} client: no search results in the response"
            ));
        }
    }
    match last_error {
        Some(error) if !answered => Err(error),
        _ => Ok(vec![]),
    }
}

/// How many videos `yt_find_videos` offers for a search.
const YT_FIND_LIMIT: usize = 20;

/// YouTube videos to choose a track's video from: search results for `query`, or just the
/// video a pasted link points to. A linked video YouTube's search doesn't turn up (e.g. one
/// longer than `MAX_VIDEO_LENGTH_SECS`) is still returned, with its id only and no title.
pub async fn yt_find_videos(client: &Client, query: &str) -> anyhow::Result<Vec<YTVideo>> {
    let Some(id) = parse_youtube_video_id(query) else {
        return yt_search_simple(client, query, YT_FIND_LIMIT).await;
    };
    // Searching for a video's id usually finds it, which gives its title and channel.
    let found = yt_search_simple(client, &id, 5)
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|video| video.id == id);
    Ok(vec![found.unwrap_or(YTVideo {
        title: String::new(),
        id,
        channel: String::new(),
        duration_secs: None,
    })])
}

pub async fn yt_create_anon_playlist(
    client: &Client,
    video_ids: &[String],
) -> anyhow::Result<String> {
    let ids = video_ids.join(",");
    let url = format!("http://www.youtube.com/watch_videos?video_ids={ids}");
    let resp = client.get(&url).send().await?;
    let final_url = resp.uri().to_string();

    let parsed = url::Url::parse(&final_url)?;
    let list_id = parsed
        .query_pairs()
        .find(|(k, _)| k == "list")
        .map(|(_, v)| v.to_string())
        .context("no list param in YouTube redirect")?;

    Ok(format!("https://music.youtube.com/watch?list={list_id}"))
}

// ── Search entry points (stream results via channel) ─────────────────────────

/// A track from one of the search backends, before a YouTube match has been resolved.
enum RawTrack {
    Bp(BPTrack),
    Sc(SCTrack),
    Vl(VLTrack),
    Tx(TXTrack),
    Sp(SpTrack),
    Am(AMTrack),
    Sz(ShazamTrack),
}

/// Words that mark a bracketed segment of a title as a mix/edit identifier, as opposed to
/// e.g. a label tag or "Official Video" annotation.
pub(crate) const MIX_KEYWORDS: &[&str] = &[
    "remix", "mix", "edit", "bootleg", "flip", "rework", "vip", "dub", "version", "rmx",
];

/// Extracts a mix/remix tag from a title, e.g. "Artist - Track (Someone Remix)" -> `Some("someone remix")`.
/// Bracketed groups are preferred; separator-delimited suffixes such as "- Extended Mix" are
/// also recognized.
fn extract_mix_tag(title: &str) -> Option<String> {
    let lower = title.to_lowercase();
    let mut result = bracketed_mix_tag(&lower);
    if result.is_none() {
        // A sizeable number of label uploads use an unbracketed suffix, e.g.
        // "Artist - Track - Extended Mix". Only inspect complete separator-delimited
        // segments here; treating any occurrence of "mix" as a version would create false
        // positives for legitimate track names.
        for segment in title_segments(&lower).into_iter().rev() {
            let segment = segment.trim();
            if MIX_KEYWORDS.iter().any(|k| {
                segment
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|word| word == *k)
            }) {
                result = Some(segment.to_string());
                break;
            }
        }
    }
    result
}

/// The last bracketed group of a lowercase title that names a mix, as in "track (someone remix)".
fn bracketed_mix_tag(lower: &str) -> Option<String> {
    let mut result = None;
    for (open, close) in [('(', ')'), ('[', ']')] {
        let mut rest = lower;
        while let Some(start) = rest.find(open) {
            let Some(end) = rest[start..].find(close) else {
                break;
            };
            let inner = rest[start + 1..start + end].trim();
            if MIX_KEYWORDS.iter().any(|k| inner.contains(k)) {
                result = Some(inner.to_string());
            }
            rest = &rest[start + end + 1..];
        }
    }
    result
}

impl RawTrack {
    fn yt_query(&self) -> String {
        let artists =
            crate::utils::strip_artist_disambiguators(&self.expected_artists().join(", "));
        let title = crate::utils::strip_feat(self.expected_title());
        match self.expected_mix() {
            Some(mix) if token_recall(&mix, &title) < 0.8 => {
                format!("{artists} {title} {mix}")
            }
            Some(_) | None => format!("{artists} {title}"),
        }
    }

    fn expected_label(&self) -> Option<&str> {
        match self {
            RawTrack::Bp(t) => Some(t.label.as_str()),
            RawTrack::Sc(t) => t.label.as_deref(),
            RawTrack::Vl(t) => Some(t.label.as_str()),
            RawTrack::Tx(t) => Some(t.label.as_str()),
            RawTrack::Sp(_) => None,
            RawTrack::Am(_) => None,
            RawTrack::Sz(_) => None,
        }
        .filter(|label| !label.trim().is_empty())
    }

    fn expected_title(&self) -> &str {
        match self {
            RawTrack::Bp(t) => &t.track_name,
            RawTrack::Sc(t) => &t.track_name,
            RawTrack::Vl(t) => &t.track_name,
            RawTrack::Tx(t) => &t.track_name,
            RawTrack::Sp(t) => &t.track_name,
            RawTrack::Am(t) => &t.track_name,
            RawTrack::Sz(t) => &t.title,
        }
    }

    fn expected_artists(&self) -> &[String] {
        match self {
            RawTrack::Bp(t) => &t.artists,
            RawTrack::Sc(t) => &t.artists,
            RawTrack::Vl(t) => &t.artists,
            RawTrack::Tx(t) => &t.artists,
            RawTrack::Sp(t) => &t.artists,
            RawTrack::Am(t) => &t.artists,
            RawTrack::Sz(t) => &t.artists,
        }
    }

    /// The remix/mix identity this track is expected to match on YouTube, if any.
    /// Beatport, Volumo, and Traxsource expose it as a separate field, and Apple Music's is split
    /// off its title when parsed; SoundCloud and Spotify embed it in the track name.
    fn expected_mix(&self) -> Option<String> {
        match self {
            RawTrack::Bp(t) => (!is_original_mix(&t.mix_name)).then(|| t.mix_name.clone()),
            RawTrack::Sc(t) => extract_mix_tag(&t.track_name),
            RawTrack::Vl(t) => (!is_original_mix(&t.mix_name)).then(|| t.mix_name.clone()),
            RawTrack::Tx(t) => (!is_original_mix(&t.mix_name)).then(|| t.mix_name.clone()),
            RawTrack::Sp(t) => extract_mix_tag(&t.track_name),
            RawTrack::Am(t) => t.mix_name.clone().filter(|m| !is_original_mix(m)),
            RawTrack::Sz(t) => extract_mix_tag(&t.title),
        }
    }

    /// The expected mix when it is set apart from the track's name: a field of its own, a
    /// bracketed tag, or Spotify's " - Someone Remix" suffix (its names never start with the
    /// artist). Otherwise, as in SoundCloud's "Good Time Extended Mix" or "Artist - Good Time
    /// Extended Mix", the tag may run into the name, whose words would pass for a remixer's.
    fn isolated_mix(&self) -> Option<String> {
        match self {
            RawTrack::Sc(t) => bracketed_mix_tag(&t.track_name.to_lowercase()),
            RawTrack::Sp(t) => {
                let lower = t.track_name.to_lowercase();
                bracketed_mix_tag(&lower).or_else(|| {
                    let segments = title_segments(&lower);
                    let segment = segments.last().filter(|_| segments.len() > 1)?.trim();
                    (extract_mix_tag(segment).as_deref() == Some(segment))
                        .then(|| segment.to_string())
                })
            }
            RawTrack::Sz(t) => bracketed_mix_tag(&t.title.to_lowercase()),
            _ => self.expected_mix(),
        }
    }

    fn source(&self) -> TrackSource {
        match self {
            RawTrack::Bp(_) => TrackSource::Beatport,
            RawTrack::Sc(_) => TrackSource::Soundcloud,
            RawTrack::Vl(_) => TrackSource::Volumo,
            RawTrack::Tx(_) => TrackSource::Traxsource,
            RawTrack::Sp(_) => TrackSource::Spotify,
            RawTrack::Am(_) => TrackSource::AppleMusic,
            RawTrack::Sz(_) => TrackSource::Shazam,
        }
    }

    fn release_date(&self) -> NaiveDate {
        match self {
            RawTrack::Bp(t) => t.release_date,
            RawTrack::Sc(t) => t.release_date,
            RawTrack::Vl(t) => t.release_date,
            RawTrack::Tx(t) => t.release_date,
            RawTrack::Sp(t) => t.release_date,
            RawTrack::Am(t) => t.release_date,
            RawTrack::Sz(_) => NaiveDate::MIN,
        }
    }

    fn into_track(self, yt_id: String) -> crate::models::Track {
        match self {
            RawTrack::Bp(t) => t.into_track(yt_id),
            RawTrack::Sc(t) => t.into_track(yt_id),
            RawTrack::Vl(t) => t.into_track(yt_id),
            RawTrack::Tx(t) => t.into_track(yt_id),
            RawTrack::Sp(t) => t.into_track(yt_id),
            RawTrack::Am(t) => t.into_track(yt_id),
            RawTrack::Sz(t) => Track {
                yt_id,
                title: t.title,
                artists: t.artists,
                ..Default::default()
            },
        }
    }
}

const MIN_MATCH_SCORE: f64 = 0.72;
const MIN_TITLE_SCORE: f64 = 0.68;
const MIN_ARTIST_SCORE: f64 = 0.50;
const YT_CANDIDATE_LIMIT: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VersionMatch {
    Match,
    Unknown,
    Contradiction,
}

#[derive(Clone, Debug)]
struct MatchScore {
    total: f64,
    title: f64,
    artist: f64,
    version: VersionMatch,
    candidate_mix: Option<String>,
    specific_version_expected: bool,
    /// The matched version credits a remixer, which stands in for the artist.
    credited_version_match: bool,
    disqualified: bool,
}

impl MatchScore {
    fn acceptable(&self) -> bool {
        !self.disqualified
            && self.version != VersionMatch::Contradiction
            && (!self.specific_version_expected || self.version == VersionMatch::Match)
            && self.title >= MIN_TITLE_SCORE
            && (self.artist >= MIN_ARTIST_SCORE
                || (self.credited_version_match && self.title >= 0.80))
            && self.total >= MIN_MATCH_SCORE
    }
}

fn match_tokens(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(String::from)
        .collect()
}

fn compact_match_text(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn token_recall(expected: &str, actual: &str) -> f64 {
    let expected = match_tokens(expected);
    if expected.is_empty() {
        return 0.0;
    }
    let actual = match_tokens(actual);
    expected.intersection(&actual).count() as f64 / expected.len() as f64
}

fn normalized_similarity(a: &str, b: &str) -> f64 {
    strsim::normalized_levenshtein(
        &crate::utils::normalize_for_match(a),
        &crate::utils::normalize_for_match(b),
    )
}

fn title_segments(title: &str) -> Vec<&str> {
    let mut segments = vec![title];
    for delimiter in [" - ", " – ", " — ", " | "] {
        segments = segments
            .into_iter()
            .flat_map(|segment| segment.split(delimiter))
            .collect();
    }
    segments
}

fn title_similarity(expected: &str, candidate: &str) -> f64 {
    let segment_similarity = title_segments(candidate)
        .into_iter()
        .map(|segment| normalized_similarity(expected, segment))
        .fold(normalized_similarity(expected, candidate), f64::max);
    0.75 * token_recall(expected, candidate) + 0.25 * segment_similarity
}

fn version_match(
    expected: Option<&str>,
    candidate: Option<&str>,
    full_title: &str,
) -> VersionMatch {
    match expected {
        None => match candidate {
            None => VersionMatch::Match,
            Some(tag) if is_original_mix(tag) => VersionMatch::Match,
            Some(_) => VersionMatch::Contradiction,
        },
        Some(expected) => {
            let full_contains_mix = token_recall(expected, full_title) >= 0.8;
            match candidate {
                Some(candidate)
                    if token_recall(expected, candidate) >= 0.8 || full_contains_mix =>
                {
                    VersionMatch::Match
                }
                Some(_) => VersionMatch::Contradiction,
                None if full_contains_mix => VersionMatch::Match,
                None => VersionMatch::Unknown,
            }
        }
    }
}

fn has_unwanted_format(raw: &RawTrack, candidate: &YTVideo) -> bool {
    const UNWANTED: &[&str] = &[
        "cover",
        "karaoke",
        "tutorial",
        "reaction",
        "slowed",
        "sped up",
        "nightcore",
        "mashup",
        "preview",
        "teaser",
    ];
    let title = candidate.title.to_lowercase();
    let expected = raw.expected_title().to_lowercase();
    UNWANTED
        .iter()
        .any(|marker| token_recall(marker, &title) == 1.0 && token_recall(marker, &expected) < 1.0)
}

/// Scores title, artist, mix identity and upload quality independently. In particular, a
/// matching title can no longer compensate for a completely absent artist.
fn match_score(raw: &RawTrack, candidate: &YTVideo) -> MatchScore {
    let title = title_similarity(raw.expected_title(), &candidate.title);
    let candidate_identity = format!("{} {}", candidate.title, candidate.channel);
    let compact_identity = compact_match_text(&candidate_identity);
    let artist = raw
        .expected_artists()
        .iter()
        .map(|expected| {
            let expected = crate::utils::strip_artist_disambiguators(expected);
            if compact_identity.contains(&compact_match_text(&expected)) {
                1.0
            } else {
                token_recall(&expected, &candidate_identity)
            }
        })
        .fold(0.0, f64::max);
    let candidate_mix = extract_mix_tag(&candidate.title);
    let expected_mix = raw.expected_mix();
    let specific_version_expected = expected_mix.is_some();
    let version = version_match(
        expected_mix.as_deref(),
        candidate_mix.as_deref(),
        &candidate.title,
    );

    let strong_version_match = expected_mix.is_some() && version == VersionMatch::Match;
    // A generic version such as "Extended Mix" says nothing about who made the upload.
    let credited_version_match = strong_version_match
        && raw.isolated_mix().is_some_and(|m| {
            // A year, as in "2024 Remix", names no one either.
            mix_credits(&m)
                .iter()
                .any(|name| name.chars().any(char::is_alphabetic))
        });
    let version_score = match version {
        VersionMatch::Match if strong_version_match => 0.25,
        VersionMatch::Match => 0.15,
        VersionMatch::Unknown => -0.10,
        VersionMatch::Contradiction => -0.40,
    };
    let topic_bonus = if candidate.channel.to_lowercase().ends_with(" - topic") {
        0.04
    } else {
        0.0
    };
    let duration_score = match candidate.duration_secs {
        Some(secs) if secs < 45 => -0.25,
        Some(_) => 0.02,
        None => 0.0,
    };
    let disqualified = has_unwanted_format(raw, candidate)
        || candidate.duration_secs.is_some_and(|secs| secs < 45);
    let format_score = if disqualified { -0.50 } else { 0.0 };

    MatchScore {
        total: 0.55 * title
            + 0.30 * artist
            + version_score
            + topic_bonus
            + duration_score
            + format_score,
        title,
        artist,
        version,
        candidate_mix,
        specific_version_expected,
        credited_version_match,
        disqualified,
    }
}

fn choose_best_match(raw: &RawTrack, candidates: Vec<YTVideo>) -> Option<YTVideo> {
    let mut scored: Vec<(MatchScore, YTVideo)> = candidates
        .into_iter()
        .map(|candidate| {
            let score = match_score(raw, &candidate);
            tracing::debug!(
                title = candidate.title,
                channel = candidate.channel,
                total = score.total,
                title_score = score.title,
                artist_score = score.artist,
                version = ?score.version,
                acceptable = score.acceptable(),
                "YouTube match candidate"
            );
            (score, candidate)
        })
        .filter(|(score, _)| score.acceptable())
        .collect();
    scored.sort_by(|(a, _), (b, _)| b.total.total_cmp(&a.total));

    let (best_score, best) = scored.first()?;
    if let Some((runner_up_score, _)) = scored.get(1) {
        // A close tie between different detected versions is evidence that the version parser
        // could not confidently disambiguate this result set. Multiple uploads of the same
        // version remain safe and are not rejected.
        if best_score.total - runner_up_score.total < 0.03
            && best_score.candidate_mix != runner_up_score.candidate_mix
        {
            return None;
        }
    }
    Some(best.clone())
}

/// Fetches a broad candidate set, then retries with the label only when the cleaner
/// artist/title query did not yield a confident match.
async fn find_best_match(client: &Client, raw: &RawTrack) -> Option<YTVideo> {
    let q = raw.yt_query();
    let candidates = yt_search_simple(client, &q, YT_CANDIDATE_LIMIT)
        .await
        .unwrap_or_default();
    if let Some(best) = choose_best_match(raw, candidates) {
        return Some(best);
    }

    let label = raw.expected_label()?;
    let fallback_q = format!("{q} {label}");
    let candidates = yt_search_simple(client, &fallback_q, YT_CANDIDATE_LIMIT)
        .await
        .unwrap_or_default();
    choose_best_match(raw, candidates)
}

/// Scores how well a raw track matches a title/artists/mix triple, higher is better. Used by
/// `fixup_track` to pick the best candidate for a track that's already in the collection,
/// as opposed to `score_match` which matches a raw track against a YouTube video.
fn score_raw_match(raw: &RawTrack, title: &str, artists: &[String], mix_name: Option<&str>) -> f64 {
    // Unlike the query sent out to search backends, scoring keeps the title's "feat. X" clause
    // (real signal once we're just comparing candidates) but still drops artist-name
    // disambiguator tags like "(US)"/"(ofc)" (Beatport-only noise, present on both sides here
    // only by scraping accident).
    let clean_artists = crate::utils::strip_artist_disambiguators(&artists.join(" "));
    let target = match mix_name.filter(|m| !is_original_mix(m)) {
        Some(mix) => format!("{clean_artists} {title} {mix}"),
        None => format!("{clean_artists} {title}"),
    };
    let expected = format!(
        "{} {}",
        raw.expected_artists().join(" "),
        raw.expected_title()
    );
    let expected = match raw.expected_mix() {
        Some(mix) => format!("{expected} {mix}"),
        None => expected,
    };
    strsim::normalized_levenshtein(
        &crate::utils::normalize_for_match(&target),
        &crate::utils::normalize_for_match(&expected),
    )
}

/// Overwrites `base`'s fields with `found`'s wherever `found` actually has a value, so a
/// confident match also cleans up existing-but-messy fields (e.g. a title/artists lifted
/// from a YouTube video title), not just missing ones. Fields `found` doesn't have (e.g.
/// `bp_id` when the match came from Traxsource/Volumo) are left as-is on `base`.
fn merge_fixup(mut base: Track, found: Track) -> Track {
    if !found.title.trim().is_empty() {
        base.title = found.title;
    }
    if !found.artists.is_empty() {
        base.artists = found.artists;
    }
    if found.mix_name.is_some() {
        base.mix_name = found.mix_name;
    }
    if found.bp_id.is_some() {
        base.bp_id = found.bp_id;
    }
    if found.release_date.is_some() {
        base.release_date = found.release_date;
    }
    if found.label.is_some() {
        base.label = found.label;
    }
    if found.genre.is_some() {
        base.genre = found.genre;
    }
    if found.bpm.is_some() {
        base.bpm = found.bpm;
    }
    if found.key.is_some() {
        base.key = found.key;
    }
    base
}

/// Searches Beatport, Traxsource, and Volumo for a match on `track`'s title/artists and uses
/// it to fill in missing fields and clean up existing ones, if a candidate scores above the
/// similarity threshold. SoundCloud is skipped since its metadata is too sparse to be useful
/// for fixups, and MusicBrainz isn't wired up to any of these backends.
pub async fn fixup_track(client: Arc<Client>, track: Track) -> anyhow::Result<Option<Track>> {
    let clean_title = crate::utils::strip_feat(&track.title);
    let clean_artists = crate::utils::strip_artist_disambiguators(&track.artists.join(" "));
    let mix_name = track.mix_name.as_deref().filter(|m| !is_original_mix(m));
    let query = match mix_name {
        Some(mix) => format!("{clean_artists} {clean_title} {mix}"),
        None => format!("{clean_artists} {clean_title}"),
    }
    .trim()
    .to_string();
    if query.is_empty() {
        return Ok(None);
    }

    let (bp_res, vl_res, tx_res) = tokio::join!(
        bp_search_tracks(&client, &query),
        vl_search_tracks(&client, &query, 10),
        tx_search_tracks(&client, &query),
    );
    let bp_tracks = bp_res.unwrap_or_else(|e| {
        tracing::warn!("Beatport fixup search failed: {e}");
        vec![]
    });
    let vl_tracks = vl_res.unwrap_or_else(|e| {
        tracing::warn!("Volumo fixup search failed: {e}");
        vec![]
    });
    let tx_tracks = tx_res.unwrap_or_else(|e| {
        tracing::warn!("Traxsource fixup search failed: {e}");
        vec![]
    });

    let candidates = bp_tracks
        .into_iter()
        .map(RawTrack::Bp)
        .chain(vl_tracks.into_iter().map(RawTrack::Vl))
        .chain(tx_tracks.into_iter().map(RawTrack::Tx));

    let best = candidates
        .map(|raw| {
            (
                score_raw_match(
                    &raw,
                    &track.title,
                    &track.artists,
                    track.mix_name.as_deref(),
                ),
                raw,
            )
        })
        .filter(|(score, _)| *score > 0.5)
        .max_by(|(a, _), (b, _)| a.total_cmp(b));

    Ok(best.map(|(_, raw)| {
        let found = raw.into_track(track.yt_id.clone());
        merge_fixup(track, found)
    }))
}

// ── Catalogs ─────────────────────────────────────────────────────────────────

/// The page size of label and artist catalogs, which is also all Beatport serves.
const CATALOG_PAGE_SIZE: usize = 100;
/// How many candidates per result a catalog may supply at most. Many candidates come to
/// nothing: about a third have no acceptable YouTube video, and more than one source can list
/// the same track. Some sources list nearly all of a big label's catalog without a video, so
/// paging on until the limit is reached could take thousands of YouTube searches.
const CATALOG_CANDIDATES_PER_RESULT: usize = 3;
/// How many pages a catalog may fetch beyond those its candidates fill: pre-orders, which
/// are left out, can fill whole pages.
const CATALOG_EXTRA_PAGES: usize = 10;
/// A catalog starts fetching its next page once it has no more candidates than this left, so
/// that the page is usually there before they run out.
const CATALOG_REFILL_THRESHOLD: usize = MATCH_CONCURRENCY;

/// A page's tracks and the next page, if the catalog may go on.
type CatalogPageResult = anyhow::Result<(Vec<RawTrack>, Option<CatalogPage>)>;

/// The request for a page of a source's catalog of a label or artist.
enum CatalogPage {
    Beatport {
        url: String,
        page: usize,
    },
    Soundcloud {
        user: SCUser,
        is_label: bool,
        cursor: Option<String>,
    },
    Volumo {
        filter: String,
        offset: usize,
    },
    Traxsource {
        url: String,
        page: usize,
    },
    /// A page given by a test.
    #[cfg(test)]
    Fixture(futures::future::BoxFuture<'static, CatalogPageResult>),
}

impl CatalogPage {
    /// The first page of `source`'s catalog of the label or artist `name`, or none if the source
    /// has no such label or artist or can't list a catalog.
    async fn first(
        client: &Client,
        source: TrackSource,
        credit: Credit,
        name: &str,
        known_bp_tracks: &KnownBpTracks,
    ) -> anyhow::Result<Option<Self>> {
        Ok(match source {
            TrackSource::Beatport => match credit {
                Credit::Label => bp_find_label(client, name, known_bp_tracks)
                    .await?
                    .map(|label| bp_label_tracks_url(&label)),
                Credit::Artist => bp_find_artist(client, name, known_bp_tracks)
                    .await?
                    .map(|artist| bp_artist_tracks_url(&artist)),
            }
            .map(|url| CatalogPage::Beatport { url, page: 1 }),
            TrackSource::Soundcloud => {
                let is_label = credit == Credit::Label;
                sc_find_person(client, name, is_label)
                    .await?
                    .map(|user| CatalogPage::Soundcloud {
                        user,
                        is_label,
                        cursor: None,
                    })
            }
            TrackSource::Volumo => vl_credit_filter(client, credit, name, None)
                .await?
                .map(|filter| CatalogPage::Volumo { filter, offset: 0 }),
            TrackSource::Traxsource => tx_credit_url(client, credit, name)
                .await?
                .map(|url| CatalogPage::Traxsource { url, page: 1 }),
            TrackSource::Spotify | TrackSource::AppleMusic | TrackSource::Shazam => None,
        })
    }

    /// The page's tracks, newest first, and the next page if the catalog may go on.
    async fn fetch(self, client: Arc<Client>) -> CatalogPageResult {
        let client = client.as_ref();
        Ok(match self {
            CatalogPage::Beatport { url, page } => {
                let (tracks, full) = bp_get_releases(client, &url, page, None).await?;
                let next = full.then(|| CatalogPage::Beatport {
                    url,
                    page: page + 1,
                });
                (raw(tracks, RawTrack::Bp), next)
            }
            CatalogPage::Soundcloud {
                user,
                is_label,
                cursor,
            } => {
                let (mut tracks, next) =
                    sc_get_user_tracks(client, user.id, CATALOG_PAGE_SIZE, cursor.as_deref())
                        .await?;
                if is_label {
                    sc_label_tracks(&mut tracks, &user);
                }
                let next = next.map(|cursor| CatalogPage::Soundcloud {
                    user,
                    is_label,
                    cursor: Some(cursor),
                });
                (raw(tracks, RawTrack::Sc), next)
            }
            CatalogPage::Volumo { filter, offset } => {
                let (rows, tracks) =
                    vl_tracks_page(client, &filter, offset, CATALOG_PAGE_SIZE).await?;
                let next = (rows >= CATALOG_PAGE_SIZE).then(|| CatalogPage::Volumo {
                    filter,
                    offset: offset + rows,
                });
                (raw(tracks, RawTrack::Vl), next)
            }
            CatalogPage::Traxsource { url, page } => {
                let (rows, tracks) = tx_credit_page(client, &url, page).await?;
                let next = (rows >= TX_PAGE_SIZE).then(|| CatalogPage::Traxsource {
                    url,
                    page: page + 1,
                });
                (raw(tracks, RawTrack::Tx), next)
            }
            #[cfg(test)]
            CatalogPage::Fixture(page) => page.await?,
        })
    }
}

/// One source's candidates for a search. A label's or artist's catalog is fetched a page at a
/// time, as the search uses up the candidates it has: tracks without a YouTube video and
/// tracks other sources found too don't become results, so a fixed number of candidates can
/// fall short of the limit. Later pages are fetched in a task of their own (see
/// [`interleave`]), which is aborted with the catalog.
struct Catalog {
    source: &'static str,
    tracks: VecDeque<RawTrack>,
    next: Option<CatalogPage>,
    refill: Option<tokio::task::JoinHandle<CatalogPageResult>>,
    /// How many more candidates pages may add.
    candidates_left: usize,
    /// How many more pages may be fetched.
    pages_left: usize,
}

impl Drop for Catalog {
    fn drop(&mut self) {
        if let Some(refill) = &self.refill {
            refill.abort();
        }
    }
}

impl Catalog {
    /// An empty catalog with a budget for `limit` results (see
    /// [`CATALOG_CANDIDATES_PER_RESULT`] and [`CATALOG_EXTRA_PAGES`]).
    fn new(source: &'static str, limit: usize) -> Self {
        let candidates = limit.saturating_mul(CATALOG_CANDIDATES_PER_RESULT).max(1);
        Catalog {
            source,
            tracks: VecDeque::new(),
            next: None,
            refill: None,
            candidates_left: candidates,
            pages_left: candidates.div_ceil(CATALOG_PAGE_SIZE) + CATALOG_EXTRA_PAGES,
        }
    }

    /// A source's search results, which aren't paged.
    fn fixed(source: TrackSource, tracks: Vec<RawTrack>) -> Self {
        let mut catalog = Catalog::new(source_name(source), tracks.len());
        catalog.add_page(tracks, None);
        catalog
    }

    /// `source`'s catalog of the label or artist `name`, with a budget for `limit` results.
    async fn open(
        client: &Arc<Client>,
        source: TrackSource,
        credit: Credit,
        name: &str,
        known_bp_tracks: &KnownBpTracks,
        limit: usize,
    ) -> anyhow::Result<Self> {
        let first = CatalogPage::first(client, source, credit, name, known_bp_tracks).await?;
        Catalog::start(client, source_name(source), first, limit).await
    }

    /// A catalog starting at `first`, which is fetched before the search begins, along with
    /// the other sources' first pages, so that a source that fails outright is reported up
    /// front.
    async fn start(
        client: &Arc<Client>,
        source: &'static str,
        first: Option<CatalogPage>,
        limit: usize,
    ) -> anyhow::Result<Self> {
        let mut catalog = Catalog::new(source, limit);
        if let Some(page) = first {
            catalog.pages_left -= 1;
            let (tracks, next) = page.fetch(Arc::clone(client)).await?;
            catalog.add_page(tracks, next);
        }
        Ok(catalog)
    }

    /// Adds a page's candidates, within the budget. Tracks released later than tomorrow are
    /// left out and don't count: they can't be on YouTube yet, and a day is left for time
    /// zones.
    fn add_page(&mut self, tracks: Vec<RawTrack>, next: Option<CatalogPage>) {
        let cutoff = chrono::Local::now().date_naive() + chrono::Duration::days(1);
        let before = self.tracks.len();
        self.tracks.extend(
            tracks
                .into_iter()
                .filter(|t| t.release_date() <= cutoff)
                .take(self.candidates_left),
        );
        self.candidates_left -= self.tracks.len() - before;
        self.next = next.filter(|_| self.candidates_left > 0 && self.pages_left > 0);
    }

    /// Starts fetching the next page if the catalog is running out of candidates.
    fn start_refill(&mut self, client: &Arc<Client>) {
        if self.refill.is_some() || self.tracks.len() > CATALOG_REFILL_THRESHOLD {
            return;
        }
        if let Some(page) = self.next.take() {
            self.pages_left -= 1;
            self.refill = Some(tokio::spawn(page.fetch(Arc::clone(client))));
        }
    }

    /// Adds the page a refill fetched. After a failed page the catalog ends with the
    /// candidates it has.
    fn finish_refill(
        &mut self,
        fetched: Result<CatalogPageResult, tokio::task::JoinError>,
    ) -> anyhow::Result<()> {
        self.refill = None;
        let (tracks, next) = fetched.context("page fetch failed")??;
        self.add_page(tracks, next);
        Ok(())
    }

    fn is_done(&self) -> bool {
        self.tracks.is_empty() && self.refill.is_none() && self.next.is_none()
    }
}

fn source_name(source: TrackSource) -> &'static str {
    match source {
        TrackSource::Beatport => "Beatport",
        TrackSource::Soundcloud => "SoundCloud",
        TrackSource::Volumo => "Volumo",
        TrackSource::Traxsource => "Traxsource",
        TrackSource::Spotify => "Spotify",
        TrackSource::AppleMusic => "Apple Music",
        TrackSource::Shazam => "Shazam",
    }
}

/// Opens the catalogs of the label or artist `name` on the selected `sources` that can list one,
/// concurrently. Sources that fail are recorded in `failures` and left out.
async fn open_catalogs(
    client: &Arc<Client>,
    credit: Credit,
    name: &str,
    sources: &[TrackSource],
    known_bp_tracks: &KnownBpTracks,
    limit: usize,
    failures: &mut Vec<String>,
) -> Vec<Catalog> {
    let opened = join_all(
        CATALOG_SOURCES
            .iter()
            .filter(|source| sources.contains(source))
            .map(|&source| async move {
                let catalog = Catalog::open(client, source, credit, name, known_bp_tracks, limit);
                (source, catalog.await)
            }),
    )
    .await;
    opened
        .into_iter()
        .filter_map(|(source, result)| {
            result
                .inspect_err(|e| {
                    let source = source_name(source);
                    tracing::warn!("{source} {} search failed: {e}", credit.noun());
                    failures.push(format!("{source}: {e}"));
                })
                .ok()
        })
        .collect()
}

async fn report_catalog_failure(tx: &mpsc::Sender<MsgServer>, source: &str, e: anyhow::Error) {
    tracing::warn!("{source} catalog page failed: {e:#}");
    report_source_failures(tx, &[format!("{source}: {e:#}")]).await;
}

/// The candidates of `catalogs`, taking one from each in turn so that every source is
/// represented whatever the limit. Sources waiting for their next page are passed over while
/// others have candidates, so a slow page holds up the search only once every source is
/// waiting. A source whose later page fails is reported and ends with the candidates it has.
fn interleave<'a>(
    client: &'a Arc<Client>,
    tx: &'a mpsc::Sender<MsgServer>,
    catalogs: Vec<Catalog>,
) -> impl Stream<Item = RawTrack> + 'a {
    stream::unfold((catalogs, 0), move |(mut catalogs, mut i)| async move {
        loop {
            // Take in finished pages, start the pages that are due and drop finished sources.
            let mut j = 0;
            while j < catalogs.len() {
                let catalog = &mut catalogs[j];
                if let Some(refill) = catalog.refill.as_mut().filter(|r| r.is_finished()) {
                    let fetched = refill.await;
                    if let Err(e) = catalog.finish_refill(fetched) {
                        report_catalog_failure(tx, catalog.source, e).await;
                    }
                }
                catalog.start_refill(client);
                if catalog.is_done() {
                    catalogs.remove(j);
                    if j < i {
                        i -= 1;
                    }
                } else {
                    j += 1;
                }
            }
            if catalogs.is_empty() {
                return None;
            }

            let n = catalogs.len();
            for step in 0..n {
                let j = (i + step) % n;
                if let Some(track) = catalogs[j].tracks.pop_front() {
                    return Some((track, (catalogs, j + 1)));
                }
            }

            // Every source is waiting for a page.
            let refills = catalogs.iter_mut().enumerate().filter_map(|(j, catalog)| {
                let refill = catalog.refill.as_mut()?;
                Some(async move { (j, refill.await) }.boxed())
            });
            let ((j, fetched), _, _) = futures::future::select_all(refills).await;
            if let Err(e) = catalogs[j].finish_refill(fetched) {
                report_catalog_failure(tx, catalogs[j].source, e).await;
            }
        }
    })
}

/// Wraps one backend's results for a [`Catalog`].
fn raw<T>(tracks: Vec<T>, wrap: fn(T) -> RawTrack) -> Vec<RawTrack> {
    tracks.into_iter().map(wrap).collect()
}

/// Unwraps one backend's results, recording a failure instead of silently returning nothing.
fn source_results<T>(
    result: anyhow::Result<Vec<T>>,
    source: &str,
    what: &str,
    failures: &mut Vec<String>,
) -> Vec<T> {
    result.unwrap_or_else(|e| {
        tracing::warn!("{source} {what} failed: {e}");
        failures.push(format!("{source}: {e}"));
        vec![]
    })
}

/// True if `candidate` names the same artist (or label) as `wanted`, ignoring case and
/// punctuation. A Beatport-style disambiguator on the candidate ("Lucho (AR)") is ignored unless
/// `wanted` carries one too, so a search for "Lucho" finds "Lucho (AR)" while "LuchQ" or "Lucho
/// Salari" never match "Luch".
fn is_same_name(candidate: &str, wanted: &str) -> bool {
    let wanted = crate::utils::normalize_for_match(wanted);
    !wanted.is_empty()
        && (crate::utils::normalize_for_match(candidate) == wanted
            || crate::utils::normalize_for_match(&crate::utils::strip_artist_disambiguators(
                candidate,
            )) == wanted)
}

/// Words that labels often append to their name, so "Defected" and "Defected Records" match.
const LABEL_SUFFIXES: &[&str] = &[
    "records",
    "recordings",
    "recordz",
    "recs",
    "music",
    "label",
    "limited",
    "ltd",
    "digital",
    "audio",
];

/// `name` without the generic words a label may end in ("Defected Records" → "Defected"), or
/// `None` if it has none or consists only of them.
fn strip_label_suffixes(name: &str) -> Option<String> {
    let bare = |w: &str| -> String {
        w.chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
            .to_lowercase()
    };
    let words: Vec<&str> = name.split_whitespace().collect();
    let mut end = words.len();
    while end > 1 && LABEL_SUFFIXES.contains(&bare(words[end - 1]).as_str()) {
        end -= 1;
    }
    (end < words.len()).then(|| words[..end].join(" "))
}

/// True if `candidate` names the same label as `wanted`, like [`is_same_name`] but ignoring
/// generic trailing words ("Records", "Music"). Sources without a label-catalog endpoint return
/// anything whose text resembles the query, such as track titles, which this filters out.
pub fn is_same_label(candidate: &str, wanted: &str) -> bool {
    fn core(name: &str) -> String {
        let cleaned: String = name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { ' ' })
            .collect();
        let mut words: Vec<&str> = cleaned.split_whitespace().collect();
        while words.len() > 1 && words.last().is_some_and(|w| LABEL_SUFFIXES.contains(w)) {
            words.pop();
        }
        words.join(" ")
    }
    let wanted_core = core(wanted);
    is_same_name(candidate, wanted) || (!wanted_core.is_empty() && core(candidate) == wanted_core)
}

/// Words that describe a mix rather than name its remixer, as in "Solomun Extended Remix".
const MIX_DESCRIPTORS: &[&str] = &[
    "extended",
    "club",
    "radio",
    "original",
    "instrumental",
    "vocal",
    "short",
    "long",
    "full",
    "12",
    "12inch",
    "album",
    "single",
    "clean",
    "explicit",
    "dirty",
    "acoustic",
    "live",
    "mixed",
];

/// The remixer names credited by a mix tag, e.g. "Luch & Nils Hoffmann Remix" ->
/// ["Luch", "Nils Hoffmann"]. Everything from the first mix keyword on is dropped. Words that
/// may describe the mix rather than name its remixer give a second, shorter reading: "Solomun
/// Extended Remix" credits "Solomun Extended" or "Solomun", "TV On The Radio Remix" "TV On The
/// Radio" or "TV On The". A tag of descriptors only, such as "Extended Mix", credits no one.
fn mix_credits(mix: &str) -> Vec<String> {
    let bare = |word: &str| -> String {
        word.chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
            .to_lowercase()
    };
    let mut words = vec![];
    for word in mix.split_whitespace() {
        if MIX_KEYWORDS.contains(&bare(word).as_str()) {
            break;
        }
        words.push(word);
    }
    let mut credited = words.len();
    while credited > 0 && MIX_DESCRIPTORS.contains(&bare(words[credited - 1]).as_str()) {
        credited -= 1;
    }
    if credited == 0 {
        return vec![];
    }
    let split = |words: &[&str]| -> Vec<String> {
        words
            .join(" ")
            .split([',', '&'])
            .flat_map(|s| s.split(" and ").flat_map(|s| s.split(" x ")))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };
    let mut names = split(&words);
    if credited < words.len() {
        names.extend(split(&words[..credited]));
    }
    names
}

/// True if a track found via plain-text search actually credits `wanted`, as an artist or as a
/// remixer. Sources without an artist-catalog endpoint return anything whose text resembles the
/// query (prefix matches, track titles), which this filters out.
pub fn credits_artist(artists: &[String], track_name: &str, mix_name: &str, wanted: &str) -> bool {
    artists.iter().any(|a| is_same_name(a, wanted))
        || [Some(mix_name.to_string()), extract_mix_tag(track_name)]
            .into_iter()
            .flatten()
            .flat_map(|mix| mix_credits(&mix))
            .any(|name| is_same_name(&name, wanted))
}

/// Tells the client which backends failed, so missing results aren't mistaken for "no matches".
async fn report_source_failures(tx: &mpsc::Sender<MsgServer>, failures: &[String]) {
    if failures.is_empty() {
        return;
    }
    let _ = tx
        .send(MsgServer::Notification {
            message: format!(
                "Some sources failed, results may be incomplete — {}",
                failures.join("; ")
            ),
            kind: "warn".into(),
        })
        .await;
}

async fn run_search<F, Fut>(tx: &mpsc::Sender<MsgServer>, query_id: i64, label: &str, f: F)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    if let Err(e) = f().await {
        tracing::error!("{label} error: {e}");
        let _ = tx
            .send(MsgServer::Notification {
                message: format!("Search failed: {e}"),
                kind: "error".into(),
            })
            .await;
    }
    let _ = tx.send(MsgServer::SearchComplete { query_id }).await;
}

/// Resolves a YouTube match for each of the catalogs' candidates (see [`interleave`]) and
/// streams it, stopping at `limit` results. The same video found via several backends is only
/// sent once.
async fn stream_raw_tracks(
    client: &Arc<Client>,
    tx: &mpsc::Sender<MsgServer>,
    query_id: i64,
    limit: usize,
    catalogs: Vec<Catalog>,
) {
    let ranked = interleave(client, tx, catalogs).map(|raw| (raw, None));
    stream_matched_tracks(client, tx, query_id, &mut HashSet::new(), limit, ranked).await;
}

async fn resolve_shazam_track(client: Arc<Client>, candidate: ShazamTrack) -> Option<Track> {
    let raw = RawTrack::Sz(candidate);
    let video = find_best_match(&client, &raw).await?;
    let base = raw.into_track(video.id);
    match fixup_track(client, base.clone()).await {
        Ok(Some(track)) => Some(track),
        Ok(None) | Err(_) => Some(base),
    }
}

/// Fetches Shazam recommendations, resolves each to a playable YouTube video, and enriches the
/// resulting metadata with the same fixup sources used by the edit dialog.
pub async fn search_shazam_similar(
    client: Arc<Client>,
    tx: mpsc::Sender<MsgServer>,
    track: Track,
    query_id: i64,
    limit: usize,
) {
    run_search(&tx, query_id, "search_shazam_similar", || async {
        let candidates = shazam_similar_tracks_for_track(&client, &track, limit).await?;
        let mut resolved = stream::iter(candidates)
            .map(|candidate| resolve_shazam_track(Arc::clone(&client), candidate))
            .buffer_unordered(4);
        let mut count = 0;

        while let Some(result) = resolved.next().await {
            let Some(track) = result else {
                continue;
            };
            if tx
                .send(MsgServer::SearchResult {
                    query_id,
                    track,
                    source: TrackSource::Shazam,
                    chart_position: None,
                })
                .await
                .is_err()
            {
                break;
            }
            count += 1;
            if count >= limit {
                break;
            }
        }
        Ok(())
    })
    .await;
}

pub async fn search_fuzzy(
    client: Arc<Client>,
    tx: mpsc::Sender<MsgServer>,
    query: String,
    query_id: i64,
    limit: usize,
    sources: Vec<TrackSource>,
) {
    run_search(&tx, query_id, "search_fuzzy", || async {
        let (bp_res, sc_res, vl_res, tx_res, sp_res, am_res) = tokio::join!(
            async {
                if sources.contains(&TrackSource::Beatport) {
                    bp_search_tracks(&client, &query).await
                } else {
                    Ok(vec![])
                }
            },
            async {
                if sources.contains(&TrackSource::Soundcloud) {
                    sc_search_tracks(&client, &query, limit).await
                } else {
                    Ok(vec![])
                }
            },
            async {
                if sources.contains(&TrackSource::Volumo) {
                    vl_search_tracks(&client, &query, limit).await
                } else {
                    Ok(vec![])
                }
            },
            async {
                if sources.contains(&TrackSource::Traxsource) {
                    tx_search_tracks(&client, &query).await
                } else {
                    Ok(vec![])
                }
            },
            async {
                if sources.contains(&TrackSource::Spotify) {
                    spotify::search_tracks(&client, &query, limit).await
                } else {
                    Ok(vec![])
                }
            },
            async {
                if sources.contains(&TrackSource::AppleMusic) {
                    apple_music::search_tracks(
                        &client,
                        &query,
                        apple_music::SearchField::Any,
                        limit,
                    )
                    .await
                } else {
                    Ok(vec![])
                }
            },
        );
        let mut failures = Vec::new();
        let bp_tracks = source_results(bp_res, "Beatport", "track search", &mut failures);
        let sc_tracks = source_results(sc_res, "SoundCloud", "track search", &mut failures);
        let vl_tracks = source_results(vl_res, "Volumo", "track search", &mut failures);
        let tx_tracks = source_results(tx_res, "Traxsource", "track search", &mut failures);
        let sp_tracks = source_results(sp_res, "Spotify", "track search", &mut failures);
        let am_tracks = source_results(am_res, "Apple Music", "track search", &mut failures);
        report_source_failures(&tx, &failures).await;
        stream_raw_tracks(
            &client,
            &tx,
            query_id,
            limit,
            vec![
                Catalog::fixed(TrackSource::Beatport, raw(bp_tracks, RawTrack::Bp)),
                Catalog::fixed(TrackSource::Soundcloud, raw(sc_tracks, RawTrack::Sc)),
                Catalog::fixed(TrackSource::Volumo, raw(vl_tracks, RawTrack::Vl)),
                Catalog::fixed(TrackSource::Traxsource, raw(tx_tracks, RawTrack::Tx)),
                Catalog::fixed(TrackSource::Spotify, raw(sp_tracks, RawTrack::Sp)),
                Catalog::fixed(TrackSource::AppleMusic, raw(am_tracks, RawTrack::Am)),
            ],
        )
        .await;
        Ok(())
    })
    .await;
}

pub async fn search_by_label(
    client: Arc<Client>,
    tx: mpsc::Sender<MsgServer>,
    label_name: String,
    query_id: i64,
    limit: usize,
    sources: Vec<TrackSource>,
    known_bp_tracks: KnownBpTracks,
) {
    run_search(&tx, query_id, "search_by_label", || async {
        if !check_catalog_sources(&tx, &sources, "search by label").await {
            return Ok(());
        }
        let mut failures = Vec::new();
        // Spotify and Apple Music aren't asked: their searches don't find tracks by label and
        // their results don't name one, so a text search would find only unrelated tracks.
        let catalogs = open_catalogs(
            &client,
            Credit::Label,
            &label_name,
            &sources,
            &known_bp_tracks,
            limit,
            &mut failures,
        )
        .await;
        report_source_failures(&tx, &failures).await;
        stream_raw_tracks(&client, &tx, query_id, limit, catalogs).await;
        Ok(())
    })
    .await;
}

pub async fn search_by_artist(
    client: Arc<Client>,
    tx: mpsc::Sender<MsgServer>,
    artist_name: String,
    query_id: i64,
    limit: usize,
    sources: Vec<TrackSource>,
    known_bp_tracks: KnownBpTracks,
) {
    run_search(&tx, query_id, "search_by_artist", || async {
        let mut failures = Vec::new();
        let (catalogs, sp_res, am_res) = tokio::join!(
            open_catalogs(
                &client,
                Credit::Artist,
                &artist_name,
                &sources,
                &known_bp_tracks,
                limit,
                &mut failures,
            ),
            async {
                // Spotify's search has no artist-catalog endpoint, so its text search results
                // are checked for the artist below.
                if sources.contains(&TrackSource::Spotify) {
                    spotify::search_tracks(&client, &artist_name, limit).await
                } else {
                    Ok(vec![])
                }
            },
            async {
                // Apple Music can search artist names only, though by text, so its results are
                // still checked for the artist below.
                if sources.contains(&TrackSource::AppleMusic) {
                    apple_music::search_tracks(
                        &client,
                        &artist_name,
                        apple_music::SearchField::Artist,
                        limit,
                    )
                    .await
                } else {
                    Ok(vec![])
                }
            },
        );
        let mut sp_tracks = source_results(sp_res, "Spotify", "artist search", &mut failures);
        let mut am_tracks = source_results(am_res, "Apple Music", "artist search", &mut failures);
        report_source_failures(&tx, &failures).await;
        sp_tracks.retain(|t| credits_artist(&t.artists, &t.track_name, "", &artist_name));
        // Apple joins its artists into one credit, whose separators also occur within names
        // ("Earth, Wind & Fire"), so it is matched as written too.
        am_tracks.retain_mut(|t| {
            t.regroup_artist(|name| is_same_name(name, &artist_name)) || {
                let mix_name = t.mix_name.as_deref().unwrap_or_default();
                credits_artist(&t.artists, &t.track_name, mix_name, &artist_name)
            }
        });
        let mut catalogs = catalogs;
        catalogs.push(Catalog::fixed(
            TrackSource::Spotify,
            raw(sp_tracks, RawTrack::Sp),
        ));
        catalogs.push(Catalog::fixed(
            TrackSource::AppleMusic,
            raw(am_tracks, RawTrack::Am),
        ));
        stream_raw_tracks(&client, &tx, query_id, limit, catalogs).await;
        Ok(())
    })
    .await;
}

/// The sources that can list an artist's or label's catalog: label searches and latest
/// releases use only these. Spotify's results name no label, and Apple Music's search doesn't
/// index labels and allows only about 20 searches a minute, too few for latest releases, which
/// searches many names at once.
pub const CATALOG_SOURCES: &[TrackSource] = &[
    TrackSource::Beatport,
    TrackSource::Soundcloud,
    TrackSource::Volumo,
    TrackSource::Traxsource,
];

/// Tells the client when none of the selected `sources` supports `what`, which would
/// otherwise just complete without results. True if some source does.
async fn check_catalog_sources(
    tx: &mpsc::Sender<MsgServer>,
    sources: &[TrackSource],
    what: &str,
) -> bool {
    if sources.iter().any(|s| CATALOG_SOURCES.contains(s)) {
        return true;
    }
    let _ = tx
        .send(MsgServer::Notification {
            message: format!(
                "Spotify and Apple Music can't {what}. Choose Beatport, SoundCloud, Volumo or \
                 Traxsource in Search options."
            ),
            kind: "warn".into(),
        })
        .await;
    false
}

/// A source that failed for one artist or label of a latest-releases search.
struct NewReleaseFailure {
    source: &'static str,
    name: String,
    error: String,
}

/// Fetches releases by a single artist or label name across the selected catalog sources,
/// dropping anything released before `cutoff`. Not every source honors a date range, so all
/// are filtered here. Failed sources are returned along with the tracks.
async fn gather_new_release_raw_tracks(
    client: &Client,
    name: &str,
    is_label: bool,
    cutoff: NaiveDate,
    sources: &[TrackSource],
    known_bp_tracks: &KnownBpTracks,
) -> (Vec<RawTrack>, Vec<NewReleaseFailure>) {
    let per_source_limit = 100;
    let (bp_res, sc_res, vl_res, tx_res) = tokio::join!(
        async {
            if !sources.contains(&TrackSource::Beatport) {
                return Ok(vec![]);
            }
            if is_label {
                match bp_find_label(client, name, known_bp_tracks).await? {
                    Some(label) => bp_get_label_releases(client, &label, Some(cutoff)).await,
                    None => Ok(vec![]),
                }
            } else {
                match bp_find_artist(client, name, known_bp_tracks).await? {
                    Some(artist) => bp_get_artist_releases(client, &artist, Some(cutoff)).await,
                    None => Ok(vec![]),
                }
            }
        },
        async {
            if sources.contains(&TrackSource::Soundcloud) {
                sc_tracks_by_person(client, name, 50, is_label).await
            } else {
                Ok(vec![])
            }
        },
        async {
            if !sources.contains(&TrackSource::Volumo) {
                Ok(vec![])
            } else if is_label {
                vl_get_label_tracks(client, name, per_source_limit, Some(cutoff)).await
            } else {
                vl_get_artist_tracks(client, name, per_source_limit, Some(cutoff)).await
            }
        },
        async {
            if !sources.contains(&TrackSource::Traxsource) {
                Ok(vec![])
            } else if is_label {
                tx_get_label_tracks(client, name, per_source_limit, Some(cutoff)).await
            } else {
                tx_get_artist_tracks(client, name, per_source_limit, Some(cutoff)).await
            }
        },
    );

    // Not-yet-released tracks (pre-order pages) sneak past a lower-bound-only filter, and
    // Beatport's own `publish_date` query param isn't reliably honored server-side, so every
    // source gets the same client-side [cutoff, today] range check regardless of backend.
    let today = chrono::Local::now().date_naive();
    let in_range = |d: NaiveDate| d >= cutoff && d <= today;
    let mut failures = vec![];
    let mut tracks = vec![];
    let mut collect = |source: &'static str, res: anyhow::Result<Vec<RawTrack>>| match res {
        Ok(found) => tracks.extend(found.into_iter().filter(|t| in_range(t.release_date()))),
        Err(e) => {
            tracing::warn!("{source} new-releases search for {name:?} failed: {e:#}");
            failures.push(NewReleaseFailure {
                source,
                name: name.to_string(),
                error: format!("{e:#}"),
            });
        }
    };
    collect("Beatport", bp_res.map(|v| raw(v, RawTrack::Bp)));
    collect("SoundCloud", sc_res.map(|v| raw(v, RawTrack::Sc)));
    collect("Volumo", vl_res.map(|v| raw(v, RawTrack::Vl)));
    collect("Traxsource", tx_res.map(|v| raw(v, RawTrack::Tx)));
    (tracks, failures)
}

/// One warning for a latest-releases search's failures, grouped by source, so an incomplete
/// search can be told apart from one without matches.
fn summarize_new_release_failures(failures: &[NewReleaseFailure], names: usize) -> Vec<String> {
    let mut by_source: Vec<(&str, Vec<&NewReleaseFailure>)> = vec![];
    for failure in failures {
        match by_source.iter_mut().find(|(s, _)| *s == failure.source) {
            Some((_, group)) => group.push(failure),
            None => by_source.push((failure.source, vec![failure])),
        }
    }
    by_source
        .into_iter()
        .map(|(source, group)| {
            let first = group[0];
            format!(
                "{source} failed for {} of {names} names (e.g. {}: {})",
                group.len(),
                first.name,
                first.error
            )
        })
        .collect()
}

/// Streams raw tracks' YouTube matches, skipping anything whose resolved YouTube id is already in
/// `seen` — for new releases it is seeded with the ids already in the collection, so this both
/// excludes existing tracks and de-duplicates results found via more than one source/artist/label.
/// YouTube-matches are resolved `MATCH_CONCURRENCY`-wide via `buffered`, which preserves
/// `raw_tracks`' order while overlapping the network round-trips — the dominant cost here, since
/// every candidate needs its own YouTube search. The dedup/send loop itself stays sequential
/// since it's cheap and mutates `seen`. Each raw track may carry a chart position, which is passed
/// on to the client as-is.
async fn stream_matched_tracks(
    client: &Client,
    tx: &mpsc::Sender<MsgServer>,
    query_id: i64,
    seen: &mut HashSet<String>,
    limit: usize,
    raw_tracks: impl Stream<Item = (RawTrack, Option<usize>)>,
) {
    let matches = raw_tracks
        .map(|(raw, chart_position)| async move {
            let vid = find_best_match(client, &raw).await;
            (raw, chart_position, vid)
        })
        .buffered(MATCH_CONCURRENCY);
    let mut matches = std::pin::pin!(matches);

    let mut count = 0;
    while let Some((raw, chart_position, vid)) = matches.next().await {
        let Some(vid) = vid else {
            continue;
        };
        if !seen.insert(vid.id.clone()) {
            continue;
        }
        let source = raw.source();
        let track = raw.into_track(vid.id);
        if tx
            .send(MsgServer::SearchResult {
                query_id,
                track,
                source,
                chart_position,
            })
            .await
            .is_err()
        {
            break;
        }
        count += 1;
        if count >= limit {
            break;
        }
    }
}

// Beatport's bot detection is the reason `create_client` impersonates Chrome's TLS
// fingerprint at all (see CLAUDE.md); keep the artist/label fan-out modest so we don't turn
// that into a losing battle. YouTube isn't as sensitive, so its match-resolution step below
// gets a wider budget.
const NEW_RELEASES_CONCURRENCY: usize = 8;
const MATCH_CONCURRENCY: usize = 12;
/// The most results a search returns, whatever the client asks for, since every result costs
/// a YouTube search. It's the client's largest option.
pub const SEARCH_MAX_LIMIT: usize = 1000;

/// Parameters for [`search_new_releases`], grouped to keep the function signature manageable.
pub struct NewReleasesQuery {
    pub query_id: i64,
    pub days: i64,
    pub limit: usize,
    pub artists: Vec<String>,
    pub labels: Vec<String>,
    pub existing_yt_ids: HashSet<String>,
    pub sources: Vec<TrackSource>,
    pub known_bp_tracks: KnownBpTracks,
}

/// Searches for recent releases by the collection's top artists and labels, streaming matches
/// via the same `SearchResult`/`SearchComplete` messages as a regular search. Tracks already in
/// the collection are excluded, and duplicates found under more than one artist/label are
/// collapsed into a single result.
pub async fn search_new_releases(
    client: Arc<Client>,
    tx: mpsc::Sender<MsgServer>,
    q: NewReleasesQuery,
) {
    let NewReleasesQuery {
        query_id,
        days,
        limit,
        artists,
        labels,
        existing_yt_ids,
        sources,
        known_bp_tracks,
    } = q;
    let limit = limit.min(SEARCH_MAX_LIMIT);
    run_search(&tx, query_id, "search_new_releases", || async {
        let cutoff = chrono::Local::now().date_naive() - chrono::Duration::days(days);
        let names: Vec<(String, bool)> = artists
            .into_iter()
            .map(|a| (a, false))
            .chain(labels.into_iter().map(|l| (l, true)))
            .collect();

        if !check_catalog_sources(&tx, &sources, "search for latest releases").await {
            return Ok(());
        }
        let name_count = names.len();
        let per_name: Vec<(Vec<RawTrack>, Vec<NewReleaseFailure>)> = stream::iter(names)
            .map(|(name, is_label)| {
                let client = client.clone();
                let sources = sources.clone();
                let known_bp_tracks = known_bp_tracks.clone();
                async move {
                    gather_new_release_raw_tracks(
                        &client,
                        &name,
                        is_label,
                        cutoff,
                        &sources,
                        &known_bp_tracks,
                    )
                    .await
                }
            })
            .buffer_unordered(NEW_RELEASES_CONCURRENCY)
            .collect()
            .await;

        let (per_name, failures): (Vec<_>, Vec<_>) = per_name.into_iter().unzip();
        let failures: Vec<NewReleaseFailure> = failures.into_iter().flatten().collect();
        report_source_failures(&tx, &summarize_new_release_failures(&failures, name_count)).await;
        let mut raw_tracks: Vec<RawTrack> = per_name.into_iter().flatten().collect();
        raw_tracks.sort_by_key(|t| std::cmp::Reverse(t.release_date()));

        let ranked = stream::iter(raw_tracks.into_iter().map(|raw| (raw, None)));
        let mut seen = existing_yt_ids;
        stream_matched_tracks(&client, &tx, query_id, &mut seen, limit, ranked).await;
        Ok(())
    })
    .await;
}

/// Streams the YouTube matches of a genre's Beatport top-100 chart, in chart order and tagged
/// with their chart position. Unlike a
/// new-releases search, tracks already in the collection are kept — it's the chart as a whole
/// that's of interest, and the results grid marks the ones already collected.
pub async fn search_beatport_top100(
    client: Arc<Client>,
    tx: mpsc::Sender<MsgServer>,
    genre_id: i64,
    query_id: i64,
    limit: usize,
) {
    run_search(&tx, query_id, "search_beatport_top100", || async {
        let chart = bp_get_genre_top100(&client, genre_id).await?;
        let ranked = stream::iter(
            chart
                .into_iter()
                .map(|(position, track)| (RawTrack::Bp(track), Some(position))),
        );
        stream_matched_tracks(&client, &tx, query_id, &mut HashSet::new(), limit, ranked).await;
        Ok(())
    })
    .await;
}

/// Streams the YouTube matches of Traxsource's (cross-genre) top-100 chart, in chart order and
/// tagged with their chart position. Like the Beatport chart, collected tracks are kept.
pub async fn search_traxsource_top100(
    client: Arc<Client>,
    tx: mpsc::Sender<MsgServer>,
    query_id: i64,
    limit: usize,
) {
    run_search(&tx, query_id, "search_traxsource_top100", || async {
        let chart = tx_get_top100(&client).await?;
        let ranked = stream::iter(
            chart
                .into_iter()
                .map(|(position, track)| (RawTrack::Tx(track), Some(position))),
        );
        stream_matched_tracks(&client, &tx, query_id, &mut HashSet::new(), limit, ranked).await;
        Ok(())
    })
    .await;
}

#[cfg(test)]
mod top100_tests {
    use super::*;

    #[test]
    fn test_bp_url_encodes_slugs() {
        assert_eq!(
            bp_url(&["artist", "rbør", "1099363", "tracks"]),
            "https://www.beatport.com/artist/rb%C3%B8r/1099363/tracks"
        );
        assert_eq!(
            bp_url(&["label", "a/b", "1", "tracks"]),
            "https://www.beatport.com/label/a%2Fb/1/tracks"
        );
    }

    #[test]
    fn test_parse_bp_top100() {
        let json = serde_json::json!({"props": {"pageProps": {"dehydratedState": {"queries": [
            {"queryKey": ["genre-6"], "state": {"data": {"id": 6, "name": "Techno"}}},
            {
                "queryKey": ["genre-6-top-100-tracks-per_page=100&hype=false"],
                "state": {"data": {"results": [
                    {
                        "id": 2, "name": "We Come 1", "mix_name": "Adam Beyer Remix",
                        "new_release_date": "2026-09-25", "bpm": 134,
                        "artists": [{"name": "Faithless"}],
                        "key": {"name": "Bb Major"},
                        "genre": {"name": "Techno (Peak Time / Driving)"},
                        "release": {"label": {"name": "Drumcode"}},
                        "isrc": "GBUR62001398"
                    },
                    {"id": 3, "name": "Missing required fields"},
                    {
                        "id": 1, "name": "Second", "mix_name": "Original Mix",
                        "new_release_date": "2026-10-01", "artists": [],
                        "release": {"label": {"name": "Label"}}
                    }
                ]}}
            }
        ]}}}});
        let chart = parse_bp_top100(&json).unwrap();
        assert_eq!(
            chart
                .iter()
                .map(|(pos, t)| (*pos, t.track_id))
                .collect::<Vec<_>>(),
            [(1, 2), (3, 1)],
            "chart order and positions are kept; an unparseable entry leaves a gap"
        );
        let tracks: Vec<BPTrack> = chart.into_iter().map(|(_, t)| t).collect();
        assert_eq!(tracks[0].track_name, "We Come 1");
        assert_eq!(tracks[0].mix_name, "Adam Beyer Remix");
        assert_eq!(tracks[0].artists, ["Faithless"]);
        assert_eq!(tracks[0].label, "Drumcode");
        assert_eq!(tracks[0].key.as_deref(), Some("Bb Major"));
        assert_eq!(tracks[0].genre, ["Techno (Peak Time / Driving)"]);
    }

    #[test]
    fn test_parse_tx_top100() {
        let row = |pos: &str, id: &str, title: &str, date: &str| {
            format!(
                r#"<div data-trid="{id}" class="trk-row play-trk">
                  <div class="trk-cell tnum-pos"><div class="tnum">{pos}</div></div>
                  <div class="trk-cell title"><img src="x.svg"><a href="/track/{id}/x">{title}</a>
                    <span class="version">Extended Remix <span class="duration">(5:51)</span></span>
                  </div>
                  <div class="trk-cell artists">
                    <a href="/artist/1/a" class="com-artists">Lovebirds</a>,
                    <a href="/artist/2/b" class="com-artists">Stee Downes</a>
                    <a href="/artist/3/c" class="com-remixers">Schmoov!</a>
                  </div>
                  <div class="trk-cell label"><a href="/label/847/w">Winding Road</a></div>
                  <div class="trk-cell genre"><a href="/genre/3/s">Soul / Funk / Disco</a></div>
                  <div class="trk-cell r-date">{date}</div>
                </div>"#
            )
        };
        let html = format!(
            r#"<div class="trklist">
              <div class="trk-row hdr"><div class="trk-cell tnum-pos">POS.</div></div>
              {}{}{}
            </div>"#,
            row("1", "15133019", "Want You In My Soul", "2026-11-13"),
            row("2", "15122054", "Missing date", ""),
            row("3", "15004120", "Vibin'", "2026-10-30"),
        );
        let chart = parse_tx_top100(&html);
        assert_eq!(
            chart
                .iter()
                .map(|(pos, t)| (*pos, t.track_id))
                .collect::<Vec<_>>(),
            [(1, 15133019), (3, 15004120)],
            "chart order and positions are kept; an unparseable row leaves a gap"
        );
        let t = &chart[0].1;
        assert_eq!(t.track_name, "Want You In My Soul");
        assert_eq!(t.mix_name, "Extended Remix");
        assert_eq!(t.artists, ["Lovebirds", "Stee Downes"]);
        assert_eq!(t.label, "Winding Road");
        assert_eq!(t.genre.as_deref(), Some("Soul / Funk / Disco"));
    }

    #[test]
    fn test_parse_bp_top100_without_chart() {
        let json = serde_json::json!({"props": {"pageProps": {"dehydratedState": {"queries": [
            {"queryKey": ["genre-6"], "state": {"data": {}}}
        ]}}}});
        assert!(parse_bp_top100(&json).is_none());
    }
}

#[cfg(test)]
mod remix_match_tests {
    use super::*;
    use crate::models::{BPTrack, SCTrack};

    fn bp_track(track_name: &str, mix_name: &str) -> BPTrack {
        BPTrack {
            artists: vec!["Aquarius Heaven".to_string()],
            bpm: None,
            key: None,
            isrc: None,
            label: "Some Label".to_string(),
            release_date: chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
            track_id: 1,
            track_name: track_name.to_string(),
            mix_name: mix_name.to_string(),
            genre: vec![],
        }
    }

    fn video(title: &str) -> YTVideo {
        video_from(title, "Channel")
    }

    fn video_from(title: &str, channel: &str) -> YTVideo {
        YTVideo {
            title: title.to_string(),
            id: "id".to_string(),
            channel: channel.to_string(),
            duration_secs: Some(4 * 60),
        }
    }

    #[test]
    fn extracts_remix_tag_from_parens() {
        assert_eq!(
            extract_mix_tag("Aquarius Heaven - Just Like Magic (Someone Remix)"),
            Some("someone remix".to_string())
        );
        assert_eq!(
            extract_mix_tag("Aquarius Heaven - Just Like Magic (Extended Mix)"),
            Some("extended mix".to_string())
        );
        assert_eq!(
            extract_mix_tag("Aquarius Heaven - Just Like Magic [Official Video]"),
            None
        );
        assert_eq!(extract_mix_tag("Aquarius Heaven - Just Like Magic"), None);
        assert_eq!(
            extract_mix_tag("Aquarius Heaven - Just Like Magic - Extended Mix"),
            Some("extended mix".to_string())
        );
    }

    #[test]
    fn original_mix_prefers_video_without_remix_tag() {
        let raw = RawTrack::Bp(bp_track("Just Like Magic", "Original Mix"));
        let original = video("Aquarius Heaven - Just Like Magic (Original Mix)");
        let remix = video("Aquarius Heaven - Just Like Magic (Someone Remix)");
        assert!(match_score(&raw, &original).total > match_score(&raw, &remix).total);
    }

    #[test]
    fn remix_prefers_video_with_matching_remix_tag() {
        let raw = RawTrack::Bp(bp_track("Just Like Magic", "Someone Remix"));
        let original = video("Aquarius Heaven - Just Like Magic (Original Mix)");
        let matching_remix = video("Aquarius Heaven - Just Like Magic (Someone Remix)");
        let other_remix = video("Aquarius Heaven - Just Like Magic (Other Artist Remix)");
        assert!(match_score(&raw, &matching_remix).total > match_score(&raw, &original).total);
        assert!(match_score(&raw, &matching_remix).total > match_score(&raw, &other_remix).total);
    }

    #[test]
    fn apple_music_remix_matches_its_video_with_a_featured_credit_after_the_version() {
        // Apple's song 499472452.
        let song = crate::apple_music::parse_track(&serde_json::json!({
            "kind": "song",
            "trackId": 499472452_i64,
            "artistName": "Above & Beyond",
            "artistViewUrl": "https://music.apple.com/us/artist/above-beyond/20318188?uo=4",
            "trackName": "You Got To Go (Seven Lions Remix) [feat. Zoë Johnston]",
            "releaseDate": "2012-02-06T12:00:00Z",
        }))
        .unwrap();
        let raw = RawTrack::Am(song);
        let score = match_score(
            &raw,
            &video("Above & Beyond - You Got To Go (Seven Lions Remix)"),
        );
        assert_eq!(score.version, VersionMatch::Match);
        assert!(score.acceptable(), "{score:?}");
    }

    #[test]
    fn parses_track_page_credits() {
        let track = serde_json::json!({
            "artists": [
                { "id": 6223, "name": "Riton", "type": "Artist", "slug": "riton" },
                { "id": 168391, "name": "Kid Enigma", "slug": "kid-enigma" },
                { "name": "no id" },
            ],
            "label": { "id": 59027, "name": "Ministry of Sound Recordings" },
        });
        let (artists, label) = parse_bp_track_credits(&track);
        let artists: Vec<_> = artists
            .iter()
            .map(|a| (a.id, a.display_name.as_str(), a.name.as_str()))
            .collect();
        assert_eq!(
            artists,
            [
                (6223, "Riton", "riton"),
                (168391, "Kid Enigma", "kid-enigma")
            ]
        );
        let label = label.unwrap();
        assert_eq!(label.id, 59027);
        assert_eq!(label.name, "ministry-of-sound-recordings");
    }

    #[test]
    fn known_bp_tracks_match_names_like_searches() {
        let known = KnownBpTracks::new(vec![
            (1, vec!["LuchQ".into()], Some("Defected".into())),
            (
                2,
                vec!["Bedouin".into(), "Luch".into()],
                Some("Crosstown Rebels".into()),
            ),
            (3, vec!["luch".into()], None),
        ]);
        assert_eq!(known.by_artist("Luch"), [2, 3]);
        assert_eq!(known.on_label("crosstown rebels"), [2]);
        assert!(known.on_label("Defected Records").is_empty());
    }

    #[test]
    fn same_artist_requires_the_whole_name() {
        assert!(is_same_name("Luch", "luch"));
        assert!(is_same_name("LUCH", "Luch"));
        assert!(!is_same_name("LuchQ", "Luch"));
        assert!(!is_same_name("LUCH!Q", "Luch"));
        assert!(!is_same_name("Lucho Salari", "Luch"));
        assert!(!is_same_name("Lucho (AR)", "Luch"));
    }

    #[test]
    fn same_artist_ignores_disambiguator_unless_searched_for() {
        assert!(is_same_name("Lucho (AR)", "Lucho"));
        assert!(is_same_name("Lucho (AR)", "Lucho (AR)"));
        assert!(!is_same_name("Lucho (VE)", "Lucho (AR)"));
    }

    fn pick_label(wanted: &str, doc: &str) -> Option<String> {
        let candidates = tx_credit_candidates(&Html::parse_document(doc), Credit::Label);
        pick_credit(Credit::Label, wanted, candidates)
    }

    #[test]
    fn tx_label_candidates_skip_other_links() {
        let doc = r#"<a href="/label/34118/traxsource-weekend-weapons" class="flt">Weekend Weapons</a>
            <div class="trk-row"><div class="trk-cell label"><a href="/label/57/ith">ITH</a></div></div>
            <div class="label-grid"><a href="/label/51/defected?x=1" class="title">Defected</a></div>"#;
        assert_eq!(pick_label("ITH", doc).as_deref(), Some("/label/57/ith"));
        assert_eq!(
            pick_label("Defected", doc).as_deref(),
            Some("/label/51/defected")
        );
        assert_eq!(pick_label("Weekend Weapons", doc), None);
    }

    #[test]
    fn exact_name_beats_an_alias_listed_first() {
        // Volumo ranks "Solid Grooves Records" above the unrelated "Solid Grooves".
        let candidates = [
            ("Solid Grooves Records".to_string(), 319168),
            ("Solid Grooves".to_string(), 906659),
        ];
        assert_eq!(
            pick_credit(Credit::Label, "Solid Grooves", candidates.clone()),
            Some(906659)
        );
        assert_eq!(
            pick_credit(Credit::Label, "Solid Grooves Records", candidates),
            Some(319168)
        );
        // Without an exact name, the source's top-ranked alias.
        let aliases = [
            ("Defected Records".to_string(), 827022),
            ("Defected Records Limited".to_string(), 318532),
        ];
        assert_eq!(
            pick_credit(Credit::Label, "Defected", aliases),
            Some(827022)
        );
        // Artists match by name only, preferring one without a disambiguator, which Volumo
        // lists after "Lucho (VE)" and "Lucho (AR)".
        let artists = [
            ("Lucho Salari".to_string(), 1),
            ("Lucho (VE)".to_string(), 2),
            ("Lucho (AR)".to_string(), 3),
            ("Lucho".to_string(), 4),
        ];
        assert_eq!(
            pick_credit(Credit::Artist, "Lucho", artists.clone()),
            Some(4)
        );
        assert_eq!(
            pick_credit(Credit::Artist, "Lucho (AR)", artists.clone()),
            Some(3)
        );
        assert_eq!(
            pick_credit(Credit::Artist, "Lucho", artists[..3].to_vec()),
            Some(2)
        );
    }

    #[tokio::test]
    async fn a_label_search_without_catalog_sources_says_so() {
        let (tx, mut rx) = mpsc::channel(8);
        let client = Arc::new(create_client().unwrap());
        let sources = vec![TrackSource::Spotify, TrackSource::AppleMusic];
        search_by_label(
            client,
            tx,
            "Defected".into(),
            1,
            10,
            sources,
            Default::default(),
        )
        .await;
        let Some(MsgServer::Notification { message, kind }) = rx.recv().await else {
            panic!("expected a notification");
        };
        assert_eq!(kind, "warn");
        assert!(message.starts_with("Spotify and Apple Music can't search by label"));
        assert!(matches!(
            rx.recv().await,
            Some(MsgServer::SearchComplete { query_id: 1 })
        ));
    }

    #[test]
    fn new_release_failures_are_grouped_by_source() {
        let failure = |source, name: &str| NewReleaseFailure {
            source,
            name: name.to_string(),
            error: "HTTP 500".to_string(),
        };
        let failures = [
            failure("Volumo", "Defected"),
            failure("Traxsource", "Solomun"),
            failure("Volumo", "Solomun"),
        ];
        assert_eq!(
            summarize_new_release_failures(&failures, 40),
            [
                "Volumo failed for 2 of 40 names (e.g. Defected: HTTP 500)",
                "Traxsource failed for 1 of 40 names (e.g. Solomun: HTTP 500)",
            ]
        );
    }

    #[test]
    fn label_searches_retry_without_the_suffix() {
        assert_eq!(
            Credit::Label.queries("Defected Records"),
            ["Defected Records", "Defected"]
        );
        assert_eq!(Credit::Label.queries("RADIANT."), ["RADIANT."]);
        assert_eq!(Credit::Label.queries("Records"), ["Records"]);
        assert_eq!(Credit::Artist.queries("Solid Records"), ["Solid Records"]);
        // Traxsource's search for "Defected" lists the label by its short name.
        let short_name_results = r#"<div class="search-grid label-grid">
            <a href="/label/51/defected" class="title">Defected</a></div>"#;
        assert_eq!(
            pick_label("Defected Records", short_name_results).as_deref(),
            Some("/label/51/defected")
        );
    }

    #[test]
    fn tx_pages_count_pre_orders_but_skip_them() {
        let row = |id: u32, date: &str| {
            format!(
                r#"<div data-trid="{id}" class="trk-row play-trk">
                <div class="trk-cell title"><a href="/track/{id}/x">Track {id}</a></div>
                <div class="trk-cell artists"><a class="com-artists" href="/artist/1/a">A</a></div>
                <div class="trk-cell label"><a href="/label/51/defected">Defected</a></div>
                <div class="trk-cell r-date">{date}</div></div>"#
            )
        };
        let html = format!(
            r#"<div class="trk-row hdr"></div>{}{}"#,
            row(
                1,
                r#"<span class="pre-ord-txt">Pre-order for 2099-01-01</span>"#
            ),
            row(2, "2024-05-01"),
        );
        let (rows, tracks) = parse_tx_page(&Html::parse_document(&html));
        assert_eq!(rows, 2);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].track_id, 2);
    }

    #[test]
    fn is_same_label_ignores_generic_suffixes() {
        assert!(is_same_label("RADIANT.", "RADIANT."));
        assert!(is_same_label("Radiant", "RADIANT."));
        assert!(is_same_label("Defected Records", "Defected"));
        assert!(is_same_label("Defected", "Defected Records"));
        assert!(!is_same_label("Tactician Sound", "RADIANT."));
        assert!(!is_same_label("Elliptical Sun Recordings", "Radiant"));
        assert!(!is_same_label("Radiant Sun Recordings", "Radiant"));
        assert!(!is_same_label("Records", ""));
    }

    #[test]
    fn credits_artist_as_artist_or_remixer() {
        let artists = |names: &[&str]| names.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(credits_artist(
            &artists(&["Bedouin", "Luch"]),
            "The Only One",
            "",
            "Luch"
        ));
        assert!(credits_artist(
            &artists(&["Nils Hoffmann"]),
            "Everlight (Luch Remix)",
            "",
            "Luch"
        ));
        assert!(credits_artist(
            &artists(&["Nils Hoffmann"]),
            "Everlight",
            "Luch & Someone Else Extended Remix",
            "Luch"
        ));
        assert!(!credits_artist(
            &artists(&["Gol'man"]),
            "Luch",
            "Original Mix",
            "Luch"
        ));
        assert!(!credits_artist(&artists(&["LuchQ"]), "Energy", "", "Luch"));
        assert!(credits_artist(
            &artists(&["Joe Smooth"]),
            "Promised Land",
            "Solomun Extended Remix",
            "Solomun"
        ));
        assert!(credits_artist(
            &artists(&["Gang Gang Dance"]),
            "First Communion (TV On The Radio Remix)",
            "",
            "TV On The Radio"
        ));
        assert!(credits_artist(
            &artists(&["Gang Gang Dance"]),
            "First Communion",
            "TV On The Radio Remix",
            "TV On The Radio"
        ));
        assert!(!credits_artist(
            &artists(&["Joe Smooth"]),
            "Promised Land",
            "Extended Mix",
            "Extended"
        ));
        assert!(!credits_artist(
            &artists(&["St.Ego", "Lucho (AR)"]),
            "Choice (Lucho AR Remix)",
            "",
            "Luch"
        ));
    }

    #[test]
    fn rejects_matching_title_when_artist_is_absent() {
        let raw = RawTrack::Bp(bp_track("Just Like Magic", "Original Mix"));
        let wrong = video_from("Wrong Artist - Just Like Magic", "Wrong Artist");
        assert!(choose_best_match(&raw, vec![wrong]).is_none());
    }

    #[test]
    fn rejects_generic_version_by_another_artist() {
        // Naarly's "BORDERLINE (Extended Mix)" and "good time.. (Extended Mix)" matched these.
        let raw = RawTrack::Bp(bp_track("BORDERLINE", "Extended Mix"));
        let wrong = video_from("Borderline (Extended Mix)", "Brooke Lee - Topic");
        assert!(choose_best_match(&raw, vec![wrong]).is_none());
        let raw = RawTrack::Bp(bp_track("good time..", "Extended Mix"));
        let wrong = video_from("Good Time (Extended Mix)", "Release - Topic");
        assert!(choose_best_match(&raw, vec![wrong]).is_none());
    }

    #[test]
    fn rejects_generic_version_labels_by_another_artist() {
        for mix in [
            "Album Version",
            "Single Version",
            "Clean Version",
            "Mixed",
            "2024 Remix",
        ] {
            let raw = RawTrack::Bp(bp_track("Borderline", mix));
            let wrong = video_from(&format!("Borderline ({mix})"), "Brooke Lee - Topic");
            assert!(choose_best_match(&raw, vec![wrong]).is_none(), "{mix}");
        }
    }

    fn sc_track(track_name: &str) -> RawTrack {
        RawTrack::Sc(SCTrack {
            artists: vec!["Naarly".to_string()],
            label: None,
            release_date: chrono::NaiveDate::from_ymd_opt(2026, 11, 20).unwrap(),
            track_name: track_name.to_string(),
            genre: vec![],
        })
    }

    #[test]
    fn unbracketed_mix_in_a_title_does_not_credit_the_title() {
        for title in ["Good Time Extended Mix", "Good Time (Extended Mix)"] {
            let raw = sc_track(title);
            let wrong = video_from(title, "Release - Topic");
            assert!(choose_best_match(&raw, vec![wrong]).is_none(), "{title}");
            let right = video_from(title, "Naarly - Topic");
            assert!(choose_best_match(&raw, vec![right]).is_some(), "{title}");
        }
    }

    fn sp_track(track_name: &str) -> RawTrack {
        RawTrack::Sp(crate::spotify::SpTrack {
            artists: vec!["Aquarius Heaven".to_string()],
            track_name: track_name.to_string(),
            album: None,
            release_date: chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
            uri: "spotify:track:id".to_string(),
        })
    }

    #[test]
    fn spotify_version_suffix_credits_its_remixer() {
        let raw = sp_track("Just Like Magic - Someone Remix");
        let label_upload = video_from("Just Like Magic (Someone Remix)", "Some Label");
        assert!(choose_best_match(&raw, vec![label_upload]).is_some());
        let raw = sp_track("Just Like Magic - Extended Mix");
        let wrong = video_from("Just Like Magic (Extended Mix)", "Brooke Lee - Topic");
        assert!(choose_best_match(&raw, vec![wrong]).is_none());
    }

    #[test]
    fn accepts_remix_credit_without_the_artist() {
        let raw = RawTrack::Bp(bp_track("Just Like Magic", "Someone Remix"));
        let label_upload = video_from("Just Like Magic (Someone Remix)", "Some Label");
        assert!(choose_best_match(&raw, vec![label_upload]).is_some());
    }

    #[test]
    fn accepts_artist_from_topic_channel() {
        let raw = RawTrack::Bp(bp_track("Just Like Magic", "Original Mix"));
        let topic_upload = video_from("Just Like Magic", "Aquarius Heaven - Topic");
        assert!(choose_best_match(&raw, vec![topic_upload]).is_some());
    }

    #[test]
    fn rejects_wrong_version_even_when_title_and_artist_match() {
        let raw = RawTrack::Bp(bp_track("Just Like Magic", "Someone Remix"));
        let wrong_version = video("Aquarius Heaven - Just Like Magic (Other Artist Remix)");
        assert!(choose_best_match(&raw, vec![wrong_version]).is_none());
    }

    #[test]
    fn rejects_covers_and_short_clips() {
        let raw = RawTrack::Bp(bp_track("Just Like Magic", "Original Mix"));
        let cover = video("Aquarius Heaven - Just Like Magic (Piano Cover)");
        let mut clip = video("Aquarius Heaven - Just Like Magic");
        clip.duration_secs = Some(30);
        assert!(choose_best_match(&raw, vec![cover]).is_none());
        assert!(choose_best_match(&raw, vec![clip]).is_none());
    }

    /// Regression test for the "Just Like Magic" case that motivated this change: Beatport
    /// lists the original plus multiple remixes, and picking YouTube's #1 hit for each
    /// query used to return the original video every time regardless of which mix was
    /// being searched for.
    #[tokio::test]
    #[ignore = "hits real YouTube API"]
    async fn real_youtube_disambiguates_remixes() {
        let client = create_client().unwrap();

        let original = RawTrack::Bp(bp_track("Just Like Magic", "Original Mix"));
        let original_match = find_best_match(&client, &original).await.unwrap();
        assert!(
            extract_mix_tag(&original_match.title).is_none(),
            "expected the plain original, got {:?}",
            original_match.title
        );

        let luciano = RawTrack::Bp(bp_track("Just Like Magic", "Luciano Remix"));
        let luciano_match = find_best_match(&client, &luciano).await.unwrap();
        assert!(
            luciano_match.title.to_lowercase().contains("luciano"),
            "expected the Luciano remix, got {:?}",
            luciano_match.title
        );

        let roman = RawTrack::Bp(bp_track("Just Like Magic", "Roman Flügel Extended Remix"));
        let roman_match = find_best_match(&client, &roman).await.unwrap();
        assert!(
            roman_match.title.to_lowercase().contains("flügel"),
            "expected a Roman Flügel remix, got {:?}",
            roman_match.title
        );
    }

    #[test]
    fn yt_query_includes_mix_name_for_remixes_but_not_originals() {
        let original = RawTrack::Bp(bp_track("Just Like Magic", "Original Mix"));
        assert!(!original.yt_query().contains("Original Mix"));

        let remix = RawTrack::Bp(bp_track("Just Like Magic", "Someone Remix"));
        assert!(remix.yt_query().contains("Someone Remix"));
    }

    #[test]
    fn parses_youtube_video_urls() {
        for url in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://music.youtube.com/watch?list=PLabc&v=dQw4w9WgXcQ&si=x",
            "https://m.youtube.com/watch?v=dQw4w9WgXcQ&t=42",
            "https://youtu.be/dQw4w9WgXcQ?si=ignored",
            "https://www.youtube.com/shorts/dQw4w9WgXcQ",
            "https://www.youtube.com/embed/dQw4w9WgXcQ",
            " https://youtube.com/live/dQw4w9WgXcQ ",
        ] {
            assert_eq!(
                parse_youtube_video_id(url),
                Some("dQw4w9WgXcQ".to_string()),
                "{url}"
            );
        }
        for input in [
            "dQw4w9WgXcQ",
            "artist title",
            "https://example.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com/watch?v=short",
            "https://youtube.com/playlist?list=PLabc123",
            "https://youtube.com/@channel",
        ] {
            assert_eq!(parse_youtube_video_id(input), None, "{input}");
        }
    }

    #[test]
    fn parses_youtube_playlist_urls() {
        assert_eq!(
            parse_youtube_playlist_id(
                "https://music.youtube.com/playlist?list=PLabc_123&si=ignored"
            ),
            Some("PLabc_123".to_string())
        );
        assert_eq!(
            parse_youtube_playlist_id("https://youtube.com/playlist?si=ignored&list=PLabc-123"),
            Some("PLabc-123".to_string())
        );
        assert_eq!(
            parse_youtube_playlist_id("https://example.com/playlist?list=PLabc123"),
            None
        );
        assert_eq!(
            parse_youtube_playlist_id("https://youtube.com/watch?v=abc123"),
            None
        );
    }

    #[test]
    fn parses_embedded_youtube_playlist_tracks() {
        let data = serde_json::json!({
            "metadata": { "playlistMetadataRenderer": { "title": "Indie" } },
            "contents": [{
                "lockupViewModel": {
                    "contentImage": {
                        "thumbnailViewModel": {
                            "image": { "sources": [{ "url": "https://i.ytimg.com/vi/video123/hq.jpg" }] }
                        }
                    },
                    "metadata": {
                        "lockupMetadataViewModel": {
                            "title": { "content": "A song" },
                            "metadata": {
                                "contentMetadataViewModel": {
                                    "metadataRows": [{ "metadataParts": [{ "text": { "content": "An Artist - Topic" } }] }]
                                }
                            }
                        }
                    }
                }
            }]
        });
        let mut tracks = Vec::new();
        collect_playlist_tracks(&data, &mut tracks);
        let tracks = unique_tracks(tracks);

        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].yt_id, "video123");
        assert_eq!(tracks[0].title, "A song");
        assert_eq!(tracks[0].artists, vec!["An Artist"]);
    }
}

#[cfg(test)]
mod shazam_tests {
    use super::*;

    #[test]
    fn parses_similar_song_cards_and_skips_duplicates() {
        let html = r#"
            <section data-test-id="track_impression_similarTracks">
              <div data-test-id="songItem">
                <a data-test-id="track_userevent_similar-songs_songTitle"
                   href="/song/101/first-song"></a>
                <a data-test-id="track_userevent_similar-songs_songTitle"
                   href="/song/101/first-song">First &amp; Song</a>
                <a data-test-id="track_userevent_similar-songs_artistName"
                   aria-label="An Artist &amp; Friends">An Artist &amp; Friends</a>
              </div>
              <div data-test-id="songItem">
                <a data-test-id="track_userevent_similar-songs_songTitle"
                   href="https://www.shazam.com/song/202/second-song">Second Song</a>
                <a data-test-id="track_userevent_similar-songs_artistName">Another Artist</a>
              </div>
              <div data-test-id="songItem">
                <a data-test-id="track_userevent_similar-songs_songTitle"
                   href="/song/101/first-song">Duplicate</a>
                <a data-test-id="track_userevent_similar-songs_artistName">Ignored Artist</a>
              </div>
              <div data-test-id="songItem">
                <a data-test-id="track_userevent_top-tracks_songTitle"
                   href="/song/303/not-similar">Not Similar</a>
              </div>
            </section>
        "#;

        let tracks = parse_shazam_similar_tracks(html, 10);

        assert_eq!(
            tracks,
            vec![
                ShazamTrack {
                    id: 101,
                    title: "First & Song".to_string(),
                    artists: vec!["An Artist & Friends".to_string()],
                    url: "https://www.shazam.com/song/101/first-song".to_string(),
                },
                ShazamTrack {
                    id: 202,
                    title: "Second Song".to_string(),
                    artists: vec!["Another Artist".to_string()],
                    url: "https://www.shazam.com/song/202/second-song".to_string(),
                },
            ]
        );
    }

    #[test]
    fn parses_limit_zero_without_html_work() {
        assert!(parse_shazam_similar_tracks("<div>", 0).is_empty());
    }
}

#[cfg(test)]
mod catalog_tests {
    use std::{
        ops::Range,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    fn track(id: i64, release_date: NaiveDate) -> RawTrack {
        RawTrack::Tx(TXTrack {
            artists: vec!["Artist".into()],
            label: "Label".into(),
            release_date,
            track_id: id,
            track_name: format!("Track {id}"),
            mix_name: String::new(),
            bpm: None,
            key: None,
            genre: None,
        })
    }

    fn released(ids: Range<i64>) -> Vec<RawTrack> {
        let date = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
        ids.map(|id| track(id, date)).collect()
    }

    fn pre_orders(ids: Range<i64>) -> Vec<RawTrack> {
        let date = chrono::Local::now().date_naive() + chrono::Duration::days(30);
        ids.map(|id| track(id, date)).collect()
    }

    fn page(tracks: Vec<RawTrack>, next: Option<CatalogPage>) -> CatalogPage {
        CatalogPage::Fixture(async move { Ok((tracks, next)) }.boxed())
    }

    /// Pages of `size` tracks without end, counting the pages fetched in `fetched`.
    fn endless(
        n: i64,
        size: i64,
        tracks: fn(Range<i64>) -> Vec<RawTrack>,
        fetched: Arc<AtomicUsize>,
    ) -> CatalogPage {
        CatalogPage::Fixture(
            async move {
                fetched.fetch_add(1, Ordering::SeqCst);
                let next = endless(n + 1, size, tracks, Arc::clone(&fetched));
                Ok((tracks(n * size..(n + 1) * size), Some(next)))
            }
            .boxed(),
        )
    }

    fn client() -> Arc<Client> {
        Arc::new(create_client().unwrap())
    }

    async fn start(client: &Arc<Client>, first: CatalogPage, limit: usize) -> Catalog {
        Catalog::start(client, "Traxsource", Some(first), limit)
            .await
            .unwrap()
    }

    fn id(track: RawTrack) -> i64 {
        match track {
            RawTrack::Tx(t) => t.track_id,
            _ => unreachable!(),
        }
    }

    async fn collect(client: &Arc<Client>, catalogs: Vec<Catalog>) -> Vec<i64> {
        let (tx, _rx) = mpsc::channel(10);
        interleave(client, &tx, catalogs).map(id).collect().await
    }

    #[tokio::test]
    async fn pages_past_pre_orders() {
        let client = client();
        let second = page(released(100..130), None);
        let catalog = start(&client, page(pre_orders(0..100), Some(second)), 10).await;
        assert_eq!(
            collect(&client, vec![catalog]).await,
            (100..130).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn stops_at_candidate_budget() {
        let client = client();
        let fetched = Arc::new(AtomicUsize::new(0));
        let catalog = start(&client, endless(0, 20, released, fetched), 10).await;
        assert_eq!(
            collect(&client, vec![catalog]).await,
            (0..30).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn stops_at_page_cap() {
        let client = client();
        let fetched = Arc::new(AtomicUsize::new(0));
        let catalog = start(
            &client,
            endless(0, 100, pre_orders, Arc::clone(&fetched)),
            10,
        )
        .await;
        assert!(collect(&client, vec![catalog]).await.is_empty());
        assert_eq!(fetched.load(Ordering::SeqCst), 1 + CATALOG_EXTRA_PAGES);
    }

    #[tokio::test]
    async fn passes_over_a_source_waiting_for_a_page() {
        let client = client();
        let stalled = CatalogPage::Fixture(futures::future::pending().boxed());
        let waiting = start(&client, page(vec![], Some(stalled)), 10).await;
        let ready = Catalog::fixed(TrackSource::Traxsource, released(0..5));
        let (tx, _rx) = mpsc::channel(10);
        let tracks = interleave(&client, &tx, vec![waiting, ready])
            .map(id)
            .take(5);
        let ids: Vec<i64> =
            tokio::time::timeout(std::time::Duration::from_secs(5), tracks.collect())
                .await
                .expect("waited for the stalled page");
        assert_eq!(ids, (0..5).collect::<Vec<_>>());
    }

    #[tokio::test]
    async fn keeps_candidates_after_a_failed_page() {
        let client = client();
        let failing = CatalogPage::Fixture(async { Err(anyhow::anyhow!("HTTP 500")) }.boxed());
        let catalog = start(&client, page(released(0..5), Some(failing)), 10).await;
        let (tx, mut rx) = mpsc::channel(10);
        let ids: Vec<i64> = interleave(&client, &tx, vec![catalog])
            .map(id)
            .collect()
            .await;
        assert_eq!(ids, (0..5).collect::<Vec<_>>());
        assert!(matches!(
            rx.try_recv(),
            Ok(MsgServer::Notification { message, .. }) if message.contains("HTTP 500")
        ));
    }
}
