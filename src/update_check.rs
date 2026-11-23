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
/// The longest single timer wait. Tokio's timers stop while the system sleeps (on macOS at
/// least), so waits are cut up and checked against the wall clock, catching up soon after a wake.
const MAX_WAIT: Duration = Duration::from_secs(5 * 60);
const CURRENT: &str = env!("CARGO_PKG_VERSION");

static STATUS: OnceLock<watch::Sender<Option<UpdateStatus>>> = OnceLock::new();

/// The newest published release (GitHub's latest skips drafts and pre-releases).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub published_at: Option<DateTime<Utc>>,
}

/// A check that failed after an earlier one succeeded, whose result is still shown.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RefreshError {
    pub message: String,
    pub failed_at: DateTime<Utc>,
}

/// How the running version compares with the latest release.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum UpdateStatus {
    UpToDate {
        latest: Release,
        checked_at: DateTime<Utc>,
        /// Set while checks fail since `checked_at`; retried hourly.
        refresh_error: Option<RefreshError>,
    },
    Available {
        latest: Release,
        checked_at: DateTime<Utc>,
        refresh_error: Option<RefreshError>,
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

/// Where release pages live; any other URL from the API or the saved file is refused.
const RELEASE_URL_PREFIX: &str = "https://github.com/buntec/stereo/releases/";

/// A version such as `1.2.3`, `v1.2.3` or `1.2.3-rc.1`: its numeric components, without
/// trailing zeros so `1.2` and `1.2.0` are equal, and whether it is a pre-release. Build
/// metadata (`+…`) is ignored.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    core: Vec<u64>,
    /// Sorts a final release (`true`) after its pre-releases.
    is_final: bool,
}

fn parse_version(version: &str) -> Option<Version> {
    let version = version.trim().trim_start_matches(['v', 'V']);
    let version = version.split('+').next()?;
    let (core, pre) = match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (version, None),
    };
    let mut core = core
        .split('.')
        .map(|part| part.parse().ok())
        .collect::<Option<Vec<u64>>>()?;
    if pre.is_some_and(str::is_empty) {
        return None;
    }
    while core.last() == Some(&0) {
        core.pop();
    }
    Some(Version {
        core,
        is_final: pre.is_none(),
    })
}

/// Whether `latest` is newer than `current`; pre-releases of the same version are older than
/// it and are not ordered among themselves.
fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

/// What to show for the last successful check, if any, and the failure since, if any.
fn status_of(saved: Option<&SavedCheck>, failure: Option<&RefreshError>) -> Option<UpdateStatus> {
    let refresh_error = failure.cloned();
    Some(match (saved, refresh_error) {
        (Some(check), refresh_error) => {
            let latest = check.latest.clone();
            let checked_at = check.checked_at;
            if is_newer(&latest.version, CURRENT) {
                UpdateStatus::Available {
                    latest,
                    checked_at,
                    refresh_error,
                }
            } else {
                UpdateStatus::UpToDate {
                    latest,
                    checked_at,
                    refresh_error,
                }
            }
        }
        (None, Some(failure)) => UpdateStatus::Error {
            message: failure.message,
            checked_at: failure.failed_at,
        },
        (None, None) => return None,
    })
}

/// How long to wait before looking at the clock again on the way to `deadline`, or `None` once
/// it has passed.
fn next_wait(deadline: DateTime<Utc>, now: DateTime<Utc>) -> Option<Duration> {
    let remaining = (deadline - now).to_std().ok().filter(|d| !d.is_zero())?;
    Some(remaining.min(MAX_WAIT))
}

/// Waits until `deadline` by the wall clock, including time the system spent asleep.
async fn sleep_until(deadline: DateTime<Utc>) {
    while let Some(wait) = next_wait(deadline, Utc::now()) {
        tokio::time::sleep(wait).await;
    }
}

/// A release whose version can be compared and whose page is on GitHub.
fn validate(release: &Release) -> anyhow::Result<()> {
    if parse_version(&release.version).is_none() {
        anyhow::bail!("unrecognized release version {:?}", release.version);
    }
    if !release.url.starts_with(RELEASE_URL_PREFIX) {
        anyhow::bail!("unexpected release URL {:?}", release.url);
    }
    Ok(())
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
    let release = Release {
        version: release
            .tag_name
            .trim()
            .trim_start_matches(['v', 'V'])
            .to_string(),
        url: release.html_url,
        published_at: release.published_at,
    };
    validate(&release)?;
    Ok(release)
}

