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
use tokio::sync::Semaphore;
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::settings::Settings;

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

type Download = Shared<BoxFuture<'static, Result<PathBuf, Arc<str>>>>;

pub struct AudioCache {
    dir: PathBuf,
    limit_bytes: u64,
    yt_dlp: String,
    key: String,
    downloads: Semaphore,
    in_flight: Mutex<HashMap<String, Download>>,
    failures: Mutex<HashMap<String, (Instant, Arc<str>)>>,
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
        // Leftovers of downloads interrupted by a shutdown.
        if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if entry.file_name().to_string_lossy().starts_with(".dl-") {
                    let _ = tokio::fs::remove_dir_all(entry.path()).await;
                }
            }
        }
        Ok(Self {
            dir,
            limit_bytes,
            yt_dlp,
            key: uuid::Uuid::new_v4().simple().to_string(),
            downloads: Semaphore::new(MAX_CONCURRENT_DOWNLOADS),
            in_flight: Mutex::new(HashMap::new()),
            failures: Mutex::new(HashMap::new()),
        })
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
                let task = tokio::spawn(async move {
                    let result = this
                        .download(&id)
                        .await
                        .map_err(|e| Arc::<str>::from(format!("{e:#}")));
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
        let started = Instant::now();
        let tmp = tempfile::Builder::new()
            .prefix(".dl-")
            .tempdir_in(&self.dir)
            .context("can't create download directory")?;
        let output = tokio::time::timeout(
            DOWNLOAD_TIMEOUT,
            tokio::process::Command::new(&self.yt_dlp)
                .arg("--ignore-config")
                .args(["--no-playlist", "--no-progress", "--no-mtime", "--quiet"])
                .args(["--format", FORMAT])
                .arg("--output")
                .arg(tmp.path().join("audio.%(ext)s"))
                .arg("--")
                .arg(format!("https://www.youtube.com/watch?v={id}"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .output(),
        )
        .await
        .context("yt-dlp timed out")?
        .context("can't run yt-dlp")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
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
        for (_, len, path) in files {
            if total <= self.limit_bytes {
                break;
            }
            if path == keep {
                continue;
            }
            // A file that is being served stays readable until it is closed.
            tokio::fs::remove_file(&path).await?;
            total -= len;
            tracing::debug!("Evicted {} from the audio cache", path.display());
        }
        Ok(())
    }
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
        let leftovers = std::fs::read_dir(cache_dir.path()).unwrap().count();
        assert_eq!(leftovers, 0, "the download directory is cleaned up");
    }

    #[tokio::test]
    async fn evicts_least_recently_played_files() {
        let cache_dir = tempfile::tempdir().unwrap();
        let cache = AudioCache::new(cache_dir.path().to_path_buf(), 25, "false".into())
            .await
            .unwrap();
        let now = SystemTime::now();
        for (i, name) in ["old.m4a", "mid.m4a", "new.m4a"].iter().enumerate() {
            let path = cache_dir.path().join(name);
            std::fs::write(&path, [0u8; 10]).unwrap();
            std::fs::File::options()
                .append(true)
                .open(&path)
                .unwrap()
                .set_modified(now - Duration::from_secs(100 - 10 * i as u64))
                .unwrap();
        }
        cache
            .evict(&cache_dir.path().join("old.m4a"))
            .await
            .unwrap();
        assert!(
            cache_dir.path().join("old.m4a").exists(),
            "kept file survives"
        );
        assert!(!cache_dir.path().join("mid.m4a").exists());
        assert!(cache_dir.path().join("new.m4a").exists());
    }
}
