import type { PlaybackStatus } from "./Playback.ts";

export type AudioDownloadStatus =
  "queued" | "preparing" | "downloading" | "done" | "failed";

/** An audio download that hasn't finished yet, or that failed. */
export type AudioDownload = {
  status: AudioDownloadStatus;
  progress?: number;
  /** Why the download failed, for "failed". */
  error?: string;
};

/** A beat of a track: its time and its strength (0–255) relative to the track's strong beats. */
export type Beat = [ms: number, strength: number];

/** A track's tempo, measured from its beats. */
export type Tempo = {
  /** As ambiguous as the beats between the beat and half or double time. */
  bpm: number;
  /** Whether the beats keep to that one tempo throughout, rather than drifting or changing. */
  steady: boolean;
};

export type TrackSource =
  | "beatport"
  | "soundcloud"
  | "volumo"
  | "traxsource"
  | "spotify"
  | "apple-music"
  | "shazam";

export interface ITrack {
  title: string;
  mix_name?: string;
  artists: string[];
  yt_id: string;
  bp_id?: number;
  sp_id?: string;
  mb_id?: string;
  release_date?: string;
  label?: string;
  genre?: string;
  bpm?: number;
  key?: string;
  rating?: number;
  play_count?: number;
  last_played?: string;
  // Only ever set on search-result tracks (see ServerMsg's "search-result"); not persisted
  // on tracks read back from the collection.
  source?: TrackSource;
  // Like `source`, only set on search results — and only for chart searches (Beatport/Traxsource top 100).
  chart_position?: number;
  // Client-only: identifies a search result for as long as it is shown, independently of
  // its `yt_id`, which the user can change (see `set-search-result-video`). Assigned by
  // App.tsx's reducer as results arrive, unique across searches.
  result_id?: number;
}

export interface ISearchResult {
  track?: ITrack;
  exists_in_db?: boolean;
  message?: string;
}

export interface ICollection {
  path: string;
  size: number;
}

export interface IScoredName {
  name: string;
  count: number;
  weighted_avg_days_since_release: number;
}

export interface INewReleasesCandidate {
  name: string;
  five_star: number;
  four_plus: number;
  is_default: boolean;
}

export interface ICollectionStats {
  top_artists_five_star: IScoredName[];
  top_artists_four_plus: IScoredName[];
  top_labels_five_star: IScoredName[];
  top_labels_four_plus: IScoredName[];
}

export interface IRepo {
  path: string;
  remote: string | null;
}

export type GitSyncState =
  | "local-changes"
  | "syncing"
  | "synced"
  | "local-only"
  | "offline"
  | "authentication-required"
  | "remote-ahead"
  | "conflict-requires-attention"
  | "corrupt-collection"
  | "error";

export type SearchKind = "fuzzy" | "by-label" | "by-artist";
export type SearchBoxKind =
  SearchKind | "new-releases" | "beatport-top-100" | "traxsource-top-100";

/// Chart search kinds, which come from a single store and ignore the search sources.
export const isChartSearchKind = (kind: SearchBoxKind) =>
  kind === "beatport-top-100" || kind === "traxsource-top-100";

/// Search kinds that run without a typed query.
export const isQuerylessSearchKind = (kind: SearchBoxKind) =>
  kind === "new-releases" || isChartSearchKind(kind);

/**
 * Whether a search kind can use a source. Label searches and latest releases need a source
 * that lists a label's or artist's catalog, which Spotify and Apple Music can't (the backend's
 * `CATALOG_SOURCES`).
 */
export const searchKindSupportsSource = (
  kind: SearchBoxKind,
  source: TrackSource,
) =>
  !(
    (kind === "by-label" || kind === "new-releases") &&
    (source === "spotify" || source === "apple-music")
  );

/** The selected sources a search kind can use. */
export const usableSearchSources = (
  kind: SearchBoxKind,
  sources: TrackSource[],
) => sources.filter((source) => searchKindSupportsSource(kind, source));

/** Whether a backend run with `--tailscale` serves its tailnet URL yet. */
export type TailscaleStatus =
  | { state: "starting" }
  | { state: "serving"; url: string }
  /** The last attempt failed; the backend keeps retrying. */
  | { state: "error"; message: string };

/** The newest published release on GitHub. */
export type Release = {
  version: string;
  url: string;
  published_at?: string | null;
};

