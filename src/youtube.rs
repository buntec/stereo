//! Playlist export to the user's YouTube account through YouTube's own web API ("InnerTube",
//! `/youtubei/v1/...`), which unlike the official Data API has no quota.
//!
//! Requests are signed in with the YouTube login of the user's browser, the way YouTube's web
//! client signs them: the login cookies, plus an `Authorization` header hashed from the
//! `SAPISID` cookies. Connecting only records which browser profile holds that login (the user
//! picks one when several are logged in); the cookies are read from it afresh for every export,
//! since Google rotates them while the browser is open and a saved copy would soon stop working.
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
    browser_cookies::{self, all_browsers, browsers_for, Cookie, Profile, Store},
    message::YouTubeLogin,
    utils,
};

const YOUTUBE_SESSION_FILE: &str = "youtube_session.json";
const ORIGIN: &str = "https://www.youtube.com";
const HOST: &str = "www.youtube.com";

/// The cookies holding a login's `SAPISID`, one of which [`sapisids`] needs; whether they're
/// still logged in is up to YouTube.
const LOGIN_COOKIES: [&str; 3] = ["SAPISID", "__Secure-3PAPISID", "__Secure-1PAPISID"];

/// Videos sent per request. YouTube accepts at least 200, but smaller batches report progress
/// more often.
pub const BATCH_SIZE: usize = 100;

/// The browser profile holding the user's YouTube login.
#[derive(Clone, Serialize, Deserialize)]
pub struct YouTubeSession {
    /// The browser, plus the profile when it has several; shown to the user.
    pub browser: String,
    /// The YouTube account or channel logged in there when connecting, as YouTube names it.
    #[serde(default)]
    pub account: Option<String>,
    store: Store,
}

