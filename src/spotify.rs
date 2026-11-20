//! Talks to Spotify's private/unofficial web-player APIs (there's no public Web API usage
//! here, since that requires an OAuth app + Spotify Premium for some features).
//!
//! Two distinct trust levels:
//! - Search is anonymous: it emulates the web player using a TOTP code and per-operation
//!   persisted-query hashes, both read out of the web-player JS bundle at runtime. No login,
//!   no cookies.
//! - Playlist creation needs a real logged-in session.
//!   Stereo asks the user to paste the `Cookie` header from an already-logged-in browser
//!   session and persists it to disk.

use std::{
    sync::{Arc, LazyLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use chrono::NaiveDate;
use regex::Regex;
use serde_json::Value;
use tokio::sync::{Mutex, RwLock};
use totp_rs::{Algorithm, Builder};
use wreq::{
    header::{HeaderMap, CONTENT_TYPE},
    Client,
};

use crate::{models::Track, utils};

const SPOTIFY_SESSION_FILE: &str = "spotify_session.json";

// ── Web-player bundle: source of the TOTP secret and the persisted-query hashes ─────────────

/// The last web-player bundle downloaded, and its URL. The TOTP secret and the persisted-query
/// hashes both come from it, needed at different times and in either order; the lock is held
/// while downloading, so they share one download.
static BUNDLE: LazyLock<Mutex<Option<CachedBundle>>> = LazyLock::new(|| Mutex::new(None));

struct CachedBundle {
    url: String,
    js: Arc<str>,
}

/// The source of the web-player bundle at `url`, downloaded unless it's the last one downloaded.
async fn web_player_bundle(client: &Client, url: &str) -> anyhow::Result<Arc<str>> {
    let mut cached = BUNDLE.lock().await;
    if let Some(bundle) = cached.as_ref().filter(|bundle| bundle.url == url) {
        return Ok(bundle.js.clone());
    }
    let resp = client.get(url).send().await?.error_for_status()?;
    // An error page served with a 200 must not stand in for the bundle.
    let content_type = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    anyhow::ensure!(
        content_type.contains("javascript"),
        "web-player bundle {url} served as {content_type:?}, not JavaScript"
    );
    let js: Arc<str> = resp.text().await?.into();
    *cached = Some(CachedBundle {
        url: url.to_string(),
        js: js.clone(),
    });
    Ok(js)
}

// ── TOTP secret (anti-bot token for the anonymous `api/token` endpoint) ─────────────────────

// The web-player bundle carries its TOTP secrets as a list of `{secret:"…",version:61}`
// literals; we use the highest version. The hardcoded copy is only for when no bundle has
// given us one (download error, or the pattern no longer matches) and has to be updated by
// hand when Spotify rotates the secret.
const FALLBACK_SECRET_VERSION: u64 = 61;
const FALLBACK_SECRET: &[u8] = &[
    44, 55, 47, 42, 70, 40, 34, 114, 76, 74, 50, 111, 120, 97, 75, 76, 94, 102, 43, 69, 49, 120,
    118, 80, 64, 78,
];

/// How long a failed lookup stands before a token fetch for that bundle tries again.
const SECRET_LOOKUP_RETRY: Duration = Duration::from_secs(5 * 60);
/// Lookups remembered: one per bundle, i.e. per Spotify deploy seen.
const MAX_SECRET_LOOKUPS: usize = 8;

enum SecretLookup {
    Found(u64, Vec<u8>),
    Failed(Instant),
}

/// TOTP secret lookups by bundle URL, oldest first. Each token fetch uses the lookup for the
/// bundle its own `fetch_session` found, so a lookup for an older bundle finishing late can't
/// stand in for a newer one.
static SECRET_LOOKUPS: LazyLock<RwLock<Vec<(String, SecretLookup)>>> =
    LazyLock::new(|| RwLock::new(Vec::new()));

/// Finds the newest TOTP secret in the web-player bundle's source: its version and the
/// secret string's characters.
fn extract_totp_secret(js: &str) -> anyhow::Result<(u64, Vec<u8>)> {
    static SECRET_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r#"secret\s*:\s*(?:"((?:[^"\\]|\\.)*)"|'((?:[^'\\]|\\.)*)')\s*,\s*version\s*:\s*(\d+)"#,
        )
        .unwrap()
    });
    let (version, literal) = SECRET_RE
        .captures_iter(js)
        .filter_map(|c| {
            let literal = c.get(1).or_else(|| c.get(2))?.as_str();
            Some((c[3].parse::<u64>().ok()?, literal))
        })
        .max_by_key(|(version, _)| *version)
        .context("no TOTP secret found")?;
    // Decoded only once chosen: an unreadable newest secret must not make way for an older one.
    let bytes = unescape_js_string(literal)
        .with_context(|| format!("TOTP secret v{version} has an escape we can't read"))?;
    Ok((version, bytes))
}

