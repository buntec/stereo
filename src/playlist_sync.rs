//! Pushing a dynamic playlist (`playlists.rs`) to Spotify or YouTube: the view's tracks are read
//! from its collection and replace the remote playlist's contents, which keeps its URL. A push is
//! skipped when the tracks are the same as last time, and automatic (unforced) syncs happen at
//! most every `AUTO_SYNC_INTERVAL_MS`.

use std::{
    collections::HashSet,
    path::Path,
    sync::{LazyLock, Mutex},
};

use anyhow::Context;
use futures::StreamExt;
use wreq::Client;

use crate::{
    db, git,
    models::Track,
    playlists::{self, Playlist, Service, SyncRecord, SyncRecords, AUTO_SYNC_INTERVAL_MS},
    spotify, youtube,
};

pub enum Outcome {
    /// An automatic sync that came too soon after the last one.
    Throttled,
    /// The view's tracks are the ones last pushed.
    Unchanged,
    Synced {
        /// Whether Spotify ids found for tracks were written to the collection.
        sp_ids_written: bool,
        /// How many of the view's tracks made it into the playlist (Spotify can't match all).
        matched: i64,
        total: i64,
    },
    /// Some tracks couldn't be looked up on Spotify (as opposed to not being there), so the
    /// remote playlist was left alone rather than replaced without them.
    LookupFailed {
        /// Whether the Spotify ids that were found were written to the collection, so that the
        /// next attempt only looks up the rest.
        sp_ids_written: bool,
        error: String,
    },
}

/// Playlist/service pairs being synced, so that two tabs can't push the same playlist at once.
static IN_FLIGHT: LazyLock<Mutex<HashSet<(String, Service)>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// Removes its playlist/service pair from `IN_FLIGHT` when the sync ends, however it ends.
pub struct InFlight((String, Service));

impl InFlight {
    /// `None` if that playlist is already being synced to that service.
    pub fn start(playlist_id: &str, service: Service) -> Option<Self> {
        let key = (playlist_id.to_string(), service);
        let mut in_flight = IN_FLIGHT.lock().expect("in-flight syncs poisoned");
        // Lazily: a `Self` built for nothing would be dropped right here, and its `drop` would
        // lock `IN_FLIGHT` again (deadlocking, with the guard still held) and remove the key of
        // the sync that is running.
        in_flight.insert(key.clone()).then(|| Self(key))
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        IN_FLIGHT
            .lock()
            .expect("in-flight syncs poisoned")
            .remove(&self.0);
    }
}

/// The URL of a service's playlist.
pub fn remote_url(service: Service, playlist_id: &str) -> Option<String> {
    match service {
        Service::Spotify => spotify::playlist_url(&format!("spotify:playlist:{playlist_id}")),
        Service::YouTube => Some(youtube::playlist_url(playlist_id)),
    }
}

/// Syncs `playlist` to `service`, recording the outcome in the sync records. `progress` is
/// called with (done, total) while tracks are matched or uploaded. `on_created` is called with
/// the id of a remote playlist the sync creates as soon as it exists, before its tracks are
/// uploaded, so that it is kept (and reused by the next attempt) even if the upload fails.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    client: &Client,
    data_home: &Path,
    repo: &Path,
    playlist: &Playlist,
    service: Service,
    force: bool,
    progress: impl Fn(i64, i64) + Send + Sync,
    on_created: impl AsyncFnOnce(String),
) -> anyhow::Result<Outcome> {
    let target = playlist
        .target(service)
        .with_context(|| format!("{} isn't exported to {}", playlist.name, service.label()))?;
    let record = SyncRecords::get(data_home, &playlist.id, service).await;
    // A record about another remote playlist (e.g. before unlinking) says nothing about this one.
    let record = if record.remote_id == target.playlist_id {
        record
    } else {
        SyncRecord::default()
    };
    let now = playlists::now_ms();
    if !force
        && record
            .checked_at
            .is_some_and(|t| now - t < AUTO_SYNC_INTERVAL_MS)
    {
        return Ok(Outcome::Throttled);
    }
    let mut created = None;
    let result = push(
        client,
        data_home,
        repo,
        playlist,
        service,
        force,
        &record,
        progress,
        &mut created,
        on_created,
    )
    .await;
    let remote_id = created.or_else(|| target.playlist_id.clone());
    SyncRecords::update(data_home, &playlist.id, service, |r| {
        r.checked_at = Some(now);
        match &result {
            Ok(Outcome::LookupFailed { error, .. }) => r.error = Some(error.clone()),
            Ok(_) => r.error = None,
            Err(e) => r.error = Some(format!("{e:#}")),
        }
        if r.remote_id != remote_id {
            r.remote_id = remote_id;
            r.hash = None;
            r.synced_at = None;
        }
    })
    .await?;
    result
}

