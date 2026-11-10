//! Reading cookies straight from the cookie stores of the user's browsers, for the sites whose
//! cookies Stereo borrows: Beatport (its Cloudflare clearance) and YouTube (its login).
//!
//! The browser is the one the requesting Stereo tab runs in, told by its User-Agent. That can't
//! pin down a single cookie store (Chromium-based browsers share Chrome's User-Agent, and each
//! profile has its own store), so callers get every store that could be it and pick by content.

use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
pub use rookie::enums::Cookie;
use serde::{Deserialize, Serialize};

/// A browser Stereo can read cookies from, by its name in rookie's browser config.
#[derive(Clone, Copy)]
pub struct Browser {
    /// Shown to the user.
    pub name: &'static str,
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
pub fn browsers_for(user_agent: &str) -> Vec<Browser> {
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

/// Where one browser profile keeps its cookies. Saved by sessions that read their cookies afresh
/// each time they're used, so it stays the same across versions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "engine", rename_all = "kebab-case")]
pub enum Store {
    /// A Chromium-based browser's cookie database; `config` names the browser in rookie's
    /// config, which says where its decryption key is kept.
    Chromium {
        config: String,
        db: PathBuf,
    },
    Firefox {
        db: PathBuf,
    },
    Safari,
}

impl Store {
    /// Identifies this store among those of [`profiles`].
    pub fn id(&self) -> String {
        match self {
            Store::Chromium { db, .. } | Store::Firefox { db } => db.display().to_string(),
            Store::Safari => "safari".to_string(),
        }
    }
}

/// One profile of a browser.
pub struct Profile {
    /// The browser, plus the profile when the browser has several; shown to the user.
    pub source: String,
    pub store: Store,
}

/// The profiles of `browser` that have a cookie store.
pub fn profiles(browser: Browser) -> Vec<Profile> {
    let stores: Vec<(Option<String>, Store)> = match browser.engine {
        Engine::Chromium(config) => existing_paths(config)
            .into_iter()
            .filter(|db| db.is_file())
            .map(|db| {
                let store = Store::Chromium {
                    config: config.to_string(),
                    db: db.clone(),
                };
                (chromium_profile_name(&db), store)
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
                (profile, Store::Firefox { db })
            })
            .collect(),
        #[cfg(target_os = "macos")]
        Engine::Safari => vec![(None, Store::Safari)],
    };
    let several = stores.len() > 1;
    stores
        .into_iter()
        .map(|(profile, store)| Profile {
            source: match profile {
                Some(profile) if several => format!("{} ({profile})", browser.name),
                _ => browser.name.to_string(),
            },
            store,
        })
        .collect()
}

/// The cookies in `store` whose domain contains `domain`. rookie's domain filter is a substring
/// match, so this includes cookies of subdomains and of unrelated look-alike domains; narrow it
/// down with [`sent_to`]. Blocking: it reads files and may wait on an OS keychain prompt.
pub fn read(store: &Store, domain: &str) -> Result<Vec<Cookie>, String> {
    let domains = Some(vec![domain.to_string()]);
    match store {
        Store::Chromium { config, db } => with_wal_applied(db, &[], |copy| {
            read_chromium(config, db, copy, domains.clone())
        }),
        Store::Firefox { db } => with_wal_applied(db, FIREFOX_SESSION_FILES, |db| {
            rookie::firefox_based(db, domains.clone()).map_err(|e| e.to_string())
        }),
        #[cfg(target_os = "macos")]
        Store::Safari => rookie::safari(domains).map_err(|e| e.to_string()),
        #[cfg(not(target_os = "macos"))]
        Store::Safari => Err("Safari's cookies can only be read on macOS".to_string()),
    }
}

/// The files next to a Firefox cookie database that rookie also reads session cookies from.
const FIREFOX_SESSION_FILES: &[&str] =
    &["sessionstore.js", "sessionstore-backups/recovery.jsonlz4"];

/// Runs `read` (a rookie reader) on cookie database `db`, including the changes still in its
/// write-ahead log. rookie opens databases as immutable, which skips the WAL, so what the browser
/// committed since its last checkpoint would be missing; Firefox keeps its cookies in WAL mode,
/// so that can be a login made a moment ago. When `db` has a WAL, `read` therefore gets a copy
/// with the WAL checkpointed into it, next to copies of the `siblings` files rookie reads beside
/// the database. Falls back to `db` itself if no such copy can be made or read.
fn with_wal_applied<T>(
    db: &Path,
    siblings: &[&str],
    read: impl Fn(PathBuf) -> Result<T, String>,
) -> Result<T, String> {
    match snapshot(db, siblings) {
        Ok(None) => return read(db.to_path_buf()),
        Ok(Some((_dir, copy))) => match read(copy) {
            Ok(cookies) => return Ok(cookies),
            Err(e) => tracing::warn!("Reading a copy of {} failed: {e}", db.display()),
        },
        Err(e) => tracing::warn!("Copying {} failed: {e:#}", db.display()),
    }
    read(db.to_path_buf())
}

/// A copy of cookie database `db` with its write-ahead log checkpointed into it, in a temporary
/// directory that lives as long as the returned handle, plus copies of the `siblings` files that
/// exist; `None` if `db` has no WAL to apply.
fn snapshot(db: &Path, siblings: &[&str]) -> anyhow::Result<Option<(tempfile::TempDir, PathBuf)>> {
    let wal = wal_path(db);
    if !std::fs::metadata(&wal).is_ok_and(|m| m.len() > 0) {
        return Ok(None);
    }
    let dir = tempfile::tempdir()?;
    let copy = dir.path().join(db.file_name().context("no file name")?);
    // The browser may checkpoint while the two files are copied, which would pair a database
    // and a WAL from different moments; copy again if the database changed meanwhile.
    let version = |db: &Path| -> anyhow::Result<_> {
        let meta = std::fs::metadata(db)?;
        Ok((meta.len(), meta.modified()?))
    };
    let mut attempts = 0;
    loop {
        let before = version(db)?;
        std::fs::copy(db, &copy)?;
        std::fs::copy(&wal, wal_path(&copy))?;
        if version(db)? == before {
            break;
        }
        attempts += 1;
        anyhow::ensure!(
            attempts < 3,
            "the database kept changing while being copied"
        );
    }
    // Opening the copy recovers the committed transactions from its WAL; switching it out of
    // WAL mode writes them into the database file, which is all an immutable reader looks at.
    let conn = rusqlite::Connection::open(&copy)?;
    let mode: String = conn.query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))?;
    anyhow::ensure!(
        mode == "delete",
        "could not apply the WAL (journal mode {mode})"
    );
    drop(conn);
    let source_dir = db.parent().context("no parent directory")?;
    for sibling in siblings {
        let source = source_dir.join(sibling);
        if source.is_file() {
            let target = dir.path().join(sibling);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(source, target)?;
        }
    }
    Ok(Some((dir, copy)))
}