/// The outcome of [`connect`].
pub enum Connected {
    /// The session connected afterwards: normally the one just found, but a disconnect or newer
    /// connect arriving meanwhile wins, and this is whatever state that left.
    Session(Option<YouTubeSession>),
    /// Several profiles are logged in to YouTube, so the user has to say which to use.
    Choose(Vec<YouTubeLogin>),
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

/// Finds the YouTube login in the browser identified by `user_agent` (in any browser without one)
/// and saves where it is. Every profile of every browser that sends this User-Agent is looked at (only the one with id
/// `login` if given, as picked from an earlier [`Connected::Choose`]), and a profile only counts
/// once YouTube confirms its cookies are logged in. A single such profile is connected; several
/// are returned for the user to choose from, since neither the User-Agent nor the order of the
/// profiles tells which one the user means. Fails if none is logged in.
pub async fn connect(
    client: &Client,
    home: &Path,
    user_agent: Option<String>,
    login: Option<String>,
) -> anyhow::Result<Connected> {
    let browsers = match &user_agent {
        Some(user_agent) => {
            let browsers = browsers_for(user_agent);
            anyhow::ensure!(
                !browsers.is_empty(),
                "Don't know how to read cookies from this browser ({user_agent})"
            );
            browsers
        }
        None => all_browsers(),
    };
    let generation = {
        let mut current = GENERATION.lock().await;
        *current += 1;
        *current
    };
    let names: Vec<&str> = browsers.iter().map(|b| b.name).collect();
    let mut found: Vec<(Profile, Option<String>)> = vec![];
    let mut logged_out = vec![];
    let mut unreadable = vec![];
    let mut matched = false;
    for browser in browsers {
        for profile in browser_cookies::profiles(browser) {
            if login.as_ref().is_some_and(|id| *id != profile.store.id()) {
                continue;
            }
            matched = true;
            if *GENERATION.lock().await != generation {
                return Ok(Connected::Session(YouTubeSession::load(home).await));
            }
            let cookies = match read_login(profile.store.clone()).await? {
                Ok(Some(cookies)) => cookies,
                Ok(None) => continue,
                Err(e) => {
                    unreadable.push(format!("{}: {e}", profile.source));
                    continue;
                }
            };
            let api = match Api::open(client, cookies).await {
                Ok(api) => api,
                Err(e) => {
                    tracing::info!("YouTube login in {} not accepted: {e:#}", profile.source);
                    logged_out.push(profile.source);
                    continue;
                }
            };
            let account = api.account().await;
            found.push((profile, account));
        }
    }
    if found.len() > 1 {
        let logins = found
            .into_iter()
            .map(|(profile, account)| YouTubeLogin {
                id: profile.store.id(),
                browser: profile.source,
                account,
            })
            .collect();
        return Ok(Connected::Choose(logins));
    }
    if let Some((profile, account)) = found.pop() {
        let session = YouTubeSession {
            browser: profile.source,
            account,
            store: profile.store,
        };
        return Ok(Connected::Session(commit(home, generation, session).await?));
    }
    anyhow::ensure!(
        matched,
        "That browser profile isn't there anymore. Connect again to pick another."
    );
    let mut message = if logged_out.is_empty() {
        format!("No YouTube login found in {}.", names.join(" / "))
    } else {
        format!(
            "YouTube didn't accept the login found in {}.",
            logged_out.join(", ")
        )
    };
    message.push_str(if user_agent.is_some() {
        " Log in to youtube.com in this browser, then connect again."
    } else {
        " Log in to youtube.com in your browser, then connect again."
    });
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
) -> anyhow::Result<Option<YouTubeSession>> {
    let current = GENERATION.lock().await;
    if *current != generation {
        return Ok(YouTubeSession::load(home).await);
    }
    utils::write_secret_file(
        &home.join(YOUTUBE_SESSION_FILE),
        &serde_json::to_vec(&session)?,
    )
    .await?;
    tracing::info!("Connected YouTube login from {}", session.browser);
    Ok(Some(session))
}

/// The cookies `store` would send to YouTube, if they include a login (`None` if not), or why
/// they couldn't be read.
async fn read_login(store: Store) -> anyhow::Result<Result<Option<Vec<Cookie>>, String>> {
    Ok(tokio::task::spawn_blocking(move || {
        let cookies = browser_cookies::read(&store, "youtube.com")?;
        let cookies = browser_cookies::sent_to(cookies, HOST, "/", browser_cookies::now());
        Ok(cookies
            .iter()
            .any(|c| LOGIN_COOKIES.contains(&c.name.as_str()))
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
    /// The brand channel the login is acting as, if not the Google account's own channel.
    delegated_session_id: Option<String>,
}

impl ClientConfig {
    fn from_ytcfg(ytcfg: &Value) -> ClientConfig {
        ClientConfig {
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
            delegated_session_id: delegated_session_id(ytcfg),
        }
    }
}

/// The brand channel a login acts as, the way yt-dlp reads it: `DELEGATED_SESSION_ID`, or else
/// the first half of `DATASYNC_ID`, which is `<delegated>||<user>` when acting as a brand channel
/// and `<user>||` otherwise.
fn delegated_session_id(ytcfg: &Value) -> Option<String> {
    if let Some(id) = ytcfg["DELEGATED_SESSION_ID"].as_str() {
        return (!id.is_empty()).then(|| id.to_string());
    }
    let (delegated, user) = ytcfg["DATASYNC_ID"].as_str()?.split_once("||")?;
    (!delegated.is_empty() && !user.is_empty()).then(|| delegated.to_string())
}

/// A logged-in connection to YouTube's API.
pub struct Api<'a> {
    client: &'a Client,
    cookie_header: String,
    /// The `SAPISID` cookies the `Authorization` header is hashed from, by their schemes.
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
        let sapisids = sapisids(&cookies);
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
        Ok(Api {
            client,
            cookie_header,
            sapisids,
            config: ClientConfig::from_ytcfg(&ytcfg),
        })
    }

    /// The account or channel the login acts as, as YouTube's account switcher names it. Only
    /// shown to the user, so `None` if YouTube doesn't say.
    async fn account(&self) -> Option<String> {
        let body = json!({
            "requestType": "ACCOUNTS_LIST_REQUEST_TYPE_CHANNEL_SWITCHER",
            "callCircumstance": "SWITCHING_USERS_FULL",
        });
        match self.post("account/accounts_list", body).await {
            Ok(data) => selected_account(&data),
            Err(e) => {
                tracing::info!("Could not tell which YouTube account is logged in: {e:#}");
                None
            }
        }
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

    /// Removes every video from playlist `playlist_id`, keeping the playlist (and its URL).
    pub async fn clear_playlist(&self, playlist_id: &str) -> anyhow::Result<()> {
        let set_video_ids = self.playlist_set_video_ids(playlist_id).await?;
        for batch in set_video_ids.chunks(BATCH_SIZE) {
            let actions: Vec<Value> = batch
                .iter()
                .map(|id| json!({ "action": "ACTION_REMOVE_VIDEO", "setVideoId": id }))
                .collect();
            let data = self
                .post(
                    "browse/edit_playlist",
                    json!({ "playlistId": playlist_id, "actions": actions }),
                )
                .await?;
            anyhow::ensure!(
                data["status"] == json!("STATUS_SUCCEEDED"),
                "YouTube refused to remove videos ({})",
                data["status"]
            );
        }
        Ok(())
    }

    /// The `setVideoId`s of a playlist's entries (which identify an entry, unlike its video id,
    /// since a video may be in it twice), read through the signed-in browse endpoint so that
    /// private playlists work.
    async fn playlist_set_video_ids(&self, playlist_id: &str) -> anyhow::Result<Vec<String>> {
        let mut page = self
            .post("browse", json!({ "browseId": format!("VL{playlist_id}") }))
            .await?;
        anyhow::ensure!(
            !find_error_alert(&page),
            "YouTube playlist {playlist_id} not found; unlink it to create a new one"
        );
        let mut ids: Vec<String> = vec![];
        let mut seen = std::collections::HashSet::new();
        // Bounded, so that a changed response format can't page forever (5000 videos at 100 a
        // page is 50 pages).
        for _ in 0..60 {
            let mut found = vec![];
            find_all(&page, "setVideoId", &mut found);
            ids.extend(
                found
                    .into_iter()
                    .filter_map(Value::as_str)
                    .filter(|id| seen.insert(id.to_string()))
                    .map(str::to_string),
            );
            let mut tokens = vec![];
            find_all(&page, "continuationCommand", &mut tokens);
            let Some(token) = tokens.iter().find_map(|c| c["token"].as_str()) else {
                break;
            };
            page = self
                .post("browse", json!({ "continuation": token }))
                .await?;
        }
        Ok(ids)
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
            .header("X-Youtube-Bootstrap-Logged-In", "true")
            .json(&body);
        // Without this, requests act as the Google account's own channel rather than the brand
        // channel selected on youtube.com.
        if let Some(v) = &self.config.delegated_session_id {
            req = req.header("X-Goog-PageId", v);
        }
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

/// The `SAPISID` cookies of a login by the `Authorization` scheme each is hashed for. Logins may
/// lack the plain `SAPISID`, which YouTube's web client then takes from `__Secure-3PAPISID`.
fn sapisids(cookies: &[Cookie]) -> Vec<(&'static str, String)> {
    let value = |name: &str| {
        cookies
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.value.clone())
    };
    [
        (
            "SAPISIDHASH",
            value("SAPISID").or_else(|| value("__Secure-3PAPISID")),
        ),
        ("SAPISID1PHASH", value("__Secure-1PAPISID")),
        ("SAPISID3PHASH", value("__Secure-3PAPISID")),
    ]
    .into_iter()
    .filter_map(|(scheme, value)| Some((scheme, value?)))
    .collect()
}

/// The selected entry of an `account/accounts_list` response, as "<name> (<handle>)".
fn selected_account(data: &Value) -> Option<String> {
    let mut items = vec![];
    find_all(data, "accountItem", &mut items);
    let item = match items.iter().find(|item| item["isSelected"] == json!(true)) {
        Some(item) => item,
        None if items.len() == 1 => items[0],
        None => return None,
    };
    let name = text(&item["accountName"])?;
    match text(&item["channelHandle"]).or_else(|| text(&item["accountByline"])) {
        Some(detail) if detail != name => Some(format!("{name} ({detail})")),
        _ => Some(name),
    }
}

/// Every value under `key` anywhere in `value`.
fn find_all<'v>(value: &'v Value, key: &str, found: &mut Vec<&'v Value>) {
    match value {
        Value::Object(obj) => {
            for (k, v) in obj {
                if k == key {
                    found.push(v);
                }
                find_all(v, key, found);
            }
        }
        Value::Array(values) => values.iter().for_each(|v| find_all(v, key, found)),
        _ => {}
    }
}

/// Whether a browse response carries an error alert (e.g. "The playlist does not exist.").
fn find_error_alert(data: &Value) -> bool {
    let mut alerts = vec![];
    find_all(data, "alertRenderer", &mut alerts);
    alerts.iter().any(|a| a["type"] == json!("ERROR"))
}

/// The text of one of YouTube's text objects (`simpleText`, or `runs` of text).
fn text(value: &Value) -> Option<String> {
    let text = match value["simpleText"].as_str() {
        Some(text) => text.to_string(),
        None => value["runs"]
            .as_array()?
            .iter()
            .filter_map(|run| run["text"].as_str())
            .collect(),
    };
    (!text.is_empty()).then_some(text)
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

    fn cookie(name: &str, value: &str) -> Cookie {
        Cookie {
            domain: ".youtube.com".to_string(),
            path: "/".to_string(),
            secure: true,
            expires: None,
            name: name.to_string(),
            value: value.to_string(),
            http_only: false,
            same_site: 0,
        }
    }

    #[test]
    fn signs_with_the_secure_sapisid_when_the_plain_one_is_missing() {
        let cookies = [
            cookie("__Secure-3PAPISID", "three"),
            cookie("LOGIN_INFO", "x"),
        ];
        assert!(cookies
            .iter()
            .any(|c| LOGIN_COOKIES.contains(&c.name.as_str())));
        assert_eq!(
            sapisids(&cookies),
            [
                ("SAPISIDHASH", "three".to_string()),
                ("SAPISID3PHASH", "three".to_string())
            ]
        );
        let cookies = [
            cookie("SAPISID", "plain"),
            cookie("__Secure-3PAPISID", "three"),
        ];
        assert_eq!(sapisids(&cookies)[0], ("SAPISIDHASH", "plain".to_string()));
    }

    #[test]
    fn keeps_the_brand_channel_a_login_acts_as() {
        let brand = ClientConfig::from_ytcfg(&json!({
            "SESSION_INDEX": 0,
            "DELEGATED_SESSION_ID": "brand",
            "DATASYNC_ID": "brand||user",
        }));
        assert_eq!(brand.session_index, "0");
        assert_eq!(brand.delegated_session_id.as_deref(), Some("brand"));
        let from_datasync = json!({ "DATASYNC_ID": "brand||user" });
        assert_eq!(
            delegated_session_id(&from_datasync).as_deref(),
            Some("brand")
        );
        assert_eq!(
            delegated_session_id(&json!({ "DATASYNC_ID": "user||" })),
            None
        );
        assert_eq!(delegated_session_id(&json!({})), None);
    }

    #[test]
    fn names_the_selected_account() {
        let item = |name: &str, handle: &str, selected: bool| {
            json!({ "accountItem": {
                "accountName": { "simpleText": name },
                "channelHandle": { "runs": [{ "text": handle }] },
                "isSelected": selected,
            }})
        };
        let data = json!({ "actions": [{ "contents": [
            item("Me", "@me", false),
            item("My Label", "@label", true),
        ]}]});
        assert_eq!(
            selected_account(&data).as_deref(),
            Some("My Label (@label)")
        );
        assert_eq!(selected_account(&json!({})), None);
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
            account: None,
            store: Store::Safari,
        };
        assert!(commit(home.path(), generation, session)
            .await
            .unwrap()
            .is_none());
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
        assert!(YouTubeSession::load(home.path()).await.is_none());
    }
}