#[allow(clippy::too_many_arguments)]
async fn push(
    client: &Client,
    data_home: &Path,
    repo: &Path,
    playlist: &Playlist,
    service: Service,
    force: bool,
    record: &SyncRecord,
    progress: impl Fn(i64, i64) + Send + Sync,
    created: &mut Option<String>,
    on_created: impl AsyncFnOnce(String),
) -> anyhow::Result<Outcome> {
    let target = playlist.target(service).context("not exported")?;
    let mut tracks = read_tracks(repo, playlist).await?;
    let unchanged =
        |hash: &str| !force && target.playlist_id.is_some() && record.hash.as_deref() == Some(hash);
    let total = tracks.len() as i64;
    let (hash, sp_ids_written, matched) = match service {
        Service::Spotify => {
            if unchanged(&spotify_hash(&tracks)) {
                return Ok(Outcome::Unchanged);
            }
            let session = spotify::SpotifySession::load(data_home)
                .await
                .context("Connect a Spotify account first")?;
            let Matches { found, failed } = match_spotify_ids(client, &mut tracks, &progress).await;
            let sp_ids_written = !found.is_empty() && save_sp_ids(repo, playlist, &found).await;
            if let Some((count, e)) = failed {
                let error = format!(
                    "Could not look up {count} of the tracks on Spotify, so the playlist was left \
                     as it was; the next sync tries again ({e:#})"
                );
                return Ok(Outcome::LookupFailed {
                    sp_ids_written,
                    error,
                });
            }
            let uris: Vec<String> = tracks
                .iter()
                .filter_map(|t| t.sp_id.as_ref())
                .map(|id| format!("spotify:track:{id}"))
                .collect();
            let playlist_uri = match &target.playlist_id {
                Some(id) => {
                    let uri = format!("spotify:playlist:{id}");
                    invalidate_hash(data_home, playlist, service).await?;
                    spotify::clear_playlist(client, &session, &uri)
                        .await
                        .context(
                            "Could not empty the Spotify playlist (unlink it to create a new one)",
                        )?;
                    uri
                }
                None => {
                    let uri = spotify::create_playlist(client, &session, &playlist.name).await?;
                    let id = uri.rsplit(':').next().unwrap_or(&uri).to_string();
                    record_created(data_home, playlist, service, id, created, on_created).await?;
                    uri
                }
            };
            spotify::add_tracks(client, &session, &playlist_uri, &uris).await?;
            // Hashed with the ids found, which are in the collection now, so that the next sync
            // sees the same hash if nothing else changed.
            (spotify_hash(&tracks), sp_ids_written, uris.len() as i64)
        }
        Service::YouTube => {
            let ids: Vec<String> = tracks.iter().map(|t| t.yt_id.clone()).collect();
            let hash = playlists::content_hash(ids.iter().map(String::as_str));
            if unchanged(&hash) {
                return Ok(Outcome::Unchanged);
            }
            // Reads the login from the browser now, so it may wait on a keychain prompt.
            let api = youtube::Api::connected(client, data_home).await?;
            let mut batches = ids.chunks(youtube::BATCH_SIZE);
            let (playlist_id, mut done) = match &target.playlist_id {
                Some(id) => {
                    invalidate_hash(data_home, playlist, service).await?;
                    api.clear_playlist(id).await?;
                    (id.clone(), 0)
                }
                None => {
                    let first = batches.next().unwrap_or_default();
                    let id = api.create_playlist(&playlist.name, first).await?;
                    record_created(
                        data_home,
                        playlist,
                        service,
                        id.clone(),
                        created,
                        on_created,
                    )
                    .await?;
                    (id, first.len() as i64)
                }
            };
            progress(done, total);
            for batch in batches {
                api.add_videos(&playlist_id, batch).await?;
                done += batch.len() as i64;
                progress(done, total);
            }
            (hash, false, total)
        }
    };
    let remote_id = created.clone().or_else(|| target.playlist_id.clone());
    SyncRecords::update(data_home, &playlist.id, service, |r| {
        r.remote_id = remote_id;
        r.hash = Some(hash);
        r.synced_at = Some(playlists::now_ms());
    })
    .await?;
    Ok(Outcome::Synced {
        sp_ids_written,
        matched,
        total,
    })
}

/// Forgets the hash of what was pushed before the remote playlist is changed: until the
/// replacement has completed, the remote content is unknown, so a replacement that fails (or is
/// interrupted) halfway is repaired by the next sync even if the view hasn't changed.
async fn invalidate_hash(
    data_home: &Path,
    playlist: &Playlist,
    service: Service,
) -> anyhow::Result<()> {
    SyncRecords::update(data_home, &playlist.id, service, |r| r.hash = None).await
}

