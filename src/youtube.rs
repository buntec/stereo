//! Playlist export to the user's YouTube account through YouTube's own web API ("InnerTube",
//! `/youtubei/v1/...`), which unlike the official Data API has no quota.
//!
//! Requests are signed in with the YouTube login of the user's browser, the way YouTube's web
//! client signs them: the login cookies, plus an `Authorization` header hashed from the
//! `SAPISID` cookies. Connecting only records which browser profile holds that login; the
//! cookies are read from it afresh for every export, since Google rotates them while the
//! browser is open and a saved copy would soon stop working.
//!
//! The API is undocumented and may change without notice.

use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use tokio::sync::Mutex;
use wreq::Client;

use crate::{
    browser_cookies::{self, browsers_for, Cookie, Store},
    utils,
};

const YOUTUBE_SESSION_FILE: &str = "youtube_session.json";
const ORIGIN: &str = "https://www.youtube.com";
const HOST: &str = "www.youtube.com";

/// Videos sent per request. YouTube accepts at least 200, but smaller batches report progress
/// more often.
pub const BATCH_SIZE: usize = 100;

/// The browser profile holding the user's YouTube login.
#[derive(Clone, Serialize, Deserialize)]
pub struct YouTubeSession {
    /// The browser, plus the profile when it has several; shown to the user.
    pub browser: String,
    store: Store,
}

/// Bumped by every connect and disconnect, so a connect still reading cookies when a disconnect
/// or a newer connect arrives gives up instead of overwriting it.
static GENERATION: Mutex<u64> = Mutex::const_new(0);

impl YouTubeSession {
    /// The connected session. A file saved by an older Stereo (for the official API) doesn't
    /// parse, and counts as not connected.
    pub async fn load(home: &Path) -> Option<YouTubeSession> {
        let data = utils::read_secret_file(&home.join(YOUTUBE_SESSION_FILE))
            .await
            .ok()?;
        serde_json::from_slice(&data).ok()
    }
}

/// The browser the YouTube login is read from, if connected.
pub async fn connected_browser(home: &Path) -> Option<String> {
    YouTubeSession::load(home).await.map(|s| s.browser)
}

/// Finds the YouTube login in the browser identified by `user_agent` and saves where it is.
/// Every profile of every browser that sends this User-Agent is looked at, and a profile is only
/// accepted once YouTube confirms its cookies are logged in. Fails if none is, i.e. the user
/// isn't logged in to youtube.com in this browser.
///
/// Returns the browser connected afterwards: normally the one just found, but a disconnect or
/// newer connect arriving meanwhile wins, and this one returns whatever state that left.
pub async fn connect(
    client: &Client,
    home: &Path,
    user_agent: String,
) -> anyhow::Result<Option<String>> {
    let browsers = browsers_for(&user_agent);
    anyhow::ensure!(
        !browsers.is_empty(),
        "Don't know how to read cookies from this browser ({user_agent})"
    );
    let generation = {
        let mut current = GENERATION.lock().await;
        *current += 1;
        *current
    };
    let names: Vec<&str> = browsers.iter().map(|b| b.name).collect();
    let mut logged_out = vec![];
    let mut unreadable = vec![];
    for browser in browsers {
        for profile in browser_cookies::profiles(browser) {
            if *GENERATION.lock().await != generation {
                return Ok(connected_browser(home).await);
            }
            let cookies = match read_login(profile.store.clone()).await? {
                Ok(Some(cookies)) => cookies,
                Ok(None) => continue,
                Err(e) => {
                    unreadable.push(format!("{}: {e}", profile.source));
                    continue;
                }
            };
            match Api::open(client, cookies).await {
                Ok(_) => {}
                Err(e) => {
                    tracing::info!("YouTube login in {} not accepted: {e:#}", profile.source);
                    logged_out.push(profile.source);
                    continue;
                }
            }
            let session = YouTubeSession {
                browser: profile.source,
                store: profile.store,
            };
            return commit(home, generation, session).await;
        }
    }
    let mut message = if logged_out.is_empty() {
        format!("No YouTube login found in {}.", names.join(" / "))
    } else {
        format!(
            "YouTube didn't accept the login found in {}.",
            logged_out.join(", ")
        )
    };
    message.push_str(" Log in to youtube.com in this browser, then connect again.");
    if !unreadable.is_empty() {
        message.push_str(&format!(
            " (Could not read cookies from {}.)",
            unreadable.join("; ")
        ));
    }
    anyhow::bail!(message)
}

