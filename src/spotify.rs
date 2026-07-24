//! Talks to Spotify's private/unofficial web-player APIs (there's no public Web API usage
//! here, since that requires an OAuth app + Spotify Premium for some features).
//!
//! Two distinct trust levels:
//! - Search is anonymous: it emulates the web player using a TOTP code derived from a secret
//!   buried in Spotify's obfuscated JS, plus per-operation persisted-query hashes scraped out
//!   of the web-player JS bundle at runtime. No login, no cookies.
//! - Playlist creation needs a real logged-in session.
//!   Stereo asks the user to paste the `Cookie` header from an already-logged-in browser
//!   session and persists it to disk.

use std::{
    sync::LazyLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use chrono::NaiveDate;
use regex::Regex;
use serde_json::Value;
use tokio::sync::RwLock;
use totp_rs::{Algorithm, TOTP};
use wreq::{header::HeaderMap, Client};

use crate::models::Track;

const SPOTIFY_SESSION_FILE: &str = "spotify_session.json";

// ── TOTP secret (anti-bot token for the anonymous `api/token` endpoint) ─────────────────────

// Spotify buries this secret in obfuscated client JS; a third party keeps a reverse-engineered
// copy up to date. Fall back to a known-good hardcoded value if that's unreachable.
const SECRET_URL: &str =
    "https://code.thetadev.de/ThetaDev/spotify-secrets/raw/branch/main/secrets/secretDict.json";
const FALLBACK_SECRET_VERSION: u64 = 61;
const FALLBACK_SECRET: &[u8] = &[
    44, 55, 47, 42, 70, 40, 34, 114, 76, 74, 50, 111, 120, 97, 75, 76, 94, 102, 43, 69, 49, 120,
    118, 80, 64, 78,
];
const SECRET_TTL: Duration = Duration::from_secs(15 * 60);

struct CachedSecret {
    version: u64,
    bytes: Vec<u8>,
    fetched_at: std::time::Instant,
}

static TOTP_SECRET: LazyLock<RwLock<Option<CachedSecret>>> = LazyLock::new(|| RwLock::new(None));

async fn fetch_totp_secret(client: &Client) -> (u64, Vec<u8>) {
    let result: anyhow::Result<(u64, Vec<u8>)> = async {
        let body: Value = client.get(SECRET_URL).send().await?.json().await?;
        let map = body.as_object().context("secret dict is not an object")?;
        let version: u64 = map
            .keys()
            .filter_map(|k| k.parse::<u64>().ok())
            .max()
            .context("no numeric secret version found")?;
        let bytes: Vec<u8> = map[&version.to_string()]
            .as_array()
            .context("secret is not an array")?
            .iter()
            .map(|v| v.as_u64().unwrap_or(0) as u8)
            .collect();
        Ok((version, bytes))
    }
    .await;

    match result {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Failed to fetch Spotify TOTP secret, using fallback: {e}");
            (FALLBACK_SECRET_VERSION, FALLBACK_SECRET.to_vec())
        }
    }
}

async fn get_totp_secret(client: &Client) -> (u64, Vec<u8>) {
    if let Some(cached) = TOTP_SECRET.read().await.as_ref() {
        if cached.fetched_at.elapsed() < SECRET_TTL {
            return (cached.version, cached.bytes.clone());
        }
    }
    let (version, bytes) = fetch_totp_secret(client).await;
    *TOTP_SECRET.write().await = Some(CachedSecret {
        version,
        bytes: bytes.clone(),
        fetched_at: std::time::Instant::now(),
    });
    (version, bytes)
}

/// Generates the current TOTP code Spotify expects on `open.spotify.com/api/token`, plus the
/// secret version it was generated with.
async fn generate_totp(client: &Client) -> anyhow::Result<(String, u64)> {
    let (version, secret) = get_totp_secret(client).await;
    let transformed: String = secret
        .iter()
        .enumerate()
        .map(|(t, &e)| (e ^ (((t % 33) as u8) + 9)).to_string())
        .collect();

    let totp = TOTP::new(Algorithm::SHA1, 6, 1, 30, transformed.into_bytes())
        .context("could not construct TOTP")?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    Ok((totp.generate(now), version))
}

// ── App-level (anonymous) auth: access token, client token, persisted-query hashes ──────────

