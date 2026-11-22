//! Downloads of a collection's SQLite file. `MsgClient::PrepareCollectionDownload` snapshots the
//! open collection with `VACUUM INTO`, which is consistent even while another session writes to
//! it, and hands back a token for `GET /collection-download/{token}`. A plain link can't send the
//! WebSocket token, so the token stands in for it: random, single use and short-lived, like a
//! pairing code. Snapshots wait in temp files, deleted once streamed, replaced or expired.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::Path as UrlPath;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, MethodRouter};
use futures::StreamExt;
use tokio::sync::Semaphore;

/// How long a prepared download waits to be fetched.
const LIFETIME: Duration = Duration::from_secs(120);
/// At most this many prepared downloads are kept; preparing another drops the oldest.
const MAX_PENDING: usize = 4;
/// At most this many snapshots are taken at once, so that, with `MAX_PENDING`, few copies of a
/// collection exist at a time.
const MAX_PREPARING: usize = 2;

struct Snapshot {
    /// Holds the file; dropping it deletes the file.
    dir: tempfile::TempDir,
    path: PathBuf,
    file_name: String,
    created: Instant,
}

static PENDING: LazyLock<Mutex<HashMap<String, Snapshot>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static PREPARING: Semaphore = Semaphore::const_new(MAX_PREPARING);

/// The route for `/collection-download/{token}`. Without its own `HEAD` handler axum would answer
/// `HEAD` with the `GET` handler, spending the token without delivering the file.
pub fn route<S: Clone + Send + Sync + 'static>() -> MethodRouter<S> {
    get(handler).head(head_handler)
}

/// Snapshots the collection at `path`, open in `pool`, and returns the token to download it with.
pub async fn prepare(pool: &sqlx::SqlitePool, path: &Path) -> anyhow::Result<String> {
    let _permit = PREPARING
        .try_acquire()
        .map_err(|_| anyhow::anyhow!("Other downloads are being prepared, try again shortly"))?;
    let dir = tempfile::tempdir()?;
    let dest = dir.path().join("collection.db");
    sqlx::query("VACUUM INTO ?")
        .bind(dest.to_string_lossy().as_ref())
        .execute(pool)
        .await?;
    let file_name = path.file_name().map_or_else(
        || "collection.db".into(),
        |n| n.to_string_lossy().into_owned(),
    );
    let token = uuid::Uuid::new_v4().simple().to_string();
    let snapshot = Snapshot {
        dir,
        path: dest,
        file_name,
        created: Instant::now(),
    };
    let dropped = insert(&mut lock(), token.clone(), snapshot);
    drop(dropped); // deletes their files, outside the lock
    let expired = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(LIFETIME).await;
        let snapshot = lock().remove(&expired);
        drop(snapshot);
    });
    Ok(token)
}

fn lock() -> std::sync::MutexGuard<'static, HashMap<String, Snapshot>> {
    PENDING.lock().expect("collection downloads poisoned")
}

/// Adds `snapshot`, returning the expired snapshots and the oldest beyond `MAX_PENDING`.
fn insert(
    pending: &mut HashMap<String, Snapshot>,
    token: String,
    snapshot: Snapshot,
) -> Vec<Snapshot> {
    let now = snapshot.created;
    let expired: Vec<String> = pending
        .iter()
        .filter(|(_, s)| now.duration_since(s.created) >= LIFETIME)
        .map(|(t, _)| t.clone())
        .collect();
    let mut dropped: Vec<Snapshot> = expired.iter().filter_map(|t| pending.remove(t)).collect();
    while pending.len() >= MAX_PENDING {
        let oldest = pending
            .iter()
            .min_by_key(|(_, s)| s.created)
            .map(|(t, _)| t.clone());
        match oldest.and_then(|t| pending.remove(&t)) {
            Some(s) => dropped.push(s),
            None => break,
        }
    }
    pending.insert(token, snapshot);
    dropped
}

fn take(pending: &mut HashMap<String, Snapshot>, token: &str, now: Instant) -> Option<Snapshot> {
    pending
        .remove(token)
        .filter(|s| now.duration_since(s.created) < LIFETIME)
}

fn expired() -> Response {
    (StatusCode::NOT_FOUND, "Download expired, try again").into_response()
}

