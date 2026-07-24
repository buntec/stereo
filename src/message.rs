use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::models::{Collection, Repo, ScoredName, Track, TrackSource};

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
        source: TrackSource,
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
    SpotifySession {
        id: i64,
        connected: bool,
    },
    SpotifyPlaylist {
        id: i64,
        url: Option<String>,
        matched: i64,
        total: i64,
    },
    SpotifyPlaylistProgress {
        id: i64,
        checked: i64,
        matched: i64,
        total: i64,
    },
    ValidateTrackReply {
        id: i64,
        is_valid: bool,
    },
    CheckSpotifyIdReply {
        id: i64,
        sp_id: String,
        exists: bool,
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
        top_artists_five_star: Vec<ScoredName>,
        top_artists_four_plus: Vec<ScoredName>,
        top_labels_five_star: Vec<ScoredName>,
        top_labels_four_plus: Vec<ScoredName>,
    },
    FixupTrackReply {
        id: i64,
        track: Option<Track>,
    },
    SpotifyBackfillProgress {
        id: i64,
        checked: i64,
        matched: i64,
        total: i64,
    },
    SpotifyBackfillComplete {
        id: i64,
        matched: i64,
        total: i64,
    },
    #[serde(rename = "youtube-session")]
    YouTubeSession {
        id: i64,
        connected: bool,
        client_id: Option<String>,
    },
    #[serde(rename = "youtube-playlist")]
    YouTubePlaylist {
        id: i64,
        url: Option<String>,
    },
    #[serde(rename = "youtube-playlist-progress")]
    YouTubePlaylistProgress {
        id: i64,
        current: i64,
        total: i64,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpotifyTrackRef {
    pub artist: String,
    pub title: String,
    #[serde(default)]
    pub sp_id: Option<String>,
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
        sources: Vec<TrackSource>,
    },
    SearchNewReleases {
        query_id: i64,
        days: i64,
        limit: i64,
        sources: Vec<TrackSource>,
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
    GetSpotifySession {
        id: i64,
    },
    SetSpotifySession {
        id: i64,
        cookie: Option<String>,
    },
    DetectSpotifySession {
        id: i64,
    },
    CreateSpotifyPlaylist {
        id: i64,
        name: String,
        tracks: Vec<SpotifyTrackRef>,
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
        new: Box<Track>,
    },
    ValidateTrack {
        id: i64,
        track: serde_json::Value,
    },
    CheckSpotifyId {
        id: i64,
        sp_id: String,
    },
    ExportTracksToCollection {
        tracks: Vec<Track>,
        collection: String,
        #[serde(default)]
        source_collection: Option<String>,
    },
    FixupTrack {
        id: i64,
        track: Track,
    },
    BackfillSpotifyIds {
        id: i64,
    },
    #[serde(rename = "get-youtube-session")]
    GetYouTubeSession {
        id: i64,
    },
    #[serde(rename = "set-youtube-credentials")]
    SetYouTubeCredentials {
        id: i64,
        client_id: String,
        client_secret: String,
    },
    #[serde(rename = "disconnect-youtube-session")]
    DisconnectYouTubeSession {
        id: i64,
    },
    #[serde(rename = "exchange-youtube-code")]
    ExchangeYouTubeCode {
        id: i64,
        code: String,
        verifier: String,
        redirect_uri: String,
    },
    #[serde(rename = "create-youtube-playlist")]
    CreateYouTubePlaylist {
        id: i64,
        name: String,
        yt_ids: Vec<String>,
    },
}
