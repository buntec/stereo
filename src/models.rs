use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn serialize_path<S: serde::Serializer>(path: &std::path::Path, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&path.to_string_lossy())
}

fn deserialize_path<'de, D: serde::Deserializer<'de>>(d: D) -> Result<PathBuf, D::Error> {
    let s = String::deserialize(d)?;
    Ok(PathBuf::from(s))
}

#[derive(Debug, Clone, Serialize)]
pub struct Repo {
    #[serde(serialize_with = "serialize_path")]
    pub path: PathBuf,
    pub remote: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collection {
    #[serde(
        serialize_with = "serialize_path",
        deserialize_with = "deserialize_path"
    )]
    pub path: PathBuf,
    pub size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Track {
    pub yt_id: String,
    pub bp_id: Option<i64>,
    pub mb_id: Option<String>,
    pub title: String,
    pub mix_name: Option<String>,
    pub artists: Vec<String>,
    pub release_date: Option<NaiveDate>,
    pub label: Option<String>,
    pub album: Option<String>,
    pub length: Option<i64>,
    pub bpm: Option<i64>,
    pub genre: Option<String>,
    pub key: Option<String>,
    pub mood: Option<String>,
    pub rating: Option<i64>,
    #[serde(default)]
    pub play_count: i64,
    pub last_played: Option<NaiveDate>,
}

impl Track {
    pub fn artists_json(&self) -> String {
        serde_json::to_string(&self.artists).unwrap_or_else(|_| "[]".to_string())
    }

    pub fn release_date_str(&self) -> Option<String> {
        self.release_date.map(|d| d.format("%Y-%m-%d").to_string())
    }

    pub fn last_played_str(&self) -> Option<String> {
        self.last_played.map(|d| d.format("%Y-%m-%d").to_string())
    }

    pub fn display_name(&self) -> String {
        if self.artists.is_empty() {
            self.title.clone()
        } else {
            format!("{} - {}", self.artists.join(", "), self.title)
        }
    }

    pub fn from_db_row(row: &sqlx::sqlite::SqliteRow) -> anyhow::Result<Self> {
        use sqlx::Row;
        let artists_str: String = row.try_get("artists").unwrap_or_default();
        let artists: Vec<String> = serde_json::from_str(&artists_str).unwrap_or_default();

        let release_date = row
            .try_get::<Option<String>, _>("release_date")
            .unwrap_or(None)
            .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok());

        let last_played = row
            .try_get::<Option<String>, _>("last_played")
            .unwrap_or(None)
            .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok());

        Ok(Track {
            yt_id: row.try_get("yt_id").unwrap_or_default(),
            bp_id: row.try_get("bp_id").unwrap_or(None),
            mb_id: row.try_get("mb_id").unwrap_or(None),
            title: row.try_get("title").unwrap_or_default(),
            mix_name: row.try_get("mix_name").unwrap_or(None),
            artists,
            release_date,
            label: row.try_get("label").unwrap_or(None),
            album: row.try_get("album").unwrap_or(None),
            length: row.try_get("length").unwrap_or(None),
            bpm: row.try_get("bpm").unwrap_or(None),
            genre: row.try_get("genre").unwrap_or(None),
            key: row.try_get("key").unwrap_or(None),
            mood: row.try_get("mood").unwrap_or(None),
            rating: row.try_get("rating").unwrap_or(None),
            play_count: row.try_get("play_count").unwrap_or(0),
            last_played,
        })
    }
}

// Scraper-internal types (not part of the public API)

pub struct BPLabel {
    pub display_name: String,
    pub name: String,
    pub id: i64,
}

pub struct BPArtist {
    pub display_name: String,
    pub name: String,
    pub id: i64,
    pub image_uri: String,
}

pub struct BPTrack {
    pub artists: Vec<String>,
    pub bpm: Option<i64>,
    pub key: Option<String>,
    pub isrc: Option<String>,
    pub label: String,
    pub release_date: chrono::NaiveDate,
    pub track_id: i64,
    pub track_name: String,
    pub mix_name: String,
    pub genre: Vec<String>,
}

impl BPTrack {
    pub fn into_track(self, yt_id: String) -> Track {
        Track {
            yt_id,
            title: self.track_name,
            mix_name: Some(self.mix_name),
            artists: self.artists,
            bp_id: Some(self.track_id),
            release_date: Some(self.release_date),
            label: Some(self.label),
            bpm: self.bpm,
            key: self.key,
            genre: Some(self.genre.join(", ")),
            ..Default::default()
        }
    }
}

pub struct SCUser {
    pub id: i64,
    pub username: String,
}

pub struct SCTrack {
    pub artists: Vec<String>,
    pub label: Option<String>,
    pub release_date: chrono::NaiveDate,
    pub track_name: String,
    pub genre: Vec<String>,
}

impl SCTrack {
    pub fn into_track(self, yt_id: String) -> Track {
        Track {
            yt_id,
            title: self.track_name,
            artists: self.artists,
            release_date: Some(self.release_date),
            label: self.label,
            genre: (!self.genre.is_empty()).then(|| self.genre.join(", ")),
            ..Default::default()
        }
    }
}

pub struct VLTrack {
    pub artists: Vec<String>,
    pub label: String,
    pub release_date: chrono::NaiveDate,
    pub track_id: i64,
    pub track_name: String,
    pub mix_name: String,
    pub isrc: Option<String>,
    pub bpm: Option<i64>,
    pub key: Option<String>,
    pub genre: Option<String>,
}

impl VLTrack {
    pub fn into_track(self, yt_id: String) -> Track {
        Track {
            yt_id,
            title: self.track_name,
            mix_name: Some(self.mix_name),
            artists: self.artists,
            release_date: Some(self.release_date),
            label: Some(self.label),
            bpm: self.bpm,
            key: self.key,
            genre: self.genre,
            ..Default::default()
        }
    }
}

pub struct TXTrack {
    pub artists: Vec<String>,
    pub label: String,
    pub release_date: chrono::NaiveDate,
    pub track_id: i64,
    pub track_name: String,
    pub mix_name: String,
    pub bpm: Option<i64>,
    pub key: Option<String>,
    pub genre: Option<String>,
}

impl TXTrack {
    pub fn into_track(self, yt_id: String) -> Track {
        Track {
            yt_id,
            title: self.track_name,
            mix_name: Some(self.mix_name),
            artists: self.artists,
            release_date: Some(self.release_date),
            label: Some(self.label),
            bpm: self.bpm,
            key: self.key,
            genre: self.genre,
            ..Default::default()
        }
    }
}

pub struct YTVideo {
    pub title: String,
    pub id: String,
    pub channel: String,
}

/// Which search backend a `SearchResult` track was found on. Purely informational for the
/// client — not persisted on `Track`/in the collection database.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TrackSource {
    Beatport,
    Soundcloud,
    Volumo,
    Traxsource,
}

pub struct MBRecording {
    pub title: String,
    pub id: String,
    pub artists: Vec<String>,
    pub release_date: Option<NaiveDate>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ScoredName {
    pub name: String,
    pub count: usize,
    pub weighted_avg_days_since_release: f64,
}