/** How the running version compares with the latest release, checked daily. */
export type UpdateStatus =
  | { state: "up-to-date"; latest: Release; checked_at: string }
  | { state: "available"; latest: Release; checked_at: string }
  /** No check has succeeded yet; the backend retries hourly. */
  | { state: "error"; message: string; checked_at: string };

export type ClientMsg =
  | { type: "run-health-check" }
  | { type: "set-collection"; path: string }
  | { type: "validate-track"; track: unknown; id?: number }
  | { type: "check-spotify-id"; sp_id: string; id?: number }
  | { type: "create-collection"; name: string }
  // `data` is the collection file, base64-encoded.
  | { type: "import-collection"; repo: string; name: string; data: string }
  | { type: "delete-collection"; path: string }
  | { type: "rename-collection"; path: string; name: string }
  | { type: "collection-contains-ids"; yt_ids: string[] }
  | { type: "get-collection-track"; yt_id: string; collection: string }
  | { type: "prepare-collection-download"; collection: string }
  | { type: "download-audio"; yt_id: string }
  | { type: "create-pairing-code" }
  | { type: "get-waveform"; yt_id: string }
  | { type: "get-track-info"; yt_id: string }
  | { type: "import-from"; path: string; keep_user_data: boolean }
  | { type: "check-import-from"; path: string }
  | { type: "inc-play-count"; yt_id: string }
  | { type: "create-yt-anon-playlist"; id?: number; yt_ids: string[] }
  | { type: "search-cancel-all" }
  | { type: "update-rating"; yt_id: string; rating: number | null }
  | {
      type: "update-bpm";
      yt_id: string;
      bpm: number | null;
      /** The collection the BPM was checked against; refused unless it is the open one. */
      collection: string;
    }
  | { type: "update-track"; old: ITrack; new: ITrack }
  | { type: "delete-tracks"; ids: string[]; collection: string }
  | {
      type: "export-tracks-to-collection";
      tracks: ITrack[];
      collection: string;
      source_collection?: string;
    }
  | {
      type: "get-rows";
      startRow: number;
      endRow: number;
      sortModel: unknown;
      filterModel: unknown;
    }
  | {
      type: "get-row-index";
      yt_id: string;
      sortModel: unknown;
      filterModel: unknown;
    }
  | { type: "add-tracks"; tracks: ITrack[]; overwrite_existing: boolean }
  | {
      type: "search";
      query: string;
      query_id: number;
      kind: SearchKind;
      limit: number;
      sources: TrackSource[];
    }
  | { type: "get-new-releases-candidates" }
  | {
      type: "search-new-releases";
      query_id: number;
      days: number;
      limit: number;
      sources: TrackSource[];
      artists: string[];
      labels: string[];
    }
  | {
      type: "search-shazam-similar";
      query_id: number;
      track: ITrack;
      limit: number;
    }
  | {
      type: "search-beatport-top100";
      query_id: number;
      genre_id: number;
      limit: number;
    }
  | { type: "search-traxsource-top100"; query_id: number; limit: number }
  | { type: "get-git-remote" }
  | { type: "set-git-remote"; repo: string; url: string | null }
  | { type: "retry-git-sync" }
  | {
      type: "resolve-git-conflict";
      strategy: "keep-local" | "keep-upstream";
    }
  | { type: "get-spotify-session"; id?: number }
  | { type: "set-spotify-session"; id?: number; cookie: string | null }
  | { type: "detect-spotify-session"; id?: number }
  | { type: "get-beatport-session"; id?: number }
  | { type: "connect-beatport-session"; id?: number; user_agent: string }
  | { type: "disconnect-beatport-session"; id?: number }
  | {
      type: "create-spotify-playlist";
      id?: number;
      name: string;
      tracks: { artist: string; title: string; sp_id?: string | null }[];
      existing_playlist?: string | null;
    }
  | { type: "get-spotify-playlist-info"; id?: number; url: string }
  | { type: "get-youtube-playlist"; id?: number; url: string }
  /** Videos to pick a track's video from; a YouTube video link gives just that video. */
  | { type: "search-youtube-videos"; id?: number; query: string }
  | { type: "set-repo"; path: string }
  | { type: "clone-repo"; url: string; id?: number }
  | { type: "list-repos" }
  | { type: "list-collections" }
  | { type: "fixup-track"; track: ITrack; id?: number }
  | { type: "fixup-tracks"; id?: number; tracks: ITrack[] }
  | { type: "cancel-fixup-tracks"; id: number }
  | { type: "backfill-spotify-ids"; id: number }
  | { type: "get-youtube-session"; id?: number }
  | {
      type: "connect-youtube-session";
      id?: number;
      /** Left out by the desktop app, to look in every browser. */
      user_agent?: string;
      /** The `id` of the login picked from a `youtube-logins` reply. */
      login?: string;
    }
  | { type: "disconnect-youtube-session"; id?: number }
  | {
      type: "create-youtube-playlist";
      id?: number;
      name: string;
      yt_ids: string[];
    }
  | { type: "cancel-youtube-playlist"; id: number }
  | { type: "list-playlists" }
  | { type: "save-playlist"; id?: number; playlist: PlaylistInput }
  | { type: "edit-playlist"; playlist_id: string; edit: PlaylistEdit }
  | { type: "delete-playlist"; playlist_id: string }
  | { type: "unlink-playlist"; playlist_id: string; service: PlaylistService }
  | {
      type: "sync-playlist";
      playlist_id: string;
      service: PlaylistService;
      /** Push even if the last sync was less than an hour ago or nothing changed. */
      force: boolean;
    };