#[derive(Default)]
struct AppAuth {
    access_token: Option<String>,
    access_token_expires_at_ms: f64,
    client_token: Option<String>,
    client_id: Option<String>,
    client_version: Option<String>,
    device_id: String,
    js_pack_url: Option<String>,
    raw_hashes: Option<String>,
    /// Whether `access_token`/`client_token` were obtained with a user session cookie attached.
    /// An anonymous token/session pair is rejected by spclient mutation endpoints (e.g. creating
    /// a playlist) even when a valid session `Cookie` header is attached to that specific
    /// request — the token itself has to have been issued while authenticated. So a cached
    /// anonymous token can't be reused once we need to make an authenticated call; it forces a
    /// re-fetch with the cookie attached instead.
    authenticated: bool,
}

static APP_AUTH: LazyLock<RwLock<AppAuth>> = LazyLock::new(|| RwLock::new(AppAuth::default()));

// Refresh a bit before real expiry to avoid skew-induced 401s.
const REFRESH_SKEW_MS: f64 = 30_000.0;

fn extract_between<'a>(haystack: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let after_start = haystack.split(start).nth(1)?;
    after_start.split(end).next()
}

async fn fetch_session(client: &Client, cookie: Option<&str>) -> anyhow::Result<()> {
    let mut req = client.get("https://open.spotify.com");
    if let Some(cookie) = cookie {
        req = req.header("Cookie", cookie);
    }
    let resp = req.send().await?;
    let device_id = resp
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|c| extract_between(c, "sp_t=", ";"))
        .unwrap_or("")
        .to_string();
    let html = resp.text().await?;

    let js_pack_url = {
        let doc = scraper::Html::parse_document(&html);
        let sel = scraper::Selector::parse("script[src]").unwrap();
        doc.select(&sel)
            .filter_map(|el| el.value().attr("src"))
            .find(|src| src.contains("web-player/web-player") && src.ends_with(".js"))
            .map(String::from)
    };

    let app_server_config = extract_between(
        &html,
        r#"<script id="appServerConfig" type="text/plain">"#,
        "</script>",
    )
    .context("could not find appServerConfig in open.spotify.com response")?;
    let decoded = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        app_server_config,
    )?;
    let cfg: Value = serde_json::from_slice(&decoded)?;
    let client_version = cfg["clientVersion"]
        .as_str()
        .context("no clientVersion in appServerConfig")?
        .to_string();

    let mut auth = APP_AUTH.write().await;
    auth.js_pack_url = js_pack_url;
    auth.client_version = Some(client_version);
    auth.device_id = device_id;
    Ok(())
}

async fn fetch_access_token(client: &Client, cookie: Option<&str>) -> anyhow::Result<()> {
    fetch_session(client, cookie).await?;
    let (totp, version) = generate_totp(client).await?;

    let mut req = client.get("https://open.spotify.com/api/token").query(&[
        ("reason", "init"),
        ("productType", "web-player"),
        ("totp", &totp),
        ("totpVer", &version.to_string()),
        ("totpServer", &totp),
    ]);
    if let Some(cookie) = cookie {
        req = req.header("Cookie", cookie);
    }
    let resp: Value = req.send().await?.json().await?;

    let access_token = resp["accessToken"]
        .as_str()
        .context("no accessToken in response")?
        .to_string();
    let client_id = resp["clientId"]
        .as_str()
        .context("no clientId in response")?
        .to_string();
    let expires_at_ms = resp["accessTokenExpirationTimestampMs"]
        .as_f64()
        .unwrap_or(0.0);

    let mut auth = APP_AUTH.write().await;
    auth.access_token = Some(access_token);
    auth.client_id = Some(client_id);
    auth.access_token_expires_at_ms = expires_at_ms;
    auth.authenticated = cookie.is_some();
    Ok(())
}

