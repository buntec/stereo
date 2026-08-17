use std::collections::HashMap;
use std::path::PathBuf;

use chrono::NaiveDate;
use stereo_backend::{
    db,
    message::{
        CombinedFilterModelItem, FilterModel, FilterModelItem, FilterModelMap, SortModelItem,
    },
    models::Track,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn setup() -> (sqlx::SqlitePool, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let pool = db::open_pool(&path).await.unwrap();
    db::init_db(&pool).await.unwrap();
    (pool, dir)
}

fn make_track(yt_id: &str) -> Track {
    Track {
        yt_id: yt_id.to_string(),
        title: format!("Track {yt_id}"),
        artists: vec!["Artist A".to_string()],
        bpm: Some(128),
        label: Some("Test Label".to_string()),
        release_date: Some(NaiveDate::from_ymd_opt(2024, 6, 1).unwrap()),
        sp_id: Some("4uLU6hMCjMI75M1A2tKUQC".to_string()),
        ..Default::default()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_init_creates_table() {
    let (pool, _dir) = setup().await;
    // If the table was not created, count_rows would error
    let n = db::count_rows(&pool, &HashMap::new()).await.unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn test_insert_and_get_track() {
    let (pool, _dir) = setup().await;
    let track = make_track("abc123");

    db::insert_track(&pool, &track, true).await.unwrap();

    let fetched = db::get_track(&pool, "abc123").await.unwrap().unwrap();
    assert_eq!(fetched.yt_id, "abc123");
    assert_eq!(fetched.title, "Track abc123");
    assert_eq!(fetched.artists, vec!["Artist A"]);
    assert_eq!(fetched.bpm, Some(128));
    assert_eq!(
        fetched.release_date,
        Some(NaiveDate::from_ymd_opt(2024, 6, 1).unwrap())
    );
    assert_eq!(fetched.sp_id.as_deref(), Some("4uLU6hMCjMI75M1A2tKUQC"));
}

#[tokio::test]
async fn test_get_nonexistent_track_returns_none() {
    let (pool, _dir) = setup().await;
    let result = db::get_track(&pool, "doesnotexist").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_insert_ignore_existing() {
    let (pool, _dir) = setup().await;
    let mut track = make_track("dup1");
    db::insert_track(&pool, &track, true).await.unwrap();

    // Update title and try to insert again with ignore_if_exists=true
    track.title = "Updated Title".to_string();
    db::insert_track(&pool, &track, true).await.unwrap();

    // Original title should remain
    let fetched = db::get_track(&pool, "dup1").await.unwrap().unwrap();
    assert_eq!(fetched.title, "Track dup1");
}

#[tokio::test]
async fn test_insert_replace_existing() {
    let (pool, _dir) = setup().await;
    let mut track = make_track("dup2");
    db::insert_track(&pool, &track, true).await.unwrap();

    track.title = "Updated Title".to_string();
    db::insert_track(&pool, &track, false).await.unwrap();

    let fetched = db::get_track(&pool, "dup2").await.unwrap().unwrap();
    assert_eq!(fetched.title, "Updated Title");
}

#[tokio::test]
async fn test_insert_tracks_batch() {
    let (pool, _dir) = setup().await;
    let tracks: Vec<Track> = (0..5).map(|i| make_track(&format!("batch{i}"))).collect();

    db::insert_tracks(&pool, &tracks, true).await.unwrap();

    let n = db::count_rows(&pool, &HashMap::new()).await.unwrap();
    assert_eq!(n, 5);
}

#[tokio::test]
async fn test_delete_track() {
    let (pool, _dir) = setup().await;
    db::insert_track(&pool, &make_track("del1"), true)
        .await
        .unwrap();

    db::delete_track(&pool, "del1").await.unwrap();

    assert!(db::get_track(&pool, "del1").await.unwrap().is_none());
    assert_eq!(db::count_rows(&pool, &HashMap::new()).await.unwrap(), 0);
}

#[tokio::test]
async fn test_delete_tracks_multiple() {
    let (pool, _dir) = setup().await;
    for i in 0..4 {
        db::insert_track(&pool, &make_track(&format!("t{i}")), true)
            .await
            .unwrap();
    }

    let to_delete = vec!["t0".to_string(), "t1".to_string()];
    db::delete_tracks(&pool, &to_delete).await.unwrap();

    assert_eq!(db::count_rows(&pool, &HashMap::new()).await.unwrap(), 2);
}

#[tokio::test]
async fn test_track_exists() {
    let (pool, _dir) = setup().await;
    db::insert_track(&pool, &make_track("ex1"), true)
        .await
        .unwrap();

    assert!(db::track_exists(&pool, "ex1").await.unwrap());
    assert!(!db::track_exists(&pool, "nothere").await.unwrap());
}

#[tokio::test]
async fn test_count_rows_with_filter() {
    let (pool, _dir) = setup().await;
    for i in 0..3 {
        let mut t = make_track(&format!("f{i}"));
        t.bpm = Some(100 + i * 10); // 100, 110, 120
        db::insert_track(&pool, &t, true).await.unwrap();
    }

    // Filter bpm > 105
    let filter: FilterModelMap = [(
        "bpm".to_string(),
        FilterModel::Simple(FilterModelItem {
            filter_type: "number".to_string(),
            filter_op: "greaterThan".to_string(),
            filter: Some(serde_json::json!(105)),
            filter_to: None,
            date_from: None,
            date_to: None,
        }),
    )]
    .into_iter()
    .collect();

    let n = db::count_rows(&pool, &filter).await.unwrap();
    assert_eq!(n, 2); // 110 and 120
}

#[tokio::test]
async fn test_count_rows_with_combined_or_filter() {
    let (pool, _dir) = setup().await;
    for (i, rating) in [Some(1), Some(4), Some(5), None].into_iter().enumerate() {
        let mut t = make_track(&format!("r{i}"));
        t.rating = rating;
        db::insert_track(&pool, &t, true).await.unwrap();
    }

    // Filter rating >= 4 OR rating is blank
    let filter: FilterModelMap = [(
        "rating".to_string(),
        FilterModel::Combined(CombinedFilterModelItem {
            filter_type: "number".to_string(),
            operator: "OR".to_string(),
            conditions: vec![
                FilterModelItem {
                    filter_type: "number".to_string(),
                    filter_op: "greaterThanOrEqual".to_string(),
                    filter: Some(serde_json::json!(4)),
                    filter_to: None,
                    date_from: None,
                    date_to: None,
                },
                FilterModelItem {
                    filter_type: "number".to_string(),
                    filter_op: "blank".to_string(),
                    filter: None,
                    filter_to: None,
                    date_from: None,
                    date_to: None,
                },
            ],
        }),
    )]
    .into_iter()
    .collect();

    let n = db::count_rows(&pool, &filter).await.unwrap();
    assert_eq!(n, 3); // rating 4, rating 5, and the blank one
}

#[tokio::test]
async fn test_get_rows_sorted() {
    let (pool, _dir) = setup().await;
    let titles = ["Charlie", "Alice", "Bob"];
    for (i, title) in titles.iter().enumerate() {
        let mut t = make_track(&format!("s{i}"));
        t.title = title.to_string();
        db::insert_track(&pool, &t, true).await.unwrap();
    }

    let sort = vec![SortModelItem {
        col_id: "title".to_string(),
        sort: "asc".to_string(),
        sort_type: None,
    }];

    let rows = db::get_rows(&pool, 0, 10, &sort, &HashMap::new())
        .await
        .unwrap();

    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].title, "Alice");
    assert_eq!(rows[1].title, "Bob");
    assert_eq!(rows[2].title, "Charlie");
}

#[tokio::test]
async fn test_get_rows_paginated() {
    let (pool, _dir) = setup().await;
    for i in 0..5 {
        db::insert_track(&pool, &make_track(&format!("p{i}")), true)
            .await
            .unwrap();
    }

    let sort = vec![SortModelItem {
        col_id: "yt_id".to_string(),
        sort: "asc".to_string(),
        sort_type: None,
    }];

    let page1 = db::get_rows(&pool, 0, 3, &sort, &HashMap::new())
        .await
        .unwrap();
    let page2 = db::get_rows(&pool, 3, 5, &sort, &HashMap::new())
        .await
        .unwrap();

    assert_eq!(page1.len(), 3);
    assert_eq!(page2.len(), 2);
}

#[tokio::test]
async fn test_get_rows_text_filter() {
    let (pool, _dir) = setup().await;
    for label in ["Alpha Records", "Beta Music", "Alpha Underground"] {
        let mut t = make_track(label);
        t.label = Some(label.to_string());
        db::insert_track(&pool, &t, true).await.unwrap();
    }

    let filter: FilterModelMap = [(
        "label".to_string(),
        FilterModel::Simple(FilterModelItem {
            filter_type: "text".to_string(),
            filter_op: "contains".to_string(),
            filter: Some(serde_json::json!("Alpha")),
            filter_to: None,
            date_from: None,
            date_to: None,
        }),
    )]
    .into_iter()
    .collect();

    let rows = db::get_rows(&pool, 0, 100, &[], &filter).await.unwrap();
    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn test_get_rows_text_filter_not_contains_and_starts_with() {
    let (pool, _dir) = setup().await;
    for label in ["Alpha Records", "Beta Music", "Alpha Underground"] {
        let mut t = make_track(label);
        t.label = Some(label.to_string());
        db::insert_track(&pool, &t, true).await.unwrap();
    }

    let not_contains: FilterModelMap = [(
        "label".to_string(),
        FilterModel::Simple(FilterModelItem {
            filter_type: "text".to_string(),
            filter_op: "notContains".to_string(),
            filter: Some(serde_json::json!("Alpha")),
            filter_to: None,
            date_from: None,
            date_to: None,
        }),
    )]
    .into_iter()
    .collect();
    let rows = db::get_rows(&pool, 0, 100, &[], &not_contains)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].label.as_deref(), Some("Beta Music"));

    let starts_with: FilterModelMap = [(
        "label".to_string(),
        FilterModel::Simple(FilterModelItem {
            filter_type: "text".to_string(),
            filter_op: "startsWith".to_string(),
            filter: Some(serde_json::json!("Alpha")),
            filter_to: None,
            date_from: None,
            date_to: None,
        }),
    )]
    .into_iter()
    .collect();
    let rows = db::get_rows(&pool, 0, 100, &[], &starts_with)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn test_count_rows_with_number_in_range_is_inclusive() {
    let (pool, _dir) = setup().await;
    for i in 0..3 {
        let mut t = make_track(&format!("ir{i}"));
        t.bpm = Some(100 + i * 10); // 100, 110, 120
        db::insert_track(&pool, &t, true).await.unwrap();
    }

    let filter: FilterModelMap = [(
        "bpm".to_string(),
        FilterModel::Simple(FilterModelItem {
            filter_type: "number".to_string(),
            filter_op: "inRange".to_string(),
            filter: Some(serde_json::json!(100)),
            filter_to: Some(serde_json::json!(120)),
            date_from: None,
            date_to: None,
        }),
    )]
    .into_iter()
    .collect();

    let n = db::count_rows(&pool, &filter).await.unwrap();
    assert_eq!(n, 3); // 120 must be included, not excluded by an exclusive upper bound
}

#[tokio::test]
async fn test_update_rating() {
    let (pool, _dir) = setup().await;
    db::insert_track(&pool, &make_track("rate1"), true)
        .await
        .unwrap();

    db::update_rating(&pool, "rate1", Some(4)).await.unwrap();

    let fetched = db::get_track(&pool, "rate1").await.unwrap().unwrap();
    assert_eq!(fetched.rating, Some(4));

    // Clear rating
    db::update_rating(&pool, "rate1", None).await.unwrap();
    let fetched = db::get_track(&pool, "rate1").await.unwrap().unwrap();
    assert_eq!(fetched.rating, None);
}

#[tokio::test]
async fn test_get_row_index() {
    let (pool, _dir) = setup().await;
    let titles = ["Zorro", "Apple", "Mango"];
    for (i, title) in titles.iter().enumerate() {
        let mut t = make_track(&format!("ri{i}"));
        t.title = title.to_string();
        db::insert_track(&pool, &t, true).await.unwrap();
    }

    let sort = vec![SortModelItem {
        col_id: "title".to_string(),
        sort: "asc".to_string(),
        sort_type: None,
    }];
    // ri0="Zorro", ri1="Apple", ri2="Mango"
    // Sorted ASC: Apple(ri1)=0, Mango(ri2)=1, Zorro(ri0)=2
    let idx = db::get_row_index(&pool, "ri1", &sort, &HashMap::new())
        .await
        .unwrap();
    assert_eq!(idx, 0); // "Apple" is first

    let idx = db::get_row_index(&pool, "ri2", &sort, &HashMap::new())
        .await
        .unwrap();
    assert_eq!(idx, 1); // "Mango" is second

    let idx = db::get_row_index(&pool, "ri0", &sort, &HashMap::new())
        .await
        .unwrap();
    assert_eq!(idx, 2); // "Zorro" is last
}

#[tokio::test]
async fn test_get_rows_ties_broken_by_yt_id() {
    let (pool, _dir) = setup().await;
    // All tracks share the same release_date (set by make_track), so sorting by
    // release_date alone leaves ties that must be broken by yt_id.
    for yt_id in ["zorro", "apple", "mango"] {
        db::insert_track(&pool, &make_track(yt_id), true)
            .await
            .unwrap();
    }

    let sort = vec![SortModelItem {
        col_id: "release_date".to_string(),
        sort: "asc".to_string(),
        sort_type: None,
    }];

    let rows = db::get_rows(&pool, 0, 10, &sort, &HashMap::new())
        .await
        .unwrap();

    assert_eq!(
        rows.iter().map(|r| r.yt_id.as_str()).collect::<Vec<_>>(),
        vec!["apple", "mango", "zorro"]
    );
}

#[tokio::test]
async fn test_get_row_index_ties_broken_by_yt_id() {
    let (pool, _dir) = setup().await;
    for yt_id in ["zorro", "apple", "mango"] {
        db::insert_track(&pool, &make_track(yt_id), true)
            .await
            .unwrap();
    }

    let sort = vec![SortModelItem {
        col_id: "release_date".to_string(),
        sort: "asc".to_string(),
        sort_type: None,
    }];

    let idx = db::get_row_index(&pool, "mango", &sort, &HashMap::new())
        .await
        .unwrap();
    assert_eq!(idx, 1); // apple(0), mango(1), zorro(2)
}

#[tokio::test]
async fn test_inc_play_count() {
    let (pool, _dir) = setup().await;
    db::insert_track(&pool, &make_track("play1"), true)
        .await
        .unwrap();

    let day1 = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
    db::inc_play_count(&pool, "play1", day1).await.unwrap();

    let fetched = db::get_track(&pool, "play1").await.unwrap().unwrap();
    assert_eq!(fetched.play_count, 1);
    assert_eq!(fetched.last_played, Some(day1));

    let day2 = NaiveDate::from_ymd_opt(2026, 7, 2).unwrap();
    db::inc_play_count(&pool, "play1", day2).await.unwrap();

    let fetched = db::get_track(&pool, "play1").await.unwrap().unwrap();
    assert_eq!(fetched.play_count, 2);
    assert_eq!(fetched.last_played, Some(day2));
}

#[tokio::test]
async fn test_inc_play_count_does_not_reorder_ties() {
    // Regression test: incrementing play count used to go through `insert_track`'s
    // `INSERT OR REPLACE`, which deletes and re-inserts the row (a new rowid), which could
    // shuffle its position among tied sort keys. `inc_play_count` uses a targeted UPDATE
    // instead, and ties are broken deterministically by yt_id, so order must be stable.
    let (pool, _dir) = setup().await;
    for yt_id in ["a_track", "b_track"] {
        db::insert_track(&pool, &make_track(yt_id), true)
            .await
            .unwrap();
    }

    let sort = vec![SortModelItem {
        col_id: "release_date".to_string(),
        sort: "asc".to_string(),
        sort_type: None,
    }];

    let before = db::get_rows(&pool, 0, 10, &sort, &HashMap::new())
        .await
        .unwrap();
    assert_eq!(
        before.iter().map(|r| r.yt_id.as_str()).collect::<Vec<_>>(),
        vec!["a_track", "b_track"]
    );

    db::inc_play_count(
        &pool,
        "b_track",
        NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(),
    )
    .await
    .unwrap();

    let after = db::get_rows(&pool, 0, 10, &sort, &HashMap::new())
        .await
        .unwrap();
    assert_eq!(
        after.iter().map(|r| r.yt_id.as_str()).collect::<Vec<_>>(),
        vec!["a_track", "b_track"]
    );
}

#[tokio::test]
async fn test_validate_db_schema_valid() {
    let (_pool, dir) = setup().await;
    let path = dir.path().join("test.db");
    assert!(db::validate_db_schema(&path).await);
}

#[tokio::test]
async fn test_validate_db_schema_missing_file() {
    assert!(!db::validate_db_schema(&PathBuf::from("/nonexistent/path.db")).await);
}

#[tokio::test]
async fn test_validate_db_schema_wrong_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wrong.db");
    let pool = db::open_pool(&path).await.unwrap();
    // Create a table with wrong schema
    sqlx::query("CREATE TABLE tracks (id INTEGER PRIMARY KEY)")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert!(!db::validate_db_schema(&path).await);
}

#[tokio::test]
async fn test_validate_db_schema_migrates_old_schema_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    let pool = db::open_pool(&path).await.unwrap();
    // Schema as it existed before the `sp_id` column was added.
    sqlx::query(
        "CREATE TABLE tracks (
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
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO tracks (yt_id, title, artists) VALUES ('abc', 'Old Track', '[]')")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    assert!(db::validate_db_schema(&path).await);

    // The migration should have preserved existing rows and left sp_id unset.
    let pool = db::open_pool(&path).await.unwrap();
    let track = db::get_track(&pool, "abc").await.unwrap().unwrap();
    assert_eq!(track.title, "Old Track");
    assert_eq!(track.sp_id, None);
}

#[tokio::test]
async fn test_import_from_db() {
    // Create source DB with some tracks
    let src_dir = tempfile::tempdir().unwrap();
    let src_path = src_dir.path().join("source.db");
    let src_pool = db::open_pool(&src_path).await.unwrap();
    db::init_db(&src_pool).await.unwrap();
    for i in 0..3 {
        db::insert_track(&src_pool, &make_track(&format!("imp{i}")), true)
            .await
            .unwrap();
    }
    src_pool.close().await;

    // Import into dest DB
    let (dst_pool, _dst_dir) = setup().await;
    db::import_from_db(&dst_pool, &src_path, false)
        .await
        .unwrap();

    let n = db::count_rows(&dst_pool, &HashMap::new()).await.unwrap();
    assert_eq!(n, 3);
}

#[tokio::test]
async fn test_track_roundtrip_preserves_dates() {
    let (pool, _dir) = setup().await;
    let mut track = make_track("dates1");
    track.release_date = Some(NaiveDate::from_ymd_opt(2023, 12, 31).unwrap());
    track.last_played = Some(NaiveDate::from_ymd_opt(2024, 1, 15).unwrap());
    track.play_count = 7;

    db::insert_track(&pool, &track, true).await.unwrap();
    let fetched = db::get_track(&pool, "dates1").await.unwrap().unwrap();

    assert_eq!(fetched.release_date, track.release_date);
    assert_eq!(fetched.last_played, track.last_played);
    assert_eq!(fetched.play_count, 7);
}
