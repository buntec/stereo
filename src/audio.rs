//! Audio downloads for client-side `<audio>` playback.
//!
//! `GET /audio/{yt_id}?key=…` downloads the track's audio stream with yt-dlp on first request,
//! caches it on disk and serves it with Range support (seeking). `HEAD` does the same without
//! the body, which the client uses to prefetch upcoming tracks.
//!
//! The route requires a random key generated per process and handed out only to authenticated
//! WebSocket clients (`MsgServer::BackendInfo`), since `<audio>` can't send the WebSocket token
//! as a header. This also stops other websites from making the backend download things.
//!
//! The cache is size-limited; least recently played files are evicted first (a file's mtime is
//! bumped whenever it is served).
//!
//! Download progress is broadcast to every WebSocket session (`subscribe`), since the request
//! that triggered a download has no way to report it, as are evictions, so clients can show which
//! tracks are downloaded.
//!
//! Waveforms of cached tracks (`waveform.rs`) are computed on first request and cached in
//! [`WAVEFORM_DIR`], deleted along with their audio.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant, SystemTime},
};

use anyhow::Context;
use axum::{
    body::Body,
    extract::{Path as UrlPath, Query, Request},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use futures::{
    future::{BoxFuture, Shared},
    FutureExt,
};
use serde::Deserialize;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    sync::{broadcast, Semaphore},
};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::{message::AudioDownloadStatus, settings::Settings, waveform};

/// Extensions yt-dlp may produce for the requested formats, with their Content-Type.
const AUDIO_TYPES: &[(&str, &str)] = &[
    ("m4a", "audio/mp4"),
    ("mp4", "audio/mp4"),
    ("webm", "audio/webm"),
    ("opus", "audio/ogg"),
    ("ogg", "audio/ogg"),
    ("mp3", "audio/mpeg"),
];

/// Prefer AAC in MP4: it plays in every browser, including Safari. Neither needs ffmpeg.
const FORMAT: &str = "bestaudio[ext=m4a]/bestaudio";
const MAX_CONCURRENT_DOWNLOADS: usize = 3;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);
/// A failed download is not retried for this long, so prefetch + play don't run it twice.
const FAILURE_TTL: Duration = Duration::from_secs(300);
/// Serving a file bumps its mtime (its recency for eviction) at most this often.
const TOUCH_INTERVAL: Duration = Duration::from_secs(3600);
/// Download progress is reported at most this often per download.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
/// Prefix of the progress lines yt-dlp prints for us (`--progress-template`).
const PROGRESS_PREFIX: &str = "stereo-progress ";
/// Locked (shared) by every process using the cache dir, which several backends may share (e.g.
/// an installed service and a development backend).
const LOCK_FILE: &str = ".lock";
/// Subdirectory of the cache dir holding the waveform of a cached track as `{id}.peaks`.
const WAVEFORM_DIR: &str = "waveforms";
/// Waveforms computed at once; decoding a track keeps a core busy for a fraction of a second.
const MAX_CONCURRENT_WAVEFORMS: usize = 2;

#[derive(Debug, Clone, PartialEq)]
pub struct DownloadUpdate {
    pub yt_id: String,
    pub status: AudioDownloadStatus,
    /// Fraction downloaded, while downloading and if the size is known.
    pub progress: Option<f64>,
    /// Why the download failed, for `Failed`.
    pub error: Option<Arc<str>>,
}

/// A change to the cache, broadcast to every session.
#[derive(Debug, Clone, PartialEq)]
pub enum CacheEvent {
    Download(DownloadUpdate),
    /// These tracks are no longer cached.
    Evicted(Vec<String>),
}

/// The cache's state when a session subscribed.
#[derive(Debug, Default)]
pub struct Snapshot {
    /// Ids of the cached tracks.
    pub cached: Vec<String>,
    /// The latest update of every download that hasn't finished.
    pub downloads: Vec<DownloadUpdate>,
}

type Download = Shared<BoxFuture<'static, Result<PathBuf, Arc<str>>>>;