export type ServerMsg =
  | {
      /** Every check's state while a health check runs; replies to its request ahead of
       * the final `health-check`. */
      type: "health-check-progress";
      id: number;
      partial: true;
      checks: HealthCheckResult[];
    }
  | {
      type: "health-check";
      id: number;
      checked_at: string;
      checks: HealthCheckResult[];
    }
  | {
      type: "notification";
      message: string;
      kind: "info" | "warn" | "warning" | "error";
    }
  | { type: "heartbeat"; timestamp: number }
  | { type: "validate-track-reply"; is_valid: boolean; id: number }
  | { type: "youtube-videos"; id: number; videos: YouTubeVideo[] }
  | {
      type: "check-spotify-id-reply";
      id: number;
      sp_id: string;
      exists: boolean;
    }
  | {
      type: "set-playlist";
      ids: string[];
      // For a queue of search results: the `result_id` of each entry, so that changing a
      // result's video also changes its queued entry.
      result_ids?: (number | undefined)[];
    }
  | { type: "play-id"; id: string }
  | { type: "play-next-track" }
  | { type: "play-prev-track" }
  | { type: "tracks"; tracks: ITrack[] }
  | { type: "track-update"; track: ITrack }
  | { type: "reload-tracks" }
  | { type: "rows"; id: number; rows: ITrack[]; last_row?: number }
  | {
      type: "row-index";
      index: number;
    }
  | {
      type: "backend-info";
      version: string;
      home_dir: string;
      audio_key?: string | null;
    }
  | { type: "tailscale"; status: TailscaleStatus }
  | { type: "update-check"; status: UpdateStatus }
  | { type: "pairing-code"; id: number; code: string; expires_in: number }
  | { type: "auth-token"; token: string }
  | {
      type: "audio-download";
      yt_id: string;
      status: AudioDownloadStatus;
      /** Fraction downloaded, while downloading and if the size is known. */
      progress?: number | null;
      /** Why the download failed, for "failed". */
      error?: string | null;
    }
  /** The tracks whose audio is downloaded, sent on connect. */
  | { type: "audio-cached"; yt_ids: string[] }
  | { type: "audio-evicted"; yt_ids: string[] }
  | {
      type: "waveform";
      id: number;
      yt_id: string;
      /** Loudness values (0–255) evenly spread over the track. */
      peaks?: number[] | null;
      /** The track's beats, as `[milliseconds, strength (0–255)]`. */
      beats?: Beat[] | null;
      /** The tempo of the beats, if there are enough of them. */
      tempo?: Tempo | null;
    }
  | { type: "yt-anon-playlist"; id: number; url: string }
  | { type: "track-info"; track: ITrack }
  | {
      type: "collection-info";
      id?: number;
      collection?: ICollection;
      error_message?: string;
      path_completions?: string[];
    }
  | {
      type: "default-collection";
      collection: ICollection;
    }
  | { type: "path-completions"; id: number; paths: string[] }
  | {
      type: "search-result";
      query_id: number;
      track: ITrack;
      source: TrackSource;
      chart_position?: number;
    }
  | { type: "search-results"; query_id: number; tracks: ITrack[] }
  | { type: "search-complete"; query_id: number }
  | { type: "import-from-valid"; path: string; is_valid: boolean }
  | {
      type: "collection-contains-ids-response";
      id: number;
      contained: string[];
    }
  | {
      /** The track as stored in the collection; null if it isn't in it or that isn't open. */
      type: "collection-track";
      id: number;
      track: ITrack | null;
    }
  | {
      /** Token for `/collection-download/{token}`: single use, valid two minutes. */
      type: "collection-download";
      id: number;
      token: string;
    }
  | { type: "git-remote"; url: string | null }
  | {
      type: "git-sync-status";
      state: GitSyncState;
      message: string;
      retryable: boolean;
    }
  | { type: "spotify-session"; id: number; connected: boolean }
  | { type: "beatport-session"; id: number; browser: string | null }
  | {
      type: "spotify-playlist";
      id: number;
      url: string | null;
      matched: number;
      total: number;
      error_message: string | null;
    }
  | {
      type: "spotify-playlist-progress";
      id: number;
      checked: number;
      matched: number;
      total: number;
    }
  | {
      type: "spotify-playlist-info";
      id: number;
      name: string | null;
    }
  | {
      type: "youtube-playlist-info";
      id: number;
      name: string | null;
      tracks: ITrack[];
      error_message: string | null;
    }
  | { type: "repos"; repos: IRepo[] }
  | { type: "clone-repo-reply"; id: number; path: string | null }
  | { type: "set-git-remote-reply"; id: number; url: string | null }
  | { type: "error-reply"; id: number; message: string }
  | { type: "collection-imported"; id: number; path: string }
  | { type: "collections"; collections: ICollection[] }
  | {
      type: "collection-stats";
      top_artists_five_star: IScoredName[];
      top_artists_four_plus: IScoredName[];
      top_labels_five_star: IScoredName[];
      top_labels_four_plus: IScoredName[];
    }
  | {
      type: "new-releases-candidates";
      id: number;
      artists: INewReleasesCandidate[];
      labels: INewReleasesCandidate[];
    }
  | { type: "fixup-track-reply"; id: number; track?: ITrack }
  | {
      type: "fixup-tracks-progress";
      id: number;
      current: number;
      matched: number;
      total: number;
    }
  | {
      type: "fixup-tracks-complete";
      id: number;
      tracks: ITrack[];
      matched: number;
      total: number;
    }
  | {
      type: "spotify-backfill-progress";
      id: number;
      checked: number;
      matched: number;
      total: number;
    }
  | {
      type: "spotify-backfill-complete";
      id: number;
      matched: number;
      total: number;
      error_message: string | null;
    }
  | {
      type: "youtube-session";
      id: number;
      /** Browser the YouTube login is read from, or null when not connected. */
      browser: string | null;
      /** YouTube account or channel logged in there when connecting, if known. */
      account: string | null;
    }
  | {
      /** Several browser profiles are logged in to YouTube; the user picks one. */
      type: "youtube-logins";
      id: number;
      logins: YouTubeLogin[];
    }
  | {
      type: "youtube-playlist";
      id: number;
      url: string | null;
      error_message: string | null;
    }
  | {
      type: "youtube-playlist-progress";
      id: number;
      current: number;
      total: number;
    }
  | { type: "playlists"; playlists: IPlaylist[] }
  | { type: "save-playlist-reply"; id: number; playlist_id: string }
  | ({ type: "playlist-sync" } & PlaylistSyncStatus);