async fn fetch_client_token(client: &Client, cookie: Option<&str>) -> anyhow::Result<()> {
    let (client_id, client_version, device_id) = {
        let auth = APP_AUTH.read().await;
        (
            auth.client_id.clone(),
            auth.client_version.clone(),
            auth.device_id.clone(),
        )
    };
    let (client_id, client_version) = match (client_id, client_version) {
        (Some(id), Some(ver)) => (id, ver),
        _ => {
            fetch_access_token(client, cookie).await?;
            let auth = APP_AUTH.read().await;
            (
                auth.client_id.clone().context("still no client_id")?,
                auth.client_version
                    .clone()
                    .context("still no client_version")?,
            )
        }
    };

    let payload = serde_json::json!({
        "client_data": {
            "client_version": client_version,
            "client_id": client_id,
            "js_sdk_data": {
                "device_brand": "unknown",
                "device_model": "unknown",
                "os": "windows",
                "os_version": "NT 10.0",
                "device_id": device_id,
                "device_type": "computer",
            }
        }
    });

    let mut req = client
        .post("https://clienttoken.spotify.com/v1/clienttoken")
        .header("Accept", "application/json");
    if let Some(cookie) = cookie {
        req = req.header("Cookie", cookie);
    }
    let resp: Value = req.json(&payload).send().await?.json().await?;

    if resp["response_type"].as_str() != Some("RESPONSE_GRANTED_TOKEN_RESPONSE") {
        anyhow::bail!("Could not get client token: {:?}", resp["response_type"]);
    }
    let token = resp["granted_token"]["token"]
        .as_str()
        .context("no granted_token.token in response")?
        .to_string();

    APP_AUTH.write().await.client_token = Some(token);
    Ok(())
}

/// Ensures the cached access/client tokens are present, unexpired, and (when `cookie` is
/// supplied) tied to that authenticated session, refreshing as needed. Returns the headers a
/// pathfinder/spclient request needs. Mirrors `BaseClient._auth_rule`.
async fn auth_headers(client: &Client, cookie: Option<&str>) -> anyhow::Result<HeaderMap> {
    let needs_auth_upgrade = cookie.is_some() && !APP_AUTH.read().await.authenticated;

    let needs_client_token = needs_auth_upgrade || APP_AUTH.read().await.client_token.is_none();
    if needs_client_token {
        fetch_client_token(client, cookie).await?;
    }

    let needs_access_token = needs_auth_upgrade || APP_AUTH.read().await.access_token.is_none();
    if needs_access_token {
        fetch_access_token(client, cookie).await?;
    }

    let expired = {
        let auth = APP_AUTH.read().await;
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0);
        auth.access_token_expires_at_ms > 0.0
            && now_ms + REFRESH_SKEW_MS >= auth.access_token_expires_at_ms
    };
    if expired {
        APP_AUTH.write().await.access_token = None;
        fetch_access_token(client, cookie).await?;
    }

    let auth = APP_AUTH.read().await;
    let mut headers = HeaderMap::new();
    headers.insert(
        "Authorization",
        format!("Bearer {}", auth.access_token.as_deref().unwrap_or(""))
            .parse()
            .unwrap(),
    );
    headers.insert(
        "Client-Token",
        auth.client_token.as_deref().unwrap_or("").parse().unwrap(),
    );
    headers.insert(
        "Spotify-App-Version",
        auth.client_version
            .as_deref()
            .unwrap_or("")
            .parse()
            .unwrap(),
    );
    Ok(headers)
}

// ── Persisted-query sha256 hashes, scraped out of the web-player JS bundle ──────────────────

type ChunkMap = Vec<(i64, String)>;