pub struct AudioCache {
    dir: PathBuf,
    limit_bytes: u64,
    yt_dlp: String,
    key: String,
    downloads: Semaphore,
    waveforms: Semaphore,
    in_flight: Mutex<HashMap<String, Download>>,
    failures: Mutex<HashMap<String, (Instant, Arc<str>)>>,
    /// The latest update of every download that hasn't finished, for sessions that connect later.
    active: Mutex<HashMap<String, DownloadUpdate>>,
    updates: broadcast::Sender<CacheEvent>,
    /// Holds a shared lock on [`LOCK_FILE`] for as long as the cache is in use.
    _lock: Option<std::fs::File>,
}

static CACHE: OnceLock<Arc<AudioCache>> = OnceLock::new();

/// Enables audio downloads, which all playback relies on, unless yt-dlp can't be run.
pub async fn init(settings: &Settings) {
    let version = match yt_dlp_version(&settings.yt_dlp).await {
        Ok(version) => version,
        Err(e) => {
            tracing::warn!(
                "Playback is unavailable: {e:#}. Install yt-dlp (and Deno, which it needs \
                 for YouTube) or pass --yt-dlp."
            );
            return;
        }
    };
    let dir = settings.audio_cache_dir();
    match AudioCache::new(
        dir.clone(),
        settings.audio_cache_limit_gb.saturating_mul(1_000_000_000),
        settings.yt_dlp.clone(),
    )
    .await
    {
        Ok(cache) => {
            tracing::info!(
                "Audio downloads enabled (yt-dlp {version}), cache {}",
                dir.display()
            );
            let _ = CACHE.set(Arc::new(cache));
        }
        Err(e) => tracing::warn!("Audio downloads disabled: {e:#}"),
    }
}

/// The key clients must pass to `/audio`, or `None` if audio downloads are disabled.
pub fn key() -> Option<String> {
    CACHE.get().map(|cache| cache.key.clone())
}

/// The cache's current state and a receiver of all further changes, or `None` if audio downloads
/// are disabled.
pub async fn subscribe() -> Option<(Snapshot, broadcast::Receiver<CacheEvent>)> {
    Some(CACHE.get()?.subscribe().await)
}

/// Starts downloading a track in the background, even if its download failed recently.
pub fn download_in_background(id: &str) -> anyhow::Result<()> {
    let cache = CACHE.get().context("Audio downloads are disabled")?;
    anyhow::ensure!(is_valid_id(id), "Invalid YouTube id");
    cache.failures.lock().unwrap().remove(id);
    let cache = cache.clone();
    let id = id.to_string();
    tokio::spawn(async move {
        // Also for a file that was already cached, which the client didn't know about: another
        // process sharing the cache may have downloaded it.
        if cache.get(&id).await.is_ok() {
            cache.report(&id, AudioDownloadStatus::Done, None, None);
        }
    });
    Ok(())
}

/// The waveform of a cached track, or `None` if it isn't cached or can't be decoded.
pub async fn waveform(id: &str) -> Option<Vec<u8>> {
    let cache = CACHE.get()?;
    if !is_valid_id(id) {
        return None;
    }
    cache.waveform(id).await
}

/// Parses a line printed by yt-dlp for `--progress-template` into the fraction downloaded,
/// `None` if the size isn't known. Fields yt-dlp doesn't know are printed as `NA`.
fn parse_progress(line: &str) -> Option<Option<f64>> {
    let mut fields = line.strip_prefix(PROGRESS_PREFIX)?.split_whitespace();
    if fields.next()? != "downloading" {
        return None;
    }
    let mut number = || fields.next().and_then(|f| f.parse::<f64>().ok());
    let (downloaded, total, estimate) = (number(), number(), number());
    let fraction = match (downloaded, total.or(estimate)) {
        (Some(done), Some(total)) if total > 0.0 => Some((done / total).clamp(0.0, 1.0)),
        _ => None,
    };
    Some(fraction)
}

async fn yt_dlp_version(yt_dlp: &str) -> anyhow::Result<String> {
    let output = tokio::time::timeout(
        Duration::from_secs(30),
        tokio::process::Command::new(yt_dlp)
            .args(["--ignore-config", "--version"])
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .with_context(|| format!("`{yt_dlp} --version` timed out"))?
    .with_context(|| format!("can't run `{yt_dlp}`"))?;
    anyhow::ensure!(output.status.success(), "`{yt_dlp} --version` failed");
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// YouTube video ids are 11 characters of URL-safe base64. Checking this keeps ids out of
/// paths and yt-dlp's option parsing (an id may start with `-`).
fn is_valid_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn content_type(path: &Path) -> &'static str {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    AUDIO_TYPES
        .iter()
        .find(|(e, _)| *e == ext)
        .map_or("application/octet-stream", |(_, t)| *t)
}

fn keys_match(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0, |acc, (x, y)| acc | (x ^ y))
            == 0
}

