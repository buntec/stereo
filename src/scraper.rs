use std::sync::{Arc, LazyLock};

use anyhow::Context;
use chrono::NaiveDate;
use futures::future::join_all;
use scraper::{Html, Selector};
use tokio::sync::{mpsc, RwLock};
use wreq::Client;
use wreq_util::Emulation;

use crate::{
    message::MsgServer,
    models::{BPArtist, BPLabel, BPTrack, SCTrack, SCUser, YTVideo},
};

pub fn create_client() -> anyhow::Result<Client> {
    Ok(Client::builder()
        .emulation(Emulation::Chrome124)
        .redirect(wreq::redirect::Policy::limited(10))
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
            'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
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

fn get_next_data(html: &str) -> Option<serde_json::Value> {
    let doc = Html::parse_document(html);
    let sel = Selector::parse("script#__NEXT_DATA__").ok()?;
    let el = doc.select(&sel).next()?;
    let text = el.text().collect::<String>();
    serde_json::from_str(&text).ok()
}

pub async fn bp_search_tracks(client: &Client, q: &str) -> anyhow::Result<Vec<BPTrack>> {
    tracing::info!("Beatport track search: {q}");
    let resp = client
        .get("https://www.beatport.com/search/tracks")
        .query(&[("q", q), ("per_page", "1000"), ("page", "1")])
        .send()
        .await?;
    let html = resp.text().await?;

    let json = match get_next_data(&html) {
        Some(j) => j,
        None => return Ok(vec![]),
    };

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
    let resp = client
        .get("https://www.beatport.com/search/labels")
        .query(&[("q", name)])
        .send()
        .await?;
    let html = resp.text().await?;
    let json = match get_next_data(&html) {
        Some(j) => j,
        None => return Ok(vec![]),
    };

    let mut labels = vec![];
    if let Some(queries) = json["props"]["pageProps"]["dehydratedState"]["queries"].as_array() {
        for query in queries {
            if let Some(items) = query["state"]["data"]["data"].as_array() {
                for item in items {
                    let display_name =
                        item["label_name"].as_str().unwrap_or("").trim().to_string();
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
    let resp = client
        .get("https://www.beatport.com/search/artists")
        .query(&[("q", name)])
        .send()
        .await?;
    let html = resp.text().await?;
    let json = match get_next_data(&html) {
        Some(j) => j,
        None => return Ok(vec![]),
    };

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
                    tracing::debug!("Artist result: display={display_name:?} slug={slug:?} id={id}");
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

async fn bp_get_releases(
    client: &Client,
    url: &str,
    params: &[(&str, String)],
) -> anyhow::Result<Vec<BPTrack>> {
    let resp = client.get(url).query(params).send().await?;
    let html = resp.text().await?;
    let json = match get_next_data(&html) {
        Some(j) => j,
        None => return Ok(vec![]),
    };

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
                for item in results {
                    if let Some(t) = parse_bp_track_from_releases(item) {
                        tracks.push(t);
                    }
                }
            }
        }
    }
    Ok(tracks)
}

pub async fn bp_get_label_releases(
    client: &Client,
    label: &BPLabel,
    start_date: Option<NaiveDate>,
) -> anyhow::Result<Vec<BPTrack>> {
    let mut params: Vec<(&str, String)> = vec![("page", "1".into()), ("per_page", "1000".into())];
    if let Some(start) = start_date {
        let end = chrono::Local::now().date_naive();
        params.push(("publish_date", format!("{}:{}", start, end)));
    }
    let url = format!(
        "https://www.beatport.com/label/{}/{}/tracks",
        label.name, label.id
    );
    bp_get_releases(client, &url, &params).await
}

pub async fn bp_get_artist_releases(
    client: &Client,
    artist: &BPArtist,
    start_date: Option<NaiveDate>,
) -> anyhow::Result<Vec<BPTrack>> {
    let mut params: Vec<(&str, String)> = vec![("page", "1".into()), ("per_page", "1000".into())];
    if let Some(start) = start_date {
        let end = chrono::Local::now().date_naive();
        params.push(("publish_date", format!("{}:{}", start, end)));
    }
    let url = format!(
        "https://www.beatport.com/artist/{}/{}/tracks",
        artist.name, artist.id
    );
    tracing::info!("Fetching artist releases: {url}");
    bp_get_releases(client, &url, &params).await
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
    let html = client
        .get("https://soundcloud.com/")
        .send()
        .await?
        .text()
        .await?;
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
        let mut pairs: Vec<(String, String)> =
            params.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
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
        return Ok(resp.json::<serde_json::Value>().await?);
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

pub async fn sc_get_user_tracks(
    client: &Client,
    user_id: i64,
    limit: usize,
) -> anyhow::Result<Vec<SCTrack>> {
    let json = sc_get(
        client,
        &format!("/users/{user_id}/tracks"),
        &[("limit", limit.to_string())],
    )
    .await?;
    Ok(json["collection"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(parse_sc_track)
        .collect())
}

/// Finds the SoundCloud account matching `name` (an artist or label both being SoundCloud
/// "people") and returns their tracks. When `set_label` is true, every returned track's
/// `label` is overwritten with the account's username, since this is used for label lookups
/// where the account *is* the label regardless of what a track's own `label_name` says.
async fn sc_tracks_by_person(
    client: &Client,
    name: &str,
    limit: usize,
    set_label: bool,
) -> anyhow::Result<Vec<SCTrack>> {
    let Some(user) = sc_search_users(client, name, 1).await?.into_iter().next() else {
        return Ok(vec![]);
    };
    let mut tracks = sc_get_user_tracks(client, user.id, limit).await?;
    if set_label {
        for track in &mut tracks {
            track.label = Some(user.username.clone());
        }
    }
    Ok(tracks)
}

// ── YouTube ───────────────────────────────────────────────────────────────────

const YT_CLIENTS: &[(&str, &str)] = &[
    ("WEB", "2.20240201.00.00"),
    ("MWEB", "2.20240201.00.00"),
    ("ANDROID", "19.09.37"),
];

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

pub async fn yt_search_simple(
    client: &Client,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<YTVideo>> {
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

        let data: serde_json::Value = match resp {
            Ok(r) => match r.json().await {
                Ok(d) => d,
                Err(_) => continue,
            },
            Err(_) => continue,
        };

        let mut results = vec![];
        let sections = &data["contents"]["twoColumnSearchResultsRenderer"]["primaryContents"]
            ["sectionListRenderer"]["contents"];
        if let Some(sections) = sections.as_array() {
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
                            results.push(YTVideo { title, id, channel });
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
    }
    Ok(vec![])
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
}

impl RawTrack {
    fn yt_query(&self) -> String {
        match self {
            RawTrack::Bp(t) => format!("{} {} {}", t.track_name, t.artists.join(", "), t.label),
            RawTrack::Sc(t) => format!(
                "{} {} {}",
                t.track_name,
                t.artists.join(", "),
                t.label.as_deref().unwrap_or("")
            ),
        }
    }

    fn into_track(self, yt_id: String) -> crate::models::Track {
        match self {
            RawTrack::Bp(t) => t.into_track(yt_id),
            RawTrack::Sc(t) => t.into_track(yt_id),
        }
    }
}

/// Round-robins two result lists together so neither source hogs the front of the queue.
fn interleave(a: Vec<BPTrack>, b: Vec<SCTrack>) -> Vec<RawTrack> {
    let cutoff = chrono::Local::now().date_naive() + chrono::Duration::days(1);
    let mut a = a.into_iter().filter(|t| t.release_date <= cutoff);
    let mut b = b.into_iter().filter(|t| t.release_date <= cutoff);
    let mut out = Vec::new();
    loop {
        let (x, y) = (a.next(), b.next());
        if x.is_none() && y.is_none() {
            break;
        }
        out.extend(x.map(RawTrack::Bp));
        out.extend(y.map(RawTrack::Sc));
    }
    out
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

/// Resolves a YouTube match for each raw track and streams it, stopping at `limit` results.
async fn stream_raw_tracks(
    client: &Client,
    tx: &mpsc::Sender<MsgServer>,
    query_id: i64,
    limit: usize,
    raw_tracks: Vec<RawTrack>,
) {
    let mut count = 0;
    for raw in raw_tracks {
        let q = raw.yt_query();
        let vids = yt_search_simple(client, &q, 1).await.unwrap_or_default();
        if let Some(vid) = vids.into_iter().next() {
            let track = raw.into_track(vid.id);
            if tx
                .send(MsgServer::SearchResult { query_id, track })
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
}

pub async fn search_fuzzy(
    client: Arc<Client>,
    tx: mpsc::Sender<MsgServer>,
    query: String,
    query_id: i64,
    limit: usize,
) {
    run_search(&tx, query_id, "search_fuzzy", || async {
        let (bp_res, sc_res) = tokio::join!(
            bp_search_tracks(&client, &query),
            sc_search_tracks(&client, &query, limit),
        );
        let bp_tracks = bp_res.unwrap_or_else(|e| {
            tracing::warn!("Beatport track search failed: {e}");
            vec![]
        });
        let sc_tracks = sc_res.unwrap_or_else(|e| {
            tracing::warn!("SoundCloud track search failed: {e}");
            vec![]
        });
        stream_raw_tracks(&client, &tx, query_id, limit, interleave(bp_tracks, sc_tracks)).await;
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
) {
    run_search(&tx, query_id, "search_by_label", || async {
        let (bp_res, sc_res) = tokio::join!(
            async {
                let labels = bp_search_labels(&client, &label_name, 1).await?;
                match labels.into_iter().next() {
                    Some(label) => bp_get_label_releases(&client, &label, None).await,
                    None => Ok(vec![]),
                }
            },
            sc_tracks_by_person(&client, &label_name, limit, true),
        );
        let bp_tracks = bp_res.unwrap_or_else(|e| {
            tracing::warn!("Beatport label search failed: {e}");
            vec![]
        });
        let sc_tracks = sc_res.unwrap_or_else(|e| {
            tracing::warn!("SoundCloud label search failed: {e}");
            vec![]
        });
        stream_raw_tracks(&client, &tx, query_id, limit, interleave(bp_tracks, sc_tracks)).await;
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
) {
    run_search(&tx, query_id, "search_by_artist", || async {
        let (bp_res, sc_res) = tokio::join!(
            async {
                let artists = bp_search_artist(&client, &artist_name, 1).await?;
                match artists.into_iter().next() {
                    Some(artist) => bp_get_artist_releases(&client, &artist, None).await,
                    None => Ok(vec![]),
                }
            },
            sc_tracks_by_person(&client, &artist_name, limit, false),
        );
        let bp_tracks = bp_res.unwrap_or_else(|e| {
            tracing::warn!("Beatport artist search failed: {e}");
            vec![]
        });
        let sc_tracks = sc_res.unwrap_or_else(|e| {
            tracing::warn!("SoundCloud artist search failed: {e}");
            vec![]
        });
        stream_raw_tracks(&client, &tx, query_id, limit, interleave(bp_tracks, sc_tracks)).await;
        Ok(())
    })
    .await;
}
