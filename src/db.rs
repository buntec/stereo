use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use anyhow::Context;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow},
    Row, SqlitePool,
};

use crate::{
    message::{FilterModel, FilterModelItem, FilterModelMap, SortModelItem},
    models::{NewReleasesCandidate, ScoredName, Track},
};

const VALID_COLUMNS: &[&str] = &[
    "yt_id",
    "bp_id",
    "sp_id",
    "mb_id",
    "title",
    "mix_name",
    "artists",
    "release_date",
    "label",
    "album",
    "length",
    "bpm",
    "genre",
    "key",
    "mood",
    "rating",
    "play_count",
    "last_played",
];

pub async fn open_pool(path: &Path) -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Delete)
        .busy_timeout(std::time::Duration::from_secs(5));
    Ok(SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await?)
}

pub async fn init_db(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS tracks (
            yt_id TEXT PRIMARY KEY,
            bp_id INTEGER,
            sp_id TEXT,
            mb_id TEXT,
            title TEXT NOT NULL,
            mix_name TEXT,
            artists TEXT NOT NULL,
            release_date TEXT,
            label TEXT,
            album TEXT,
            length INTEGER,
            bpm INTEGER,
            genre TEXT,
            key TEXT,
            mood TEXT,
            rating INTEGER,
            play_count INTEGER DEFAULT 0,
            last_played TEXT
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_track(
    pool: &SqlitePool,
    track: &Track,
    ignore_if_exists: bool,
) -> anyhow::Result<()> {
    let on_conflict = if ignore_if_exists {
        "IGNORE"
    } else {
        "REPLACE"
    };
    let sql = format!(
        "INSERT OR {on_conflict} INTO tracks \
         (yt_id, bp_id, sp_id, mb_id, title, mix_name, artists, release_date, label, album, \
          length, bpm, genre, key, mood, rating, play_count, last_played) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    );
    sqlx::query(&sql)
        .bind(&track.yt_id)
        .bind(track.bp_id)
        .bind(&track.sp_id)
        .bind(&track.mb_id)
        .bind(&track.title)
        .bind(&track.mix_name)
        .bind(track.artists_json())
        .bind(track.release_date_str())
        .bind(&track.label)
        .bind(&track.album)
        .bind(track.length)
        .bind(track.bpm)
        .bind(&track.genre)
        .bind(&track.key)
        .bind(&track.mood)
        .bind(track.rating)
        .bind(track.play_count)
        .bind(track.last_played_str())
        .execute(pool)
        .await?;
    Ok(())
}

/// Inserts `tracks`, returning how many were written: with `ignore_existing`, those not already
/// in the collection.
pub async fn insert_tracks(
    pool: &SqlitePool,
    tracks: &[Track],
    ignore_existing: bool,
) -> anyhow::Result<u64> {
    if tracks.is_empty() {
        return Ok(0);
    }
    let on_conflict = if ignore_existing { "IGNORE" } else { "REPLACE" };
    let sql = format!(
        "INSERT OR {on_conflict} INTO tracks \
         (yt_id, bp_id, sp_id, mb_id, title, mix_name, artists, release_date, label, album, \
          length, bpm, genre, key, mood, rating, play_count, last_played) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    );
    let mut tx = pool.begin().await?;
    let mut written = 0;
    for track in tracks {
        written += sqlx::query(&sql)
            .bind(&track.yt_id)
            .bind(track.bp_id)
            .bind(&track.sp_id)
            .bind(&track.mb_id)
            .bind(&track.title)
            .bind(&track.mix_name)
            .bind(track.artists_json())
            .bind(track.release_date_str())
            .bind(&track.label)
            .bind(&track.album)
            .bind(track.length)
            .bind(track.bpm)
            .bind(&track.genre)
            .bind(&track.key)
            .bind(&track.mood)
            .bind(track.rating)
            .bind(track.play_count)
            .bind(track.last_played_str())
            .execute(&mut *tx)
            .await?
            .rows_affected();
    }
    tx.commit().await?;
    Ok(written)
}

/// Writes `track`'s metadata (everything but the user data: rating, play count, last played) to
/// the row currently keyed by `old_yt_id`, re-keying it if `track.yt_id` differs. Fails rather
/// than overwrite a different track that already has the new id.
pub async fn update_track_metadata(
    pool: &SqlitePool,
    old_yt_id: &str,
    track: &Track,
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    if track.yt_id != old_yt_id {
        let taken = sqlx::query("SELECT 1 FROM tracks WHERE yt_id = ?")
            .bind(&track.yt_id)
            .fetch_optional(&mut *tx)
            .await?
            .is_some();
        anyhow::ensure!(
            !taken,
            "another track with YouTube ID {} is already in the collection",
            track.yt_id
        );
    }
    let updated = sqlx::query(
        "UPDATE tracks SET yt_id = ?, bp_id = ?, sp_id = ?, mb_id = ?, title = ?, mix_name = ?, \
         artists = ?, release_date = ?, label = ?, album = ?, length = ?, bpm = ?, genre = ?, \
         key = ?, mood = ? WHERE yt_id = ?",
    )
    .bind(&track.yt_id)
    .bind(track.bp_id)
    .bind(&track.sp_id)
    .bind(&track.mb_id)
    .bind(&track.title)
    .bind(&track.mix_name)
    .bind(track.artists_json())
    .bind(track.release_date_str())
    .bind(&track.label)
    .bind(&track.album)
    .bind(track.length)
    .bind(track.bpm)
    .bind(&track.genre)
    .bind(&track.key)
    .bind(&track.mood)
    .bind(old_yt_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    anyhow::ensure!(
        updated > 0,
        "track {old_yt_id} is no longer in the collection"
    );
    tx.commit().await?;
    Ok(())
}

pub async fn delete_track(pool: &SqlitePool, yt_id: &str) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM tracks WHERE yt_id = ?")
        .bind(yt_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_tracks(pool: &SqlitePool, yt_ids: &[String]) -> anyhow::Result<u64> {
    if yt_ids.is_empty() {
        return Ok(0);
    }
    let mut tx = pool.begin().await?;
    let mut total = 0u64;
    for id in yt_ids {
        let result = sqlx::query("DELETE FROM tracks WHERE yt_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        total += result.rows_affected();
    }
    tx.commit().await?;
    Ok(total)
}

pub async fn get_track(pool: &SqlitePool, yt_id: &str) -> anyhow::Result<Option<Track>> {
    let row: Option<SqliteRow> = sqlx::query("SELECT * FROM tracks WHERE yt_id = ?")
        .bind(yt_id)
        .fetch_optional(pool)
        .await?;
    row.map(|r| Track::from_db_row(&r)).transpose()
}

pub async fn track_exists(pool: &SqlitePool, yt_id: &str) -> anyhow::Result<bool> {
    let row = sqlx::query("SELECT 1 FROM tracks WHERE yt_id = ? LIMIT 1")
        .bind(yt_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

/// The subset of `yt_ids` present in the collection, in no particular order.
pub async fn existing_yt_ids(pool: &SqlitePool, yt_ids: &[String]) -> anyhow::Result<Vec<String>> {
    if yt_ids.is_empty() {
        return Ok(vec![]);
    }
    // One bound JSON array instead of a placeholder per id, which could exceed SQLite's
    // host-parameter limit for a large search.
    let rows =
        sqlx::query("SELECT yt_id FROM tracks WHERE yt_id IN (SELECT value FROM json_each(?))")
            .bind(serde_json::to_string(yt_ids)?)
            .fetch_all(pool)
            .await?;
    rows.iter()
        .map(|r| r.try_get::<String, _>("yt_id").map_err(Into::into))
        .collect()
}

/// `(bp_id, artists, label)` of every track that has a Beatport id, so Beatport artist/label ids
/// can be looked up via those tracks instead of Beatport's search.
pub async fn get_bp_credits(
    pool: &SqlitePool,
) -> anyhow::Result<Vec<crate::scraper::BpTrackCredits>> {
    let rows = sqlx::query("SELECT bp_id, artists, label FROM tracks WHERE bp_id IS NOT NULL")
        .fetch_all(pool)
        .await?;
    rows.iter()
        .map(|r| {
            let artists: String = r.try_get("artists").unwrap_or_default();
            Ok((
                r.try_get("bp_id")?,
                serde_json::from_str(&artists).unwrap_or_default(),
                r.try_get("label").unwrap_or(None),
            ))
        })
        .collect()
}

pub async fn get_all_yt_ids(pool: &SqlitePool) -> anyhow::Result<HashSet<String>> {
    let rows = sqlx::query("SELECT yt_id FROM tracks")
        .fetch_all(pool)
        .await?;
    rows.iter()
        .map(|r| r.try_get::<String, _>("yt_id").map_err(Into::into))
        .collect()
}

pub async fn update_rating(
    pool: &SqlitePool,
    yt_id: &str,
    rating: Option<i64>,
) -> anyhow::Result<u64> {
    Ok(sqlx::query("UPDATE tracks SET rating = ? WHERE yt_id = ?")
        .bind(rating)
        .bind(yt_id)
        .execute(pool)
        .await?
        .rows_affected())
}

pub async fn update_bpm(pool: &SqlitePool, yt_id: &str, bpm: Option<i64>) -> anyhow::Result<u64> {
    Ok(sqlx::query("UPDATE tracks SET bpm = ? WHERE yt_id = ?")
        .bind(bpm)
        .bind(yt_id)
        .execute(pool)
        .await?
        .rows_affected())
}

pub async fn inc_play_count(
    pool: &SqlitePool,
    yt_id: &str,
    last_played: chrono::NaiveDate,
) -> anyhow::Result<u64> {
    Ok(sqlx::query(
        "UPDATE tracks SET play_count = play_count + 1, last_played = ? WHERE yt_id = ?",
    )
    .bind(last_played.format("%Y-%m-%d").to_string())
    .bind(yt_id)
    .execute(pool)
    .await?
    .rows_affected())
}

pub async fn get_tracks_missing_sp_id(pool: &SqlitePool) -> anyhow::Result<Vec<Track>> {
    let rows = sqlx::query("SELECT * FROM tracks WHERE sp_id IS NULL")
        .fetch_all(pool)
        .await?;
    rows.iter().map(Track::from_db_row).collect()
}

pub async fn update_sp_id(pool: &SqlitePool, yt_id: &str, sp_id: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE tracks SET sp_id = ? WHERE yt_id = ?")
        .bind(sp_id)
        .bind(yt_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ── Dynamic query helpers ─────────────────────────────────────────────────────

#[derive(Clone, Debug)]
enum SqlParam {
    Text(String),
    Int(i64),
    Float(f64),
    Null,
}

impl SqlParam {
    fn from_json(v: &serde_json::Value) -> Self {
        match v {
            serde_json::Value::String(s) => Self::Text(s.clone()),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Self::Int(i)
                } else if let Some(f) = n.as_f64() {
                    Self::Float(f)
                } else {
                    Self::Null
                }
            }
            _ => Self::Null,
        }
    }
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// ag-Grid's date filter sends `YYYY-MM-DD hh:mm:ss`, but dates are stored as `YYYY-MM-DD`
/// text and compared as strings, so the time part must be dropped — otherwise `equals` never
/// matches and the boundary day is mis-handled by every range comparison.
fn date_param(value: Option<&str>) -> SqlParam {
    let value = value.unwrap_or_default();
    SqlParam::Text(value.get(..10).unwrap_or(value).to_string())
}

fn build_condition_clause(field: &str, item: &FilterModelItem) -> (Vec<String>, Vec<SqlParam>) {
    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<SqlParam> = Vec::new();

    {
        match item.filter_type.as_str() {
            "text" => match item.filter_op.as_str() {
                "contains" => {
                    clauses.push(format!("{field} LIKE ? ESCAPE '\\'"));
                    let v = item.filter.as_ref().and_then(|v| v.as_str()).unwrap_or("");
                    params.push(SqlParam::Text(format!("%{}%", escape_like(v))));
                }
                "notContains" => {
                    clauses.push(format!("{field} NOT LIKE ? ESCAPE '\\'"));
                    let v = item.filter.as_ref().and_then(|v| v.as_str()).unwrap_or("");
                    params.push(SqlParam::Text(format!("%{}%", escape_like(v))));
                }
                "startsWith" => {
                    clauses.push(format!("{field} LIKE ? ESCAPE '\\'"));
                    let v = item.filter.as_ref().and_then(|v| v.as_str()).unwrap_or("");
                    params.push(SqlParam::Text(format!("{}%", escape_like(v))));
                }
                "endsWith" => {
                    clauses.push(format!("{field} LIKE ? ESCAPE '\\'"));
                    let v = item.filter.as_ref().and_then(|v| v.as_str()).unwrap_or("");
                    params.push(SqlParam::Text(format!("%{}", escape_like(v))));
                }
                "equals" => {
                    clauses.push(format!("{field} = ?"));
                    params.push(
                        item.filter
                            .as_ref()
                            .map(SqlParam::from_json)
                            .unwrap_or(SqlParam::Null),
                    );
                }
                "notEqual" => {
                    clauses.push(format!("{field} <> ?"));
                    params.push(
                        item.filter
                            .as_ref()
                            .map(SqlParam::from_json)
                            .unwrap_or(SqlParam::Null),
                    );
                }
                "blank" => clauses.push(format!("{field} IS NULL")),
                "notBlank" => clauses.push(format!("{field} IS NOT NULL")),
                _ => {}
            },
            "number" => {
                let v = item.filter.as_ref().map(SqlParam::from_json);
                let v2 = item.filter_to.as_ref().map(SqlParam::from_json);
                match item.filter_op.as_str() {
                    "greaterThan" => {
                        clauses.push(format!("{field} > ?"));
                        params.push(v.unwrap_or(SqlParam::Null));
                    }
                    "greaterThanOrEqual" => {
                        clauses.push(format!("{field} >= ?"));
                        params.push(v.unwrap_or(SqlParam::Null));
                    }
                    "lessThan" => {
                        clauses.push(format!("{field} < ?"));
                        params.push(v.unwrap_or(SqlParam::Null));
                    }
                    "lessThanOrEqual" => {
                        clauses.push(format!("{field} <= ?"));
                        params.push(v.unwrap_or(SqlParam::Null));
                    }
                    "equals" => {
                        clauses.push(format!("{field} = ?"));
                        params.push(v.unwrap_or(SqlParam::Null));
                    }
                    "notEqual" => {
                        clauses.push(format!("{field} <> ?"));
                        params.push(v.unwrap_or(SqlParam::Null));
                    }
                    "blank" => clauses.push(format!("{field} IS NULL")),
                    "notBlank" => clauses.push(format!("{field} IS NOT NULL")),
                    "inRange" => {
                        clauses.push(format!("{field} >= ? AND {field} <= ?"));
                        params.push(v.unwrap_or(SqlParam::Null));
                        params.push(v2.unwrap_or(SqlParam::Null));
                    }
                    _ => {}
                }
            }
            "date" => match item.filter_op.as_str() {
                "equals" => {
                    clauses.push(format!("{field} = ?"));
                    params.push(date_param(item.date_from.as_deref()));
                }
                "notEqual" => {
                    clauses.push(format!("{field} <> ?"));
                    params.push(date_param(item.date_from.as_deref()));
                }
                "greaterThan" => {
                    clauses.push(format!("{field} > ?"));
                    params.push(date_param(item.date_from.as_deref()));
                }
                "greaterThanOrEqual" => {
                    clauses.push(format!("{field} >= ?"));
                    params.push(date_param(item.date_from.as_deref()));
                }
                "lessThan" => {
                    clauses.push(format!("{field} < ?"));
                    params.push(date_param(item.date_from.as_deref()));
                }
                "lessThanOrEqual" => {
                    clauses.push(format!("{field} <= ?"));
                    params.push(date_param(item.date_from.as_deref()));
                }
                "inRange" => {
                    clauses.push(format!("{field} >= ? AND {field} <= ?"));
                    params.push(date_param(item.date_from.as_deref()));
                    params.push(date_param(item.date_to.as_deref()));
                }
                "blank" => clauses.push(format!("{field} IS NULL")),
                "notBlank" => clauses.push(format!("{field} IS NOT NULL")),
                _ => {}
            },
            _ => {}
        }
    }

    (clauses, params)
}

fn build_where_clause(filter_model: &FilterModelMap) -> (String, Vec<SqlParam>) {
    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<SqlParam> = Vec::new();

    for (field, model) in filter_model {
        if !VALID_COLUMNS.contains(&field.as_str()) {
            continue;
        }
        match model {
            FilterModel::Simple(item) => {
                let (item_clauses, item_params) = build_condition_clause(field, item);
                clauses.extend(item_clauses);
                params.extend(item_params);
            }
            FilterModel::Combined(combined) => {
                let joiner = if combined.operator.eq_ignore_ascii_case("or") {
                    " OR "
                } else {
                    " AND "
                };
                let mut cond_clauses: Vec<String> = Vec::new();
                for condition in &combined.conditions {
                    let (item_clauses, item_params) = build_condition_clause(field, condition);
                    cond_clauses.extend(item_clauses);
                    params.extend(item_params);
                }
                if !cond_clauses.is_empty() {
                    clauses.push(format!("({})", cond_clauses.join(joiner)));
                }
            }
        }
    }

    let sql = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    (sql, params)
}

fn bind_params<'q>(
    query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    params: &'q [SqlParam],
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
    // sqlx requires homogeneous chaining; we work around dynamic types by converting all to
    // a string representation for text/date params and using typed binds for numbers.
    // SQLite's affinity rules handle the comparisons correctly.
    let mut q = query;
    for p in params {
        q = match p {
            SqlParam::Text(s) => q.bind(s.as_str()),
            SqlParam::Int(i) => q.bind(*i),
            SqlParam::Float(f) => q.bind(*f),
            SqlParam::Null => q.bind(Option::<String>::None),
        };
    }
    q
}

pub async fn count_rows(pool: &SqlitePool, filter_model: &FilterModelMap) -> anyhow::Result<i64> {
    let (where_clause, params) = build_where_clause(filter_model);
    let sql = format!("SELECT COUNT(*) FROM tracks{where_clause}");
    let row = bind_params(sqlx::query(&sql), &params)
        .fetch_one(pool)
        .await?;
    Ok(row.get::<i64, _>(0))
}

/// Builds `ORDER BY` clause parts from an ag-Grid sort model, always appending `yt_id ASC` as a
/// final tiebreaker (unless already present) so that rows with equal sort-key values keep a
/// stable, deterministic order across requests instead of falling back to whatever order SQLite
/// happens to return them in (which can shift when a row is rewritten, e.g. by `INSERT OR
/// REPLACE`).
fn build_order_by_parts(sort_model: &[SortModelItem]) -> Vec<String> {
    let mut parts: Vec<String> = sort_model
        .iter()
        .filter(|s| VALID_COLUMNS.contains(&s.col_id.as_str()))
        .map(|s| {
            let dir = if s.sort == "asc" { "ASC" } else { "DESC" };
            format!("{} {dir}", s.col_id)
        })
        .collect();
    if !sort_model.iter().any(|s| s.col_id == "yt_id") {
        parts.push("yt_id ASC".to_string());
    }
    parts
}

pub async fn get_rows(
    pool: &SqlitePool,
    start_row: i64,
    end_row: i64,
    sort_model: &[SortModelItem],
    filter_model: &FilterModelMap,
) -> anyhow::Result<Vec<Track>> {
    let (where_clause, mut params) = build_where_clause(filter_model);

    let parts = build_order_by_parts(sort_model);
    let order_by = format!(" ORDER BY {}", parts.join(", "));

    params.push(SqlParam::Int(end_row - start_row));
    params.push(SqlParam::Int(start_row));

    let sql = format!("SELECT * FROM tracks{where_clause}{order_by} LIMIT ? OFFSET ?");
    let rows = bind_params(sqlx::query(&sql), &params)
        .fetch_all(pool)
        .await?;

    rows.iter().map(Track::from_db_row).collect()
}

pub async fn get_row_index(
    pool: &SqlitePool,
    yt_id: &str,
    sort_model: &[SortModelItem],
    filter_model: &FilterModelMap,
) -> anyhow::Result<i64> {
    let (where_clause, mut params) = build_where_clause(filter_model);

    let order_by = build_order_by_parts(sort_model).join(", ");

    let inner_where = if where_clause.is_empty() {
        String::new()
    } else {
        where_clause.clone()
    };

    let sql = format!(
        "SELECT row_idx FROM (
            SELECT yt_id,
                   (ROW_NUMBER() OVER (ORDER BY {order_by})) - 1 AS row_idx
            FROM tracks{inner_where}
         ) AS numbered_rows
         WHERE yt_id = ?"
    );

    params.push(SqlParam::Text(yt_id.to_string()));

    let row = bind_params(sqlx::query(&sql), &params)
        .fetch_optional(pool)
        .await?;

    Ok(row.map(|r| r.get::<i64, _>(0)).unwrap_or(-1))
}

/// Brings an existing `tracks` table up to date with columns added after the collection was
/// created (there's no version-tracked migration system — collections are just `.db` files that
/// may have been created by an older build of the app). No-op if the table doesn't look like a
/// `tracks` table yet, or already has the column.
async fn migrate_schema(pool: &SqlitePool) -> anyhow::Result<()> {
    let rows = sqlx::query("PRAGMA table_info(tracks)")
        .fetch_all(pool)
        .await?;
    let found_cols: std::collections::HashSet<String> =
        rows.iter().map(|r| r.get::<String, _>("name")).collect();

    if found_cols.contains("yt_id") && !found_cols.contains("sp_id") {
        sqlx::query("ALTER TABLE tracks ADD COLUMN sp_id TEXT")
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Copies the collection file `data` to `dest`, which must not exist, as is, and returns its
/// number of tracks. The file is checked in a temp dir first, then copied with `VACUUM INTO` to a
/// temp file beside `dest`, which takes `dest`'s name only once complete and without overwriting,
/// so that a failed import leaves nothing in the repo.
pub async fn import_collection(data: &[u8], dest: &Path) -> anyhow::Result<i64> {
    ensure_standalone(data)?;
    let dir = tempfile::tempdir()?;
    let src = dir.path().join("import.db");
    tokio::fs::write(&src, data).await?;
    anyhow::ensure!(
        validate_db_schema(&src).await,
        "not a Stereo collection file"
    );
    let parent = dest
        .parent()
        .ok_or_else(|| anyhow::anyhow!("no directory for {}", dest.display()))?;
    // Not named `*.db`, so not listed as a collection meanwhile; `VACUUM INTO` takes an empty file.
    let staged = tempfile::Builder::new()
        .prefix(".stereo-import-")
        .suffix(".tmp")
        .tempfile_in(parent)?;
    let opts = SqliteConnectOptions::new()
        .filename(&src)
        .create_if_missing(false);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await?;
    let result = async {
        let n = count_rows(&pool, &Default::default()).await?;
        sqlx::query("VACUUM INTO ?")
            .bind(staged.path().to_string_lossy().as_ref())
            .execute(&pool)
            .await?;
        anyhow::Ok(n)
    }
    .await;
    pool.close().await;
    let n = result?;
    staged.persist_noclobber(dest).map_err(|e| e.error)?;
    Ok(n)
}

/// Refuses a database in WAL mode, whose latest changes may be in a `-wal` file beside it that
/// wasn't uploaded. Stereo's own files, and its downloads, use a rollback journal.
fn ensure_standalone(data: &[u8]) -> anyhow::Result<()> {
    anyhow::ensure!(
        data.starts_with(b"SQLite format 3\0") && data.len() >= 100,
        "not an SQLite database"
    );
    // The header's file format write and read versions: 2 for WAL.
    anyhow::ensure!(
        data[18] != 2 && data[19] != 2,
        "the file is in WAL mode, so some of its changes may be in a separate -wal file. \
         Import a standalone copy, such as Stereo's download of it or one made with \
         `sqlite3 file.db \"VACUUM INTO 'copy.db'\"`"
    );
    Ok(())
}

pub async fn validate_db_schema(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }

    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false);

    let pool = match SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
    {
        Ok(p) => p,
        Err(_) => return false,
    };

    let result = async {
        migrate_schema(&pool).await?;

        let rows = sqlx::query("PRAGMA table_info(tracks)")
            .fetch_all(&pool)
            .await?;

        if rows.is_empty() {
            return Ok(false);
        }

        let found_cols: std::collections::HashSet<String> =
            rows.iter().map(|r| r.get::<String, _>("name")).collect();

        let pk_cols: std::collections::HashSet<String> = rows
            .iter()
            .filter(|r| r.get::<i64, _>("pk") == 1)
            .map(|r| r.get::<String, _>("name"))
            .collect();

        let expected: std::collections::HashSet<&str> = [
            "yt_id",
            "bp_id",
            "sp_id",
            "mb_id",
            "title",
            "mix_name",
            "artists",
            "release_date",
            "label",
            "album",
            "length",
            "bpm",
            "genre",
            "key",
            "mood",
            "rating",
            "play_count",
            "last_played",
        ]
        .iter()
        .copied()
        .collect();

        let all_present = expected.iter().all(|c| found_cols.contains(*c));
        let has_pk = pk_cols.contains("yt_id");

        anyhow::Ok(all_present && has_pk)
    }
    .await;

    pool.close().await;
    result.unwrap_or(false)
}

pub struct CollectionStats {
    pub top_artists_five_star: Vec<ScoredName>,
    pub top_artists_four_plus: Vec<ScoredName>,
    pub top_labels_five_star: Vec<ScoredName>,
    pub top_labels_four_plus: Vec<ScoredName>,
}

#[derive(Default)]
struct NameAcc {
    count: usize,
    weighted_days_since_release_sum: f64,
    weight_sum: f64,
}

// Ratings act as weights so that, within the 4+-star bucket, 5-star tracks pull
// the average days-since-release harder toward their own recency than 4-star ones.
fn accumulate(
    map: &mut HashMap<String, NameAcc>,
    name: &str,
    days_since_release: Option<i64>,
    rating: i64,
) {
    let acc = map.entry(name.to_string()).or_default();
    acc.count += 1;
    if let Some(days) = days_since_release {
        let weight = rating as f64;
        acc.weighted_days_since_release_sum += days as f64 * weight;
        acc.weight_sum += weight;
    }
}

fn top_by_count(counts: &HashMap<String, NameAcc>, limit: usize) -> Vec<ScoredName> {
    let mut top: Vec<ScoredName> = counts
        .iter()
        .map(|(name, acc)| {
            let weighted_avg_days_since_release = if acc.weight_sum > 0.0 {
                acc.weighted_days_since_release_sum / acc.weight_sum
            } else {
                0.0
            };
            ScoredName {
                name: name.clone(),
                count: acc.count,
                weighted_avg_days_since_release,
            }
        })
        .collect();
    // Ties in count are broken by recency (lower weighted average days since
    // release first) so a single ranking can serve both the plain and
    // recency-weighted views.
    top.sort_by(|a, b| {
        b.count.cmp(&a.count).then(
            a.weighted_avg_days_since_release
                .partial_cmp(&b.weighted_avg_days_since_release)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    top.truncate(limit);
    top
}

#[derive(Default)]
struct RatingCounts {
    artist_five_star: HashMap<String, NameAcc>,
    artist_four_plus: HashMap<String, NameAcc>,
    label_five_star: HashMap<String, NameAcc>,
    label_four_plus: HashMap<String, NameAcc>,
}

async fn get_rating_counts(pool: &SqlitePool) -> anyhow::Result<RatingCounts> {
    let rows =
        sqlx::query("SELECT artists, label, rating, release_date FROM tracks WHERE rating > 0")
            .fetch_all(pool)
            .await?;

    let today = chrono::Local::now().date_naive();

    let mut c = RatingCounts::default();

    for row in &rows {
        let artists_str: String = row.try_get("artists").unwrap_or_default();
        let artists: Vec<String> = serde_json::from_str(&artists_str).unwrap_or_default();
        let label: Option<String> = row.try_get("label").unwrap_or(None);
        let rating: i64 = row.try_get("rating").unwrap_or(0);
        let release_date_str: Option<String> = row.try_get("release_date").unwrap_or(None);
        let days_since_release = release_date_str
            .as_deref()
            .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
            .map(|d| (today - d).num_days());

        for artist in &artists {
            if rating >= 4 {
                accumulate(&mut c.artist_four_plus, artist, days_since_release, rating);
            }
            if rating == 5 {
                accumulate(&mut c.artist_five_star, artist, days_since_release, rating);
            }
        }
        if let Some(label) = label.filter(|l| !l.is_empty()) {
            if rating >= 4 {
                accumulate(&mut c.label_four_plus, &label, days_since_release, rating);
            }
            if rating == 5 {
                accumulate(&mut c.label_five_star, &label, days_since_release, rating);
            }
        }
    }

    Ok(c)
}

pub async fn get_collection_stats(pool: &SqlitePool) -> anyhow::Result<CollectionStats> {
    let c = get_rating_counts(pool).await?;
    Ok(CollectionStats {
        top_artists_five_star: top_by_count(&c.artist_five_star, TOP_NAMES),
        top_artists_four_plus: top_by_count(&c.artist_four_plus, TOP_NAMES),
        top_labels_five_star: top_by_count(&c.label_five_star, TOP_NAMES),
        top_labels_four_plus: top_by_count(&c.label_four_plus, TOP_NAMES),
    })
}

/// How many top names the collection stats show, and how many of them a new-releases search
/// selects by default.
const TOP_NAMES: usize = 10;
/// How many artists/labels the new-releases dialog offers, beyond the default selection.
const NEW_RELEASES_CANDIDATES: usize = 50;

pub struct NewReleasesCandidates {
    pub artists: Vec<NewReleasesCandidate>,
    pub labels: Vec<NewReleasesCandidate>,
}

/// Ranks names by their 4+-star track count (the same ranking as the stats) and marks the
/// default selection: the top artists by 5-star and by 4+-star count, and the top labels by
/// 4+-star count. Default-selected names are always included, even past the candidate limit.
fn rank_candidates(
    five_star: &HashMap<String, NameAcc>,
    four_plus: &HashMap<String, NameAcc>,
    include_five_star_top: bool,
) -> Vec<NewReleasesCandidate> {
    let mut defaults: HashSet<String> = top_by_count(four_plus, TOP_NAMES)
        .into_iter()
        .map(|s| s.name)
        .collect();
    if include_five_star_top {
        defaults.extend(
            top_by_count(five_star, TOP_NAMES)
                .into_iter()
                .map(|s| s.name),
        );
    }
    top_by_count(four_plus, usize::MAX)
        .into_iter()
        .enumerate()
        .filter(|(i, s)| *i < NEW_RELEASES_CANDIDATES || defaults.contains(&s.name))
        .map(|(_, s)| NewReleasesCandidate {
            five_star: five_star.get(&s.name).map_or(0, |a| a.count),
            four_plus: s.count,
            is_default: defaults.contains(&s.name),
            name: s.name,
        })
        .collect()
}

pub async fn get_new_releases_candidates(
    pool: &SqlitePool,
) -> anyhow::Result<NewReleasesCandidates> {
    let c = get_rating_counts(pool).await?;
    Ok(NewReleasesCandidates {
        artists: rank_candidates(&c.artist_five_star, &c.artist_four_plus, true),
        labels: rank_candidates(&c.label_five_star, &c.label_four_plus, false),
    })
}

pub async fn import_from_db(
    pool: &SqlitePool,
    source_db: &Path,
    keep_user_data: bool,
) -> anyhow::Result<()> {
    let source_str = source_db
        .to_str()
        .context("source path is not valid UTF-8")?;

    let mut cols = vec![
        "yt_id",
        "bp_id",
        "sp_id",
        "mb_id",
        "title",
        "mix_name",
        "artists",
        "release_date",
        "label",
        "album",
        "length",
        "bpm",
        "genre",
        "key",
        "mood",
    ];

    if keep_user_data {
        cols.extend_from_slice(&["rating", "play_count", "last_played"]);
    }

    let col_list = cols.join(", ");

    let mut conn = pool.acquire().await?;

    sqlx::query("ATTACH DATABASE ? AS source_db")
        .bind(source_str)
        .execute(&mut *conn)
        .await?;

    let result = sqlx::query(&format!(
        "INSERT OR IGNORE INTO main.tracks ({col_list}) \
         SELECT {col_list} FROM source_db.tracks"
    ))
    .execute(&mut *conn)
    .await;

    sqlx::query("DETACH DATABASE source_db")
        .execute(&mut *conn)
        .await?;

    let affected = result?.rows_affected();
    tracing::info!("import_from_db: {affected} tracks imported");

    Ok(())
}