impl AudioCache {
    pub async fn new(dir: PathBuf, limit_bytes: u64, yt_dlp: String) -> anyhow::Result<Self> {
        tokio::fs::create_dir_all(&dir)
            .await
            .with_context(|| format!("can't create {}", dir.display()))?;
        let lock = {
            let dir = dir.clone();
            tokio::task::spawn_blocking(move || lock_and_clean(&dir)).await??
        };
        Ok(Self {
            dir,
            limit_bytes,
            yt_dlp,
            key: uuid::Uuid::new_v4().simple().to_string(),
            downloads: Semaphore::new(MAX_CONCURRENT_DOWNLOADS),
            waveforms: Semaphore::new(MAX_CONCURRENT_WAVEFORMS),
            in_flight: Mutex::new(HashMap::new()),
            failures: Mutex::new(HashMap::new()),
            active: Mutex::new(HashMap::new()),
            updates: broadcast::channel(1024).0,
            _lock: lock,
        })
    }

    pub async fn subscribe(&self) -> (Snapshot, broadcast::Receiver<CacheEvent>) {
        // Subscribe first so no change falls between the snapshot and the receiver.
        let receiver = self.updates.subscribe();
        let downloads = self.active.lock().unwrap().values().cloned().collect();
        let cached = self.cached_ids().await.unwrap_or_else(|e| {
            tracing::warn!("Can't list the audio cache: {e:#}");
            Vec::new()
        });
        (Snapshot { cached, downloads }, receiver)
    }

    async fn cached_ids(&self) -> anyhow::Result<Vec<String>> {
        let mut ids = Vec::new();
        let mut entries = tokio::fs::read_dir(&self.dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if is_valid_id(id) && AUDIO_TYPES.iter().any(|(e, _)| *e == ext) {
                ids.push(id.to_string());
            }
        }
        Ok(ids)
    }

    fn report(
        &self,
        id: &str,
        status: AudioDownloadStatus,
        progress: Option<f64>,
        error: Option<Arc<str>>,
    ) {
        let update = DownloadUpdate {
            yt_id: id.to_string(),
            status,
            progress,
            error,
        };
        {
            let mut active = self.active.lock().unwrap();
            match status {
                AudioDownloadStatus::Done | AudioDownloadStatus::Failed => active.remove(id),
                _ => active.insert(id.to_string(), update.clone()),
            };
        }
        // No receivers just means no session is connected.
        let _ = self.updates.send(CacheEvent::Download(update));
    }

    async fn cached(&self, id: &str) -> Option<PathBuf> {
        for (ext, _) in AUDIO_TYPES {
            let path = self.dir.join(format!("{id}.{ext}"));
            if tokio::fs::metadata(&path).await.is_ok_and(|m| m.is_file()) {
                return Some(path);
            }
        }
        None
    }

    /// The cached file for `id`, downloading it first if needed. Concurrent requests for the same
    /// id share one download, which runs to completion even if every requester goes away.
    pub async fn get(self: &Arc<Self>, id: &str) -> Result<PathBuf, Arc<str>> {
        if let Some(path) = self.cached(id).await {
            return Ok(path);
        }
        {
            let mut failures = self.failures.lock().unwrap();
            failures.retain(|_, (at, _)| at.elapsed() < FAILURE_TTL);
            if let Some((_, error)) = failures.get(id) {
                return Err(error.clone());
            }
        }
        let download = self
            .in_flight
            .lock()
            .unwrap()
            .entry(id.to_string())
            .or_insert_with(|| {
                let this = self.clone();
                let id = id.to_string();
                self.report(&id, AudioDownloadStatus::Queued, None, None);
                let task = tokio::spawn(async move {
                    let result = this
                        .download(&id)
                        .await
                        .map_err(|e| Arc::<str>::from(format!("{e:#}")));
                    let (status, error) = match &result {
                        Ok(_) => (AudioDownloadStatus::Done, None),
                        Err(error) => (AudioDownloadStatus::Failed, Some(error.clone())),
                    };
                    this.report(&id, status, None, error);
                    if let Err(error) = &result {
                        tracing::warn!("Audio download of {id} failed: {error}");
                        this.failures
                            .lock()
                            .unwrap()
                            .insert(id.clone(), (Instant::now(), error.clone()));
                    }
                    this.in_flight.lock().unwrap().remove(&id);
                    result
                });
                async move {
                    task.await
                        .unwrap_or_else(|e| Err(Arc::from(format!("download task failed: {e}"))))
                }
                .boxed()
                .shared()
            })
            .clone();
        download.await
    }

