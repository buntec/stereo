//! Beatport's Cloudflare clearance, imported from the user's browser.
//!
//! Beatport puts its search pages (`/search/...`) behind a Cloudflare bot check that an HTTP
//! client can't pass on its own; chart, catalog and track pages are not checked. Solving the
//! check in a real browser leaves a `cf_clearance` cookie there, which lets Stereo's searches
//! through too, but only when sent with the exact User-Agent of the browser that earned it.
//! Both are therefore captured together: the User-Agent comes from the Stereo tab that asks to
//! connect, and the cookie from the cookie store of that same browser. Which store that is can't
//! be told for sure (Chromium-based browsers share Chrome's User-Agent, and each profile has its
//! own store), so each candidate clearance is tried with a Beatport search before one is kept.

use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
    time::{SystemTime, UNIX_EPOCH},
};

use rookie::enums::Cookie;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};
use wreq::{Client, RequestBuilder};

use crate::{scraper, utils};

const BEATPORT_SESSION_FILE: &str = "beatport_session.json";

/// The search page a clearance is checked against. Its cookies are the ones that matter, since
/// only Beatport's search is behind the bot check.
pub const SEARCH_URL: &str = "https://www.beatport.com/search/tracks";
const SEARCH_HOST: &str = "www.beatport.com";
const SEARCH_PATH: &str = "/search/tracks";

/// Only Cloudflare's own cookies are kept: they are all the bot check needs, and leaving out the
/// rest means a Beatport login in that browser is never shared with Stereo.
const CLOUDFLARE_COOKIES: &[&str] = &["cf_clearance", "__cf_bm", "_cfuvid"];

#[derive(Clone, Serialize, Deserialize)]
pub struct BeatportSession {
    /// `Cookie` header value holding the Cloudflare cookies.
    pub cookie: String,
    /// User-Agent of the browser the cookies were read from; Cloudflare rejects the clearance
    /// with any other.
    pub user_agent: String,
    /// Name of that browser, shown to the user.
    pub browser: String,
}

/// The connected session, shared by every WebSocket connection since Beatport requests are made
/// on behalf of the one local user. Loaded at startup by [`init`].
static SESSION: LazyLock<RwLock<Option<BeatportSession>>> = LazyLock::new(|| RwLock::new(None));

/// Bumped by every connect and disconnect. A connect only commits its session while the
/// generation is still the one it started with, so a slow cookie import can't undo a later
/// disconnect or overwrite a later connect.
static GENERATION: Mutex<u64> = Mutex::const_new(0);

/// Loads the saved session, if any, so Beatport requests use it from the start.
pub async fn init(home: &Path) {
    let session = match utils::read_secret_file(&home.join(BEATPORT_SESSION_FILE)).await {
        Ok(data) => serde_json::from_slice(&data).ok(),
        Err(_) => None,
    };
    *SESSION.write().await = session;
}

/// The browser the current session was imported from, if connected.
pub async fn connected_browser() -> Option<String> {
    SESSION.read().await.as_ref().map(|s| s.browser.clone())
}

/// Attaches the session's cookie and User-Agent to a Beatport request, if connected.
pub async fn authorize(req: RequestBuilder) -> RequestBuilder {
    match SESSION.read().await.as_ref() {
        Some(session) => req
            .header("Cookie", &session.cookie)
            .header("User-Agent", &session.user_agent),
        None => req,
    }
}

