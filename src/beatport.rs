//! Beatport's Cloudflare clearance, imported from the user's browser.
//!
//! Beatport puts its search pages (`/search/...`) behind a Cloudflare bot check that an HTTP
//! client can't pass on its own; chart, catalog and track pages are not checked. Solving the
//! check in a real browser leaves a `cf_clearance` cookie there, which lets Stereo's searches
//! through too, but only when sent with the exact User-Agent of the browser that earned it.
//! Both are therefore captured together: the User-Agent comes from the Stereo tab that asks to
//! connect, and the cookie from the cookie store of that same browser.

use std::{
    path::Path,
    sync::LazyLock,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use wreq::RequestBuilder;

use crate::utils;

const BEATPORT_SESSION_FILE: &str = "beatport_session.json";

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

/// Reads the Cloudflare clearance from the browser identified by `user_agent` and saves it
/// together with that User-Agent. Fails if that browser has no unexpired clearance, i.e. the
/// user hasn't visited beatport.com in it recently.
pub async fn connect(home: &Path, user_agent: String) -> anyhow::Result<BeatportSession> {
    let candidates = browsers_for(&user_agent);
    anyhow::ensure!(
        !candidates.is_empty(),
        "Don't know how to read cookies from this browser ({user_agent})"
    );
    let (browser, cookie) =
        tokio::task::spawn_blocking(move || read_clearance(candidates)).await??;
    let session = BeatportSession {
        cookie,
        user_agent,
        browser,
    };
    utils::write_secret_file(
        &home.join(BEATPORT_SESSION_FILE),
        &serde_json::to_vec(&session)?,
    )
    .await?;
    *SESSION.write().await = Some(session.clone());
    tracing::info!("Connected Beatport session from {}", session.browser);
    Ok(session)
}

pub async fn disconnect(home: &Path) -> anyhow::Result<()> {
    *SESSION.write().await = None;
    match tokio::fs::remove_file(home.join(BEATPORT_SESSION_FILE)).await {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

type BrowserCookieFn = fn(Option<Vec<String>>) -> rookie::Result<Vec<rookie::enums::Cookie>>;

/// The installed browsers whose User-Agent looks like `user_agent`. Several Chromium-based
/// browsers (Brave, Arc, Vivaldi) send exactly Chrome's User-Agent, so all of them are tried;
/// the first with a clearance wins.
fn browsers_for(user_agent: &str) -> Vec<(&'static str, BrowserCookieFn)> {
    let mut browsers: Vec<(&'static str, BrowserCookieFn)> = vec![];
    if user_agent.contains("Firefox/") {
        browsers.extend([
            ("Firefox", rookie::firefox as BrowserCookieFn),
            ("Zen", rookie::zen),
            ("LibreWolf", rookie::librewolf),
        ]);
    } else if user_agent.contains("Edg/") {
        browsers.push(("Edge", rookie::edge));
    } else if user_agent.contains("OPR/") {
        browsers.extend([
            ("Opera", rookie::opera as BrowserCookieFn),
            ("Opera GX", rookie::opera_gx),
        ]);
    } else if user_agent.contains("Chrome/") {
        browsers.extend([
            ("Chrome", rookie::chrome as BrowserCookieFn),
            ("Brave", rookie::brave),
            ("Arc", rookie::arc),
            ("Vivaldi", rookie::vivaldi),
            ("Chromium", rookie::chromium),
        ]);
    } else if user_agent.contains("Safari/") {
        #[cfg(target_os = "macos")]
        browsers.push(("Safari", rookie::safari));
    }
    browsers
}

fn read_clearance(
    browsers: Vec<(&'static str, BrowserCookieFn)>,
) -> anyhow::Result<(String, String)> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let names: Vec<&str> = browsers.iter().map(|(name, _)| *name).collect();
    for (name, browser_fn) in browsers {
        let Ok(cookies) = browser_fn(Some(vec!["beatport.com".to_string()])) else {
            continue;
        };
        let cookies: Vec<_> = cookies
            .into_iter()
            .filter(|c| CLOUDFLARE_COOKIES.contains(&c.name.as_str()))
            .filter(|c| c.expires.is_none_or(|e| e > now))
            .collect();
        if cookies.iter().any(|c| c.name == "cf_clearance") {
            let header = cookies
                .iter()
                .map(|c| format!("{}={}", c.name, c.value))
                .collect::<Vec<_>>()
                .join("; ");
            return Ok((name.to_string(), header));
        }
    }
    anyhow::bail!(
        "No current Beatport clearance found in {}. Open beatport.com and run a search there \
         in this browser, then connect again.",
        names.join(" / ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(user_agent: &str) -> Vec<&'static str> {
        browsers_for(user_agent)
            .into_iter()
            .map(|(n, _)| n)
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
}