fn extract_mappings(js: &str) -> anyhow::Result<(ChunkMap, ChunkMap)> {
    // Spotify's bundle embeds several `{123:"foo",456:"bar",...}` object literals mapping chunk
    // ids to names/hashes; the two we need are (empirically) the 4th and 5th ones.
    // The 4th holds hashes (e.g. `1328:"a67ceabe"`), the 5th holds names (e.g.
    // `1328:"xpui-routes-search"`) for a subset of those same chunk ids — return (name, hash).
    let block_re = Regex::new(r#"\{\d+:"[^"]+"(?:,\d+:"[^"]+")*\}"#).unwrap();
    let pair_re = Regex::new(r#"(\d+):"([^"]+)""#).unwrap();

    let blocks: Vec<&str> = block_re.find_iter(js).map(|m| m.as_str()).collect();
    anyhow::ensure!(blocks.len() > 4, "could not find both JS chunk mappings");

    let parse_block = |block: &str| -> Vec<(i64, String)> {
        pair_re
            .captures_iter(block)
            .filter_map(|c| Some((c[1].parse().ok()?, c[2].to_string())))
            .collect()
    };

    Ok((parse_block(blocks[4]), parse_block(blocks[3])))
}

async fn fetch_raw_hashes(client: &Client) -> anyhow::Result<String> {
    let js_pack_url = {
        let auth = APP_AUTH.read().await;
        auth.js_pack_url.clone()
    };
    let js_pack_url = match js_pack_url {
        Some(url) => url,
        None => {
            fetch_session(client, None).await?;
            APP_AUTH
                .read()
                .await
                .js_pack_url
                .clone()
                .context("could not locate web-player JS pack")?
        }
    };

    let mut raw_hashes = client.get(&js_pack_url).send().await?.text().await?;

    let (name_map, hash_map) = extract_mappings(&raw_hashes)?;
    let hash_by_key: std::collections::HashMap<i64, &String> =
        hash_map.iter().map(|(k, v)| (*k, v)).collect();
    let chunk_urls: Vec<String> = name_map
        .iter()
        .filter_map(|(key, name)| {
            hash_by_key.get(key).map(|hash| {
                format!("https://open.spotifycdn.com/cdn/build/web-player/{name}.{hash}.js")
            })
        })
        .collect();

    for url in chunk_urls {
        raw_hashes += &client.get(&url).send().await?.text().await?;
    }

    Ok(raw_hashes)
}

/// Looks up the persisted-query `sha256Hash` for a named pathfinder operation (query or
/// mutation), scraping/caching the web-player JS bundle on first use.
async fn part_hash(client: &Client, operation: &str) -> anyhow::Result<String> {
    let cached = APP_AUTH.read().await.raw_hashes.clone();
    let raw_hashes = match cached {
        Some(h) => h,
        None => {
            let h = fetch_raw_hashes(client).await?;
            APP_AUTH.write().await.raw_hashes = Some(h.clone());
            h
        }
    };

    for kind in ["query", "mutation"] {
        if let Some(hash) =
            extract_between(&raw_hashes, &format!("\"{operation}\",\"{kind}\",\""), "\"")
        {
            return Ok(hash.to_string());
        }
    }
    anyhow::bail!("could not find persisted-query hash for operation {operation}")
}

/// POSTs a persisted-query request to Spotify's pathfinder API. `cookie`, when supplied, is the
/// user's imported browser session (required for mutations like creating/editing playlists).
async fn pathfinder_query(
    client: &Client,
    operation: &str,
    variables: Value,
    cookie: Option<&str>,
) -> anyhow::Result<Value> {
    let hash = part_hash(client, operation).await?;
    let mut headers = auth_headers(client, cookie).await?;
    if let Some(cookie) = cookie {
        headers.insert("Cookie", cookie.parse()?);
    }

    let params = [
        ("operationName".to_string(), operation.to_string()),
        ("variables".to_string(), variables.to_string()),
        (
            "extensions".to_string(),
            serde_json::json!({
                "persistedQuery": { "version": 1, "sha256Hash": hash }
            })
            .to_string(),
        ),
    ];

    let resp = client
        .post("https://api-partner.spotify.com/pathfinder/v1/query")
        .headers(headers)
        .query(&params)
        .send()
        .await?;
    let status = resp.status();
    let text = resp.text().await?;
    let body: Value = serde_json::from_str(&text)
        .with_context(|| format!("pathfinder query returned non-JSON body ({status}): {text}"))?;
    anyhow::ensure!(status.is_success(), "pathfinder query failed: {body}");
    Ok(body)
}

// ── Search ───────────────────────────────────────────────────────────────────────────────────

pub struct SpTrack {
    pub artists: Vec<String>,
    pub track_name: String,
    pub album: Option<String>,
    pub release_date: NaiveDate,
    pub uri: String,
}

impl SpTrack {
    /// The bare Spotify track id, extracted from `uri` (`spotify:track:<id>`).
    pub fn id(&self) -> &str {
        self.uri.rsplit(':').next().unwrap_or(&self.uri)
    }

    pub fn into_track(self, yt_id: String) -> Track {
        let sp_id = Some(self.id().to_string());
        Track {
            yt_id,
            sp_id,
            title: self.track_name,
            artists: self.artists,
            album: self.album,
            release_date: Some(self.release_date),
            ..Default::default()
        }
    }
}

fn parse_sp_track(item: &Value) -> Option<SpTrack> {
    let data = &item["item"]["data"];
    let track_name = data["name"].as_str()?.trim().to_string();
    let uri = data["uri"].as_str()?.to_string();
    let artists: Vec<String> = data["artists"]["items"]
        .as_array()?
        .iter()
        .filter_map(|a| a["profile"]["name"].as_str().map(String::from))
        .collect();
    let album = data["albumOfTrack"]["name"].as_str().map(String::from);
    // Spotify's search endpoint doesn't expose a release date on the track item itself; the
    // album date lives on a separate query, so default to "now" (never filtered out as future).
    let release_date = chrono::Local::now().date_naive();

    Some(SpTrack {
        artists,
        track_name,
        album,
        release_date,
        uri,
    })
}

/// Searches Spotify's catalog for tracks matching `query`. Anonymous — no session required.
pub async fn search_tracks(
    client: &Client,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<SpTrack>> {
    let variables = serde_json::json!({
        "searchTerm": query,
        "offset": 0,
        "limit": limit,
        "numberOfTopResults": 5,
        "includeAudiobooks": false,
        "includeArtistHasConcertsField": false,
        "includePreReleases": true,
        "includeLocalConcertsField": false,
    });

    let resp = pathfinder_query(client, "searchDesktop", variables, None).await?;
    let items = resp["data"]["searchV2"]["tracksV2"]["items"]
        .as_array()
        .context("no tracksV2.items in search response")?;
    Ok(items.iter().filter_map(parse_sp_track).collect())
}

/// Best-effort match of an existing collection track (identified by artist/title, since stereo
/// doesn't persist a Spotify id) against Spotify's catalog, for playlist export.
pub async fn match_track(
    client: &Client,
    artist: &str,
    title: &str,
) -> anyhow::Result<Option<SpTrack>> {
    let query = format!("{artist} {title}");
    let candidates = search_tracks(client, &query, 5).await?;
    let target = query.to_lowercase();
    Ok(candidates
        .into_iter()
        .map(|t| {
            let expected = format!("{} {}", t.artists.join(" "), t.track_name).to_lowercase();
            (strsim::normalized_levenshtein(&target, &expected), t)
        })
        .filter(|(score, _)| *score > 0.5)
        .max_by(|(a, _), (b, _)| a.total_cmp(b))
        .map(|(_, t)| t))
}

// ── Session (cookie import) + playlist creation ─────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize)]
pub struct SpotifySession {
    pub cookie: String,
}

impl SpotifySession {
    pub async fn load(home: &std::path::Path) -> Option<SpotifySession> {
        let path = home.join(SPOTIFY_SESSION_FILE);
        let data = tokio::fs::read(&path).await.ok()?;
        serde_json::from_slice(&data).ok()
    }

    pub async fn save(home: &std::path::Path, cookie: &str) -> anyhow::Result<()> {
        let path = home.join(SPOTIFY_SESSION_FILE);
        let data = serde_json::to_vec(&SpotifySession {
            cookie: cookie.to_string(),
        })?;
        tokio::fs::write(&path, data).await?;
        Ok(())
    }

    pub async fn clear(home: &std::path::Path) -> anyhow::Result<()> {
        let path = home.join(SPOTIFY_SESSION_FILE);
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

type BrowserCookieFn = fn(Option<Vec<String>>) -> rookie::Result<Vec<rookie::enums::Cookie>>;

/// Tries each installed browser in turn for a logged-in `open.spotify.com` session (signaled by
/// the presence of the long-lived `sp_dc` cookie), returning the first one found. This reads
/// browsers' cookie stores directly off disk (decrypting via the OS keychain where needed, e.g.
/// Chrome on macOS) as an alternative to the user manually pasting the `Cookie` header.
pub async fn detect_session() -> anyhow::Result<SpotifySession> {
    tokio::task::spawn_blocking(|| {
        use rookie::enums::CookieToString;

        let mut browsers: Vec<(&str, BrowserCookieFn)> = vec![
            ("Firefox", rookie::firefox as BrowserCookieFn),
            ("Chrome", rookie::chrome),
            ("Brave", rookie::brave),
            ("Edge", rookie::edge),
            ("Vivaldi", rookie::vivaldi),
            ("Opera", rookie::opera),
            ("Opera GX", rookie::opera_gx),
            ("Arc", rookie::arc),
            ("Zen", rookie::zen),
            ("LibreWolf", rookie::librewolf),
            ("Chromium", rookie::chromium),
        ];
        #[cfg(target_os = "macos")]
        browsers.push(("Safari", rookie::safari));
        #[cfg(target_os = "linux")]
        browsers.push(("Cachy", rookie::cachy));
        #[cfg(target_os = "windows")]
        browsers.push(("Internet Explorer", rookie::internet_explorer));

        for (name, browser_fn) in browsers {
            let Ok(cookies) = browser_fn(Some(vec!["spotify.com".to_string()])) else {
                continue;
            };
            if cookies.iter().any(|c| c.name == "sp_dc") {
                tracing::info!("Found a logged-in Spotify session in {name}");
                return Ok(SpotifySession {
                    cookie: cookies.to_string(),
                });
            }
        }
        anyhow::bail!("No logged-in Spotify session found in any installed browser")
    })
    .await?
}

/// Fetches the Spotify username for the imported session — needed to add a newly created
/// playlist to the user's library. Relies purely on the session cookie, no app-level auth.
async fn get_username(client: &Client, session: &SpotifySession) -> anyhow::Result<String> {
    let resp: Value = client
        .get("https://www.spotify.com/api/account-settings/v1/profile")
        .header("Cookie", &session.cookie)
        .send()
        .await?
        .json()
        .await?;
    resp["profile"]["username"]
        .as_str()
        .map(String::from)
        .context("no profile.username in response")
}

async fn stage_create_playlist(
    client: &Client,
    session: &SpotifySession,
    name: &str,
) -> anyhow::Result<String> {
    let payload = serde_json::json!({
        "ops": [{
            "kind": 6,
            "updateListAttributes": {
                "newAttributes": {
                    "values": { "name": name, "formatAttributes": [], "pictureSize": [] },
                    "noValue": [],
                }
            }
        }]
    });

    let headers = auth_headers(client, Some(&session.cookie)).await?;
    let resp = client
        .post("https://spclient.wg.spotify.com/playlist/v2/playlist")
        .headers(headers)
        .header("Cookie", &session.cookie)
        .json(&payload)
        .send()
        .await?
        .text()
        .await?;

    let re = Regex::new(r"spotify:playlist:[a-zA-Z0-9]+").unwrap();
    re.find(&resp)
        .map(|m| m.as_str().to_string())
        .context("could not find playlist id in staging response")
}

/// Creates a new Spotify playlist named `name` and adds it to the user's library. Returns its
/// `spotify:playlist:...` URI (usable directly with `add_tracks`).
pub async fn create_playlist(
    client: &Client,
    session: &SpotifySession,
    name: &str,
) -> anyhow::Result<String> {
    let playlist_uri = stage_create_playlist(client, session, name).await?;
    let username = get_username(client, session).await?;

    let payload = serde_json::json!({
        "deltas": [{
            "ops": [{
                "kind": 2,
                "add": {
                    "items": [{
                        "uri": playlist_uri,
                        "attributes": {
                            "timestamp": SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
                            "formatAttributes": [],
                            "availableSignals": [],
                        }
                    }],
                    "addFirst": true,
                }
            }],
            "info": { "source": { "client": 5 } },
        }],
        "wantResultingRevisions": false,
        "wantSyncResult": false,
        "nonces": [],
    });

    let headers = auth_headers(client, Some(&session.cookie)).await?;
    let resp = client
        .post(format!(
            "https://spclient.wg.spotify.com/playlist/v2/user/{username}/rootlist/changes"
        ))
        .headers(headers)
        .header("Cookie", &session.cookie)
        .json(&payload)
        .send()
        .await?;
    anyhow::ensure!(
        resp.status().is_success(),
        "could not add playlist to library: {}",
        resp.status()
    );

    Ok(playlist_uri)
}

/// Turns a `spotify:playlist:...` URI into an `open.spotify.com` URL suitable for opening in a
/// browser.
pub fn playlist_url(playlist_uri: &str) -> Option<String> {
    let id = playlist_uri.rsplit(':').next()?;
    Some(format!("https://open.spotify.com/playlist/{id}"))
}

/// Adds tracks (by Spotify URI) to an existing playlist.
///
/// Sent in chunks: the URIs go out as a query-string parameter, so a single
/// request with too many tracks produces a URL long enough that Spotify (or a
/// CDN in front of it) rejects it with a non-JSON error body.
pub async fn add_tracks(
    client: &Client,
    session: &SpotifySession,
    playlist_uri: &str,
    track_uris: &[String],
) -> anyhow::Result<()> {
    const CHUNK_SIZE: usize = 100;
    for chunk in track_uris.chunks(CHUNK_SIZE) {
        let variables = serde_json::json!({
            "playlistItemUris": chunk,
            "playlistUri": playlist_uri,
            "newPosition": { "moveType": "BOTTOM_OF_PLAYLIST", "fromUid": null },
        });
        pathfinder_query(client, "addToPlaylist", variables, Some(&session.cookie)).await?;
    }
    Ok(())
}
