//! Downloads of a collection's SQLite file. `MsgClient::PrepareCollectionDownload` snapshots the
//! open collection with `VACUUM INTO`, which is consistent even while another session writes to
//! it, and hands back a token for `GET /collection-download/{token}`. A plain link can't send the
//! WebSocket token, so the token stands in for it: random, single use and short-lived, like a
//! pairing code. Snapshots are kept in memory until fetched; collections are small.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use axum::extract::Path as UrlPath;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

/// How long a prepared download waits to be fetched.
const LIFETIME: Duration = Duration::from_secs(120);
/// At most this many prepared downloads are kept; preparing another drops the oldest.
const MAX_PENDING: usize = 4;

struct Snapshot {
    file_name: String,
    data: Vec<u8>,
    created: Instant,
}

static PENDING: LazyLock<Mutex<HashMap<String, Snapshot>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Snapshots the collection at `path`, open in `pool`, and returns the token to download it with.
pub async fn prepare(pool: &sqlx::SqlitePool, path: &Path) -> anyhow::Result<String> {
    let dir = tempfile::tempdir()?;
    let dest = dir.path().join("collection.db");
    sqlx::query("VACUUM INTO ?")
        .bind(dest.to_string_lossy().as_ref())
        .execute(pool)
        .await?;
    let data = tokio::fs::read(&dest).await?;
    let file_name = path.file_name().map_or_else(
        || "collection.db".into(),
        |n| n.to_string_lossy().into_owned(),
    );
    let token = uuid::Uuid::new_v4().simple().to_string();
    let mut pending = PENDING.lock().expect("collection downloads poisoned");
    insert(&mut pending, token.clone(), file_name, data, Instant::now());
    Ok(token)
}

fn insert(
    pending: &mut HashMap<String, Snapshot>,
    token: String,
    file_name: String,
    data: Vec<u8>,
    now: Instant,
) {
    pending.retain(|_, s| now.duration_since(s.created) < LIFETIME);
    while pending.len() >= MAX_PENDING {
        let oldest = pending
            .iter()
            .min_by_key(|(_, s)| s.created)
            .map(|(t, _)| t.clone());
        match oldest {
            Some(t) => pending.remove(&t),
            None => break,
        };
    }
    pending.insert(
        token,
        Snapshot {
            file_name,
            data,
            created: now,
        },
    );
}

fn take(pending: &mut HashMap<String, Snapshot>, token: &str, now: Instant) -> Option<Snapshot> {
    pending
        .remove(token)
        .filter(|s| now.duration_since(s.created) < LIFETIME)
}

pub async fn handler(UrlPath(token): UrlPath<String>) -> Response {
    let snapshot = take(
        &mut PENDING.lock().expect("collection downloads poisoned"),
        &token,
        Instant::now(),
    );
    let Some(snapshot) = snapshot else {
        return (StatusCode::NOT_FOUND, "Download expired, try again").into_response();
    };
    let disposition = content_disposition(&snapshot.file_name);
    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/vnd.sqlite3"),
            ),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(&disposition)
                    .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
            ),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
        snapshot.data,
    )
        .into_response()
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

    #[test]
    fn tokens_are_single_use_and_expire() {
        let mut pending = HashMap::new();
        let t0 = Instant::now();
        insert(&mut pending, "a".into(), "a.db".into(), vec![1], t0);
        insert(&mut pending, "b".into(), "b.db".into(), vec![2], t0);
        assert_eq!(take(&mut pending, "a", t0).map(|s| s.data), Some(vec![1]));
        assert!(take(&mut pending, "a", t0).is_none());
        assert!(take(&mut pending, "b", t0 + LIFETIME).is_none());
    }

    #[test]
    fn drops_oldest_beyond_limit() {
        let mut pending = HashMap::new();
        let t0 = Instant::now();
        for i in 0..=MAX_PENDING {
            let t = t0 + Duration::from_millis(i as u64);
            insert(&mut pending, i.to_string(), "c.db".into(), vec![], t);
        }
        assert_eq!(pending.len(), MAX_PENDING);
        assert!(!pending.contains_key("0"));
    }

    #[test]
    fn encodes_file_names() {
        assert_eq!(
            content_disposition("Tänze \"1\".db"),
            "attachment; filename=\"T_nze _1_.db\"; filename*=UTF-8''T%C3%A4nze%20%221%22.db"
        );
    }

    #[tokio::test]
    async fn snapshots_open_collection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mine.db");
        let pool = crate::db::open_pool(&path).await.unwrap();
        crate::db::init_db(&pool).await.unwrap();
        let token = prepare(&pool, &path).await.unwrap();
        let response = handler(UrlPath(token.clone())).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_DISPOSITION],
            "attachment; filename=\"mine.db\"; filename*=UTF-8''mine.db"
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let copy = dir.path().join("copy.db");
        std::fs::write(&copy, &body).unwrap();
        assert!(crate::db::validate_db_schema(&copy).await);
        let again = handler(UrlPath(token)).await;
        assert_eq!(again.status(), StatusCode::NOT_FOUND);
    }
}