/// Imports a Cloudflare clearance from the browser identified by `user_agent` and saves it
/// together with that User-Agent. Every profile of every browser that sends this User-Agent is
/// looked at, and a clearance is only accepted once a Beatport search with it gets through: a
/// clearance earned in another browser, browser version or profile is useless with this
/// User-Agent, so merely finding one proves nothing. Fails if none works, i.e. the user hasn't
/// run a search on beatport.com in this browser recently.
///
/// Returns the browser connected afterwards. That is normally the one just imported, but a
/// disconnect or newer connect arriving while this one was still reading cookies wins, and this
/// one then returns whatever state that left instead of overwriting it.
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
    let mut rejected = vec![];
    let mut unreadable = vec![];
    for browser in browsers {
        if !is_current(generation).await {
            return Ok(connected_browser().await);
        }
        let (clearances, errors) =
            tokio::task::spawn_blocking(move || read_clearances(browser)).await?;
        unreadable.extend(errors);
        for clearance in clearances {
            if !is_current(generation).await {
                return Ok(connected_browser().await);
            }
            if !scraper::bp_clearance_works(client, &clearance.cookie, &user_agent).await? {
                rejected.push(clearance.source);
                continue;
            }
            let session = BeatportSession {
                cookie: clearance.cookie,
                user_agent,
                browser: clearance.source,
            };
            return commit(home, generation, session).await;
        }
    }
    let mut message = if rejected.is_empty() {
        format!(
            "No current Beatport clearance found in {}.",
            names.join(" / ")
        )
    } else {
        format!(
            "Beatport rejected the clearance found in {}, probably because it was earned in a \
             different browser or browser version.",
            rejected.join(", ")
        )
    };
    message
        .push_str(" Open beatport.com and run a search there in this browser, then connect again.");
    if rejected.is_empty() && !unreadable.is_empty() {
        message.push_str(&format!(
            " (Could not read cookies from {}.)",
            unreadable.join("; ")
        ));
    }
    anyhow::bail!(message)
}

