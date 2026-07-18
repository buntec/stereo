use std::{collections::HashMap, path::Path};

use anyhow::Context;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteRow},
    Row, SqlitePool,
};

use crate::{
    message::{FilterModel, FilterModelMap, SortModelItem},
    models::{ScoredName, Track},
};

const VALID_COLUMNS: &[&str] = &[
    "yt_id",
    "bp_id",
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
        .create_if_missing(true);
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
         (yt_id, bp_id, mb_id, title, mix_name, artists, release_date, label, album, \
          length, bpm, genre, key, mood, rating, play_count, last_played) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    );
    sqlx::query(&sql)
        .bind(&track.yt_id)
        .bind(track.bp_id)
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

pub async fn insert_tracks(
    pool: &SqlitePool,
    tracks: &[Track],
    ignore_existing: bool,
) -> anyhow::Result<()> {
    if tracks.is_empty() {
        return Ok(());
    }
    let on_conflict = if ignore_existing { "IGNORE" } else { "REPLACE" };
    let sql = format!(
        "INSERT OR {on_conflict} INTO tracks \
         (yt_id, bp_id, mb_id, title, mix_name, artists, release_date, label, album, \
          length, bpm, genre, key, mood, rating, play_count, last_played) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    );
    let mut tx = pool.begin().await?;
    for track in tracks {
        sqlx::query(&sql)
            .bind(&track.yt_id)
            .bind(track.bp_id)
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
            .await?;
    }
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

pub async fn update_rating(
    pool: &SqlitePool,
    yt_id: &str,
    rating: Option<i64>,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE tracks SET rating = ? WHERE yt_id = ?")
        .bind(rating)
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

fn build_where_clause(filter_model: &FilterModelMap) -> (String, Vec<SqlParam>) {
    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<SqlParam> = Vec::new();

    for (field, model) in filter_model {
        if !VALID_COLUMNS.contains(&field.as_str()) {
            continue;
        }
        let item = match model {
            FilterModel::Simple(item) => item,
            FilterModel::Combined(_) => continue, // not implemented
        };

        match item.filter_type.as_str() {
            "text" => match item.filter_op.as_str() {
                "contains" => {
                    clauses.push(format!("{field} LIKE ?"));
                    let v = item.filter.as_ref().and_then(|v| v.as_str()).unwrap_or("");
                    params.push(SqlParam::Text(format!("%{v}%")));
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
                        clauses.push(format!("{field} >= ? AND {field} < ?"));
                        params.push(v.unwrap_or(SqlParam::Null));
                        params.push(v2.unwrap_or(SqlParam::Null));
                    }
                    _ => {}
                }
            }
            "date" => match item.filter_op.as_str() {
                "equals" => {
                    clauses.push(format!("{field} = ?"));
                    params.push(SqlParam::Text(item.date_from.clone().unwrap_or_default()));
                }
                "notEqual" => {
                    clauses.push(format!("{field} <> ?"));
                    params.push(SqlParam::Text(item.date_from.clone().unwrap_or_default()));
                }
                "greaterThan" => {
                    clauses.push(format!("{field} > ?"));
                    params.push(SqlParam::Text(item.date_from.clone().unwrap_or_default()));
                }
                "lessThan" => {
                    clauses.push(format!("{field} < ?"));
                    params.push(SqlParam::Text(item.date_from.clone().unwrap_or_default()));
                }
                "lessThanOrEqual" => {
                    clauses.push(format!("{field} <= ?"));
                    params.push(SqlParam::Text(item.date_from.clone().unwrap_or_default()));
                }
                "inRange" => {
                    clauses.push(format!("{field} >= ? AND {field} < ?"));
                    params.push(SqlParam::Text(item.date_from.clone().unwrap_or_default()));
                    params.push(SqlParam::Text(item.date_to.clone().unwrap_or_default()));
                }
                _ => {}
            },
            _ => {}
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

pub async fn get_rows(
    pool: &SqlitePool,
    start_row: i64,
    end_row: i64,
    sort_model: &[SortModelItem],
    filter_model: &FilterModelMap,
) -> anyhow::Result<Vec<Track>> {
    let (where_clause, mut params) = build_where_clause(filter_model);

    let order_by = if sort_model.is_empty() {
        String::new()
    } else {
        let parts: Vec<String> = sort_model
            .iter()
            .filter(|s| VALID_COLUMNS.contains(&s.col_id.as_str()))
            .map(|s| {
                let dir = if s.sort == "asc" { "ASC" } else { "DESC" };
                format!("{} {dir}", s.col_id)
            })
            .collect();
        if parts.is_empty() {
            String::new()
        } else {
            format!(" ORDER BY {}", parts.join(", "))
        }
    };

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

    let order_by = if sort_model.is_empty() {
        "yt_id ASC".to_string()
    } else {
        let parts: Vec<String> = sort_model
            .iter()
            .filter(|s| VALID_COLUMNS.contains(&s.col_id.as_str()))
            .map(|s| {
                let dir = if s.sort == "asc" { "ASC" } else { "DESC" };
                format!("{} {dir}", s.col_id)
            })
            .collect();
        if parts.is_empty() {
            "yt_id ASC".to_string()
        } else {
            parts.join(", ")
        }
    };

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

fn top_by_count(counts: HashMap<String, NameAcc>) -> Vec<ScoredName> {
    let mut top: Vec<ScoredName> = counts
        .into_iter()
        .map(|(name, acc)| {
            let weighted_avg_days_since_release = if acc.weight_sum > 0.0 {
                acc.weighted_days_since_release_sum / acc.weight_sum
            } else {
                0.0
            };
            ScoredName {
                name,
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
    top.truncate(10);
    top
}

pub async fn get_collection_stats(pool: &SqlitePool) -> anyhow::Result<CollectionStats> {
    let rows =
        sqlx::query("SELECT artists, label, rating, release_date FROM tracks WHERE rating > 0")
            .fetch_all(pool)
            .await?;

    let today = chrono::Local::now().date_naive();

    let mut artist_five_star: HashMap<String, NameAcc> = HashMap::new();
    let mut artist_four_plus: HashMap<String, NameAcc> = HashMap::new();
    let mut label_five_star: HashMap<String, NameAcc> = HashMap::new();
    let mut label_four_plus: HashMap<String, NameAcc> = HashMap::new();

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
                accumulate(&mut artist_four_plus, artist, days_since_release, rating);
            }
            if rating == 5 {
                accumulate(&mut artist_five_star, artist, days_since_release, rating);
            }
        }
        if let Some(label) = label.filter(|l| !l.is_empty()) {
            if rating >= 4 {
                accumulate(&mut label_four_plus, &label, days_since_release, rating);
            }
            if rating == 5 {
                accumulate(&mut label_five_star, &label, days_since_release, rating);
            }
        }
    }

    Ok(CollectionStats {
        top_artists_five_star: top_by_count(artist_five_star),
        top_artists_four_plus: top_by_count(artist_four_plus),
        top_labels_five_star: top_by_count(label_five_star),
        top_labels_four_plus: top_by_count(label_four_plus),
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