    async fn download(&self, id: &str) -> anyhow::Result<PathBuf> {
        let _permit = self.downloads.acquire().await?;
        // A download that finished just before this one was registered.
        if let Some(path) = self.cached(id).await {
            return Ok(path);
        }
        // Before the transfer starts, yt-dlp spends a few seconds extracting the stream URL.
        self.report(id, AudioDownloadStatus::Preparing, None, None);
        let started = Instant::now();
        let tmp = tempfile::Builder::new()
            .prefix(".dl-")
            .tempdir_in(&self.dir)
            .context("can't create download directory")?;
        let mut child = tokio::process::Command::new(&self.yt_dlp)
            .arg("--ignore-config")
            .args(["--no-playlist", "--no-mtime", "--quiet"])
            // `--progress` overrides `--quiet` for progress, which then goes to stdout.
            .args(["--progress", "--newline", "--progress-template"])
            .arg(format!(
                "download:{PROGRESS_PREFIX}%(progress.status)s %(progress.downloaded_bytes)s \
                 %(progress.total_bytes)s %(progress.total_bytes_estimate)s"
            ))
            .args(["--format", FORMAT])
            .arg("--output")
            .arg(tmp.path().join("audio.%(ext)s"))
            .arg("--")
            .arg(format!("https://www.youtube.com/watch?v={id}"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .context("can't run yt-dlp")?;
        let stdout = child.stdout.take().context("no yt-dlp stdout")?;
        let mut stderr_pipe = child.stderr.take().context("no yt-dlp stderr")?;

        let progress = async {
            let mut lines = BufReader::new(stdout).lines();
            let mut last_report: Option<Instant> = None;
            while let Ok(Some(line)) = lines.next_line().await {
                let Some(fraction) = parse_progress(&line) else {
                    continue;
                };
                if last_report.is_none_or(|at| at.elapsed() >= PROGRESS_INTERVAL) {
                    last_report = Some(Instant::now());
                    self.report(id, AudioDownloadStatus::Downloading, fraction, None);
                }
            }
        };
        let stderr = async {
            let mut buf = Vec::new();
            let _ = stderr_pipe.read_to_end(&mut buf).await;
            buf
        };
        let (_, stderr, status) = tokio::time::timeout(DOWNLOAD_TIMEOUT, async {
            tokio::join!(progress, stderr, child.wait())
        })
        .await
        .context("yt-dlp timed out")?;
        let status = status.context("can't run yt-dlp")?;

        if !status.success() {
            let stderr = String::from_utf8_lossy(&stderr);
            let mut lines = stderr.lines().map(str::trim).filter(|l| !l.is_empty());
            let message = lines
                .clone()
                .rfind(|l| l.starts_with("ERROR:"))
                .or_else(|| lines.next_back())
                .unwrap_or("no error output");
            anyhow::bail!("yt-dlp failed: {message}");
        }

        let mut file = None;
        let mut entries = tokio::fs::read_dir(tmp.path()).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if AUDIO_TYPES.iter().any(|(e, _)| *e == ext) {
                file = Some((path.clone(), ext.to_string()));
            }
        }
        let (file, ext) = file.context("yt-dlp produced no audio file")?;
        let dest = self.dir.join(format!("{id}.{ext}"));
        tokio::fs::rename(&file, &dest).await?;
        tracing::info!("Downloaded audio of {id} in {:.1?}", started.elapsed());

        if let Err(e) = self.evict(&dest).await {
            tracing::warn!("Audio cache eviction failed: {e:#}");
        }
        Ok(dest)
    }

    fn waveform_path(&self, id: &str) -> PathBuf {
        self.dir.join(WAVEFORM_DIR).join(format!("{id}.peaks"))
    }

    async fn waveform(&self, id: &str) -> Option<Vec<u8>> {
        let path = self.waveform_path(id);
        if let Ok(peaks) = tokio::fs::read(&path).await {
            if peaks.len() == waveform::BUCKETS {
                return Some(peaks);
            }
        }
        let audio = self.cached(id).await?;
        let _permit = self.waveforms.acquire().await.ok()?;
        let dir = self.dir.join(WAVEFORM_DIR);
        let result = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<u8>> {
            let started = Instant::now();
            let peaks = waveform::compute(&audio)?;
            tracing::debug!(
                "Computed the waveform of {} in {:.1?}",
                audio.display(),
                started.elapsed()
            );
            // Written whole and renamed, so a concurrent request never reads half a file.
            std::fs::create_dir_all(&dir)?;
            let mut tmp = tempfile::NamedTempFile::new_in(&dir)?;
            std::io::Write::write_all(&mut tmp, &peaks)?;
            tmp.persist(&path)?;
            Ok(peaks)
        })
        .await;
        match result {
            Ok(Ok(peaks)) => Some(peaks),
            Ok(Err(e)) => {
                tracing::debug!("No waveform for {id}: {e:#}");
                None
            }
            Err(e) => {
                tracing::warn!("Waveform task for {id} failed: {e}");
                None
            }
        }
    }

