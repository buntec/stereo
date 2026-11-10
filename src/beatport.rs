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

use std::{path::Path, sync::LazyLock};

use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};
use wreq::{Client, RequestBuilder};

use crate::{
    browser_cookies::{self, browsers_for, Browser, Cookie},
    scraper, utils,
};

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
    let now = browser_cookies::now();
    let mut clearances = vec![];
    let mut errors = vec![];
    for profile in browser_cookies::profiles(browser) {
        match browser_cookies::read(&profile.store, "beatport.com") {
            Ok(cookies) => {
                if let Some((cookie, expires)) = search_clearance(cookies, now) {
                    let source = profile.source;
                    clearances.push((expires, Clearance { source, cookie }));
                }
            }
            Err(e) => errors.push(format!("{}: {e}", profile.source)),
        }
    }
    clearances.sort_by_key(|(expires, _)| std::cmp::Reverse(*expires));
    (clearances.into_iter().map(|(_, c)| c).collect(), errors)
}

/// The `Cookie` header a browser holding `cookies` would send to Beatport's search, limited to
/// Cloudflare's unexpired cookies, together with the clearance's expiry; `None` without a
/// clearance among them. `cookies` can also hold cookies of other Beatport hosts and of unrelated
/// domains merely containing "beatport.com", which are left out the way the browser would. Of
/// several applicable cookies with the same name, only the most specific one is kept, so the
/// header never offers Cloudflare a choice.
fn search_clearance(cookies: Vec<Cookie>, now: u64) -> Option<(String, u64)> {
    let chosen: Vec<Cookie> = browser_cookies::sent_to(cookies, SEARCH_HOST, SEARCH_PATH, now)
        .into_iter()
        .filter(|c| CLOUDFLARE_COOKIES.contains(&c.name.as_str()))
        .collect();
    let clearance = chosen.iter().find(|c| c.name == "cf_clearance")?;
    let expires = clearance.expires.unwrap_or(u64::MAX);
    Some((browser_cookies::header(&chosen), expires))
}

#[cfg(test)]
mod tests {
    use super::*;

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