pub async fn disconnect(home: &Path) -> anyhow::Result<()> {
    let mut generation = GENERATION.lock().await;
    *generation += 1;
    match tokio::fs::remove_file(home.join(YOUTUBE_SESSION_FILE)).await {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// Saves `session`, unless a disconnect or newer connect has superseded the connect that started
/// at `generation`. The check and the save happen under one lock, so a disconnect can't slip in
/// between.
async fn commit(
    home: &Path,
    generation: u64,
    session: YouTubeSession,
) -> anyhow::Result<Option<String>> {
    let current = GENERATION.lock().await;
    if *current != generation {
        return Ok(connected_browser(home).await);
    }
    utils::write_secret_file(
        &home.join(YOUTUBE_SESSION_FILE),
        &serde_json::to_vec(&session)?,
    )
    .await?;
    tracing::info!("Connected YouTube login from {}", session.browser);
    Ok(Some(session.browser))
}

/// The cookies `store` would send to YouTube, if they include a login (`None` if not), or why
/// they couldn't be read.
async fn read_login(store: Store) -> anyhow::Result<Result<Option<Vec<Cookie>>, String>> {
    Ok(tokio::task::spawn_blocking(move || {
        let cookies = browser_cookies::read(&store, "youtube.com")?;
        let cookies = browser_cookies::sent_to(cookies, HOST, "/", browser_cookies::now());
        Ok(cookies
            .iter()
            .any(|c| c.name == "SAPISID")
            .then_some(cookies))
    })
    .await?)
}

/// The parts of YouTube's web client config (`ytcfg`) its API expects.
struct ClientConfig {
    context: Value,
    session_index: String,
    visitor_data: Option<String>,
    client_name: Option<String>,
    client_version: Option<String>,
}

/// A logged-in connection to YouTube's API.
pub struct Api<'a> {
    client: &'a Client,
    cookie_header: String,
    /// The `SAPISID` cookies the `Authorization` header is hashed from, with their schemes.
    sapisids: Vec<(&'static str, String)>,
    config: ClientConfig,
}

impl<'a> Api<'a> {
    /// Signs in with the connected session's login, read from its browser now.
    pub async fn connected(client: &'a Client, home: &Path) -> anyhow::Result<Api<'a>> {
        let session = YouTubeSession::load(home)
            .await
            .context("Connect your YouTube account first (Settings → YouTube)")?;
        let cookies = read_login(session.store)
            .await?
            .map_err(|e| anyhow::anyhow!("Could not read cookies from {}: {e}", session.browser))?
            .with_context(|| {
                format!(
                    "Not logged in to YouTube in {} anymore: log in to youtube.com there and \
                     try again",
                    session.browser
                )
            })?;
        Api::open(client, cookies).await.with_context(|| {
            format!(
                "YouTube didn't accept the login in {}: log in to youtube.com there again",
                session.browser
            )
        })
    }

    /// Loads YouTube's client config with `cookies`, failing unless YouTube considers them
    /// logged in.
    async fn open(client: &'a Client, cookies: Vec<Cookie>) -> anyhow::Result<Api<'a>> {
        let cookie_header = browser_cookies::header(&cookies);
        let sapisids = [
            ("SAPISID", "SAPISIDHASH"),
            ("__Secure-1PAPISID", "SAPISID1PHASH"),
            ("__Secure-3PAPISID", "SAPISID3PHASH"),
        ]
        .into_iter()
        .filter_map(|(name, scheme)| {
            let cookie = cookies.iter().find(|c| c.name == name)?;
            Some((scheme, cookie.value.clone()))
        })
        .collect();
        let resp = client
            .get(ORIGIN)
            .header("Cookie", &cookie_header)
            .send()
            .await?;
        anyhow::ensure!(
            resp.status().is_success(),
            "loading YouTube failed ({})",
            resp.status()
        );
        let ytcfg =
            parse_ytcfg(&resp.text().await?).context("YouTube's client config not found")?;
        anyhow::ensure!(
            ytcfg["LOGGED_IN"] == json!(true),
            "YouTube doesn't consider these cookies logged in"
        );
        let config = ClientConfig {
            context: ytcfg["INNERTUBE_CONTEXT"].clone(),
            session_index: match &ytcfg["SESSION_INDEX"] {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => "0".to_string(),
            },
            visitor_data: ytcfg["VISITOR_DATA"].as_str().map(str::to_string),
            client_name: match &ytcfg["INNERTUBE_CONTEXT_CLIENT_NAME"] {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            },
            client_version: ytcfg["INNERTUBE_CONTEXT_CLIENT_VERSION"]
                .as_str()
                .map(str::to_string),
        };
        Ok(Api {
            client,
            cookie_header,
            sapisids,
            config,
        })
    }

    /// Creates a private playlist named `title` holding `video_ids`, returning its id. Ids of
    /// unavailable videos are skipped by YouTube rather than failing the request.
    pub async fn create_playlist(
        &self,
        title: &str,
        video_ids: &[String],
    ) -> anyhow::Result<String> {
        let data = self
            .post(
                "playlist/create",
                json!({ "title": title, "privacyStatus": "PRIVATE", "videoIds": video_ids }),
            )
            .await?;
        data["playlistId"]
            .as_str()
            .map(str::to_string)
            .context("no playlistId in YouTube's response")
    }

    /// Appends `video_ids` to the end of playlist `playlist_id`.
    pub async fn add_videos(&self, playlist_id: &str, video_ids: &[String]) -> anyhow::Result<()> {
        let actions: Vec<Value> = video_ids
            .iter()
            .map(|id| json!({ "action": "ACTION_ADD_VIDEO", "addedVideoId": id }))
            .collect();
        let data = self
            .post(
                "browse/edit_playlist",
                json!({ "playlistId": playlist_id, "actions": actions }),
            )
            .await?;
        anyhow::ensure!(
            data["status"] == json!("STATUS_SUCCEEDED"),
            "YouTube refused to add videos ({})",
            data["status"]
        );
        Ok(())
    }

    async fn post(&self, endpoint: &str, mut body: Value) -> anyhow::Result<Value> {
        body["context"] = self.config.context.clone();
        let mut req = self
            .client
            .post(format!("{ORIGIN}/youtubei/v1/{endpoint}?prettyPrint=false"))
            .header("Cookie", &self.cookie_header)
            .header("Authorization", self.authorization())
            .header("Origin", ORIGIN)
            .header("X-Origin", ORIGIN)
            .header("Referer", format!("{ORIGIN}/"))
            .header("X-Goog-AuthUser", &self.config.session_index)
            .json(&body);
        if let Some(v) = &self.config.visitor_data {
            req = req.header("X-Goog-Visitor-Id", v);
        }
        if let Some(v) = &self.config.client_name {
            req = req.header("X-Youtube-Client-Name", v);
        }
        if let Some(v) = &self.config.client_version {
            req = req.header("X-Youtube-Client-Version", v);
        }
        let resp = req.send().await?;
        let status = resp.status();
        let data: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            let message = data["error"]["message"].as_str().unwrap_or_default();
            anyhow::bail!("YouTube's {endpoint} failed ({status}) {message}");
        }
        Ok(data)
    }

    /// The `Authorization` header of YouTube's web client.
    fn authorization(&self) -> String {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        sapisid_authorization(&self.sapisids, timestamp)
    }
}

pub fn playlist_url(playlist_id: &str) -> String {
    format!("{ORIGIN}/playlist?list={playlist_id}")
}

/// `<scheme> <timestamp>_<sha1("<timestamp> <cookie> <origin>")>` for each `SAPISID` cookie.
fn sapisid_authorization(sapisids: &[(&str, String)], timestamp: u64) -> String {
    sapisids
        .iter()
        .map(|(scheme, value)| {
            let hash = Sha1::digest(format!("{timestamp} {value} {ORIGIN}"));
            let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
            format!("{scheme} {timestamp}_{hex}")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Merges the `ytcfg.set({...})` objects of a YouTube page; `None` without the API context.
fn parse_ytcfg(html: &str) -> Option<Value> {
    static RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(?s)ytcfg\.set\((\{.*?\})\);").unwrap());
    let mut config = serde_json::Map::new();
    for cap in RE.captures_iter(html) {
        if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(&cap[1]) {
            config.extend(obj);
        }
    }
    config
        .contains_key("INNERTUBE_CONTEXT")
        .then_some(Value::Object(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_like_youtubes_web_client() {
        let sapisids = [
            ("SAPISIDHASH", "abc".to_string()),
            ("SAPISID3PHASH", "abc".to_string()),
        ];
        // sha1("1700000000 abc https://www.youtube.com")
        let hash = "27b236f59d4ec583d7530f2c7055d2f9c6aecf92";
        assert_eq!(
            sapisid_authorization(&sapisids, 1_700_000_000),
            format!("SAPISIDHASH 1700000000_{hash} SAPISID3PHASH 1700000000_{hash}")
        );
    }

    #[test]
    fn merges_ytcfg_objects() {
        let html = r#"<script>ytcfg.set({"LOGGED_IN":true,"SESSION_INDEX":0});</script>
            <script>ytcfg.set({"INNERTUBE_CONTEXT":{"client":{"hl":"en"}}});</script>
            <script>ytcfg.set({broken});</script>"#;
        let config = parse_ytcfg(html).unwrap();
        assert_eq!(config["LOGGED_IN"], json!(true));
        assert_eq!(config["INNERTUBE_CONTEXT"]["client"]["hl"], json!("en"));
        assert!(parse_ytcfg(r#"ytcfg.set({"LOGGED_IN":true});"#).is_none());
    }

    #[tokio::test]
    async fn disconnect_supersedes_a_pending_connect() {
        let home = tempfile::tempdir().unwrap();
        let generation = {
            let mut current = GENERATION.lock().await;
            *current += 1;
            *current
        };
        disconnect(home.path()).await.unwrap();
        let session = YouTubeSession {
            browser: "Chrome".to_string(),
            store: Store::Safari,
        };
        assert_eq!(
            commit(home.path(), generation, session).await.unwrap(),
            None
        );
        assert!(!home.path().join(YOUTUBE_SESSION_FILE).exists());
    }

    #[tokio::test]
    async fn an_old_oauth_session_file_counts_as_disconnected() {
        let home = tempfile::tempdir().unwrap();
        utils::write_secret_file(
            &home.path().join(YOUTUBE_SESSION_FILE),
            br#"{"client_id":"x","client_secret":"y","access_token":null,"refresh_token":"z","expires_at":0}"#,
        )
        .await
        .unwrap();
        assert!(connected_browser(home.path()).await.is_none());
    }
}
