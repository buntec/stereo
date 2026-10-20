export type TrackSource =
  | "beatport"
  | "soundcloud"
  | "volumo"
  | "traxsource"
  | "spotify"
  | "shazam";

export interface ITrack {
  title: string;
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
export type SearchBoxKind = SearchKind | "new-releases";

export type ClientMsg =
  | { type: "set-collection"; path: string }
  | { type: "validate-track"; track: unknown; id?: number }
  | { type: "check-spotify-id"; sp_id: string; id?: number }
  | { type: "create-collection"; name: string }
  | { type: "collection-contains-ids"; yt_ids: string[] }
  | { type: "get-track-info"; yt_id: string }
  | { type: "import-from"; path: string; keep_user_data: boolean }
  | { type: "check-import-from"; path: string }
  | { type: "inc-play-count"; yt_id: string }
  | { type: "create-yt-anon-playlist"; id?: number; yt_ids: string[] }
  | { type: "search-cancel-all" }
  | { type: "update-rating"; yt_id: string; rating: number | null }
  | { type: "update-track"; old: ITrack; new: ITrack }
  | { type: "delete-tracks"; ids: string[] }
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
  | { type: "get-git-remote" }
  | { type: "set-git-remote"; url: string | null }
  | { type: "retry-git-sync" }
  | {
      type: "resolve-git-conflict";
      strategy: "keep-local" | "keep-upstream";
    }
  | { type: "get-spotify-session"; id?: number }
  | { type: "set-spotify-session"; id?: number; cookie: string | null }
  | { type: "detect-spotify-session"; id?: number }
  | {
      type: "create-spotify-playlist";
      id?: number;
      name: string;
      tracks: { artist: string; title: string; sp_id?: string | null }[];
      existing_playlist?: string | null;
    }
  | { type: "get-spotify-playlist-info"; id?: number; url: string }
  | { type: "get-youtube-playlist"; id?: number; url: string }
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
      type: "set-youtube-credentials";
      id?: number;
      client_id: string;
      client_secret: string;
    }
  | { type: "disconnect-youtube-session"; id?: number }
  | {
      type: "exchange-youtube-code";
      id?: number;
      code: string;
      verifier: string;
      redirect_uri: string;
    }
  | {
      type: "create-youtube-playlist";
      id?: number;
      name: string;
      yt_ids: string[];
    }
  | { type: "cancel-youtube-playlist"; id: number };

export type ServerMsg =
  | {
      type: "notification";
      message: string;
      kind: "info" | "warn" | "warning" | "error";
    }
  | { type: "heartbeat"; timestamp: number }
  | { type: "validate-track-reply"; is_valid: boolean; id: number }
  | {
      type: "check-spotify-id-reply";
      id: number;
      sp_id: string;
      exists: boolean;
    }
  | { type: "set-playlist"; ids: string[] }
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
  | { type: "backend-info"; version: string; home_dir: string }
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
    }
  | { type: "search-results"; query_id: number; tracks: ITrack[] }
  | { type: "search-complete"; query_id: number }
  | { type: "import-from-valid"; path: string; is_valid: boolean }
  | {
      type: "collection-contains-ids-response";
      id: number;
      contained: string[];
    }
  | { type: "git-remote"; url: string | null }
  | {
      type: "git-sync-status";
      state: GitSyncState;
      message: string;
      retryable: boolean;
    }
  | { type: "spotify-session"; id: number; connected: boolean }
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
  | { type: "error-reply"; id: number; message: string }
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
      connected: boolean;
      client_id: string | null;
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
    };

export type Action =
  | ServerMsg
  | { type: "player-state-change"; id: string; state: number }
  | { type: "set-player2-visible"; visible: boolean }
  | { type: "set-title"; title?: string }
  | { type: "set-import-from"; path: string }
  | { type: "set-import-keep-user-data"; keep: boolean }
  | { type: "set-notification-show"; show: boolean }
  | { type: "dismiss-connection-closed-notification" }
  | { type: "clear-title" }
  | { type: "collection-is-valid" }
  | { type: "grid-refreshed" }
  | { type: "grid-purged" }
  | { type: "search-box-input"; input: string }
  | { type: "selection-changed"; selected: string[] }
  | { type: "search-results-selection-changed"; selected: ITrack[] }
  | { type: "set-track-rating"; yt_id: string; rating: number | null }
  | { type: "clear-search-results" }
  | { type: "set-search-failure" }
  | { type: "set-search-busy"; query_id: number }
  | { type: "set-search-limit"; limit: number }
  | { type: "set-search-kind"; kind: SearchBoxKind }
  | { type: "set-search-sources"; sources: TrackSource[] }
  | { type: "set-new-releases-days"; days: number }
  | {
      type: "set-playback-progress";
      percent: number;
      duration: number;
      current_time: number;
    }
  | { type: "rows" }
  | { type: "toggle-shuffle" }
  | { type: "toggle-show-player2" }
  | { type: "start-spotify-backfill"; id: number }
  | { type: "start-spotify-playlist-export"; id: number; total: number }
  | { type: "clear-spotify-playlist-open-url" }
  | { type: "start-youtube-playlist-export"; id: number; total: number }
  | { type: "clear-youtube-playlist-open-url" }
  | { type: "cancel-youtube-playlist"; reason: "cancelled" | "timeout" }
  | { type: "start-youtube-fixup"; id: number; name: string; total: number }
  | { type: "clear-youtube-fixup-result" }
  | { type: "cancel-youtube-fixup" };

export type RequestReply = (
  request: ClientMsg,
  cb: (response: ServerMsg | { type: string }) => void,
  timeout?: number,
) => void;

export type AppearanceType = "system" | "light" | "dark";

export type Settings = {
  appearance: AppearanceType;
  repoPath: string;
  collectionPath: string;
  video: boolean;
  backgroundVideo: boolean;
  shufflePlay: boolean;
  fullScreen: boolean;
};
