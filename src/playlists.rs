//! Dynamic playlists: saved views (a collection plus the grid's filter and sort) that can be
//! exported to Spotify and YouTube, and kept up to date there.
//!
//! The definitions live in [`PLAYLISTS_FILE`] at the root of the repo, next to the collections, so
//! they are committed and synced like them (see `git.rs`); concurrent edits on two machines are
//! reconciled per playlist by [`merge`]. What was last pushed to which service is machine-local
//! state in the data dir ([`SyncRecords`]), so that routine syncs don't produce commits.

use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use tokio::sync::Mutex;

use crate::message::{FilterModelMap, SortModelItem};

/// The repo file holding the playlist definitions.
pub const PLAYLISTS_FILE: &str = "playlists.json";

/// The data-dir file holding what was last pushed for each playlist and service.
const SYNC_RECORDS_FILE: &str = "playlist_sync.json";

/// Automatic syncs of a playlist to a service happen at most this often.
pub const AUTO_SYNC_INTERVAL_MS: i64 = 60 * 60 * 1000;

/// The most tracks a synced playlist gets, whatever its limit: YouTube refuses playlists beyond
/// 5000 videos, and Spotify playlists that large are unwieldy anyway.
pub const MAX_SYNCED_TRACKS: i64 = 5000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Service {
    Spotify,
    #[serde(rename = "youtube")]
    YouTube,
}

impl Service {
    pub fn label(self) -> &'static str {
        match self {
            Self::Spotify => "Spotify",
            Self::YouTube => "YouTube",
        }
    }
}

/// Where a playlist is exported to on one service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportTarget {
    /// Whether the client keeps the remote playlist up to date while it is open.
    #[serde(default)]
    pub auto_sync: bool,
    /// The remote playlist, once the first sync created it; later syncs replace its contents.
    #[serde(default)]
    pub playlist_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    /// The collection's file name within the repo, e.g. `techno.db`.
    pub collection: String,
    #[serde(default)]
    pub filter_model: FilterModelMap,
    #[serde(default)]
    pub sort_model: Vec<SortModelItem>,
    /// Only the first `limit` tracks of the view, if set.
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub spotify: Option<ExportTarget>,
    #[serde(default)]
    pub youtube: Option<ExportTarget>,
    /// Milliseconds since the epoch of the last edit, which wins when both sides of a merge
    /// edited the same playlist.
    #[serde(default)]
    pub updated_at: i64,
}

impl Playlist {
    pub fn target(&self, service: Service) -> Option<&ExportTarget> {
        match service {
            Service::Spotify => self.spotify.as_ref(),
            Service::YouTube => self.youtube.as_ref(),
        }
    }

    pub fn target_mut(&mut self, service: Service) -> Option<&mut ExportTarget> {
        match service {
            Service::Spotify => self.spotify.as_mut(),
            Service::YouTube => self.youtube.as_mut(),
        }
    }