pub async fn disconnect(home: &Path) -> anyhow::Result<()> {
    // Bumping the generation makes any connect still in flight give up instead of committing.
    let mut generation = GENERATION.lock().await;
    *generation += 1;
    *SESSION.write().await = None;
    match tokio::fs::remove_file(home.join(BEATPORT_SESSION_FILE)).await {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

async fn is_current(generation: u64) -> bool {
    *GENERATION.lock().await == generation
}

/// Saves and activates `session`, unless a disconnect or newer connect has superseded the
/// connect that started at `generation`. The check and the commit happen under one lock, so a
/// disconnect can't slip in between.
async fn commit(
    home: &Path,
    generation: u64,
    session: BeatportSession,
) -> anyhow::Result<Option<String>> {
    let current = GENERATION.lock().await;
    if *current != generation {
        return Ok(connected_browser().await);
    }
    utils::write_secret_file(
        &home.join(BEATPORT_SESSION_FILE),
        &serde_json::to_vec(&session)?,
    )
    .await?;
    tracing::info!("Connected Beatport session from {}", session.browser);
    let browser = session.browser.clone();
    *SESSION.write().await = Some(session);
    Ok(Some(browser))
}

/// A browser Stereo can read cookies from, by its name in rookie's browser config.
#[derive(Clone, Copy)]
struct Browser {
    /// Shown to the user.
    name: &'static str,
    engine: Engine,
}

#[derive(Clone, Copy)]
enum Engine {
    Chromium(&'static str),
    Firefox(&'static str),
    #[cfg(target_os = "macos")]
    Safari,
}

const fn chromium(name: &'static str, config: &'static str) -> Browser {
    Browser {
        name,
        engine: Engine::Chromium(config),
    }
}

const fn firefox(name: &'static str, config: &'static str) -> Browser {
    Browser {
        name,
        engine: Engine::Firefox(config),
    }
}

/// The browsers whose User-Agent looks like `user_agent`. Several Chromium-based browsers
/// (Brave, Arc, Vivaldi) send exactly Chrome's User-Agent, so all of them are candidates.
fn browsers_for(user_agent: &str) -> Vec<Browser> {
    if user_agent.contains("Firefox/") {
        vec![
            firefox("Firefox", "firefox"),
            firefox("Zen", "zen"),
            firefox("LibreWolf", "librewolf"),
        ]
    } else if user_agent.contains("Edg/") {
        vec![chromium("Edge", "edge")]
    } else if user_agent.contains("OPR/") {
        vec![chromium("Opera", "opera"), chromium("Opera GX", "opera_gx")]
    } else if user_agent.contains("Chrome/") {
        vec![
            chromium("Chrome", "chrome"),
            chromium("Brave", "brave"),
            chromium("Arc", "arc"),
            chromium("Vivaldi", "vivaldi"),
            chromium("Chromium", "chromium"),
        ]
    } else if user_agent.contains("Safari/") {
        #[cfg(target_os = "macos")]
        return vec![Browser {
            name: "Safari",
            engine: Engine::Safari,
        }];
        #[cfg(not(target_os = "macos"))]
        vec![]
    } else {
        vec![]
    }
}

/// The cookies read from one browser profile, or why they couldn't be.
type CookieRead = Result<Vec<Cookie>, String>;

/// A clearance found in one browser profile.
struct Clearance {
    /// The browser, plus the profile when the browser has several.
    source: String,
    /// `Cookie` header with the Cloudflare cookies that apply to Beatport's search.
    cookie: String,
}

/// The clearances in all of `browser`'s profiles, freshest first since the one the user just
/// earned is the likeliest to work, plus a description of each profile that couldn't be read.
fn read_clearances(browser: Browser) -> (Vec<Clearance>, Vec<String>) {
    let domains = || Some(vec!["beatport.com".to_string()]);
    let stores: Vec<(Option<String>, CookieRead)> = match browser.engine {
        Engine::Chromium(config) => existing_paths(config)
            .into_iter()
            .filter(|db| db.is_file())
            .map(|db| {
                (
                    chromium_profile_name(&db),
                    read_chromium(config, db, domains()),
                )
            })
            .collect(),
        Engine::Firefox(config) => firefox_cookie_dbs(config)
            .into_iter()
            .map(|db| {
                // Profile directories are named "<random>.<profile name>".
                let profile = db.parent().and_then(|dir| dir.file_name()).map(|dir| {
                    let dir = dir.to_string_lossy();
                    dir.split_once('.')
                        .map_or(&*dir, |(_, name)| name)
                        .to_string()
                });
                let cookies = rookie::firefox_based(db, domains()).map_err(|e| e.to_string());
                (profile, cookies)
            })
            .collect(),
        #[cfg(target_os = "macos")]
        Engine::Safari => vec![(None, rookie::safari(domains()).map_err(|e| e.to_string()))],
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let several = stores.len() > 1;
    let mut clearances = vec![];
    let mut errors = vec![];
    for (profile, cookies) in stores {
        let source = match profile {
            Some(profile) if several => format!("{} ({profile})", browser.name),
            _ => browser.name.to_string(),
        };
        match cookies {
            Ok(cookies) => {
                if let Some((cookie, expires)) = search_clearance(cookies, now) {
                    clearances.push((expires, Clearance { source, cookie }));
                }
            }
            Err(e) => errors.push(format!("{source}: {e}")),
        }
    }
    clearances.sort_by_key(|(expires, _)| std::cmp::Reverse(*expires));
    (clearances.into_iter().map(|(_, c)| c).collect(), errors)
}

/// The `Cookie` header a browser holding `cookies` would send to Beatport's search, limited to
/// Cloudflare's unexpired cookies, together with the clearance's expiry; `None` without a
/// clearance among them. rookie's domain filter is a substring match, so `cookies` can also hold
/// cookies of other Beatport hosts and of unrelated domains merely containing "beatport.com",
/// which are left out the way the browser would. Of several applicable cookies with the same
/// name, only the most specific one is kept, so the header never offers Cloudflare a choice.
fn search_clearance(cookies: Vec<Cookie>, now: u64) -> Option<(String, u64)> {
    let specificity = |c: &Cookie| {
        (
            c.path.len(),
            c.domain.trim_start_matches('.').len(),
            c.expires.unwrap_or(u64::MAX),
        )
    };
    let mut chosen: Vec<Cookie> = vec![];
    for cookie in cookies {
        if !CLOUDFLARE_COOKIES.contains(&cookie.name.as_str())
            || cookie.expires.is_some_and(|e| e <= now)
            || !applies_to_search(&cookie)
        {
            continue;
        }
        match chosen.iter_mut().find(|c| c.name == cookie.name) {
            Some(c) if specificity(&cookie) > specificity(c) => *c = cookie,
            Some(_) => {}
            None => chosen.push(cookie),
        }
    }
    let clearance = chosen.iter().find(|c| c.name == "cf_clearance")?;
    let expires = clearance.expires.unwrap_or(u64::MAX);
    let header = chosen
        .iter()
        .map(|c| format!("{}={}", c.name, c.value))
        .collect::<Vec<_>>()
        .join("; ");
    Some((header, expires))
}

/// Whether a browser sends `cookie` to Beatport's search, by RFC 6265 domain and path matching.
/// A domain with a leading dot covers its subdomains; one without is host-only, as browsers
/// store them.
fn applies_to_search(cookie: &Cookie) -> bool {
    let domain = cookie.domain.to_ascii_lowercase();
    let domain_matches = match domain.strip_prefix('.') {
        Some(parent) => {
            SEARCH_HOST == parent
                || SEARCH_HOST
                    .strip_suffix(parent)
                    .is_some_and(|sub| sub.ends_with('.'))
        }
        None => SEARCH_HOST == domain,
    };
    let path = if cookie.path.is_empty() {
        "/"
    } else {
        cookie.path.as_str()
    };
    let path_matches = SEARCH_PATH
        .strip_prefix(path)
        .is_some_and(|rest| rest.is_empty() || path.ends_with('/') || rest.starts_with('/'));
    domain_matches && path_matches
}

/// The existing files or directories matching rookie's path patterns for browser `config`.
/// rookie itself only reads the first, which for Chromium browsers is the `Default` profile even
/// when the user browses in another one, so profiles are enumerated here instead.
fn existing_paths(config: &str) -> Vec<PathBuf> {
    let config = rookie::config::get_browser_config(config);
    let channels = config
        .channels
        .clone()
        .unwrap_or_else(|| vec![String::new()]);
    let mut paths = vec![];
    for pattern in &config.paths {
        for channel in &channels {
            let Some(pattern) = expand_path(&pattern.replace("{channel}", channel)) else {
                continue;
            };
            let Ok(matches) = glob::glob(&pattern) else {
                continue;
            };
            for path in matches.flatten() {
                if path.exists() && !paths.contains(&path) {
                    paths.push(path);
                }
            }
        }
    }
    paths
}

/// The cookie databases of all profiles of the Firefox-based browser `config`.
fn firefox_cookie_dbs(config: &str) -> Vec<PathBuf> {
    let mut dbs = vec![];
    for dir in existing_paths(config) {
        let dir = glob::Pattern::escape(&dir.to_string_lossy());
        for pattern in [
            format!("{dir}/*/cookies.sqlite"),
            format!("{dir}/Profiles/*/cookies.sqlite"),
        ] {
            let Ok(matches) = glob::glob(&pattern) else {
                continue;
            };
            for db in matches.flatten() {
                if db.is_file() && !dbs.contains(&db) {
                    dbs.push(db);
                }
            }
        }
    }
    dbs
}

/// Expands the home directory placeholders in rookie's path patterns, escaping their values so
/// they match literally.
#[cfg(unix)]
fn expand_path(pattern: &str) -> Option<String> {
    let home = glob::Pattern::escape(&std::env::var("HOME").ok()?);
    Some(pattern.replace('~', &home).replace("$HOME", &home))
}

#[cfg(windows)]
fn expand_path(pattern: &str) -> Option<String> {
    let mut pattern = pattern.to_string();
    for var in ["APPDATA", "LOCALAPPDATA"] {
        let placeholder = format!("%{var}%");
        if pattern.contains(&placeholder) {
            let value = glob::Pattern::escape(&std::env::var(var).ok()?);
            pattern = pattern.replace(&placeholder, &value);
        }
    }
    Some(pattern)
}

/// The `Local State` file of the Chromium user data dir holding cookie database `db`, found the
/// way rookie finds it.
fn local_state(db: &Path) -> Option<PathBuf> {
    let dir = db.parent()?;
    ["../../Local State", "../Local State", "Local State"]
        .iter()
        .map(|p| dir.join(p))
        .find(|p| p.is_file())
}

/// The name the user gave the Chromium profile holding cookie database `db`, falling back to the
/// profile's directory name.
fn chromium_profile_name(db: &Path) -> Option<String> {
    let mut dir = db.parent()?;
    if dir.file_name().is_some_and(|name| name == "Network") {
        dir = dir.parent()?;
    }
    let dir_name = dir.file_name()?.to_str()?;
    let name = local_state(db)
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|data| serde_json::from_slice::<serde_json::Value>(&data).ok())
        .and_then(|state| {
            state["profile"]["info_cache"][dir_name]["name"]
                .as_str()
                .map(str::to_string)
        });
    Some(name.unwrap_or_else(|| dir_name.to_string()))
}

#[cfg(unix)]
fn read_chromium(config: &str, db: PathBuf, domains: Option<Vec<String>>) -> CookieRead {
    rookie::chromium_based(rookie::config::get_browser_config(config), db, domains)
        .map_err(|e| e.to_string())
}

#[cfg(windows)]
fn read_chromium(_config: &str, db: PathBuf, domains: Option<Vec<String>>) -> CookieRead {
    let key = local_state(&db).ok_or("no Local State file found next to its cookies")?;
    rookie::chromium_based(key, db, domains).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(user_agent: &str) -> Vec<&'static str> {
        browsers_for(user_agent)
            .into_iter()
            .map(|b| b.name)
            .collect()
    }

    #[test]
    fn picks_browsers_by_user_agent() {
        let chrome = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
                      (KHTML, like Gecko) Chrome/157.0.0.0 Safari/537.36";
        assert_eq!(names(chrome)[0], "Chrome");
        assert_eq!(names(&format!("{chrome} Edg/157.0.0.0")), ["Edge"]);
        assert_eq!(
            names("Mozilla/5.0 (Macintosh; rv:151.0) Gecko/20100101 Firefox/151.0")[0],
            "Firefox"
        );
    }

    fn cookie(domain: &str, path: &str, name: &str, value: &str, expires: Option<u64>) -> Cookie {
        Cookie {
            domain: domain.to_string(),
            path: path.to_string(),
            secure: true,
            expires,
            name: name.to_string(),
            value: value.to_string(),
            http_only: true,
            same_site: 0,
        }
    }

    const NOW: u64 = 1_000_000;

    fn header(cookies: Vec<Cookie>) -> Option<String> {
        search_clearance(cookies, NOW).map(|(header, _)| header)
    }

    #[test]
    fn keeps_only_cookies_sent_to_the_search() {
        let later = Some(NOW + 3600);
        assert_eq!(
            header(vec![
                cookie(".beatport.com", "/", "cf_clearance", "search", later),
                cookie(".beatport.com", "/", "__cf_bm", "bm", later),
                cookie(".beatport.com", "/", "session", "login", later),
                cookie("www.beatport.com", "/", "_cfuvid", "uvid", None),
            ])
            .as_deref(),
            Some("cf_clearance=search; __cf_bm=bm; _cfuvid=uvid")
        );
        // Other hosts, look-alike domains, other paths and expired cookies don't count.
        for c in [
            cookie(".support.beatport.com", "/", "cf_clearance", "x", later),
            cookie("beatport.com", "/", "cf_clearance", "x", later),
            cookie(".notbeatport.com", "/", "cf_clearance", "x", later),
            cookie(".beatport.com.evil.net", "/", "cf_clearance", "x", later),
            cookie(".beatport.com", "/track", "cf_clearance", "x", later),
            cookie(
                ".beatport.com",
                "/search/tracksx",
                "cf_clearance",
                "x",
                later,
            ),
            cookie(".beatport.com", "/", "cf_clearance", "x", Some(NOW)),
        ] {
            let (domain, path) = (c.domain.clone(), c.path.clone());
            assert_eq!(header(vec![c]), None, "{domain} {path}");
        }
        assert!(header(vec![cookie(
            ".beatport.com",
            "/search",
            "cf_clearance",
            "x",
            later
        )])
        .is_some());
    }

    #[test]
    fn keeps_the_most_specific_of_same_named_cookies() {
        let later = Some(NOW + 3600);
        let (header, expires) = search_clearance(
            vec![
                cookie(
                    ".support.beatport.com",
                    "/",
                    "cf_clearance",
                    "support",
                    later,
                ),
                cookie(
                    ".beatport.com",
                    "/",
                    "cf_clearance",
                    "site",
                    Some(NOW + 7200),
                ),
                cookie("www.beatport.com", "/", "cf_clearance", "host", later),
            ],
            NOW,
        )
        .unwrap();
        assert_eq!(header, "cf_clearance=host");
        assert_eq!(expires, NOW + 3600);
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
        let session = BeatportSession {
            cookie: "cf_clearance=x".to_string(),
            user_agent: "test".to_string(),
            browser: "Chrome".to_string(),
        };
        assert_eq!(
            commit(home.path(), generation, session).await.unwrap(),
            None
        );
        assert!(connected_browser().await.is_none());
        assert!(!home.path().join(BEATPORT_SESSION_FILE).exists());
    }
}