    /// Deletes the least recently played files until the cache fits its limit, keeping `keep`.
    async fn evict(&self, keep: &Path) -> anyhow::Result<()> {
        let mut files = Vec::new();
        let mut total = 0u64;
        let mut entries = tokio::fs::read_dir(&self.dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let meta = entry.metadata().await?;
            if !meta.is_file() {
                continue;
            }
            total += meta.len();
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            files.push((modified, meta.len(), entry.path()));
        }
        files.sort();
        let mut evicted = Vec::new();
        let mut result = Ok(());
        for (_, len, path) in files {
            if total <= self.limit_bytes {
                break;
            }
            if path == keep {
                continue;
            }
            // A file that is being served stays readable until it is closed.
            if let Err(e) = tokio::fs::remove_file(&path).await {
                result = Err(e.into());
                break;
            }
            total -= len;
            tracing::debug!("Evicted {} from the audio cache", path.display());
            if let Some(id) = path.file_stem().and_then(|s| s.to_str()) {
                let _ = tokio::fs::remove_file(self.waveform_path(id)).await;
                evicted.push(id.to_string());
            }
        }
        if !evicted.is_empty() {
            let _ = self.updates.send(CacheEvent::Evicted(evicted));
        }
        result
    }
}

/// Takes a shared lock on the cache dir, first deleting the leftovers of downloads interrupted by
/// a shutdown if no other process uses the cache: otherwise they may be its live downloads. The
/// lock is `None` if the file system doesn't support locking, and then nothing is deleted.
fn lock_and_clean(dir: &Path) -> anyhow::Result<Option<std::fs::File>> {
    let path = dir.join(LOCK_FILE);
    let lock = std::fs::File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .with_context(|| format!("can't open {}", path.display()))?;
    match lock.try_lock() {
        Ok(()) => {
            for entry in std::fs::read_dir(dir)?.flatten() {
                if entry.file_name().to_string_lossy().starts_with(".dl-") {
                    let _ = std::fs::remove_dir_all(entry.path());
                }
            }
            lock.unlock()?;
        }
        // Another process uses the cache; its leftovers wait until a process has it to itself.
        Err(std::fs::TryLockError::WouldBlock) => {}
        Err(std::fs::TryLockError::Error(e)) => {
            tracing::warn!("Can't lock {}: {e}", path.display());
            return Ok(None);
        }
    }
    // Waits only while another process that just started cleans up.
    lock.lock_shared()
        .with_context(|| format!("can't lock {}", path.display()))?;
    Ok(Some(lock))
}

