use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::models::{Collection, NewReleasesCandidate, Repo, ScoredName, Track, TrackSource};
use crate::playlists::{ExportTarget, Playlist, Service};

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
    /// The tailnet status of a server run with `--tailscale`, sent on connect and on change.
    Tailscale {
        status: crate::tailscale::TailscaleStatus,
    },
    /// Whether a newer release is out, sent on connect once known and after each daily check.
    UpdateCheck {
        status: crate::update_check::UpdateStatus,
    },
    /// Reply to `CheckForUpdates`: the status after the check.
    UpdateCheckReply {
        id: i64,
        status: crate::update_check::UpdateStatus,
    },
    /// Reply to `CreatePairingCode`: a single-use code that signs a browser on the tailnet in,
    /// valid for `expires_in` seconds.
    PairingCode {
        id: i64,
        code: String,
        expires_in: u64,
    },
    /// The token to authenticate with from now on, sent to a browser that connected with a
    /// pairing code.
    AuthToken {
        token: String,
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
    /// Reply to `GetWaveform`: loudness values (0–255) evenly spread over the track, its beats
    /// as `[milliseconds, strength (0–255)]` and the tempo they keep (`None` without enough beats);
    /// all `None` if the track isn't downloaded or its audio can't be decoded.
    Waveform {
        id: i64,
        yt_id: String,
        peaks: Option<Vec<u8>>,
        beats: Option<Vec<crate::beats::Beat>>,
        tempo: Option<crate::beats::Tempo>,
    },
    /// Generic failure reply to a request/reply message (see `MsgClient::request_id`).
    ErrorReply {
        id: i64,
        message: String,
    },
    /// Reply to `SetGitRemote`: the repo's remote now.
    SetGitRemoteReply {
        id: i64,
        url: Option<String>,
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
    /// Every check's state while a health check runs, sent at its start and whenever a check
    /// finishes; `partial` keeps the client's reply callback for the final `HealthCheck`.
    HealthCheckProgress {
        id: i64,
        partial: bool,
        checks: Vec<crate::health::CheckResult>,
    },
    /// Read-only diagnostics of the search providers, returned after all bounded probes finish.
    HealthCheck {
        id: i64,
        checked_at: String,
        checks: Vec<crate::health::CheckResult>,
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
        /// No video was found that names the track's version, so this one may be another, such
        /// as the radio edit of an extended mix.
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        version_unconfirmed: bool,
    },
    SearchComplete {
        query_id: i64,
    },
    /// Reply to `CollectionContainsIds`: the subset of the requested ids in the collection.
    CollectionContainsIdsResponse {
        id: i64,
        contained: Vec<String>,
    },
    /// Reply to `FindSimilarTracks`: for each requested track, in order, the collection's tracks
    /// that seem to be the same recording under another video.
    SimilarTracks {
        id: i64,
        similar: Vec<Vec<Track>>,
    },
    /// Reply to `GetCollectionTrack`: the track as stored in the collection, `None` if it isn't in
    /// it or the collection isn't the open one.
    CollectionTrack {
        id: i64,
        track: Option<Track>,
    },
    /// Reply to `ImportCollection`: the new collection's path.
    CollectionImported {
        id: i64,
        path: String,
    },
    /// Reply to `ImportTracks`: how many tracks were added, and how many skipped as already in
    /// the collection.
    TracksImported {
        id: i64,
        imported: u64,
        skipped: u64,
    },
    /// Reply to `PrepareCollectionDownload` and `PrepareTracksDownload`: the token for
    /// `/collection-download/{token}`.
    CollectionDownload {
        id: i64,
        token: String,
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
    /// Reply to `SearchYouTubeVideos`.
    #[serde(rename = "youtube-videos")]
    YouTubeVideos {
        id: i64,
        videos: Vec<crate::models::YTVideo>,
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
    /// The current repo's dynamic playlists, sent once the repo is synced and whenever they
    /// change.
    Playlists {
        playlists: Vec<PlaylistView>,
    },
    /// Reply to `SavePlaylist`: the id of the saved playlist.
    SavePlaylistReply {
        id: i64,
        playlist_id: String,
    },
    /// Progress and outcome of syncing a playlist to a service (`SyncPlaylist`).
    PlaylistSync(PlaylistSync),
}

#[derive(Debug, Clone, Serialize)]
pub struct PlaylistSync {
    pub playlist_id: String,
    pub service: Service,
    pub state: PlaylistSyncState,
    /// Tracks matched or uploaded so far, while running.
    pub current: Option<i64>,
    pub total: Option<i64>,
    /// For `Synced`: how many of the view's tracks are in the remote playlist.
    pub matched: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PlaylistSyncState {
    Running,
    Synced,
    /// Nothing to push: the view's tracks are the ones pushed last time.
    Unchanged,
    /// An automatic sync skipped because the last one was less than an hour ago.
    Throttled,
    /// Another sync of the same playlist to the same service is still running.
    Busy,
    Failed,
}

/// A playlist as the client sees it: its collection as the collection's full path, plus this
/// machine's sync state for each service it is exported to.
#[derive(Debug, Clone, Serialize)]
pub struct PlaylistView {
    #[serde(flatten)]
    pub playlist: Playlist,
    pub spotify_status: Option<PlaylistTargetStatus>,
    pub youtube_status: Option<PlaylistTargetStatus>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlaylistTargetStatus {
    pub url: Option<String>,
    pub synced_at: Option<i64>,
    /// Why the last sync from this machine failed, if it did.
    pub error: Option<String>,
}

/// A new playlist (`SavePlaylist`). The remote playlist ids of its targets are ignored: syncs set
/// those, and `UnlinkPlaylist` clears them.
#[derive(Debug, Clone, Deserialize)]
pub struct PlaylistInput {
    pub name: String,
    /// The collection's full path, as in `SetCollection`.
    pub collection: String,
    #[serde(default)]
    pub filter_model: FilterModelMap,
    #[serde(default)]
    pub sort_model: Vec<SortModelItem>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub spotify: Option<ExportTarget>,
    #[serde(default)]
    pub youtube: Option<ExportTarget>,
}

/// One change to an existing playlist (`EditPlaylist`). Each changes only its own fields of the
/// playlist as the server has it, so that quick edits in a row can't undo each other.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PlaylistEdit {
    Rename {
        name: String,
    },
    SetLimit {
        limit: Option<i64>,
    },
    /// Replaces the collection, filters and sorting.
    SetView {
        /// The collection's full path, as in `SetCollection`.
        collection: String,
        #[serde(default)]
        filter_model: FilterModelMap,
        #[serde(default)]
        sort_model: Vec<SortModelItem>,
    },
    /// Turns the export to `service` on or off (`None`), or changes its settings. The remote
    /// playlist id of the target is ignored: an export that stays on keeps its remote playlist.
    SetExport {
        service: Service,
        target: Option<ExportTarget>,
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
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub sort_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterModelItem {
    pub filter_type: String,
    #[serde(rename = "type")]
    pub filter_op: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_to: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
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
    /// Sets the BPM of a track in the open collection, e.g. to the tempo measured from its audio.
    UpdateBpm {
        yt_id: String,
        bpm: Option<i64>,
        /// The collection the BPM was checked against; refused unless it is the open one.
        collection: String,
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
    /// Adds a collection file to `repo`, which must be the current repo, as `name` (without the
    /// `.db` extension), as is, with its ratings and play counts, and opens it. Answered with
    /// `CollectionImported`.
    ImportCollection {
        id: i64,
        repo: String,
        name: String,
        /// The file's contents, base64-encoded.
        data: String,
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
    RunHealthCheck {
        id: i64,
    },
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
    /// Looks for tracks in the open collection like each of `tracks` (`MsgServer::SimilarTracks`).
    FindSimilarTracks {
        id: i64,
        tracks: Vec<crate::similar::TrackIdentity>,
        /// Answered with no similar tracks unless this is the open collection.
        collection: String,
    },
    /// The track with `yt_id` as stored in `collection`, if that is the open collection.
    GetCollectionTrack {
        id: i64,
        yt_id: String,
        collection: String,
    },
    /// Snapshots `collection`, which must be the open one, for download
    /// (`MsgServer::CollectionDownload`, see `collection_download.rs`).
    PrepareCollectionDownload {
        id: i64,
        collection: String,
    },
    /// Exports `tracks` to a JSON file named `name` (without the `.json` extension), with their
    /// ratings and play history if `include_user_data`, for download like a collection
    /// (`MsgServer::CollectionDownload`, see `tracks_file.rs`).
    PrepareTracksDownload {
        id: i64,
        name: String,
        tracks: Vec<Track>,
        include_user_data: bool,
    },
    /// Adds the tracks of an exported JSON file (`data`, parsed) to `collection`, which must be
    /// the open one, skipping those already in it. Answered with `TracksImported`.
    ImportTracks {
        id: i64,
        collection: String,
        data: serde_json::Value,
        keep_user_data: bool,
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
    /// Checks for a newer release now (`MsgServer::UpdateCheckReply`), unless the last check
    /// was within the minute; every session also gets the result as `MsgServer::UpdateCheck`.
    CheckForUpdates {
        id: i64,
    },
    /// Asks for a pairing code (`MsgServer::PairingCode`), with `--tailscale` only.
    CreatePairingCode {
        id: i64,
    },
    GetGitRemote,
    /// Sets (or with `None`, removes) the remote of `repo`, named rather than taken from the
    /// session, so a request queued across a reconnect can't land on another repository.
    SetGitRemote {
        id: i64,
        repo: String,
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
    /// YouTube videos for `query` (`MsgServer::YouTubeVideos`), to pick another video for a track
    /// whose automatic match is wrong; a YouTube video link gives just that video.
    #[serde(rename = "search-youtube-videos")]
    SearchYouTubeVideos {
        id: i64,
        query: String,
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
    /// tab's `navigator.userAgent`, or of any browser without one (the desktop app, whose webview
    /// is no browser of the user's); `login` is the profile the user picked from
    /// `youtube-logins`.
    #[serde(rename = "connect-youtube-session")]
    ConnectYouTubeSession {
        id: i64,
        #[serde(default)]
        user_agent: Option<String>,
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
    ListPlaylists,
    /// Creates a dynamic playlist in the current repo.
    SavePlaylist {
        id: i64,
        playlist: PlaylistInput,
    },
    /// Changes a dynamic playlist of the current repo.
    EditPlaylist {
        playlist_id: String,
        edit: PlaylistEdit,
    },
    DeletePlaylist {
        playlist_id: String,
    },
    /// Forgets the remote playlist a playlist is exported to, so the next sync creates a new one.
    UnlinkPlaylist {
        playlist_id: String,
        service: Service,
    },
    /// Pushes a playlist's tracks to a service. Unless `force`d, at most once an hour, and only
    /// if they changed since the last push.
    SyncPlaylist {
        playlist_id: String,
        service: Service,
        #[serde(default)]
        force: bool,
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
            | Self::FindSimilarTracks { id, .. }
            | Self::GetCollectionTrack { id, .. }
            | Self::PrepareCollectionDownload { id, .. }
            | Self::PrepareTracksDownload { id, .. }
            | Self::ImportTracks { id, .. }
            | Self::ImportCollection { id, .. }
            | Self::GetWaveform { id, .. }
            | Self::CreatePairingCode { id }
            | Self::CheckForUpdates { id }
            | Self::GetNewReleasesCandidates { id }
            | Self::RunHealthCheck { id }
            | Self::CreateYtAnonPlaylist { id, .. }
            | Self::GetSpotifySession { id }
            | Self::SetSpotifySession { id, .. }
            | Self::DetectSpotifySession { id }
            | Self::GetBeatportSession { id }
            | Self::ConnectBeatportSession { id, .. }
            | Self::DisconnectBeatportSession { id }
            | Self::GetSpotifyPlaylistInfo { id, .. }
            | Self::GetYouTubePlaylist { id, .. }
            | Self::SearchYouTubeVideos { id, .. }
            | Self::CloneRepo { id, .. }
            | Self::SetGitRemote { id, .. }
            | Self::ValidateTrack { id, .. }
            | Self::CheckSpotifyId { id, .. }
            | Self::FixupTrack { id, .. }
            | Self::GetYouTubeSession { id }
            | Self::ConnectYouTubeSession { id, .. }
            | Self::DisconnectYouTubeSession { id }
            | Self::SavePlaylist { id, .. } => Some(*id),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_check_protocol_matches_status_tags() {
        use crate::update_check::{Release, UpdateStatus};
        let latest = Release {
            version: "0.2.0".into(),
            url: "https://github.com/buntec/stereo/releases/tag/v0.2.0".into(),
            published_at: Some("2026-11-22T16:18:52Z".parse().unwrap()),
        };
        let checked_at: chrono::DateTime<chrono::Utc> = "2026-11-23T06:00:00Z".parse().unwrap();
        let message = |status| serde_json::to_value(MsgServer::UpdateCheck { status }).unwrap();

        let available = message(UpdateStatus::Available {
            latest: latest.clone(),
            checked_at,
            refresh_error: None,
        });
        assert_eq!(available["type"], "update-check");
        assert_eq!(available["status"]["state"], "available");
        assert_eq!(available["status"]["latest"]["version"], "0.2.0");
        assert_eq!(available["status"]["latest"]["url"], latest.url);
        assert_eq!(
            available["status"]["latest"]["published_at"],
            "2026-11-22T16:18:52Z"
        );
        assert_eq!(available["status"]["checked_at"], "2026-11-23T06:00:00Z");
        assert!(available["status"]["refresh_error"].is_null());

        let up_to_date = message(UpdateStatus::UpToDate {
            latest,
            checked_at,
            refresh_error: Some(crate::update_check::RefreshError {
                message: "proxy refused".into(),
                failed_at: "2026-11-28T06:00:00Z".parse().unwrap(),
                is_stale: true,
            }),
        });
        assert_eq!(up_to_date["status"]["state"], "up-to-date");
        assert_eq!(
            up_to_date["status"]["refresh_error"]["message"],
            "proxy refused"
        );
        assert_eq!(
            up_to_date["status"]["refresh_error"]["failed_at"],
            "2026-11-28T06:00:00Z"
        );
        assert_eq!(up_to_date["status"]["refresh_error"]["is_stale"], true);

        let error = message(UpdateStatus::Error {
            message: "GitHub replied 403".into(),
            checked_at,
        });
        assert_eq!(error["status"]["state"], "error");
        assert_eq!(error["status"]["message"], "GitHub replied 403");

        let request: MsgClient = serde_json::from_value(serde_json::json!({
            "type": "check-for-updates", "id": 7,
        }))
        .unwrap();
        assert_eq!(request.request_id(), Some(7));
        let reply = serde_json::to_value(MsgServer::UpdateCheckReply {
            id: 7,
            status: UpdateStatus::Error {
                message: "GitHub replied 403".into(),
                checked_at,
            },
        })
        .unwrap();
        assert_eq!(reply["type"], "update-check-reply");
        assert_eq!(reply["id"], 7);
        assert_eq!(reply["status"]["state"], "error");
    }

    #[test]
    fn health_check_protocol_matches_request_reply_and_status_tags() {
        let request: MsgClient = serde_json::from_value(serde_json::json!({
            "type": "run-health-check", "id": 42,
        }))
        .unwrap();
        assert_eq!(request.request_id(), Some(42));
        let reply = serde_json::to_value(MsgServer::HealthCheck {
            id: 42,
            checked_at: "2026-11-13T12:00:00Z".into(),
            checks: vec![crate::health::CheckResult {
                name: "YouTube".into(),
                probe: "Known song".into(),
                status: crate::health::CheckStatus::Failed,
                message: "HTTP 429".into(),
                duration_ms: 100,
            }],
        })
        .unwrap();
        assert_eq!(reply["type"], "health-check");
        assert_eq!(reply["id"], 42);
        assert_eq!(reply["checks"][0]["status"], "failed");
        assert_eq!(reply["checks"][0]["duration_ms"], 100);
        let progress = serde_json::to_value(MsgServer::HealthCheckProgress {
            id: 42,
            partial: true,
            checks: vec![],
        })
        .unwrap();
        assert_eq!(progress["type"], "health-check-progress");
        assert_eq!(progress["id"], 42);
        // The client keeps the request's callback for the final reply only on this mark.
        assert_eq!(progress["partial"], true);
    }

    #[test]
    fn waveform_beats_are_time_and_strength_pairs() {
        let msg = MsgServer::Waveform {
            id: 7,
            yt_id: "dQw4w9WgXcQ".into(),
            peaks: Some(vec![0, 255]),
            beats: Some(vec![(250, 200), (719, 255)]),
            tempo: Some(crate::beats::Tempo {
                bpm: 124.5,
                steady: true,
            }),
        };
        assert_eq!(
            serde_json::to_value(&msg).unwrap(),
            serde_json::json!({
                "type": "waveform",
                "id": 7,
                "yt_id": "dQw4w9WgXcQ",
                "peaks": [0, 255],
                "beats": [[250, 200], [719, 255]],
                "tempo": {"bpm": 124.5, "steady": true},
            })
        );
    }
}