/// Keeps the id of a remote playlist the sync just created, before anything else can fail: in the
/// sync records, in `created` for `run`, and through `on_created` in the playlist itself.
async fn record_created(
    data_home: &Path,
    playlist: &Playlist,
    service: Service,
    id: String,
    created: &mut Option<String>,
    on_created: impl AsyncFnOnce(String),
) -> anyhow::Result<()> {
    *created = Some(id.clone());
    on_created(id.clone()).await;
    SyncRecords::update(data_home, &playlist.id, service, |r| {
        r.remote_id = Some(id);
        r.hash = None;
        r.synced_at = None;
    })
    .await
}

/// The tracks of the playlist's view, from the top, as many as it syncs.
async fn read_tracks(repo: &Path, playlist: &Playlist) -> anyhow::Result<Vec<Track>> {
    let path = repo.join(&playlist.collection);
    let name = path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    // The repo lock keeps a sync from replacing the database while it is read.
    let _guard = git::lock_repository(repo).await;
    anyhow::ensure!(
        db::validate_db_schema(&path).await,
        "The collection {name} doesn't exist anymore"
    );
    let pool = db::open_pool(&path).await?;
    let rows = db::get_rows(
        &pool,
        0,
        playlist.sync_limit(),
        &playlist.sort_model,
        &playlist.filter_model,
    )
    .await;
    pool.close().await;
    rows
}

fn spotify_hash(tracks: &[Track]) -> String {
    let items: Vec<String> = tracks
        .iter()
        .map(|t| format!("{}\t{}", t.yt_id, t.sp_id.as_deref().unwrap_or("")))
        .collect();
    playlists::content_hash(items.iter().map(String::as_str))
}

struct Matches {
    /// The (yt_id, sp_id) pairs found.
    found: Vec<(String, String)>,
    /// How many lookups failed (rather than finding nothing), and the first one's error.
    failed: Option<(usize, anyhow::Error)>,
}

/// Looks up the Spotify ids of tracks without one, filling them in.
async fn match_spotify_ids(
    client: &Client,
    tracks: &mut [Track],
    progress: &(impl Fn(i64, i64) + Send + Sync),
) -> Matches {
    let total = tracks.len() as i64;
    let known = tracks.iter().filter(|t| t.sp_id.is_some()).count() as i64;
    progress(known, total);
    let missing: Vec<(usize, Track)> = tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.sp_id.is_none())
        .map(|(i, t)| (i, t.clone()))
        .collect();
    let mut done = known;
    let mut found = vec![];
    let mut failed: Option<(usize, anyhow::Error)> = None;
    let mut results = futures::stream::iter(missing)
        .map(|(i, track)| async move {
            let artist = track.artists.join(", ");
            let sp = spotify::match_track(client, &artist, &track.title, track.mix_name.as_deref())
                .await
                .inspect_err(|e| {
                    tracing::warn!("Spotify match failed for {artist} - {}: {e}", track.title);
                });
            (i, sp.map(|sp| sp.map(|sp| sp.id().to_string())))
        })
        .buffer_unordered(5);
    while let Some((i, sp_id)) = results.next().await {
        done += 1;
        progress(done, total);
        match sp_id {
            Ok(Some(sp_id)) => {
                found.push((tracks[i].yt_id.clone(), sp_id.clone()));
                tracks[i].sp_id = Some(sp_id);
            }
            Ok(None) => {}
            Err(e) => match &mut failed {
                Some((count, _)) => *count += 1,
                None => failed = Some((1, e)),
            },
        }
    }
    Matches { found, failed }
}

/// Stores Spotify ids found during a sync in the collection, so that they aren't looked up
/// again. Returns whether any were written; failing only costs the lookups next time.
async fn save_sp_ids(repo: &Path, playlist: &Playlist, found: &[(String, String)]) -> bool {
    let path = repo.join(&playlist.collection);
    let _guard = git::lock_repository(repo).await;
    let result: anyhow::Result<()> = async {
        let pool = db::open_pool(&path).await?;
        for (yt_id, sp_id) in found {
            db::update_sp_id(&pool, yt_id, sp_id).await?;
        }
        pool.close().await;
        Ok(())
    }
    .await;
    match result {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("Saving Spotify ids of {} failed: {e:#}", playlist.name);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_flight_refuses_a_second_sync_of_the_same_playlist() {
        let first = InFlight::start("test-in-flight", Service::Spotify).expect("first sync");
        // Used to deadlock: the refused `InFlight` was built anyway and dropped with the lock held.
        assert!(InFlight::start("test-in-flight", Service::Spotify).is_none());
        // The refusal leaves the running sync's entry alone.
        assert!(InFlight::start("test-in-flight", Service::Spotify).is_none());
        assert!(InFlight::start("test-in-flight", Service::YouTube).is_some());
        drop(first);
        assert!(InFlight::start("test-in-flight", Service::Spotify).is_some());
    }
}