async fn handler(UrlPath(token): UrlPath<String>) -> Response {
    let snapshot = take(&mut lock(), &token, Instant::now());
    let Some(snapshot) = snapshot else {
        return expired();
    };
    let file = match tokio::fs::File::open(&snapshot.path).await {
        Ok(file) => file,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let len = file.metadata().await.map(|m| m.len()).ok();
    let headers = headers(&snapshot.file_name, len);
    // The stream owns the temp dir, so the file is deleted once it is sent (or the client goes).
    let dir = snapshot.dir;
    let body = Body::from_stream(tokio_util::io::ReaderStream::new(file).map(move |chunk| {
        let _ = &dir;
        chunk
    }));
    (headers, body).into_response()
}

/// The `GET` response's headers, leaving the token for the `GET`.
async fn head_handler(UrlPath(token): UrlPath<String>) -> Response {
    let found = lock()
        .get(&token)
        .filter(|s| s.created.elapsed() < LIFETIME)
        .map(|s| (s.file_name.clone(), s.path.clone()));
    let Some((file_name, path)) = found else {
        return expired();
    };
    let len = tokio::fs::metadata(&path).await.map(|m| m.len()).ok();
    headers(&file_name, len).into_response()
}

fn headers(file_name: &str, len: Option<u64>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/vnd.sqlite3"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&content_disposition(file_name))
            .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Some(len) = len {
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    }
    headers
}

/// An attachment named `file_name`: an ASCII fallback plus the RFC 5987 UTF-8 name.
fn content_disposition(file_name: &str) -> String {
    let ascii: String = file_name
        .chars()
        .map(|c| match c {
            ' '..='~' if c != '"' && c != '\\' => c,
            _ => '_',
        })
        .collect();
    let mut encoded = String::new();
    for b in file_name.bytes() {
        if b.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&b) {
            encoded.push(b as char);
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{Method, Request};
    use tower::ServiceExt;

    fn snapshot(created: Instant) -> Snapshot {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("collection.db");
        std::fs::write(&path, b"x").unwrap();
        Snapshot {
            dir,
            path,
            file_name: "c.db".into(),
            created,
        }
    }

    #[test]
    fn tokens_are_single_use_and_expire() {
        let mut pending = HashMap::new();
        let t0 = Instant::now();
        insert(&mut pending, "a".into(), snapshot(t0));
        insert(&mut pending, "b".into(), snapshot(t0));
        assert!(take(&mut pending, "a", t0).is_some());
        assert!(take(&mut pending, "a", t0).is_none());
        assert!(take(&mut pending, "b", t0 + LIFETIME).is_none());
    }

    #[test]
    fn drops_expired_and_oldest_beyond_limit() {
        let mut pending = HashMap::new();
        let t0 = Instant::now();
        let mut files = Vec::new();
        for i in 0..MAX_PENDING {
            let s = snapshot(t0 + Duration::from_millis(i as u64));
            files.push(s.path.clone());
            assert!(insert(&mut pending, i.to_string(), s).is_empty());
        }
        let dropped = insert(&mut pending, "new".into(), snapshot(t0 + LIFETIME));
        assert_eq!(dropped.len(), 1, "only the first has expired");
        drop(dropped);
        assert!(!files[0].exists());
        let later = t0 + LIFETIME + Duration::from_millis(10);
        let dropped = insert(&mut pending, "newer".into(), snapshot(later));
        assert_eq!(dropped.len(), 3, "the rest have expired too");
        assert_eq!(pending.len(), 2);
    }

    #[test]
    fn encodes_file_names() {
        assert_eq!(
            content_disposition("Tänze \"1\".db"),
            "attachment; filename=\"T_nze _1_.db\"; filename*=UTF-8''T%C3%A4nze%20%221%22.db"
        );
    }

    #[tokio::test]
    async fn serves_snapshot_once_after_head() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mine.db");
        let pool = crate::db::open_pool(&path).await.unwrap();
        crate::db::init_db(&pool).await.unwrap();
        let token = prepare(&pool, &path).await.unwrap();
        let file = lock()[&token].path.clone();
        let router = axum::Router::new().route("/collection-download/{token}", route());
        let request = |method: Method| {
            Request::builder()
                .method(method)
                .uri(format!("/collection-download/{token}"))
                .body(Body::empty())
                .unwrap()
        };

        let head = router.clone().oneshot(request(Method::HEAD)).await.unwrap();
        assert_eq!(head.status(), StatusCode::OK);
        assert!(file.exists(), "HEAD leaves the token");

        let response = router.clone().oneshot(request(Method::GET)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_DISPOSITION],
            "attachment; filename=\"mine.db\"; filename*=UTF-8''mine.db"
        );
        assert!(file.exists(), "kept until sent");
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(!file.exists(), "deleted once sent");
        let copy = dir.path().join("copy.db");
        std::fs::write(&copy, &body).unwrap();
        assert!(crate::db::validate_db_schema(&copy).await);

        let again = router.oneshot(request(Method::GET)).await.unwrap();
        assert_eq!(again.status(), StatusCode::NOT_FOUND);
    }
}