/// The write-ahead log SQLite keeps next to database `db`.
fn wal_path(db: &Path) -> PathBuf {
    let mut path = db.as_os_str().to_owned();
    path.push("-wal");
    PathBuf::from(path)
}

/// Seconds since the Unix epoch, which cookie expiries are given in.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Those of `cookies` a browser sends with a request to `https://{host}{path}`: unexpired, and
/// matching by RFC 6265 domain and path rules. Of several with the same name, only the most
/// specific one is kept, as the one the server would read first.
pub fn sent_to(cookies: Vec<Cookie>, host: &str, path: &str, now: u64) -> Vec<Cookie> {
    let specificity = |c: &Cookie| {
        (
            c.path.len(),
            c.domain.trim_start_matches('.').len(),
            c.expires.unwrap_or(u64::MAX),
        )
    };
    let mut chosen: Vec<Cookie> = vec![];
    for cookie in cookies {
        if cookie.expires.is_some_and(|e| e <= now) || !applies_to(&cookie, host, path) {
            continue;
        }
        match chosen.iter_mut().find(|c| c.name == cookie.name) {
            Some(c) if specificity(&cookie) > specificity(c) => *c = cookie,
            Some(_) => {}
            None => chosen.push(cookie),
        }
    }
    chosen
}

/// The `Cookie` request header holding `cookies`.
pub fn header<'a>(cookies: impl IntoIterator<Item = &'a Cookie>) -> String {
    cookies
        .into_iter()
        .map(|c| format!("{}={}", c.name, c.value))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Whether a browser sends `cookie` to `host` and `path`. A domain with a leading dot covers its
/// subdomains; one without is host-only, as browsers store them.
fn applies_to(cookie: &Cookie, host: &str, path: &str) -> bool {
    let domain = cookie.domain.to_ascii_lowercase();
    let domain_matches = match domain.strip_prefix('.') {
        Some(parent) => {
            host == parent
                || host
                    .strip_suffix(parent)
                    .is_some_and(|sub| sub.ends_with('.'))
        }
        None => host == domain,
    };
    let cookie_path = if cookie.path.is_empty() {
        "/"
    } else {
        cookie.path.as_str()
    };
    let path_matches = path
        .strip_prefix(cookie_path)
        .is_some_and(|rest| rest.is_empty() || cookie_path.ends_with('/') || rest.starts_with('/'));
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

/// Reads Chromium cookie database `db`, a copy of `store_db` or that itself.
#[cfg(unix)]
fn read_chromium(
    config: &str,
    _store_db: &Path,
    db: PathBuf,
    domains: Option<Vec<String>>,
) -> Result<Vec<Cookie>, String> {
    rookie::chromium_based(rookie::config::get_browser_config(config), db, domains)
        .map_err(|e| e.to_string())
}

/// Reads Chromium cookie database `db`, a copy of `store_db` or that itself. The decryption key
/// is next to `store_db`.
#[cfg(windows)]
fn read_chromium(
    _config: &str,
    store_db: &Path,
    db: PathBuf,
    domains: Option<Vec<String>>,
) -> Result<Vec<Cookie>, String> {
    let key = local_state(store_db).ok_or("no Local State file found next to its cookies")?;
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

    #[test]
    fn reads_cookies_still_in_the_write_ahead_log() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("cookies.sqlite");
        // Like a running Firefox: a WAL-mode database held open, with a commit since the last
        // checkpoint.
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE moz_cookies (host TEXT, path TEXT, isSecure INTEGER, expiry INTEGER,
                 name TEXT, value TEXT, isHttpOnly INTEGER, sameSite INTEGER);
             PRAGMA wal_checkpoint(TRUNCATE);
             INSERT INTO moz_cookies VALUES ('.youtube.com', '/', 1, 4102444800000, 'SAPISID',
                 'fresh', 0, 0);",
        )
        .unwrap();
        assert!(std::fs::metadata(wal_path(&db)).unwrap().len() > 0);
        let cookies = read(&Store::Firefox { db: db.clone() }, "youtube.com").unwrap();
        assert_eq!(
            cookies
                .iter()
                .map(|c| (&*c.name, &*c.value))
                .collect::<Vec<_>>(),
            [("SAPISID", "fresh")]
        );
        // The browser's own database and WAL are left as they were.
        assert!(std::fs::metadata(wal_path(&db)).unwrap().len() > 0);
        drop(conn);
    }

    #[test]
    fn saved_stores_keep_their_shape() {
        let store = Store::Chromium {
            config: "chrome".to_string(),
            db: PathBuf::from("/p/Cookies"),
        };
        let json = serde_json::to_string(&store).unwrap();
        assert_eq!(
            json,
            r#"{"engine":"chromium","config":"chrome","db":"/p/Cookies"}"#
        );
        assert_eq!(serde_json::from_str::<Store>(&json).unwrap(), store);
    }
}
