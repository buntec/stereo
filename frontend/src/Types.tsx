export interface ITrack {
  title: string;
  artists: string[];
  yt_id: string;
  bp_id?: number;
  mb_id?: string;
  release_date?: string;
  label?: string;
  bpm?: number;
  key?: string;
  rating?: number;
  play_count?: number;
  last_played?: string;
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

export type SearchKind = "fuzzy" | "by-label" | "by-artist";

export type ClientMsg =
  | { type: "set-collection"; path: string }
  | { type: "create-collection"; path: string }
  | { type: "collection-contains-id"; yt_id: string }
  | { type: "get-track-info"; yt_id: string }
  | { type: "inc-play-count"; yt_id: string }
  | { type: "search-cancel-all" }
  | { type: "update-rating"; yt_id: string; rating: number | null }
  | { type: "delete-tracks"; ids: string[] }
  | {
      type: "get-rows";
      startRow: number;
      endRow: number;
      sortModel: any;
      filterModel: any;
    }
  | { type: "add-tracks"; tracks: ITrack[]; overwrite_existing: boolean }
  | {
      type: "search";
      query: string;
      query_id: number;
      kind: SearchKind;
      limit: number;
    };

export type ServerMsg =
  | { type: "pong" }
  | {
      type: "notification";
      message: string;
      kind: "info" | "warn" | "warning" | "error";
    }
  | { type: "heartbeat"; timestamp: number }
  | { type: "play-id"; ids: string }
  | { type: "play-ids"; ids: string[] }
  | { type: "tracks"; tracks: ITrack[] }
  | { type: "track-update"; track: ITrack }
  | { type: "reload-tracks" }
  | { type: "rows"; id: number; rows: ITrack[]; last_row?: number }
  | { type: "track-found"; id: number; track: ITrack; exists_in_db: boolean }
  | { type: "track-not-found"; id: number }
  | { type: "backend-info"; version: string }
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
  | { type: "search-result"; query_id: number; track: ITrack }
  | { type: "search-results"; query_id: number; tracks: ITrack[] }
  | { type: "search-complete"; query_id: number }
  | {
      type: "collection-contains-id-response";
      id: number;
      contains_id: boolean;
    };

export type Action =
  | ServerMsg
  | { type: "player-state-change"; id: string; state: number }
  | { type: "set-title"; title?: string }
  | { type: "set-notification-show"; show: boolean }
  | { type: "clear-title" }
  | { type: "collection-is-valid" }
  | { type: "collection-path-completions"; paths?: string[] }
  | { type: "grid-refreshed" }
  | { type: "grid-purged" }
  | { type: "search-box-input"; input: string }
  | { type: "selection-changed"; selected: string[] }
  | { type: "search-results-selection-changed"; selected: ITrack[] }
  | { type: "clear-search-results" }
  | { type: "set-search-failure" }
  | { type: "set-search-busy" }
  | { type: "set-search-limit"; limit: number }
  | { type: "set-search-kind"; kind: SearchKind }
  | {
      type: "set-playback-progress";
      percent: number;
      duration: number;
      current_time: number;
    }
  | { type: "rows" }
  | { type: "toggle-shuffle" }
  | { type: "toggle-show-player" };

export type RequestReply = (
  request: ClientMsg,
  cb: (response: ServerMsg | { type: string }) => void,
  timeout?: number,
) => void;

export type AppearanceType = "light" | "dark";

export type Settings = {
  appearance: AppearanceType;
  collectionPath: string;
  useDefaultCollection: boolean;
};