/// Unescapes a JS string literal's body. Only single-character escapes of ASCII punctuation
/// (`\\`, `\"`, `\'`, …) are expected in a secret; anything else (`\n`, `\x41`, `A`,
/// non-ASCII) gives `None` rather than a wrong secret.
fn unescape_js_string(literal: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(literal.len());
    let mut chars = literal.chars();
    while let Some(c) = chars.next() {
        let c = match c {
            '\\' => chars.next().filter(|c| c.is_ascii_punctuation())?,
            c if c.is_ascii() => c,
            _ => return None,
        };
        bytes.push(c as u8);
    }
    Some(bytes)
}

/// The newest secret any bundle has given us, else the hardcoded one.
fn newest_known_secret(lookups: &[(String, SecretLookup)]) -> (u64, Vec<u8>) {
    lookups
        .iter()
        .filter_map(|(_, lookup)| match lookup {
            SecretLookup::Found(version, bytes) => Some((*version, bytes)),
            SecretLookup::Failed(_) => None,
        })
        .max_by_key(|(version, _)| *version)
        .map(|(version, bytes)| (version, bytes.clone()))
        .unwrap_or_else(|| (FALLBACK_SECRET_VERSION, FALLBACK_SECRET.to_vec()))
}

/// The TOTP secret of the web-player bundle at `bundle_url`, looked up the first time it's
/// asked for (and again a while after a failed lookup). Without one, the newest secret known.
async fn get_totp_secret(client: &Client, bundle_url: Option<&str>) -> (u64, Vec<u8>) {
    let Some(bundle_url) = bundle_url else {
        let secret = newest_known_secret(&SECRET_LOOKUPS.read().await);
        tracing::warn!(
            "Spotify's web-player bundle not found, using TOTP secret v{}",
            secret.0
        );
        return secret;
    };
    {
        let lookups = SECRET_LOOKUPS.read().await;
        match lookups.iter().find(|(url, _)| url == bundle_url) {
            Some((_, SecretLookup::Found(version, bytes))) => return (*version, bytes.clone()),
            Some((_, SecretLookup::Failed(at))) if at.elapsed() < SECRET_LOOKUP_RETRY => {
                return newest_known_secret(&lookups)
            }
            _ => {}
        }
    }

    let found = async { extract_totp_secret(&web_player_bundle(client, bundle_url).await?) }.await;
    let mut lookups = SECRET_LOOKUPS.write().await;
    lookups.retain(|(url, _)| url != bundle_url);
    let secret = match found {
        Ok((version, bytes)) => {
            let lookup = SecretLookup::Found(version, bytes.clone());
            lookups.push((bundle_url.to_string(), lookup));
            (version, bytes)
        }
        Err(e) => {
            lookups.push((bundle_url.to_string(), SecretLookup::Failed(Instant::now())));
            let secret = newest_known_secret(&lookups);
            tracing::warn!(
                "No TOTP secret from Spotify's web-player bundle {bundle_url}, using v{}: {e:#}",
                secret.0
            );
            secret
        }
    };
    if lookups.len() > MAX_SECRET_LOOKUPS {
        lookups.remove(0);
    }
    secret
}