// ── Dynamic playlists ────────────────────────────────────────────────────────

export type PlaylistService = "spotify" | "youtube";

/** ag-Grid's filter model, keyed by column id. */
export type FilterModel = Record<string, unknown>;

export type SortModelItem = { colId: string; sort: "asc" | "desc" };

/** Where a playlist is exported to on one service. */
export type PlaylistTarget = {
  /** Keep the remote playlist up to date (at most hourly) while Stereo is open. */
  auto_sync: boolean;
  /** The remote playlist, once the first sync created it. Ignored when saving. */
  playlist_id?: string | null;
};

/** This machine's sync state of a playlist on one service. */
export type PlaylistTargetStatus = {
  url: string | null;
  synced_at: number | null;
  /** Why the last sync failed, if it did. */
  error: string | null;
};

/** A saved view: a collection with column filters and sorting. */
export interface IPlaylist {
  id: string;
  name: string;
  /** The collection's full path. */
  collection: string;
  filter_model: FilterModel;
  sort_model: SortModelItem[];
  /** Only the first `limit` tracks, if set. */
  limit: number | null;
  spotify: PlaylistTarget | null;
  youtube: PlaylistTarget | null;
  updated_at: number;
  spotify_status: PlaylistTargetStatus | null;
  youtube_status: PlaylistTargetStatus | null;
}

