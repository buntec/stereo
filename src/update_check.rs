//! Checks GitHub once a day for a newer release, for the icon beside the version number.
//!
//! The last result is saved in the data dir's `update_check.json`, so restarts don't check again
//! within the day, and is compared with the running version each time it's loaded.

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/buntec/stereo/releases/latest";
const CHECK_FILE: &str = "update_check.json";
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const RETRY_INTERVAL: Duration = Duration::from_secs(60 * 60);
const CURRENT: &str = env!("CARGO_PKG_VERSION");

static STATUS: OnceLock<watch::Sender<Option<UpdateStatus>>> = OnceLock::new();

/// The newest published release (GitHub's latest skips drafts and pre-releases).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub published_at: Option<DateTime<Utc>>,
}

/// How the running version compares with the latest release.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum UpdateStatus {
    UpToDate {
        latest: Release,
        checked_at: DateTime<Utc>,
    },
    Available {
        latest: Release,
        checked_at: DateTime<Utc>,
    },
    /// No check has succeeded yet; retried hourly.
    Error {
        message: String,
        checked_at: DateTime<Utc>,
    },
}

#[derive(Serialize, Deserialize)]
struct SavedCheck {
    latest: Release,
    checked_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    published_at: Option<DateTime<Utc>>,
}

/// The numeric components of `1.2.3`, `v1.2.3` or `1.2.3-rc.1` (whose suffix is ignored).
fn parse_version(version: &str) -> Option<Vec<u64>> {
    let version = version.trim().trim_start_matches('v');
    let core = version.split(['-', '+']).next()?;
    core.split('.').map(|part| part.parse().ok()).collect()
}

fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

fn status_of(latest: Release, checked_at: DateTime<Utc>) -> UpdateStatus {
    if is_newer(&latest.version, CURRENT) {
        UpdateStatus::Available { latest, checked_at }
    } else {
        UpdateStatus::UpToDate { latest, checked_at }
    }
}

async fn fetch_latest(client: &wreq::Client) -> anyhow::Result<Release> {
    let response = client
        .get(LATEST_RELEASE_URL)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", concat!("stereo/", env!("CARGO_PKG_VERSION")))
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("GitHub replied {status}");
    }
    let release: GitHubRelease = response.json().await?;
    Ok(Release {
        version: release.tag_name.trim_start_matches('v').to_string(),
        url: release.html_url,
        published_at: release.published_at,
    })
}

async fn load(path: &Path) -> Option<SavedCheck> {
    serde_json::from_slice(&tokio::fs::read(path).await.ok()?).ok()
}

async fn save(path: &Path, check: &SavedCheck) {
    let result = async {
        let tmp = path.with_extension("json.tmp");
        tokio::fs::write(&tmp, serde_json::to_vec_pretty(check)?).await?;
        tokio::fs::rename(&tmp, path).await?;
        anyhow::Ok(())
    };
    if let Err(e) = result.await {
        tracing::warn!("saving the update check failed: {e:#}");
    }
}

async fn run(path: PathBuf, status: watch::Sender<Option<UpdateStatus>>) {
    let client = match crate::scraper::create_client() {
        Ok(client) => client,
        Err(e) => {
            tracing::warn!("update check disabled: {e:#}");
            return;
        }
    };
    let mut saved = load(&path).await;
    loop {
        if let Some(check) = &saved {
            status.send_replace(Some(status_of(check.latest.clone(), check.checked_at)));
            let age = (Utc::now() - check.checked_at).to_std().unwrap_or_default();
            tokio::time::sleep(CHECK_INTERVAL.saturating_sub(age)).await;
        }
        loop {
            let checked_at = Utc::now();
            match fetch_latest(&client).await {
                Ok(latest) => {
                    let check = SavedCheck { latest, checked_at };
                    save(&path, &check).await;
                    saved = Some(check);
                    break;
                }
                Err(e) => {
                    tracing::warn!("checking for a newer release failed: {e:#}");
                    // An earlier result stays on show, if there is one.
                    if saved.is_none() {
                        status.send_replace(Some(UpdateStatus::Error {
                            message: format!("{e:#}"),
                            checked_at,
                        }));
                    }
                    tokio::time::sleep(RETRY_INTERVAL).await;
                }
            }
        }
    }
}

/// Starts the daily check in the background.
pub fn spawn(home: &Path) {
    let status = watch::Sender::new(None);
    if STATUS.set(status.clone()).is_ok() {
        tokio::spawn(run(home.join(CHECK_FILE), status));
    }
}

/// The current status and its changes, if checks run.
pub fn subscribe() -> Option<watch::Receiver<Option<UpdateStatus>>> {
    STATUS.get().map(watch::Sender::subscribe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_numerically() {
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(is_newer("v0.2.0", "0.1.74"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.1.74", "0.1.74"));
        assert!(!is_newer("0.1.73", "0.1.74"));
        assert!(!is_newer("0.1.74-rc.1", "0.1.74"));
        assert!(!is_newer("nightly", "0.1.74"));
    }
}
