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

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AudioDownloadStatus {
    /// Waiting for another download to finish.
    Queued,
    /// yt-dlp is looking up the audio stream.
    Preparing,
    Downloading,
    Done,
    Failed,
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
        /// Key for the `/audio` route, or `None` if audio downloads are unavailable (yt-dlp can't
        /// be run), in which case nothing can be played.
        audio_key: Option<String>,
    },
    Repos {
        repos: Vec<Repo>,
    },
    /// Progress of an audio download for `/audio` (see `audio.rs`), sent to every session.
    AudioDownload {
        yt_id: String,
        status: AudioDownloadStatus,
        /// Fraction downloaded, while downloading and if the size is known.
        progress: Option<f64>,
        /// Why the download failed, for `Failed`.
        error: Option<String>,
    },
    /// The tracks whose audio is downloaded, sent on connect; `AudioDownload`s that are `Done` and
    /// `AudioEvicted` follow as it changes.
    AudioCached {
        yt_ids: Vec<String>,
    },
    /// These tracks' audio was evicted from the cache.
    AudioEvicted {
        yt_ids: Vec<String>,
    },
    /// Reply to `GetWaveform`: loudness values (0–255) evenly spread over the track, and its beats
    /// as `[milliseconds, strength (0–255)]`; both `None` if the track isn't downloaded or its audio
    /// can't be decoded.
    Waveform {
        id: i64,
        yt_id: String,
        peaks: Option<Vec<u8>>,
        beats: Option<Vec<crate::beats::Beat>>,
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
        /// 1-based position in the chart, for chart searches (Beatport/Traxsource top 100).
        #[serde(skip_serializing_if = "Option::is_none")]
        chart_position: Option<usize>,
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
    /// Reply to the Beatport session requests: the browser the connected clearance came from,
    /// or `None` when not connected.
    BeatportSession {
        id: i64,
        browser: Option<String>,
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
    /// Reply to the YouTube session requests: the browser the YouTube login is read from, or
    /// `None` when not connected, and the account logged in there when connecting, if known.
    #[serde(rename = "youtube-session")]
    YouTubeSession {
        id: i64,
        browser: Option<String>,
        account: Option<String>,
    },
    /// Reply to `connect-youtube-session` when several browser profiles are logged in to
    /// YouTube: the user picks one, which is connected by sending its `id` as `login`.
    #[serde(rename = "youtube-logins")]
    YouTubeLogins {
        id: i64,
        logins: Vec<YouTubeLogin>,
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

/// A browser profile logged in to YouTube.
#[derive(Debug, Clone, Serialize)]
pub struct YouTubeLogin {
    pub id: String,
    /// The browser, plus the profile when it has several.
    pub browser: String,
    /// The YouTube account or channel logged in there, if YouTube said.
    pub account: Option<String>,
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
        /// The collection the ids were chosen from; refused unless it is the open one.
        collection: String,
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
    /// Deletes a collection of the current repo; the deletion is committed by the next sync.
    DeleteCollection {
        path: String,
    },
    /// Renames a collection of the current repo to `name` (without the `.db` extension); the
    /// rename is committed by the next sync.
    RenameCollection {
        path: String,
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
    /// Beatport's top-100 chart for a genre, identified by its Beatport genre id.
    SearchBeatportTop100 {
        query_id: i64,
        genre_id: i64,
        limit: i64,
    },
    /// Traxsource's top-100 chart, across all genres.
    SearchTraxsourceTop100 {
        query_id: i64,
        limit: i64,
    },
    SearchCancelAll {},
    /// Downloads a track's audio in the background, even if its download failed recently.
    DownloadAudio {
        yt_id: String,
    },
    /// Asks for the waveform of a downloaded track (`MsgServer::Waveform`).
    GetWaveform {
        id: i64,
        yt_id: String,
    },
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
    GetBeatportSession {
        id: i64,
    },
    /// Imports Beatport's Cloudflare clearance from the browser the requesting tab runs in,
    /// identified by that tab's `navigator.userAgent`.
    ConnectBeatportSession {
        id: i64,
        user_agent: String,
    },
    DisconnectBeatportSession {
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
    /// Finds the YouTube login of the browser the requesting tab runs in, identified by that
    /// tab's `navigator.userAgent`; `login` is the profile the user picked from `youtube-logins`.
    #[serde(rename = "connect-youtube-session")]
    ConnectYouTubeSession {
        id: i64,
        user_agent: String,
        #[serde(default)]
        login: Option<String>,
    },
    #[serde(rename = "disconnect-youtube-session")]
    DisconnectYouTubeSession {
        id: i64,
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
            | Self::GetWaveform { id, .. }
            | Self::GetNewReleasesCandidates { id }
            | Self::CreateYtAnonPlaylist { id, .. }
            | Self::GetSpotifySession { id }
            | Self::SetSpotifySession { id, .. }
            | Self::DetectSpotifySession { id }
            | Self::GetBeatportSession { id }
            | Self::ConnectBeatportSession { id, .. }
            | Self::DisconnectBeatportSession { id }
            | Self::GetSpotifyPlaylistInfo { id, .. }
            | Self::GetYouTubePlaylist { id, .. }
            | Self::CloneRepo { id, .. }
            | Self::ValidateTrack { id, .. }
            | Self::CheckSpotifyId { id, .. }
            | Self::FixupTrack { id, .. }
            | Self::GetYouTubeSession { id }
            | Self::ConnectYouTubeSession { id, .. }
            | Self::DisconnectYouTubeSession { id } => Some(*id),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waveform_beats_are_time_and_strength_pairs() {
        let msg = MsgServer::Waveform {
            id: 7,
            yt_id: "dQw4w9WgXcQ".into(),
            peaks: Some(vec![0, 255]),
            beats: Some(vec![(250, 200), (719, 255)]),
        };
        assert_eq!(
            serde_json::to_value(&msg).unwrap(),
            serde_json::json!({
                "type": "waveform",
                "id": 7,
                "yt_id": "dQw4w9WgXcQ",
                "peaks": [0, 255],
                "beats": [[250, 200], [719, 255]],
            })
        );
    }
}