/** A new playlist, for `save-playlist`. */
export type PlaylistInput = Pick<
  IPlaylist,
  | "name"
  | "collection"
  | "filter_model"
  | "sort_model"
  | "limit"
  | "spotify"
  | "youtube"
>;

/**
 * One change to an existing playlist, for `edit-playlist`. Each changes only its own fields of
 * the server's copy, so that quick edits in a row (before the server's reply) can't undo each
 * other.
 */
export type PlaylistEdit =
  | { kind: "rename"; name: string }
  | { kind: "set-limit"; limit: number | null }
  | ({ kind: "set-view" } & Pick<
      IPlaylist,
      "collection" | "filter_model" | "sort_model"
    >)
  | {
      kind: "set-export";
      service: PlaylistService;
      target: PlaylistTarget | null;
    };

export type PlaylistSyncState =
  "running" | "synced" | "unchanged" | "throttled" | "busy" | "failed";

export type PlaylistSyncStatus = {
  playlist_id: string;
  service: PlaylistService;
  state: PlaylistSyncState;
  current: number | null;
  total: number | null;
  matched: number | null;
  error: string | null;
};

export const playlistSyncKey = (playlistId: string, service: PlaylistService) =>
  `${playlistId}/${service}`;

export type Action =
  | ServerMsg
  | { type: "player-state-change"; id: string; state: PlaybackStatus }
  | { type: "set-player-loading"; loading: boolean }
  | { type: "set-import-from"; path: string }
  | { type: "set-import-keep-user-data"; keep: boolean }
  | { type: "consume-notifications"; ids: number[] }
  | { type: "dismiss-connection-closed-notification" }
  | { type: "collection-is-valid" }
  | { type: "grid-refreshed" }
  | { type: "grid-purged" }
  | { type: "search-box-input"; input: string }
  | { type: "search-results-selection-changed"; selected: ITrack[] }
  | { type: "set-track-rating"; yt_id: string; rating: number | null }
  // Replaces the YouTube video matched to a search result.
  | { type: "set-search-result-video"; result_id: number; yt_id: string }
  | { type: "clear-search-results" }
  | { type: "set-search-failure" }
  // `is_chart` marks a chart search, whose results are shown in chart order.
  | { type: "set-search-busy"; query_id: number; is_chart?: boolean }
  | { type: "set-search-limit"; limit: number }
  | { type: "set-search-kind"; kind: SearchBoxKind }
  | { type: "set-search-sources"; sources: TrackSource[] }
  | { type: "set-new-releases-days"; days: number }
  | { type: "rows" }
  | { type: "toggle-shuffle" }
  | { type: "start-spotify-backfill"; id: number }
  | { type: "start-spotify-playlist-export"; id: number; total: number }
  | { type: "clear-spotify-playlist-open-url" }
  | { type: "start-youtube-playlist-export"; id: number; total: number }
  | { type: "clear-youtube-playlist-open-url" }
  | { type: "cancel-youtube-playlist"; reason: "cancelled" | "timeout" }
  | { type: "start-youtube-fixup"; id: number; name: string; total: number }
  | { type: "clear-youtube-fixup-result" }
  | { type: "cancel-youtube-fixup" }
  | { type: "clear-playlists" }
  | {
      type: "start-playlist-sync";
      playlist_id: string;
      service: PlaylistService;
    };

export type RequestReply = (
  request: ClientMsg,
  cb: (response: ServerMsg | { type: string }) => void,
  timeout?: number,
) => void;

export type HealthCheckResult = {
  name: string;
  probe: string;
  /** "running" only in progress reports. */
  status: "running" | "passed" | "failed";
  message: string;
  duration_ms: number;
};