/// Marks a file as recently played, for eviction.
fn touch(path: &Path) {
    let recent = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .is_ok_and(|t| t.elapsed().is_ok_and(|age| age < TOUCH_INTERVAL));
    if !recent {
        if let Err(e) = std::fs::File::options()
            .append(true)
            .open(path)
            .and_then(|f| f.set_modified(SystemTime::now()))
        {
            tracing::debug!("Can't update mtime of {}: {e}", path.display());
        }
    }
}

#[derive(Deserialize)]
pub struct AudioQuery {
    key: Option<String>,
}

pub async fn handler(
    UrlPath(id): UrlPath<String>,
    Query(query): Query<AudioQuery>,
    request: Request,
) -> Response {
    match CACHE.get() {
        Some(cache) => serve(cache, &id, query.key.as_deref(), request).await,
        None => (StatusCode::NOT_FOUND, "Audio downloads are disabled").into_response(),
    }
}

async fn serve(cache: &Arc<AudioCache>, id: &str, key: Option<&str>, request: Request) -> Response {
    if !key.is_some_and(|key| keys_match(key, &cache.key)) {
        return (StatusCode::FORBIDDEN, "Invalid audio key").into_response();
    }
    if !is_valid_id(id) {
        return (StatusCode::BAD_REQUEST, "Invalid YouTube id").into_response();
    }
    let path = match cache.get(id).await {
        Ok(path) => path,
        Err(error) => return (StatusCode::BAD_GATEWAY, error.to_string()).into_response(),
    };
    touch(&path);

    let mut response = match ServeFile::new(&path).oneshot(request).await {
        Ok(response) => response.map(Body::new),
        Err(never) => match never {},
    };
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type(&path)),
    );
    // The key changes on restart, so URLs (and thus cache entries) don't outlive the process.
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=86400"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Method;

    #[test]
    fn validates_youtube_ids() {
        assert!(is_valid_id("dQw4w9WgXcQ"));
        assert!(is_valid_id("-abc_DEF123"));
        assert!(!is_valid_id("dQw4w9WgXc"));
        assert!(!is_valid_id("../../etc/p"));
        assert!(!is_valid_id("dQw4w9WgXcQQ"));
        assert!(!is_valid_id("dQw4w9 gXcQ"));
    }

    #[test]
    fn parses_progress_lines() {
        assert_eq!(
            parse_progress("stereo-progress downloading 25 100 NA"),
            Some(Some(0.25))
        );
        assert_eq!(
            parse_progress("stereo-progress downloading 25 NA 50.0"),
            Some(Some(0.5))
        );
        assert_eq!(
            parse_progress("stereo-progress downloading 25 NA NA"),
            Some(None)
        );
        assert_eq!(parse_progress("stereo-progress finished 100 100 NA"), None);
        assert_eq!(parse_progress("[download] 25%"), None);
    }

    #[test]
    fn compares_keys() {
        assert!(keys_match("abc", "abc"));
        assert!(!keys_match("abc", "abd"));
        assert!(!keys_match("abc", "abcd"));
    }

    #[cfg(unix)]
    /// A stand-in for yt-dlp that writes `content` to the requested output and counts its runs.
    fn fake_yt_dlp(dir: &Path, script_body: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("yt-dlp");
        std::fs::write(&path, format!("#!/bin/sh\n{script_body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.display().to_string()
    }

    const WRITE_OUTPUT: &str = r#"
echo run >> "$(dirname "$0")/runs"
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--output" ]; then out="$2"; fi
  shift
done
sleep 0.2
echo "stereo-progress downloading 5 10 NA"
echo "stereo-progress finished 10 10 NA"
printf '0123456789' > "$(echo "$out" | sed 's/%(ext)s/m4a/')"
"#;

    fn request(method: Method, range: Option<&str>) -> Request {
        let mut builder = Request::builder().method(method).uri("/audio/x");
        if let Some(range) = range {
            builder = builder.header(header::RANGE, range);
        }
        builder.body(Body::empty()).unwrap()
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn downloads_once_and_serves_ranges() {
        let tools = tempfile::tempdir().unwrap();
        let cache_dir = tempfile::tempdir().unwrap();
        let cache = Arc::new(
            AudioCache::new(
                cache_dir.path().to_path_buf(),
                u64::MAX,
                fake_yt_dlp(tools.path(), WRITE_OUTPUT),
            )
            .await
            .unwrap(),
        );
        let key = cache.key.clone();
        let (snapshot, mut updates) = cache.subscribe().await;
        assert!(snapshot.downloads.is_empty());
        assert!(snapshot.cached.is_empty());

        let (a, b) = tokio::join!(
            serve(
                &cache,
                "dQw4w9WgXcQ",
                Some(&key),
                request(Method::HEAD, None)
            ),
            serve(
                &cache,
                "dQw4w9WgXcQ",
                Some(&key),
                request(Method::GET, None)
            ),
        );
        assert_eq!(a.status(), StatusCode::OK);
        assert_eq!(b.status(), StatusCode::OK);
        assert_eq!(b.headers()[header::CONTENT_TYPE], "audio/mp4");
        let runs = std::fs::read_to_string(tools.path().join("runs")).unwrap();
        assert_eq!(
            runs.lines().count(),
            1,
            "concurrent requests share one download"
        );

        let mut seen = Vec::new();
        while let Ok(event) = updates.try_recv() {
            let CacheEvent::Download(update) = event else {
                panic!("unexpected {event:?}");
            };
            assert_eq!(update.yt_id, "dQw4w9WgXcQ");
            seen.push((update.status, update.progress));
        }
        assert_eq!(
            seen,
            [
                (AudioDownloadStatus::Queued, None),
                (AudioDownloadStatus::Preparing, None),
                (AudioDownloadStatus::Downloading, Some(0.5)),
                (AudioDownloadStatus::Done, None),
            ]
        );
        let (snapshot, _) = cache.subscribe().await;
        assert!(
            snapshot.downloads.is_empty(),
            "finished downloads aren't active"
        );
        assert_eq!(snapshot.cached, ["dQw4w9WgXcQ"]);
        assert_eq!(
            cache.waveform("dQw4w9WgXcQ").await,
            None,
            "the fake audio can't be decoded"
        );

        let partial = serve(
            &cache,
            "dQw4w9WgXcQ",
            Some(&key),
            request(Method::GET, Some("bytes=2-4")),
        )
        .await;
        assert_eq!(partial.status(), StatusCode::PARTIAL_CONTENT);
        let body = axum::body::to_bytes(partial.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(&body[..], b"234");
        assert!(cache_dir.path().join("dQw4w9WgXcQ.m4a").is_file());
    }

    #[tokio::test]
    async fn rejects_bad_keys_and_ids() {
        let cache_dir = tempfile::tempdir().unwrap();
        let cache = Arc::new(
            AudioCache::new(cache_dir.path().to_path_buf(), u64::MAX, "false".into())
                .await
                .unwrap(),
        );
        let key = cache.key.clone();
        let res = serve(&cache, "dQw4w9WgXcQ", None, request(Method::GET, None)).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        let res = serve(
            &cache,
            "dQw4w9WgXcQ",
            Some("nope"),
            request(Method::GET, None),
        )
        .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        let res = serve(
            &cache,
            "--exec=evil",
            Some(&key),
            request(Method::GET, None),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn reports_and_remembers_failures() {
        let tools = tempfile::tempdir().unwrap();
        let cache_dir = tempfile::tempdir().unwrap();
        let yt_dlp = fake_yt_dlp(
            tools.path(),
            r#"echo run >> "$(dirname "$0")/runs"
echo "WARNING: something" >&2
echo "ERROR: [youtube] dQw4w9WgXcQ: Video unavailable" >&2
exit 1"#,
        );
        let cache = Arc::new(
            AudioCache::new(cache_dir.path().to_path_buf(), u64::MAX, yt_dlp)
                .await
                .unwrap(),
        );
        let (_, mut updates) = cache.subscribe().await;
        for _ in 0..2 {
            let error = cache.get("dQw4w9WgXcQ").await.unwrap_err();
            assert_eq!(
                &*error,
                "yt-dlp failed: ERROR: [youtube] dQw4w9WgXcQ: Video unavailable"
            );
        }
        let runs = std::fs::read_to_string(tools.path().join("runs")).unwrap();
        assert_eq!(
            runs.lines().count(),
            1,
            "failures are not retried right away"
        );
        let mut failure = None;
        while let Ok(event) = updates.try_recv() {
            if let CacheEvent::Download(update) = event {
                if update.status == AudioDownloadStatus::Failed {
                    failure = update.error;
                }
            }
        }
        assert_eq!(
            failure.as_deref(),
            Some("yt-dlp failed: ERROR: [youtube] dQw4w9WgXcQ: Video unavailable"),
            "the failure is reported with its error"
        );
        let (snapshot, _) = cache.subscribe().await;
        assert!(
            snapshot.downloads.is_empty(),
            "failed downloads aren't active"
        );
        let leftovers = std::fs::read_dir(cache_dir.path())
            .unwrap()
            .filter(|e| e.as_ref().unwrap().file_name() != LOCK_FILE)
            .count();
        assert_eq!(leftovers, 0, "the download directory is cleaned up");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn keeps_downloads_of_other_processes_sharing_the_cache() {
        let tools = tempfile::tempdir().unwrap();
        let cache_dir = tempfile::tempdir().unwrap();
        let leftover = cache_dir.path().join(".dl-leftover");
        std::fs::create_dir(&leftover).unwrap();
        // Signals that it started, then waits to be released before writing its output.
        let yt_dlp = fake_yt_dlp(
            tools.path(),
            &format!(
                "touch \"$(dirname \"$0\")/started\"\n\
                 while [ ! -e \"$(dirname \"$0\")/release\" ]; do sleep 0.05; done\n{WRITE_OUTPUT}"
            ),
        );
        let new_cache =
            || AudioCache::new(cache_dir.path().to_path_buf(), u64::MAX, yt_dlp.clone());

        let first = Arc::new(new_cache().await.unwrap());
        assert!(!leftover.exists(), "a sole process cleans up leftovers");
        std::fs::create_dir(&leftover).unwrap();

        let download = tokio::spawn({
            let first = first.clone();
            async move { first.get("dQw4w9WgXcQ").await }
        });
        while !tools.path().join("started").exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let second = new_cache().await.unwrap();
        std::fs::write(tools.path().join("release"), "").unwrap();
        let path = download.await.unwrap().unwrap();
        assert!(path.is_file(), "the first process's download survives");
        assert!(
            leftover.exists(),
            "leftovers stay while the cache is shared"
        );

        drop(first);
        drop(second);
        new_cache().await.unwrap();
        assert!(!leftover.exists());
    }

    #[tokio::test]
    async fn evicts_least_recently_played_files() {
        let cache_dir = tempfile::tempdir().unwrap();
        let cache = AudioCache::new(cache_dir.path().to_path_buf(), 25, "false".into())
            .await
            .unwrap();
        let (_, mut updates) = cache.subscribe().await;
        let now = SystemTime::now();
        let waveforms = cache_dir.path().join(WAVEFORM_DIR);
        std::fs::create_dir(&waveforms).unwrap();
        for (i, id) in ["oldoldoldol", "midmidmidmi", "newnewnewne"]
            .iter()
            .enumerate()
        {
            std::fs::write(waveforms.join(format!("{id}.peaks")), [0u8; 3]).unwrap();
            let path = cache_dir.path().join(format!("{id}.m4a"));
            std::fs::write(&path, [0u8; 10]).unwrap();
            std::fs::File::options()
                .append(true)
                .open(&path)
                .unwrap()
                .set_modified(now - Duration::from_secs(100 - 10 * i as u64))
                .unwrap();
        }
        cache
            .evict(&cache_dir.path().join("oldoldoldol.m4a"))
            .await
            .unwrap();
        assert!(
            cache_dir.path().join("oldoldoldol.m4a").exists(),
            "kept file survives"
        );
        assert!(!cache_dir.path().join("midmidmidmi.m4a").exists());
        assert!(cache_dir.path().join("newnewnewne.m4a").exists());
        assert!(
            !waveforms.join("midmidmidmi.peaks").exists(),
            "the waveform goes with its audio"
        );
        assert!(waveforms.join("newnewnewne.peaks").exists());
        assert_eq!(
            updates.try_recv().unwrap(),
            CacheEvent::Evicted(vec!["midmidmidmi".to_string()])
        );
        let (snapshot, _) = cache.subscribe().await;
        let mut cached = snapshot.cached;
        cached.sort();
        assert_eq!(cached, ["newnewnewne", "oldoldoldol"]);
    }
}
