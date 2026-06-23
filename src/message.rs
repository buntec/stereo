use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::models::{Collection, Repo, ScoredName, Track};

// ── Server → Client ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum MsgServer {
    CollectionInfo {
        id: Option<i64>,
        collection: Option<Collection>,
        error_message: Option<String>,
        path_completions: Option<Vec<String>>,
    },
    DefaultCollection {
        collection: Collection,
    },
    BackendInfo {
        version: String,
        home_dir: String,
    },
    Repos {
        repos: Vec<Repo>,
    },
    Collections {
        collections: Vec<Collection>,
    },
    Heartbeat {
        timestamp: i64,
    },
    TrackUpdate {
        track: Track,
    },
    ReloadTracks {},
    Rows {
        id: i64,
        rows: Vec<Track>,
        last_row: Option<i64>,
    },
    TrackInfo {
        track: Track,
    },
    PathCompletions {
        id: i64,
        paths: Vec<String>,
    },
    SearchResult {
        query_id: i64,
        track: Track,
    },
    SearchComplete {
        query_id: i64,
    },
    CollectionContainsIdResponse {
        id: i64,
        contains_id: bool,
    },
    ImportFromValid {
        path: String,
        is_valid: bool,
    },
    YtAnonPlaylist {
        id: i64,
        url: String,
    },
    GitRemote {
        url: Option<String>,
    },
    ValidateTrackReply {
        id: i64,
        is_valid: bool,
    },
    RowIndex {
        id: i64,
        index: i64,
    },
    Notification {
        message: String,
        kind: String,
    },
    CollectionStats {
        top_artists: Vec<ScoredName>,
        top_labels: Vec<ScoredName>,
    },
}

// ── Client → Server ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum SearchKind {
    #[default]
    Fuzzy,
    ByArtist,
    ByLabel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortModelItem {
    #[serde(rename = "colId")]
    pub col_id: String,
    pub sort: String,
    #[serde(rename = "type")]
    pub sort_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterModelItem {
    pub filter_type: String,
    #[serde(rename = "type")]
    pub filter_op: String,
    pub filter: Option<serde_json::Value>,
    pub filter_to: Option<serde_json::Value>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CombinedFilterModelItem {
    pub filter_type: String,
    pub operator: String,
    pub conditions: Vec<FilterModelItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FilterModel {
    Combined(CombinedFilterModelItem),
    Simple(FilterModelItem),
}

pub type FilterModelMap = HashMap<String, FilterModel>;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetRowsContent {
    pub id: i64,
    pub start_row: i64,
    pub end_row: i64,
    #[serde(default)]
    pub sort_model: Vec<SortModelItem>,
    #[serde(default)]
    pub filter_model: FilterModelMap,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetRowIndexContent {
    pub id: i64,
    #[serde(rename = "yt_id")]
    pub yt_id: String,
    #[serde(default)]
    pub sort_model: Vec<SortModelItem>,
    #[serde(default)]
    pub filter_model: FilterModelMap,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum MsgClient {
    Heartbeat {
        timestamp: i64,
    },
    DeleteTracks {
        ids: Vec<String>,
    },
    UpdateRating {
        yt_id: String,
        rating: Option<i64>,
    },
    IncPlayCount {
        yt_id: String,
    },
    GetRows(GetRowsContent),
    GetRowIndex(GetRowIndexContent),
    AddTrack {
        track: Track,
        #[serde(default)]
        overwrite_existing: bool,
    },
    AddTracks {
        tracks: Vec<Track>,
        #[serde(default)]
        overwrite_existing: bool,
    },
    GetTrackInfo {
        yt_id: String,
    },
    SetCollection {
        id: i64,
        path: String,
    },
    CreateCollection {
        name: String,
    },
    GetPathCompletions {
        id: i64,
        path_prefix: String,
    },
    Search {
        query: String,
        query_id: i64,
        limit: i64,
        #[serde(default)]
        kind: SearchKind,
    },
    SearchCancelAll {},
    CollectionContainsId {
        id: i64,
        yt_id: String,
    },
    CheckImportFrom {
        path: String,
    },
    ImportFrom {
        path: String,
        keep_user_data: bool,
    },
    CreateYtAnonPlaylist {
        id: i64,
        yt_ids: Vec<String>,
    },
    GetGitRemote,
    SetGitRemote {
        url: Option<String>,
    },
    SetRepo {
        path: String,
    },
    CloneRepo {
        url: String,
    },
    ListRepos,
    ListCollections,
    UpdateTrack {
        old: Track,
        new: Track,
    },
    ValidateTrack {
        id: i64,
        track: serde_json::Value,
    },
    ExportTracksToCollection {
        tracks: Vec<Track>,
        collection: String,
    },
}
