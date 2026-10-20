use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::models::{Collection, NewReleasesCandidate, Repo, ScoredName, Track, TrackSource};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum GitSyncState {
    LocalChanges,
    Syncing,
    Synced,
    LocalOnly,
    Offline,
    AuthenticationRequired,
    RemoteAhead,
    ConflictRequiresAttention,
    CorruptCollection,
    Error,
}

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
    /// Generic failure reply to a request/reply message (see `MsgClient::request_id`).
    ErrorReply {
        id: i64,
        message: String,
    },
    /// Reply to `CloneRepo`: the path of the repo that is now active, or `None` if cloning failed.
    CloneRepoReply {
        id: i64,
        path: Option<String>,
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
    /// Reply to `CollectionContainsIds`: the subset of the requested ids in the collection.
    CollectionContainsIdsResponse {
        id: i64,
        contained: Vec<String>,
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
    GitSyncStatus {
        state: GitSyncState,
        message: String,
        retryable: bool,
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
        error_message: Option<String>,
    },
    SpotifyPlaylistProgress {
        id: i64,
        checked: i64,
        matched: i64,
        total: i64,
    },
    SpotifyPlaylistInfo {
        id: i64,
        name: Option<String>,
    },
    #[serde(rename = "youtube-playlist-info")]
    YouTubePlaylistInfo {
        id: i64,
        name: Option<String>,
        tracks: Vec<Track>,
        error_message: Option<String>,
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
    /// Reply to `GetNewReleasesCandidates`.
    NewReleasesCandidates {
        id: i64,
        artists: Vec<NewReleasesCandidate>,
        labels: Vec<NewReleasesCandidate>,
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
        error_message: Option<String>,
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
        error_message: Option<String>,
    },
    #[serde(rename = "youtube-playlist-progress")]
    YouTubePlaylistProgress {
        id: i64,
        current: i64,
        total: i64,
    },
    FixupTracksProgress {
        id: i64,
        current: i64,
        matched: i64,
        total: i64,
    },
    FixupTracksComplete {
        id: i64,
        tracks: Vec<Track>,
        matched: i64,
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

#[derive(Deserialize)]
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
    GetNewReleasesCandidates {
        id: i64,
    },
    SearchNewReleases {
        query_id: i64,
        days: i64,
        limit: i64,
        sources: Vec<TrackSource>,
        artists: Vec<String>,
        labels: Vec<String>,
    },
    SearchShazamSimilar {
        query_id: i64,
        track: Track,
        limit: i64,
    },
    SearchCancelAll {},
    CollectionContainsIds {
        id: i64,
        yt_ids: Vec<String>,
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
    RetryGitSync,
    ResolveGitConflict {
        strategy: GitConflictStrategy,
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
        #[serde(default)]
        existing_playlist: Option<String>,
    },
    GetSpotifyPlaylistInfo {
        id: i64,
        url: String,
    },
    #[serde(rename = "get-youtube-playlist")]
    GetYouTubePlaylist {
        id: i64,
        url: String,
    },
    SetRepo {
        path: String,
    },
    CloneRepo {
        id: i64,
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
    FixupTracks {
        id: i64,
        tracks: Vec<Track>,
    },
    CancelFixupTracks {
        id: i64,
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
    #[serde(rename = "cancel-youtube-playlist")]
    CancelYouTubePlaylist {
        id: i64,
    },
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitConflictStrategy {
    KeepLocal,
    KeepUpstream,
}

impl MsgClient {
    /// The id of a request the client is awaiting a reply to (sent via its `requestReply`), so a
    /// failed request can be answered with `MsgServer::ErrorReply`. Messages whose `id` is a
    /// job id from a separate client-side counter (exports, fixups, backfills) are excluded, as
    /// their ids could collide with pending request ids.
    pub fn request_id(&self) -> Option<i64> {
        match self {
            Self::GetRows(content) => Some(content.id),
            Self::GetRowIndex(content) => Some(content.id),
            Self::SetCollection { id, .. }
            | Self::GetPathCompletions { id, .. }
            | Self::CollectionContainsIds { id, .. }
            | Self::GetNewReleasesCandidates { id }
            | Self::CreateYtAnonPlaylist { id, .. }
            | Self::GetSpotifySession { id }
            | Self::SetSpotifySession { id, .. }
            | Self::DetectSpotifySession { id }
            | Self::GetSpotifyPlaylistInfo { id, .. }
            | Self::GetYouTubePlaylist { id, .. }
            | Self::CloneRepo { id, .. }
            | Self::ValidateTrack { id, .. }
            | Self::CheckSpotifyId { id, .. }
            | Self::FixupTrack { id, .. }
            | Self::GetYouTubeSession { id }
            | Self::SetYouTubeCredentials { id, .. }
            | Self::DisconnectYouTubeSession { id }
            | Self::ExchangeYouTubeCode { id, .. } => Some(*id),
            _ => None,
        }
    }
}
