use std::collections::HashMap;
use std::path::PathBuf;

use chrono::NaiveDate;
use stereo_backend::{
    db,
    message::{FilterModel, FilterModelItem, FilterModelMap, SortModelItem},
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
    db::insert_track(&pool, &make_track("del1"), true).await.unwrap();

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
    db::insert_track(&pool, &make_track("ex1"), true).await.unwrap();

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
async fn test_update_rating() {
    let (pool, _dir) = setup().await;
    db::insert_track(&pool, &make_track("rate1"), true).await.unwrap();

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
    db::import_from_db(&dst_pool, &src_path, false).await.unwrap();

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
