use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, LazyLock},
};

use anyhow::Context;
use chrono::NaiveDate;
use futures::{future::join_all, stream, StreamExt};
use scraper::{Html, Selector};
use tokio::sync::{mpsc, RwLock};
use wreq::Client;
use wreq_util::Emulation;

use crate::{
    message::MsgServer,
    models::{
        BPArtist, BPLabel, BPTrack, SCTrack, SCUser, TXTrack, Track, TrackSource, VLTrack, YTVideo,
    },
    spotify::{self, SpTrack},
    utils::is_original_mix,
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

// ── Volumo ───────────────────────────────────────────────────────────────────

// Volumo's genre list is small (~30 entries) and effectively static — cache it for the
// process lifetime rather than refetching on every search.
static VL_GENRES: LazyLock<RwLock<Option<HashMap<i64, String>>>> =
    LazyLock::new(|| RwLock::new(None));

async fn fetch_vl_genres(client: &Client) -> anyhow::Result<HashMap<i64, String>> {
    let json: serde_json::Value = client
        .get("https://volumo.com/api/v1/genres")
        .header("Accept", "application/json")
        .send()
        .await?
        .json()
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
    let json: serde_json::Value = resp?.json().await?;
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

pub async fn tx_search_tracks(client: &Client, q: &str) -> anyhow::Result<Vec<TXTrack>> {
    tracing::info!("Traxsource track search: {q}");
    let resp = client
        .get("https://www.traxsource.com/search")
        .query(&[("term", q)])
        .send()
        .await?;
    let html = resp.text().await?;
    let doc = Html::parse_document(&html);
    let row_sel = Selector::parse("div.trk-row").unwrap();
    let tracks: Vec<TXTrack> = doc
        .select(&row_sel)
        .filter_map(|row| parse_tx_track(&row))
        .collect();
    tracing::info!("Traxsource returned {} tracks", tracks.len());
    Ok(tracks)
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
    Vl(VLTrack),
    Tx(TXTrack),
    Sp(SpTrack),
}

/// Words that mark a bracketed segment of a title as a mix/edit identifier, as opposed to
/// e.g. a label tag or "Official Video" annotation.
const MIX_KEYWORDS: &[&str] = &[
    "remix", "mix", "edit", "bootleg", "flip", "rework", "vip", "dub", "version", "rmx",
];

/// Extracts a mix/remix tag from a title, e.g. "Artist - Track (Someone Remix)" -> `Some("someone remix")`.
/// Bracketed groups are preferred; separator-delimited suffixes such as "- Extended Mix" are
/// also recognized.
fn extract_mix_tag(title: &str) -> Option<String> {
    let lower = title.to_lowercase();
    let mut result = None;
    for (open, close) in [('(', ')'), ('[', ']')] {
        let mut rest = lower.as_str();
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
        }
    }

    fn expected_artists(&self) -> &[String] {
        match self {
            RawTrack::Bp(t) => &t.artists,
            RawTrack::Sc(t) => &t.artists,
            RawTrack::Vl(t) => &t.artists,
            RawTrack::Tx(t) => &t.artists,
            RawTrack::Sp(t) => &t.artists,
        }
    }

    /// The remix/mix identity this track is expected to match on YouTube, if any.
    /// Beatport, Volumo, and Traxsource expose it as a separate field; SoundCloud embeds it
    /// in the track name.
    fn expected_mix(&self) -> Option<String> {
        match self {
            RawTrack::Bp(t) => (!is_original_mix(&t.mix_name)).then(|| t.mix_name.clone()),
            RawTrack::Sc(t) => extract_mix_tag(&t.track_name),
            RawTrack::Vl(t) => (!is_original_mix(&t.mix_name)).then(|| t.mix_name.clone()),
            RawTrack::Tx(t) => (!is_original_mix(&t.mix_name)).then(|| t.mix_name.clone()),
            RawTrack::Sp(t) => extract_mix_tag(&t.track_name),
        }
    }

    fn source(&self) -> TrackSource {
        match self {
            RawTrack::Bp(_) => TrackSource::Beatport,
            RawTrack::Sc(_) => TrackSource::Soundcloud,
            RawTrack::Vl(_) => TrackSource::Volumo,
            RawTrack::Tx(_) => TrackSource::Traxsource,
            RawTrack::Sp(_) => TrackSource::Spotify,
        }
    }

    fn release_date(&self) -> NaiveDate {
        match self {
            RawTrack::Bp(t) => t.release_date,
            RawTrack::Sc(t) => t.release_date,
            RawTrack::Vl(t) => t.release_date,
            RawTrack::Tx(t) => t.release_date,
            RawTrack::Sp(t) => t.release_date,
        }
    }

    fn into_track(self, yt_id: String) -> crate::models::Track {
        match self {
            RawTrack::Bp(t) => t.into_track(yt_id),
            RawTrack::Sc(t) => t.into_track(yt_id),
            RawTrack::Vl(t) => t.into_track(yt_id),
            RawTrack::Tx(t) => t.into_track(yt_id),
            RawTrack::Sp(t) => t.into_track(yt_id),
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
    strong_version_match: bool,
    disqualified: bool,
}

impl MatchScore {
    fn acceptable(&self) -> bool {
        !self.disqualified
            && self.version != VersionMatch::Contradiction
            && (!self.specific_version_expected || self.version == VersionMatch::Match)
            && self.title >= MIN_TITLE_SCORE
            && (self.artist >= MIN_ARTIST_SCORE
                || (self.strong_version_match && self.title >= 0.80))
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
        strong_version_match,
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

/// Round-robins result lists from all backends together so no single source hogs the front
/// of the queue.
fn interleave(
    a: Vec<BPTrack>,
    b: Vec<SCTrack>,
    c: Vec<VLTrack>,
    d: Vec<TXTrack>,
    e: Vec<SpTrack>,
) -> Vec<RawTrack> {
    let cutoff = chrono::Local::now().date_naive() + chrono::Duration::days(1);
    let mut a = a.into_iter().filter(|t| t.release_date <= cutoff);
    let mut b = b.into_iter().filter(|t| t.release_date <= cutoff);
    let mut c = c.into_iter().filter(|t| t.release_date <= cutoff);
    let mut d = d.into_iter().filter(|t| t.release_date <= cutoff);
    let mut e = e.into_iter().filter(|t| t.release_date <= cutoff);
    let mut out = Vec::new();
    loop {
        let (v, w, x, y, z) = (a.next(), b.next(), c.next(), d.next(), e.next());
        if v.is_none() && w.is_none() && x.is_none() && y.is_none() && z.is_none() {
            break;
        }
        out.extend(v.map(RawTrack::Bp));
        out.extend(w.map(RawTrack::Sc));
        out.extend(x.map(RawTrack::Vl));
        out.extend(y.map(RawTrack::Tx));
        out.extend(z.map(RawTrack::Sp));
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
        let best = find_best_match(client, &raw).await;
        if let Some(vid) = best {
            let source = raw.source();
            let track = raw.into_track(vid.id);
            if tx
                .send(MsgServer::SearchResult {
                    query_id,
                    track,
                    source,
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
        let (bp_res, sc_res, vl_res, tx_res, sp_res) = tokio::join!(
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
        );
        let bp_tracks = bp_res.unwrap_or_else(|e| {
            tracing::warn!("Beatport track search failed: {e}");
            vec![]
        });
        let sc_tracks = sc_res.unwrap_or_else(|e| {
            tracing::warn!("SoundCloud track search failed: {e}");
            vec![]
        });
        let vl_tracks = vl_res.unwrap_or_else(|e| {
            tracing::warn!("Volumo track search failed: {e}");
            vec![]
        });
        let tx_tracks = tx_res.unwrap_or_else(|e| {
            tracing::warn!("Traxsource track search failed: {e}");
            vec![]
        });
        let sp_tracks = sp_res.unwrap_or_else(|e| {
            tracing::warn!("Spotify track search failed: {e}");
            vec![]
        });
        stream_raw_tracks(
            &client,
            &tx,
            query_id,
            limit,
            interleave(bp_tracks, sc_tracks, vl_tracks, tx_tracks, sp_tracks),
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
) {
    run_search(&tx, query_id, "search_by_label", || async {
        let (bp_res, sc_res, vl_res, tx_res, sp_res) = tokio::join!(
            async {
                if !sources.contains(&TrackSource::Beatport) {
                    return Ok(vec![]);
                }
                let labels = bp_search_labels(&client, &label_name, 1).await?;
                match labels.into_iter().next() {
                    Some(label) => bp_get_label_releases(&client, &label, None).await,
                    None => Ok(vec![]),
                }
            },
            async {
                if sources.contains(&TrackSource::Soundcloud) {
                    sc_tracks_by_person(&client, &label_name, limit, true).await
                } else {
                    Ok(vec![])
                }
            },
            async {
                // Volumo has no dedicated label-catalog endpoint; approximate with a text search.
                if sources.contains(&TrackSource::Volumo) {
                    vl_search_tracks(&client, &label_name, limit).await
                } else {
                    Ok(vec![])
                }
            },
            async {
                // Same for Traxsource.
                if sources.contains(&TrackSource::Traxsource) {
                    tx_search_tracks(&client, &label_name).await
                } else {
                    Ok(vec![])
                }
            },
            async {
                // Spotify's search API also has no dedicated label-catalog endpoint.
                if sources.contains(&TrackSource::Spotify) {
                    spotify::search_tracks(&client, &label_name, limit).await
                } else {
                    Ok(vec![])
                }
            },
        );
        let bp_tracks = bp_res.unwrap_or_else(|e| {
            tracing::warn!("Beatport label search failed: {e}");
            vec![]
        });
        let sc_tracks = sc_res.unwrap_or_else(|e| {
            tracing::warn!("SoundCloud label search failed: {e}");
            vec![]
        });
        let vl_tracks = vl_res.unwrap_or_else(|e| {
            tracing::warn!("Volumo label search failed: {e}");
            vec![]
        });
        let tx_tracks = tx_res.unwrap_or_else(|e| {
            tracing::warn!("Traxsource label search failed: {e}");
            vec![]
        });
        let sp_tracks = sp_res.unwrap_or_else(|e| {
            tracing::warn!("Spotify label search failed: {e}");
            vec![]
        });
        stream_raw_tracks(
            &client,
            &tx,
            query_id,
            limit,
            interleave(bp_tracks, sc_tracks, vl_tracks, tx_tracks, sp_tracks),
        )
        .await;
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
) {
    run_search(&tx, query_id, "search_by_artist", || async {
        let (bp_res, sc_res, vl_res, tx_res, sp_res) = tokio::join!(
            async {
                if !sources.contains(&TrackSource::Beatport) {
                    return Ok(vec![]);
                }
                let artists = bp_search_artist(&client, &artist_name, 1).await?;
                match artists.into_iter().next() {
                    Some(artist) => bp_get_artist_releases(&client, &artist, None).await,
                    None => Ok(vec![]),
                }
            },
            async {
                if sources.contains(&TrackSource::Soundcloud) {
                    sc_tracks_by_person(&client, &artist_name, limit, false).await
                } else {
                    Ok(vec![])
                }
            },
            async {
                // Volumo has no dedicated artist-catalog endpoint; approximate with a text search.
                if sources.contains(&TrackSource::Volumo) {
                    vl_search_tracks(&client, &artist_name, limit).await
                } else {
                    Ok(vec![])
                }
            },
            async {
                // Same for Traxsource.
                if sources.contains(&TrackSource::Traxsource) {
                    tx_search_tracks(&client, &artist_name).await
                } else {
                    Ok(vec![])
                }
            },
            async {
                // Same for Spotify.
                if sources.contains(&TrackSource::Spotify) {
                    spotify::search_tracks(&client, &artist_name, limit).await
                } else {
                    Ok(vec![])
                }
            },
        );
        let bp_tracks = bp_res.unwrap_or_else(|e| {
            tracing::warn!("Beatport artist search failed: {e}");
            vec![]
        });
        let sc_tracks = sc_res.unwrap_or_else(|e| {
            tracing::warn!("SoundCloud artist search failed: {e}");
            vec![]
        });
        let vl_tracks = vl_res.unwrap_or_else(|e| {
            tracing::warn!("Volumo artist search failed: {e}");
            vec![]
        });
        let tx_tracks = tx_res.unwrap_or_else(|e| {
            tracing::warn!("Traxsource artist search failed: {e}");
            vec![]
        });
        let sp_tracks = sp_res.unwrap_or_else(|e| {
            tracing::warn!("Spotify artist search failed: {e}");
            vec![]
        });
        stream_raw_tracks(
            &client,
            &tx,
            query_id,
            limit,
            interleave(bp_tracks, sc_tracks, vl_tracks, tx_tracks, sp_tracks),
        )
        .await;
        Ok(())
    })
    .await;
}

/// Fetches releases by a single artist or label name across the selected sources, dropping
/// anything released before `cutoff`. Beatport supports a native date-range query; the other
/// backends only offer plain-text search, so their results are filtered client-side.
async fn gather_new_release_raw_tracks(
    client: &Client,
    name: &str,
    is_label: bool,
    cutoff: NaiveDate,
    sources: &[TrackSource],
) -> Vec<RawTrack> {
    let per_source_limit = 50;
    let (bp_res, sc_res, vl_res, tx_res) = tokio::join!(
        async {
            if !sources.contains(&TrackSource::Beatport) {
                return Ok(vec![]);
            }
            if is_label {
                match bp_search_labels(client, name, 1).await?.into_iter().next() {
                    Some(label) => bp_get_label_releases(client, &label, Some(cutoff)).await,
                    None => Ok(vec![]),
                }
            } else {
                match bp_search_artist(client, name, 1).await?.into_iter().next() {
                    Some(artist) => bp_get_artist_releases(client, &artist, Some(cutoff)).await,
                    None => Ok(vec![]),
                }
            }
        },
        async {
            if sources.contains(&TrackSource::Soundcloud) {
                sc_tracks_by_person(client, name, per_source_limit, is_label).await
            } else {
                Ok(vec![])
            }
        },
        async {
            if sources.contains(&TrackSource::Volumo) {
                vl_search_tracks(client, name, per_source_limit).await
            } else {
                Ok(vec![])
            }
        },
        async {
            if sources.contains(&TrackSource::Traxsource) {
                tx_search_tracks(client, name).await
            } else {
                Ok(vec![])
            }
        },
    );

    // Not-yet-released tracks (pre-order pages) sneak past a lower-bound-only filter, and
    // Beatport's own `publish_date` query param isn't reliably honored server-side, so every
    // source gets the same client-side [cutoff, today] range check regardless of backend.
    let today = chrono::Local::now().date_naive();
    let in_range = |d: NaiveDate| d >= cutoff && d <= today;

    let bp_tracks: Vec<BPTrack> = bp_res
        .unwrap_or_else(|e| {
            tracing::warn!("Beatport new-releases search for {name:?} failed: {e}");
            vec![]
        })
        .into_iter()
        .filter(|t| in_range(t.release_date))
        .collect();
    let sc_tracks: Vec<SCTrack> = sc_res
        .unwrap_or_else(|e| {
            tracing::warn!("SoundCloud new-releases search for {name:?} failed: {e}");
            vec![]
        })
        .into_iter()
        .filter(|t| in_range(t.release_date))
        .collect();
    let vl_tracks: Vec<VLTrack> = vl_res
        .unwrap_or_else(|e| {
            tracing::warn!("Volumo new-releases search for {name:?} failed: {e}");
            vec![]
        })
        .into_iter()
        .filter(|t| in_range(t.release_date))
        .collect();
    let tx_tracks: Vec<TXTrack> = tx_res
        .unwrap_or_else(|e| {
            tracing::warn!("Traxsource new-releases search for {name:?} failed: {e}");
            vec![]
        })
        .into_iter()
        .filter(|t| in_range(t.release_date))
        .collect();

    bp_tracks
        .into_iter()
        .map(RawTrack::Bp)
        .chain(sc_tracks.into_iter().map(RawTrack::Sc))
        .chain(vl_tracks.into_iter().map(RawTrack::Vl))
        .chain(tx_tracks.into_iter().map(RawTrack::Tx))
        .collect()
}

/// Like `stream_raw_tracks`, but skips anything whose resolved YouTube id is already in `seen`
/// — which is seeded with the ids already in the collection, so this both excludes existing
/// tracks and de-duplicates results found via more than one artist/label.
/// YouTube-matches are resolved `NEW_RELEASES_MATCH_CONCURRENCY`-wide via `buffered`, which
/// preserves `raw_tracks`' order (already sorted newest-first) while overlapping the network
/// round-trips — the dominant cost here, since each of up to `NEW_RELEASES_LIMIT` candidates
/// needs its own YouTube search. The dedup/send loop itself stays sequential since it's cheap
/// and mutates `seen`.
async fn stream_new_release_tracks(
    client: &Client,
    tx: &mpsc::Sender<MsgServer>,
    query_id: i64,
    seen: &mut HashSet<String>,
    limit: usize,
    raw_tracks: Vec<RawTrack>,
) {
    let mut matches = stream::iter(raw_tracks)
        .map(|raw| async move {
            let vid = find_best_match(client, &raw).await;
            (raw, vid)
        })
        .buffered(NEW_RELEASES_MATCH_CONCURRENCY);

    let mut count = 0;
    while let Some((raw, vid)) = matches.next().await {
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
const NEW_RELEASES_MATCH_CONCURRENCY: usize = 12;
// Safety cap regardless of what the client asks for, since every result costs a YouTube search.
const NEW_RELEASES_MAX_LIMIT: usize = 1000;

/// Parameters for [`search_new_releases`], grouped to keep the function signature manageable.
pub struct NewReleasesQuery {
    pub query_id: i64,
    pub days: i64,
    pub limit: usize,
    pub artists: Vec<String>,
    pub labels: Vec<String>,
    pub existing_yt_ids: HashSet<String>,
    pub sources: Vec<TrackSource>,
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
    } = q;
    let limit = limit.min(NEW_RELEASES_MAX_LIMIT);
    run_search(&tx, query_id, "search_new_releases", || async {
        let cutoff = chrono::Local::now().date_naive() - chrono::Duration::days(days);
        let names: Vec<(String, bool)> = artists
            .into_iter()
            .map(|a| (a, false))
            .chain(labels.into_iter().map(|l| (l, true)))
            .collect();

        let per_name: Vec<Vec<RawTrack>> = stream::iter(names)
            .map(|(name, is_label)| {
                let client = client.clone();
                let sources = sources.clone();
                async move {
                    gather_new_release_raw_tracks(&client, &name, is_label, cutoff, &sources).await
                }
            })
            .buffer_unordered(NEW_RELEASES_CONCURRENCY)
            .collect()
            .await;

        let mut raw_tracks: Vec<RawTrack> = per_name.into_iter().flatten().collect();
        raw_tracks.sort_by_key(|t| std::cmp::Reverse(t.release_date()));

        let mut seen = existing_yt_ids;
        stream_new_release_tracks(&client, &tx, query_id, &mut seen, limit, raw_tracks).await;
        Ok(())
    })
    .await;
}

#[cfg(test)]
mod remix_match_tests {
    use super::*;
    use crate::models::BPTrack;

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
    fn rejects_matching_title_when_artist_is_absent() {
        let raw = RawTrack::Bp(bp_track("Just Like Magic", "Original Mix"));
        let wrong = video_from("Wrong Artist - Just Like Magic", "Wrong Artist");
        assert!(choose_best_match(&raw, vec![wrong]).is_none());
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