/** A YouTube video, as found by `search-youtube-videos`. */
export type YouTubeVideo = {
  id: string;
  /** Empty for a linked video YouTube's search didn't turn up. */
  title: string;
  channel: string;
  duration_secs: number | null;
};

export type YouTubeLogin = {
  id: string;
  /** The browser, plus the profile when it has several. */
  browser: string;
  account: string | null;
};

/** The connected YouTube login. */
export type YouTubeConnection = { browser: string; account: string | null };

/** The connection a `youtube-session` reply describes. */
export const youTubeConnection = (r: {
  browser: string | null;
  account?: string | null;
}): YouTubeConnection | null =>
  r.browser === null
    ? null
    : { browser: r.browser, account: r.account ?? null };

export type AppearanceType = "system" | "light" | "dark";

export type AppNotification = {
  id: number;
  message: string;
  kind?: "info" | "warn" | "warning" | "error";
};

/** The air the header's speaker stacks push out with the beats (SpeakerStack.tsx). */
export type AirSettings = {
  /** Whether the stacks push air at all. */
  isOn: boolean;
  /** How long a beat's pressure wave travels, in ms. */
  waveDuration: number;
  /** How far it travels, in cabinet widths. */
  waveTravel: number;
  /** The turbulence's displacement in still air, in logo units (64 to the logo). */
  calmTurbulence: number;
  /** The turbulence's displacement at a full beat, in logo units. */
  beatTurbulence: number;
  /** How quickly the turbulence settles after a beat: its time constant in ms. */
  settleTime: number;
};

/** The air's defaults; SettingsMenu.tsx adjusts them. Off unless turned on. */
export const DEFAULT_AIR: AirSettings = {
  isOn: false,
  waveDuration: 650,
  waveTravel: 1.6,
  calmTurbulence: 3,
  beatTurbulence: 8,
  settleTime: 180,
};

/** How the background artwork looks (App.css's `.background-player`), in % of the
 * look it was designed with; SettingsMenu.tsx adjusts them. */
export type BackgroundSettings = {
  /** Its colour saturation (the animation's range, scaled). */
  saturation: number;
  /** Its opacity between beats; the beats brighten it towards full. */
  opacity: number;
  /** How strongly it's blurred (the animation's range, scaled); 0 is sharp. */
  blur: number;
};

/** Whether the background artwork is shown, unless set otherwise. */
export const DEFAULT_BACKGROUND_SHOWN = true;

/** The background's defaults: the look it was designed with. */
export const DEFAULT_BACKGROUND: BackgroundSettings = {
  saturation: 100,
  opacity: 50,
  blur: 100,
};

/** The disco ball hanging from the top of the window (DiscoBall.tsx). */
export type DiscoBallSettings = {
  /** Whether it's shown at all. */
  isShown: boolean;
  /** How fast it turns, in revolutions per minute; 0 holds it still. */
  speed: number;
  /** Its diameter, in % of the window's shorter side. */
  size: number;
  /** The length of the cord it hangs from, from the window's top, in px. */
  cord: number;
};

/** The disco ball's defaults; SettingsMenu.tsx adjusts them. Still unless set
 * turning: each redraw takes about 1.5ms of main thread, and the compositor
 * blurs the frosted controls over it again. */
export const DEFAULT_DISCO_BALL: DiscoBallSettings = {
  isShown: false,
  speed: 0,
  size: 40,
  cord: 8,
};

export type Settings = {
  appearance: AppearanceType;
  repoPath: string;
  collectionPath: string;
  /**
   * Show the playing track's thumbnail, blurred, behind the app (historical
   * name); unset for the default, `DEFAULT_BACKGROUND_SHOWN`.
   */
  backgroundVideo?: boolean;
  /** How strongly the background pulses with the beats, 0–100 (%); unset in older stored settings. */
  backgroundPulse?: number;
  /** The background's saturation, opacity and blur; unset in older stored settings. */
  background?: Partial<BackgroundSettings>;
  /** The air the header's speaker stacks push with the beats; unset in older stored settings. */
  air?: Partial<AirSettings>;
  /** The disco ball hanging from the top; unset in older stored settings. */
  discoBall?: Partial<DiscoBallSettings>;
  shufflePlay: boolean;
  /** Playback volume, 0–100 (%, perceptual); unset in older stored settings. */
  volume?: number;
  muted?: boolean;
};