    /// How many tracks a sync takes from the top of the view.
    pub fn sync_limit(&self) -> i64 {
        self.limit
            .filter(|n| *n > 0)
            .map_or(MAX_SYNCED_TRACKS, |n| n.min(MAX_SYNCED_TRACKS))
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PlaylistsFile {
    #[serde(default = "file_version")]
    version: u32,
    #[serde(default)]
    playlists: Vec<Playlist>,
}

fn file_version() -> u32 {
    1
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

pub fn playlists_path(repo: &Path) -> PathBuf {
    repo.join(PLAYLISTS_FILE)
}

fn parse(bytes: &[u8]) -> anyhow::Result<Vec<Playlist>> {
    let file: PlaylistsFile = serde_json::from_slice(bytes).context("malformed playlists file")?;
    Ok(file.playlists)
}

/// Serialized with sorted keys (serde_json's maps are ordered), so that unchanged playlists stay
/// byte-identical and diffs stay small.
fn serialize(playlists: &[Playlist]) -> anyhow::Result<Vec<u8>> {
    let value = serde_json::to_value(PlaylistsFile {
        version: file_version(),
        playlists: playlists.to_vec(),
    })?;
    let mut bytes = serde_json::to_vec_pretty(&value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// The repo's playlists; none if it has no playlists file yet.
pub async fn load(repo: &Path) -> anyhow::Result<Vec<Playlist>> {
    match tokio::fs::read(playlists_path(repo)).await {
        Ok(bytes) => parse(&bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e.into()),
    }
}

/// Replaces the repo's playlists file. The caller holds the repo lock and schedules the sync.
pub async fn save(repo: &Path, playlists: &[Playlist]) -> anyhow::Result<()> {
    let path = playlists_path(repo);
    let temp = path.with_extension("json.tmp");
    tokio::fs::write(&temp, serialize(playlists)?).await?;
    tokio::fs::rename(&temp, &path).await?;
    Ok(())
}

fn same(a: &Playlist, b: &Playlist) -> bool {
    serde_json::to_value(a).ok() == serde_json::to_value(b).ok()
}

/// Three-way merge of two versions of the playlists, per playlist: a playlist only one side
/// changed takes that side's version; one both sides changed takes the newer edit; a deletion
/// loses against an edit on the other side.
pub fn merge(base: &[Playlist], local: &[Playlist], remote: &[Playlist]) -> Vec<Playlist> {
    let find = |list: &[Playlist], id: &str| list.iter().find(|p| p.id == id).cloned();
    let mut ids: Vec<&str> = local.iter().map(|p| p.id.as_str()).collect();
    ids.extend(
        remote
            .iter()
            .map(|p| p.id.as_str())
            .filter(|id| !local.iter().any(|p| p.id == *id)),
    );
    ids.into_iter()
        .filter_map(|id| {
            let base = find(base, id);
            let local = find(local, id);
            let remote = find(remote, id);
            let changed = |side: &Option<Playlist>| match (&base, side) {
                (Some(b), Some(s)) => !same(b, s),
                (None, None) => false,
                _ => true,
            };
            match (changed(&local), changed(&remote)) {
                (_, false) => local,
                (false, true) => remote,
                (true, true) => match (local, remote) {
                    (Some(l), Some(r)) => {
                        let (mut winner, other) = if r.updated_at > l.updated_at {
                            (r, l)
                        } else {
                            (l, r)
                        };
                        // A remote playlist that the other side's sync created would otherwise
                        // be forgotten, and the next sync would create another one.
                        for service in [Service::Spotify, Service::YouTube] {
                            let theirs = other.target(service).and_then(|t| t.playlist_id.clone());
                            if let Some(target) = winner.target_mut(service) {
                                if target.playlist_id.is_none() {
                                    target.playlist_id = theirs;
                                }
                            }
                        }
                        Some(winner)
                    }
                    (l, r) => l.or(r),
                },
            }
        })
        .collect()
}

/// [`merge`] of the serialized files; `base` is `None` if the file didn't exist there.
pub fn merge_files(base: Option<&[u8]>, local: &[u8], remote: &[u8]) -> anyhow::Result<Vec<u8>> {
    let base = match base {
        Some(bytes) => parse(bytes)?,
        None => vec![],
    };
    serialize(&merge(&base, &parse(local)?, &parse(remote)?))
}

/// The collection file name a client-side collection path refers to, if it is one of `repo`'s.
pub fn collection_name(repo: &Path, path: &str) -> Option<String> {
    let path = Path::new(path);
    if path.parent() != Some(repo) || path.extension().is_none_or(|e| e != "db") {
        return None;
    }
    path.file_name()?.to_str().map(str::to_string)
}

// ── Sync records ─────────────────────────────────────────────────────────────

/// What was last pushed for one playlist to one service, from this machine.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncRecord {
    /// The remote playlist the record is about; a different one (e.g. after unlinking) starts
    /// from scratch.
    pub remote_id: Option<String>,
    /// Hash of the content last pushed.
    pub hash: Option<String>,
    /// When the remote playlist was last updated.
    pub synced_at: Option<i64>,
    /// When a sync was last attempted, which throttles automatic syncs.
    pub checked_at: Option<i64>,
    /// Why the last attempt failed, if it did.
    pub error: Option<String>,
}

static SYNC_RECORDS_LOCK: Mutex<()> = Mutex::const_new(());

fn record_key(playlist_id: &str, service: Service) -> String {
    let service = match service {
        Service::Spotify => "spotify",
        Service::YouTube => "youtube",
    };
    format!("{playlist_id}/{service}")
}

/// Machine-local sync state of all playlists, in the data dir.
pub struct SyncRecords;

impl SyncRecords {
    async fn read_all(data_home: &Path) -> HashMap<String, SyncRecord> {
        match tokio::fs::read(data_home.join(SYNC_RECORDS_FILE)).await {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => HashMap::new(),
        }
    }

    pub async fn get(data_home: &Path, playlist_id: &str, service: Service) -> SyncRecord {
        let _guard = SYNC_RECORDS_LOCK.lock().await;
        Self::read_all(data_home)
            .await
            .remove(&record_key(playlist_id, service))
            .unwrap_or_default()
    }

    pub async fn all(data_home: &Path) -> HashMap<String, SyncRecord> {
        let _guard = SYNC_RECORDS_LOCK.lock().await;
        Self::read_all(data_home).await
    }

    pub fn lookup<'a>(
        records: &'a HashMap<String, SyncRecord>,
        playlist_id: &str,
        service: Service,
    ) -> Option<&'a SyncRecord> {
        records.get(&record_key(playlist_id, service))
    }

    pub async fn update(
        data_home: &Path,
        playlist_id: &str,
        service: Service,
        f: impl FnOnce(&mut SyncRecord),
    ) -> anyhow::Result<()> {
        let _guard = SYNC_RECORDS_LOCK.lock().await;
        let mut records = Self::read_all(data_home).await;
        f(records.entry(record_key(playlist_id, service)).or_default());
        Self::write_all(data_home, &records).await
    }

    /// Forgets the records of playlists that no longer exist in `keep`.
    pub async fn retain(data_home: &Path, keep: &BTreeSet<String>) -> anyhow::Result<()> {
        let _guard = SYNC_RECORDS_LOCK.lock().await;
        let mut records = Self::read_all(data_home).await;
        let before = records.len();
        records.retain(|key, _| {
            key.split_once('/')
                .is_some_and(|(playlist_id, _)| keep.contains(playlist_id))
        });
        if records.len() != before {
            Self::write_all(data_home, &records).await?;
        }
        Ok(())
    }

    async fn write_all(
        data_home: &Path,
        records: &HashMap<String, SyncRecord>,
    ) -> anyhow::Result<()> {
        let path = data_home.join(SYNC_RECORDS_FILE);
        let temp = path.with_extension("json.tmp");
        tokio::fs::write(&temp, serde_json::to_vec_pretty(records)?).await?;
        tokio::fs::rename(&temp, &path).await?;
        Ok(())
    }
}

/// A hash identifying the content of a playlist, to tell whether it changed since the last push.
pub fn content_hash<'a>(items: impl IntoIterator<Item = &'a str>) -> String {
    let mut hasher = Sha1::new();
    for item in items {
        hasher.update(item.as_bytes());
        hasher.update(b"\n");
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playlist(id: &str, name: &str, updated_at: i64) -> Playlist {
        Playlist {
            id: id.into(),
            name: name.into(),
            collection: "music.db".into(),
            filter_model: Default::default(),
            sort_model: vec![],
            limit: None,
            spotify: None,
            youtube: None,
            updated_at,
        }
    }

    fn names(playlists: &[Playlist]) -> Vec<&str> {
        playlists.iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn merge_keeps_additions_from_both_sides() {
        let base = vec![playlist("a", "A", 1)];
        let local = vec![playlist("a", "A", 1), playlist("b", "B", 2)];
        let remote = vec![playlist("a", "A", 1), playlist("c", "C", 3)];
        assert_eq!(names(&merge(&base, &local, &remote)), ["A", "B", "C"]);
    }

    #[test]
    fn merge_takes_the_side_that_changed() {
        let base = vec![playlist("a", "A", 1), playlist("b", "B", 1)];
        let local = vec![playlist("a", "A local", 2), playlist("b", "B", 1)];
        let remote = vec![playlist("a", "A", 1), playlist("b", "B remote", 2)];
        assert_eq!(
            names(&merge(&base, &local, &remote)),
            ["A local", "B remote"]
        );
    }

    #[test]
    fn merge_prefers_the_newer_edit_of_both_sides() {
        let base = vec![playlist("a", "A", 1)];
        let local = vec![playlist("a", "A local", 3)];
        let remote = vec![playlist("a", "A remote", 2)];
        assert_eq!(names(&merge(&base, &local, &remote)), ["A local"]);
        let remote = vec![playlist("a", "A remote", 4)];
        assert_eq!(names(&merge(&base, &local, &remote)), ["A remote"]);
    }

    #[test]
    fn merge_keeps_remote_playlists_created_on_the_losing_side() {
        let target = |id: Option<&str>| {
            Some(ExportTarget {
                auto_sync: true,
                playlist_id: id.map(str::to_string),
            })
        };
        let mut base = playlist("a", "A", 1);
        base.spotify = target(None);
        let mut local = base.clone();
        local.spotify = target(Some("created-here"));
        local.updated_at = 2;
        let mut remote = base.clone();
        remote.name = "A renamed".into();
        remote.updated_at = 3;
        let merged = merge(&[base], &[local], &[remote]);
        assert_eq!(merged[0].name, "A renamed");
        assert_eq!(
            merged[0].spotify.as_ref().unwrap().playlist_id.as_deref(),
            Some("created-here")
        );
    }

    #[test]
    fn merge_deletes_unless_edited_on_the_other_side() {
        let base = vec![playlist("a", "A", 1), playlist("b", "B", 1)];
        let local = vec![playlist("b", "B", 1)];
        let remote = vec![playlist("a", "A edited", 2)];
        // `a` was deleted locally but edited upstream: the edit wins. `b` was deleted upstream
        // and left alone locally: it goes.
        assert_eq!(names(&merge(&base, &local, &remote)), ["A edited"]);
    }

    #[test]
    fn files_round_trip_and_merge() {
        let base = serialize(&[playlist("a", "A", 1)]).unwrap();
        let local = serialize(&[playlist("a", "A", 1), playlist("b", "B", 2)]).unwrap();
        let remote = serialize(&[]).unwrap();
        let merged = merge_files(Some(&base), &local, &remote).unwrap();
        assert_eq!(names(&parse(&merged).unwrap()), ["B"]);
    }

    #[test]
    fn collection_name_requires_a_db_in_the_repo() {
        let repo = Path::new("/home/me/repo");
        assert_eq!(
            collection_name(repo, "/home/me/repo/techno.db").as_deref(),
            Some("techno.db")
        );
        assert_eq!(collection_name(repo, "/home/me/other/techno.db"), None);
        assert_eq!(collection_name(repo, "/home/me/repo/playlists.json"), None);
        assert_eq!(collection_name(repo, "/home/me/repo/sub/techno.db"), None);
    }

    #[test]
    fn sync_limit_is_capped() {
        let mut p = playlist("a", "A", 1);
        assert_eq!(p.sync_limit(), MAX_SYNCED_TRACKS);
        p.limit = Some(50);
        assert_eq!(p.sync_limit(), 50);
        p.limit = Some(100_000);
        assert_eq!(p.sync_limit(), MAX_SYNCED_TRACKS);
    }
}