/// The saved result, unless it's missing, malformed or fails validation (then checked again).
async fn load(path: &Path) -> Option<SavedCheck> {
    let check: SavedCheck = serde_json::from_slice(&tokio::fs::read(path).await.ok()?).ok()?;
    validate(&check.latest).ok()?;
    Some(check)
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
    let mut failure: Option<RefreshError> = None;
    let mut next_check = saved
        .as_ref()
        .map_or_else(Utc::now, |check| check.checked_at + CHECK_INTERVAL);
    loop {
        if let Some(current) = status_of(saved.as_ref(), failure.as_ref()) {
            status.send_replace(Some(current));
        }
        sleep_until(next_check).await;
        let checked_at = Utc::now();
        match fetch_latest(&client).await {
            Ok(latest) => {
                let check = SavedCheck { latest, checked_at };
                save(&path, &check).await;
                saved = Some(check);
                failure = None;
                next_check = checked_at + CHECK_INTERVAL;
            }
            Err(e) => {
                tracing::warn!("checking for a newer release failed: {e:#}");
                failure = Some(RefreshError {
                    message: format!("{e:#}"),
                    failed_at: checked_at,
                });
                next_check = checked_at + RETRY_INTERVAL;
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
        assert!(is_newer("V0.2.0", "0.1.74"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.1.74", "0.1.74"));
        assert!(!is_newer("0.1.73", "0.1.74"));
        assert!(!is_newer("nightly", "0.1.74"));
        assert!(!is_newer("0.1.74+build.5", "0.1.74"));
    }

    #[test]
    fn ignores_trailing_zeros() {
        assert!(!is_newer("0.2.0", "0.2"));
        assert!(!is_newer("0.2", "0.2.0"));
        assert!(is_newer("0.2.1", "0.2"));
    }

    #[test]
    fn a_final_release_is_newer_than_its_pre_releases() {
        assert!(is_newer("0.2.0", "0.2.0-rc.1"));
        assert!(!is_newer("0.2.0-rc.1", "0.2.0"));
        assert!(is_newer("0.2.0-rc.1", "0.1.74"));
        assert!(!is_newer("0.2.0-rc.2", "0.2.0-rc.1"));
        assert!(!is_newer("0.2.0-", "0.1.0"));
    }

    #[test]
    fn waits_are_bounded_and_measured_by_the_wall_clock() {
        let now: DateTime<Utc> = "2026-11-23T06:00:00Z".parse().unwrap();
        assert_eq!(next_wait(now + CHECK_INTERVAL, now), Some(MAX_WAIT));
        assert_eq!(
            next_wait(now + Duration::from_secs(30), now),
            Some(Duration::from_secs(30))
        );
        assert_eq!(next_wait(now, now), None);
        // After a long sleep the deadline has passed: check right away.
        assert_eq!(next_wait(now, now + CHECK_INTERVAL), None);
    }

    #[test]
    fn a_failed_refresh_is_reported_with_the_earlier_result() {
        let failed_at: DateTime<Utc> = "2026-11-28T06:00:00Z".parse().unwrap();
        let failure = RefreshError {
            message: "proxy refused".into(),
            failed_at,
        };
        assert_eq!(status_of(None, None), None);
        assert_eq!(
            status_of(None, Some(&failure)),
            Some(UpdateStatus::Error {
                message: "proxy refused".into(),
                checked_at: failed_at,
            })
        );
        let check = SavedCheck {
            latest: release(CURRENT, "https://github.com/buntec/stereo/releases/tag/v0"),
            checked_at: "2026-11-23T06:00:00Z".parse().unwrap(),
        };
        let Some(UpdateStatus::UpToDate {
            checked_at,
            refresh_error,
            ..
        }) = status_of(Some(&check), Some(&failure))
        else {
            panic!("expected up-to-date");
        };
        assert_eq!(checked_at, check.checked_at);
        assert_eq!(refresh_error, Some(failure));
        let newer = SavedCheck {
            latest: release(
                "999.0.0",
                "https://github.com/buntec/stereo/releases/tag/v999.0.0",
            ),
            ..check
        };
        assert!(matches!(
            status_of(Some(&newer), None),
            Some(UpdateStatus::Available {
                refresh_error: None,
                ..
            })
        ));
    }

    fn release(version: &str, url: &str) -> Release {
        Release {
            version: version.to_string(),
            url: url.to_string(),
            published_at: None,
        }
    }

    #[test]
    fn validates_version_and_url() {
        let url = "https://github.com/buntec/stereo/releases/tag/v0.2.0";
        assert!(validate(&release("0.2.0", url)).is_ok());
        assert!(validate(&release("nightly", url)).is_err());
        assert!(validate(&release("0.2.0", "javascript:alert(1)")).is_err());
        assert!(validate(&release(
            "0.2.0",
            "http://github.com/buntec/stereo/releases/"
        ))
        .is_err());
        assert!(validate(&release("0.2.0", "https://example.com/")).is_err());
    }

    #[tokio::test]
    async fn saved_checks_round_trip_and_bad_files_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CHECK_FILE);
        assert!(load(&path).await.is_none());

        let latest = Release {
            published_at: Some("2026-11-22T16:18:52Z".parse().unwrap()),
            ..release(
                "0.2.0",
                "https://github.com/buntec/stereo/releases/tag/v0.2.0",
            )
        };
        let checked_at: DateTime<Utc> = "2026-11-23T06:00:00Z".parse().unwrap();
        save(
            &path,
            &SavedCheck {
                latest: latest.clone(),
                checked_at,
            },
        )
        .await;
        let loaded = load(&path).await.unwrap();
        assert_eq!(loaded.latest, latest);
        assert_eq!(loaded.checked_at, checked_at);
        assert!(!path.with_extension("json.tmp").exists());

        tokio::fs::write(&path, b"{not json").await.unwrap();
        assert!(load(&path).await.is_none());

        let tampered = SavedCheck {
            latest: release("0.2.0", "https://example.com/"),
            checked_at,
        };
        tokio::fs::write(&path, serde_json::to_vec(&tampered).unwrap())
            .await
            .unwrap();
        assert!(load(&path).await.is_none());
    }
}