/// Generates the current TOTP code Spotify expects on `open.spotify.com/api/token`, plus the
/// secret version it was generated with, for the web-player bundle at `bundle_url`.
async fn generate_totp(client: &Client, bundle_url: Option<&str>) -> anyhow::Result<(String, u64)> {
    let (version, secret) = get_totp_secret(client, bundle_url).await;
    let transformed: String = secret
        .iter()
        .enumerate()
        .map(|(t, &e)| (e ^ (((t % 33) as u8) + 9)).to_string())
        .collect();

    let totp = Builder::new()
        .with_algorithm(Algorithm::SHA1)
        .with_digits(6)
        .with_skew(1)
        .with_step_duration(30)
        .with_secret(transformed.into_bytes())
        .build()
        .context("could not construct TOTP")?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    Ok((totp.generate(now).to_string(), version))
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

/// Loads open.spotify.com, recording what the token requests need. Returns the URL of the
/// web-player bundle it uses.
async fn fetch_session(client: &Client, cookie: Option<&str>) -> anyhow::Result<Option<String>> {
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
    auth.js_pack_url = js_pack_url.clone();
    auth.client_version = Some(client_version);
    auth.device_id = device_id;
    Ok(js_pack_url)
}

async fn fetch_access_token(client: &Client, cookie: Option<&str>) -> anyhow::Result<()> {
    let bundle_url = fetch_session(client, cookie).await?;
    let (totp, version) = generate_totp(client, bundle_url.as_deref()).await?;

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
        None => fetch_session(client, None)
            .await?
            .context("could not locate web-player JS pack")?,
    };

    let mut raw_hashes = web_player_bundle(client, &js_pack_url).await?.to_string();

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
        .post("https://api-partner.spotify.com/pathfinder/v2/query")
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

/// Checks whether a track id exists on Spotify, via the public, unauthenticated oEmbed endpoint
/// (200 if the track exists, 404 otherwise). Much lighter than the pathfinder/TOTP machinery
/// above, which is only needed for search and session-authenticated mutations.
pub async fn track_exists(client: &Client, id: &str) -> anyhow::Result<bool> {
    let url = format!("https://open.spotify.com/oembed?url=https://open.spotify.com/track/{id}");
    let status = client.get(&url).send().await?.status();
    Ok(status.is_success())
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
/// doesn't persist a Spotify id) against Spotify's catalog, for playlist export and id backfill.
pub async fn match_track(
    client: &Client,
    artist: &str,
    title: &str,
    mix_name: Option<&str>,
) -> anyhow::Result<Option<SpTrack>> {
    let clean_artist = crate::utils::strip_artist_disambiguators(artist);
    let mix_name = mix_name.filter(|m| !crate::utils::is_original_mix(m));

    // The query sent to Spotify's own search should carry as little noise as possible — a
    // redundant "feat. X" clause or a Beatport-only "(US)"-style disambiguator tag empirically
    // buries the correct track outside the first few results entirely. A mix name is the same
    // risk in disguise: Spotify frequently doesn't carry it in the searchable text at all (its
    // own entry might just be the bare title), so appending it can bury the correct track just
    // as badly as "feat." does. Rather than guess, search both ways and pool the candidates.
    let clean_title = crate::utils::strip_feat(title);
    let base_query = format!("{clean_artist} {clean_title}");
    let mut candidates = search_tracks(client, &base_query, 5).await?;
    if let Some(mix) = mix_name {
        let mix_query = format!("{base_query} {mix}");
        candidates.extend(search_tracks(client, &mix_query, 5).await?);
    }

    // Scoring, on the other hand, should keep whatever real information the title carries —
    // a "feat. X" credit not already covered by the artists list is still useful signal once
    // we're just comparing candidates rather than driving Spotify's own relevance ranking.
    let target = match mix_name {
        Some(mix) => format!("{clean_artist} {title} {mix}"),
        None => format!("{clean_artist} {title}"),
    };
    let target = crate::utils::normalize_for_match(&target);

    let mut seen = std::collections::HashSet::new();
    Ok(candidates
        .into_iter()
        .filter(|t| seen.insert(t.uri.clone()))
        .map(|t| {
            let expected = format!(
                "{} {}",
                t.artists.join(" "),
                crate::utils::strip_generic_mix_suffix(&t.track_name)
            );
            (
                strsim::normalized_levenshtein(
                    &target,
                    &crate::utils::normalize_for_match(&expected),
                ),
                t,
            )
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
        let data = utils::read_secret_file(&path).await.ok()?;
        serde_json::from_slice(&data).ok()
    }

    pub async fn save(home: &std::path::Path, cookie: &str) -> anyhow::Result<()> {
        let path = home.join(SPOTIFY_SESSION_FILE);
        let data = serde_json::to_vec(&SpotifySession {
            cookie: cookie.to_string(),
        })?;
        utils::write_secret_file(&path, &data).await?;
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

        // Not Internet Explorer: rookie's reader can shut down processes holding its cookie
        // store, and the libesedb stub (Cargo.toml) fails to open it anyway.
        let browsers: &[(&str, BrowserCookieFn)] = &[
            ("Firefox", rookie::firefox),
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
            #[cfg(target_os = "macos")]
            ("Safari", rookie::safari),
            #[cfg(target_os = "linux")]
            ("Cachy", rookie::cachy),
        ];

        for &(name, browser_fn) in browsers {
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

/// Extracts a bare playlist id from a pasted `open.spotify.com/playlist/...` URL, a
/// `spotify:playlist:...` URI, or a bare id typed directly.
pub fn parse_playlist_id(input: &str) -> Option<String> {
    let input = input.trim();
    if let Some(id) = input.strip_prefix("spotify:playlist:") {
        return Some(id.to_string());
    }
    if let Some(rest) = input.split_once("open.spotify.com/playlist/") {
        return Some(rest.1.split(['?', '#', '/']).next()?.to_string());
    }
    (!input.is_empty() && input.chars().all(|c| c.is_ascii_alphanumeric()))
        .then(|| input.to_string())
}

pub struct PlaylistInfo {
    pub name: String,
}

/// Looks up a playlist's display name via Spotify's public oEmbed endpoint (anonymous, no
/// session needed) -- used to confirm the right playlist with the user before overwriting its
/// tracks.
pub async fn playlist_info(client: &Client, id: &str) -> anyhow::Result<PlaylistInfo> {
    let url = format!("https://open.spotify.com/oembed?url=https://open.spotify.com/playlist/{id}");
    let resp = client.get(&url).send().await?;
    anyhow::ensure!(resp.status().is_success(), "playlist not found");
    let body: Value = resp.json().await?;
    let name = body["title"]
        .as_str()
        .context("no title in oembed response")?
        .to_string();
    Ok(PlaylistInfo { name })
}

/// Fetches every item currently in `playlist_uri`, paginating `fetchPlaylistContents`, and
/// returns their per-item `uid`s -- the identifier `removeFromPlaylist` needs (not the track
/// URI, since a track can appear more than once in a playlist).
async fn fetch_playlist_uids(
    client: &Client,
    session: &SpotifySession,
    playlist_uri: &str,
) -> anyhow::Result<Vec<String>> {
    const PAGE_SIZE: i64 = 100;
    let mut uids = Vec::new();
    let mut offset: i64 = 0;
    loop {
        let variables = serde_json::json!({
            "uri": playlist_uri,
            "offset": offset,
            "limit": PAGE_SIZE,
            "includeEpisodeContentRatingsV2": true,
        });
        let resp = pathfinder_query(
            client,
            "fetchPlaylistContents",
            variables,
            Some(&session.cookie),
        )
        .await?;
        let content = &resp["data"]["playlistV2"]["content"];
        let items = content["items"]
            .as_array()
            .context("no content.items in fetchPlaylistContents response")?;
        if items.is_empty() {
            break;
        }
        uids.extend(
            items
                .iter()
                .filter_map(|item| item["uid"].as_str().map(String::from)),
        );
        offset += items.len() as i64;
        if offset >= content["totalCount"].as_i64().unwrap_or(0) {
            break;
        }
    }
    Ok(uids)
}

/// Removes every track currently in `playlist_uri`, leaving the (now-empty) playlist itself --
/// and crucially its id/URL -- intact, so an existing playlist can be overwritten in place
/// instead of replaced by a new one.
pub async fn clear_playlist(
    client: &Client,
    session: &SpotifySession,
    playlist_uri: &str,
) -> anyhow::Result<()> {
    let uids = fetch_playlist_uids(client, session, playlist_uri).await?;
    const CHUNK_SIZE: usize = 100;
    for chunk in uids.chunks(CHUNK_SIZE) {
        let variables = serde_json::json!({
            "playlistUri": playlist_uri,
            "uids": chunk,
        });
        pathfinder_query(
            client,
            "removeFromPlaylist",
            variables,
            Some(&session.cookie),
        )
        .await?;
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    // From web-player.f9aabf78.js (November 2026); v60's `\\` is one backslash.
    const BUNDLE_EXCERPT: &str = r#"var eU=r(84765),eM=r(185).hp;let ej=[{secret:',7/*F("rLJ2oxaKL^f+E1xvP@N',version:61},{secret:'OmE{ZA.J^":0FG\\Uz?[@WW',version:60},{secret:"{iOFn;4}<1PFYKPV?5{%u14]M>/V0hDH",version:59}].map(e=>{var t;let r,i;return{secret:(t=e.secret,"#;

    #[test]
    fn test_extract_totp_secret_takes_newest() {
        assert_eq!(
            extract_totp_secret(BUNDLE_EXCERPT).unwrap(),
            (FALLBACK_SECRET_VERSION, FALLBACK_SECRET.to_vec())
        );
    }

    #[test]
    fn test_extract_totp_secret_unescapes() {
        let v60 = BUNDLE_EXCERPT.replace("version:61", "version:1");
        let (version, bytes) = extract_totp_secret(&v60).unwrap();
        assert_eq!(version, 60);
        assert_eq!(bytes, br#"OmE{ZA.J^":0FG\Uz?[@WW"#.to_vec());
    }

    #[test]
    fn test_extract_totp_secret_rejects_unknown_escapes() {
        assert!(extract_totp_secret(r#"{secret:"ab\x41",version:62}"#).is_err());
        assert!(extract_totp_secret("let t={secret:e,version:n}").is_err());
    }

    #[test]
    fn test_extract_totp_secret_unreadable_newest_is_an_error() {
        let bundle = format!(r#"{{secret:"ab\x41",version:62}},{BUNDLE_EXCERPT}"#);
        let err = extract_totp_secret(&bundle).unwrap_err();
        assert!(err.to_string().contains("v62"), "{err}");
    }

    #[test]
    fn test_newest_known_secret() {
        assert_eq!(
            newest_known_secret(&[]),
            (FALLBACK_SECRET_VERSION, FALLBACK_SECRET.to_vec())
        );
        let lookups = [
            ("b".to_string(), SecretLookup::Found(62, b"new".to_vec())),
            ("c".to_string(), SecretLookup::Failed(Instant::now())),
            ("a".to_string(), SecretLookup::Found(61, b"old".to_vec())),
        ];
        assert_eq!(newest_known_secret(&lookups), (62, b"new".to_vec()));
    }

    /// Serves `responses` (status, content type, body) at a bundle URL in turn, the last one
    /// from then on. Returns the URL and the number of requests served.
    async fn serve_bundle(
        responses: Vec<(u16, &'static str, &'static str)>,
    ) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/web-player.js", listener.local_addr().unwrap());
        let served = Arc::new(AtomicUsize::new(0));
        let count = served.clone();
        let handler = move || {
            let n = count.fetch_add(1, Ordering::SeqCst);
            let (status, content_type, body) = responses[n.min(responses.len() - 1)];
            async move {
                (
                    axum::http::StatusCode::from_u16(status).unwrap(),
                    [(axum::http::header::CONTENT_TYPE, content_type)],
                    body,
                )
            }
        };
        let router = axum::Router::new().route("/web-player.js", axum::routing::get(handler));
        tokio::spawn(async move { axum::serve(listener, router).await });
        (url, served)
    }

    async fn expire_failed_lookup(bundle_url: &str) {
        for (url, lookup) in SECRET_LOOKUPS.write().await.iter_mut() {
            if url == bundle_url {
                let at = Instant::now().checked_sub(SECRET_LOOKUP_RETRY).unwrap();
                *lookup = SecretLookup::Failed(at);
            }
        }
    }

    // The only test that records lookups, since they're process-wide.
    #[tokio::test]
    async fn test_totp_secret_lookup_retries_failures_and_shares_the_bundle() {
        use std::sync::atomic::Ordering;
        let bundle = r#"let ej=[{secret:"abc",version:62}]"#;
        let (url, served) = serve_bundle(vec![
            (503, "text/plain", "unavailable"),
            (200, "text/html", "<html>error</html>"),
            (200, "text/javascript", bundle),
        ])
        .await;
        let client = crate::scraper::create_client().unwrap();
        let hardcoded = (FALLBACK_SECRET_VERSION, FALLBACK_SECRET.to_vec());

        // The 503, then no new request until the retry interval is up.
        assert_eq!(get_totp_secret(&client, Some(&url)).await, hardcoded);
        assert_eq!(get_totp_secret(&client, Some(&url)).await, hardcoded);
        assert_eq!(served.load(Ordering::SeqCst), 1);

        // The error page with a 200 isn't taken for the bundle.
        expire_failed_lookup(&url).await;
        assert_eq!(get_totp_secret(&client, Some(&url)).await, hardcoded);
        assert_eq!(served.load(Ordering::SeqCst), 2);

        expire_failed_lookup(&url).await;
        assert_eq!(
            get_totp_secret(&client, Some(&url)).await,
            (62, b"abc".to_vec())
        );
        assert_eq!(
            get_totp_secret(&client, Some(&url)).await,
            (62, b"abc".to_vec())
        );
        // The persisted-query hashes get the bundle the secret came from, without a download.
        assert_eq!(&*web_player_bundle(&client, &url).await.unwrap(), bundle);
        assert_eq!(served.load(Ordering::SeqCst), 3);
    }
}
