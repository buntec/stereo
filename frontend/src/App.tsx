import "./App.css";

import {
  useState,
  useReducer,
  useEffect,
  useRef,
  useCallback,
  useMemo,
} from "react";

import { AgGridReact } from "ag-grid-react";
import { type GridState, type StateUpdatedEvent } from "ag-grid-community";
import logger from "./logger.tsx";

import {
  PlusIcon,
  Cross2Icon,
  VideoIcon,
  MagicWandIcon,
  ResetIcon,
  EnterFullScreenIcon,
  ExitFullScreenIcon,
  TrashIcon,
  ShuffleIcon,
  PlayIcon,
  TrackNextIcon,
  PauseIcon,
  TrackPreviousIcon,
} from "@radix-ui/react-icons";

import { Theme } from "@astryxdesign/core/theme";
import { stereoTheme } from "./theme";
import { Slider } from "@astryxdesign/core/Slider";
import { Tooltip } from "@astryxdesign/core/Tooltip";
import { Kbd } from "@astryxdesign/core/Kbd";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack, StackItem } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { Divider } from "@astryxdesign/core/Divider";
import { AlertDialog } from "@astryxdesign/core/AlertDialog";

import type {
  ITrack,
  ICollection,
  ICollectionStats,
  IRepo,
  SearchBoxKind,
  TrackSource,
  Action,
  ClientMsg,
  ServerMsg,
  Settings,
} from "./Types.tsx";
import { isQuerylessSearchKind } from "./Types.tsx";
import { DEFAULT_BEATPORT_GENRE_ID } from "./BeatportGenres.tsx";

import { useWebSocket } from "./WebSocket.tsx";
import { useYTPlayer, type YTPlayerOptions } from "./YT.tsx";
import {
  formatDuration,
  useLocalStorage,
  shuffleArray,
  hasCollectionStats,
} from "./Utils.tsx";
import { TracksGrid, SearchResultsGrid, ColumnsMenu } from "./Grid.tsx";
import SearchBox from "./SearchBox.tsx";
import Rating from "./Rating.tsx";
import CollectionSelector, { collectionLabel } from "./CollectionSelector.tsx";
import ImportMenu from "./ImportMenu.tsx";
import ExportDropDown from "./ExportDropDown.tsx";
import {
  YouTubePlaylistDialog,
  YouTubePlaylistProgressDialog,
} from "./YouTubePlaylistDialog.tsx";
import { YouTubeFixupProgressDialog } from "./YouTubePlaylistImportDialog.tsx";
import YouTubePlaylistStagingDialog from "./YouTubePlaylistStagingDialog.tsx";
import { useYouTubeOAuthCallback } from "./YouTubeSessionDialog.tsx";
import SpotifyPlaylistDialog, {
  SpotifyPlaylistProgressDialog,
  type SpotifyPlaylistTarget,
} from "./SpotifyPlaylistDialog.tsx";
import AddTracksDialog from "./AddTracksDialog.tsx";
import AppMenu from "./AppMenu.tsx";
import Notification from "./Notification.tsx";
import Title from "./Title.tsx";
import { useKeyboardActions } from "./Keyboard.tsx";
import NowPlayingMenu from "./NowPlayingMenu.tsx";
import CollectionSummary from "./CollectionSummary.tsx";
import NewReleasesDialog, {
  type NewReleasesOverrides,
} from "./NewReleasesDialog.tsx";
import GitSyncStatus from "./GitSyncStatus.tsx";

const wsUrl = `${window.location.protocol === "https:" ? "wss" : "ws"}://${window.location.host}/ws`;

const wsTokenFromFragment = new URLSearchParams(
  window.location.hash.slice(1),
).get("stereo-token");
if (wsTokenFromFragment) {
  window.sessionStorage.setItem("stereo-ws-token", wsTokenFromFragment);
  window.history.replaceState(
    null,
    "",
    `${window.location.pathname}${window.location.search}`,
  );
}
const wsToken =
  wsTokenFromFragment ?? window.sessionStorage.getItem("stereo-ws-token");
const wsProtocols = wsToken
  ? [
      `stereo.auth.${Array.from(new TextEncoder().encode(wsToken), (byte) =>
        byte.toString(16).padStart(2, "0"),
      ).join("")}`,
    ]
  : undefined;

const YTPlayerState = {
  UNSTARTED: -1,
  ENDED: 0,
  PLAYING: 1,
  PAUSED: 2,
  BUFFERING: 3,
  CUED: 5,
};

const createSilentAudioSource = (): string => {
  const sampleRate = 8000;
  const durationSeconds = 10;
  const bytesPerSample = 2;
  const dataSize = sampleRate * durationSeconds * bytesPerSample;
  const buffer = new ArrayBuffer(44 + dataSize);
  const view = new DataView(buffer);

  const writeAscii = (offset: number, value: string) => {
    for (let i = 0; i < value.length; i++) {
      view.setUint8(offset + i, value.charCodeAt(i));
    }
  };

  writeAscii(0, "RIFF");
  view.setUint32(4, 36 + dataSize, true);
  writeAscii(8, "WAVE");
  writeAscii(12, "fmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * bytesPerSample, true);
  view.setUint16(32, bytesPerSample, true);
  view.setUint16(34, bytesPerSample * 8, true);
  writeAscii(36, "data");
  view.setUint32(40, dataSize, true);

  return URL.createObjectURL(
    new Blob([buffer], {
      type: "audio/wav",
    }),
  );
};

const getVideoData = (player: YT.Player): YT.VideoData | undefined => {
  try {
    return player.getVideoData();
  } catch {
    return undefined;
  }
};

const defaultSettings: Settings = {
  appearance: "system",
  repoPath: "",
  collectionPath: "",
  video: false,
  backgroundVideo: true,
  shufflePlay: false,
  fullScreen: false,
};

type State = {
  backend_version?: string;
  home_dir?: string;
  repos?: IRepo[];
  collections?: ICollection[];

  collection?: ICollection;
  collection_is_valid?: boolean;
  collection_last_update?: number;

  current_id?: string;
  player_id?: string;
  duration?: number;
  current_time?: number;
  title?: string;
  playlist: string[];
  player_state: number;
  playback_progress_pct?: number;
  show_player2: boolean;

  should_refresh_grid: boolean;
  should_purge_grid: boolean;

  search_busy: boolean;
  search_query_id?: number;
  search_results?: ITrack[];
  search_results_selection?: ITrack[];
  search_error_message?: string;
  search_box_input?: string;
  search_limit: number;
  search_kind: SearchBoxKind;
  search_sources: TrackSource[];
  new_releases_days: number;

  track_selection: string[];
  track_info?: ITrack;

  notification_msg?: string;
  notification_show: boolean;
  notification_kind?: "info" | "warn" | "warning" | "error";

  import_from?: string;
  import_from_keep_user_data?: boolean;
  import_from_is_valid?: boolean;

  git_remote?: string | null;
  git_sync_state?: import("./Types.tsx").GitSyncState;
  git_sync_message?: string;
  git_sync_retryable?: boolean;
  collection_stats?: ICollectionStats;

  spotify_backfill_id?: number;
  spotify_backfill_running: boolean;
  spotify_backfill_checked?: number;
  spotify_backfill_matched?: number;
  spotify_backfill_total?: number;

  spotify_playlist_id?: number;
  spotify_playlist_running: boolean;
  spotify_playlist_checked?: number;
  spotify_playlist_matched?: number;
  spotify_playlist_total?: number;
  spotify_playlist_open_url?: string | null;

  youtube_connected: boolean;
  youtube_client_id?: string | null;

  youtube_playlist_id?: number;
  youtube_playlist_running: boolean;
  youtube_playlist_current?: number;
  youtube_playlist_total?: number;
  youtube_playlist_open_url?: string | null;

  youtube_fixup_id?: number;
  youtube_fixup_name?: string;
  youtube_fixup_running: boolean;
  youtube_fixup_current?: number;
  youtube_fixup_matched?: number;
  youtube_fixup_total?: number;
  youtube_fixup_tracks?: ITrack[];
};

const reducer = (state: State, action: Action): State => {
  switch (action.type) {
    case "set-playlist": {
      if (!state.current_id && action.ids.length > 0) {
        return { ...state, playlist: action.ids, current_id: action.ids[0] };
      }
      return { ...state, playlist: action.ids };
    }

    case "play-id":
      return { ...state, current_id: action.id };

    case "play-next-track": {
      if (state.playlist.length > 0) {
        const id = state.current_id ?? state.playlist[0];
        const i = state.playlist.indexOf(id);
        console.log("play-next-track", id, i);
        if (i >= 0) {
          return {
            ...state,
            current_id: state.playlist[(i + 1) % state.playlist.length],
          };
        }
        return { ...state, current_id: state.playlist[0] };
      }
      return { ...state };
    }

    case "play-prev-track": {
      if (state.playlist.length > 0) {
        const id = state.current_id ?? state.playlist[0];
        const i = state.playlist.indexOf(id);
        console.log("play-prev-track", id, i);
        if (i >= 0) {
          return {
            ...state,
            current_id:
              state.playlist[
                (i + state.playlist.length - 1) % state.playlist.length
              ],
          };
        }
        return { ...state, current_id: state.playlist[0] };
      }
      return { ...state };
    }

    case "player-state-change":
      return { ...state, player_state: action.state, player_id: action.id };

    case "set-player2-visible":
      return { ...state, show_player2: action.visible };

    case "backend-info":
      return {
        ...state,
        backend_version: action.version,
        home_dir: action.home_dir,
      };

    case "set-title":
      return { ...state, title: action.title };

    case "set-import-from":
      return { ...state, import_from: action.path };

    case "set-import-keep-user-data":
      return { ...state, import_from_keep_user_data: action.keep };

    case "clear-title":
      return { ...state, title: undefined };

    case "collection-is-valid":
      return { ...state, collection_is_valid: true };

    case "collection-info":
      if (action.error_message) {
        return {
          ...state,
          collection: undefined,
          collection_is_valid: false,
          should_purge_grid: true,
          collection_last_update: Date.now(),
        };
      }
      return {
        ...state,
        collection: action.collection,
        collection_is_valid: true,
        should_purge_grid: true,
        collection_last_update: Date.now(),
      };

    case "track-update":
      if (action.track.yt_id === state.current_id) {
        return {
          ...state,
          should_refresh_grid: true,
          track_info: action.track,
          collection_last_update: Date.now(),
        };
      }
      return {
        ...state,
        should_refresh_grid: true,
        collection_last_update: Date.now(),
      };

    case "reload-tracks":
      return { ...state, should_refresh_grid: true, track_info: undefined };

    case "import-from-valid":
      if (action.path === state.import_from) {
        return { ...state, import_from_is_valid: action.is_valid };
      }
      return { ...state };

    case "grid-refreshed":
      return { ...state, should_refresh_grid: false };

    case "grid-purged":
      return { ...state, should_purge_grid: false };

    case "search-box-input":
      return { ...state, search_box_input: action.input, search_busy: false };

    case "selection-changed":
      return { ...state, track_selection: action.selected };

    case "search-results-selection-changed":
      return { ...state, search_results_selection: action.selected };

    case "set-track-rating": {
      const updateTrack = (track: ITrack): ITrack =>
        track.yt_id === action.yt_id
          ? { ...track, rating: action.rating ?? undefined }
          : track;

      return {
        ...state,
        track_info: state.track_info
          ? updateTrack(state.track_info)
          : undefined,
        search_results: state.search_results?.map(updateTrack),
        search_results_selection:
          state.search_results_selection?.map(updateTrack),
      };
    }

    case "clear-search-results":
      return {
        ...state,
        search_results: undefined,
        search_results_selection: undefined,
        search_busy: false,
        search_query_id: undefined,
      };

    // Results and completions are only accepted for the search that is currently active
    // (set by "set-search-busy"); anything else is a straggler from a cancelled search.
    case "search-result": {
      if (action.query_id !== state.search_query_id) return state;
      const track = { ...action.track, source: action.source };
      return {
        ...state,
        search_results: [...(state.search_results ?? []), track],
      };
    }

    case "search-results":
      if (action.query_id !== state.search_query_id) return state;
      return {
        ...state,
        search_results: [...(state.search_results ?? []), ...action.tracks],
      };

    case "set-search-failure":
      return {
        ...state,
        search_error_message: "no tracks founds",
        search_results: undefined,
        search_busy: false,
      };

    case "search-complete":
      if (action.query_id !== state.search_query_id) return state;
      return {
        ...state,
        search_busy: false,
        search_results: state.search_results ?? [],
      };

    case "set-search-busy":
      return { ...state, search_busy: true, search_query_id: action.query_id };

    case "set-search-limit":
      return { ...state, search_limit: action.limit };

    case "set-search-kind":
      return { ...state, search_kind: action.kind };

    case "set-search-sources":
      return { ...state, search_sources: action.sources };

    case "set-new-releases-days":
      return { ...state, new_releases_days: action.days };

    case "set-playback-progress":
      return {
        ...state,
        playback_progress_pct: action.percent,
        duration: action.duration,
        current_time: action.current_time,
      };

    case "toggle-show-player2":
      return { ...state, show_player2: !state.show_player2 };

    case "track-info":
      return { ...state, track_info: action.track };

    case "set-notification-show":
      return { ...state, notification_show: action.show };

    case "dismiss-connection-closed-notification":
      if (
        state.notification_show &&
        state.notification_msg?.startsWith("WebSocket connection closed")
      ) {
        return { ...state, notification_show: false };
      }
      return state;

    case "notification":
      return {
        ...state,
        notification_kind: action.kind,
        notification_msg: action.message,
        notification_show: true,
      };

    case "git-remote":
      return { ...state, git_remote: action.url };

    case "git-sync-status":
      return {
        ...state,
        git_sync_state: action.state,
        git_sync_message: action.message,
        git_sync_retryable: action.retryable,
      };

    case "repos":
      return { ...state, repos: action.repos };

    case "collections":
      return { ...state, collections: action.collections };

    case "collection-stats":
      return {
        ...state,
        collection_stats: {
          top_artists_five_star: action.top_artists_five_star,
          top_artists_four_plus: action.top_artists_four_plus,
          top_labels_five_star: action.top_labels_five_star,
          top_labels_four_plus: action.top_labels_four_plus,
        },
      };

    case "default-collection":
    case "rows":
    case "yt-anon-playlist":
    case "spotify-session":
    case "tracks":
    case "row-index":
    case "heartbeat":
    case "collection-contains-ids-response":
    case "path-completions":
    case "validate-track-reply":
    case "fixup-track-reply":
    case "check-spotify-id-reply":
    case "spotify-playlist-info":
    case "youtube-playlist-info":
    case "clone-repo-reply":
    case "new-releases-candidates":
    case "error-reply": // the server also sends a notification with the reason
      return { ...state };

    case "start-spotify-backfill":
      return {
        ...state,
        spotify_backfill_id: action.id,
        spotify_backfill_running: true,
        spotify_backfill_checked: 0,
        spotify_backfill_matched: 0,
        spotify_backfill_total: 0,
      };

    case "spotify-backfill-progress":
      if (action.id !== state.spotify_backfill_id) return { ...state };
      return {
        ...state,
        spotify_backfill_checked: action.checked,
        spotify_backfill_matched: action.matched,
        spotify_backfill_total: action.total,
      };

    case "spotify-backfill-complete":
      if (action.id !== state.spotify_backfill_id) return { ...state };
      return {
        ...state,
        spotify_backfill_running: false,
        notification_kind: action.error_message ? "error" : "info",
        notification_msg:
          action.error_message ??
          (action.total === 0
            ? "No tracks needed a Spotify ID"
            : `Spotify backfill complete: ${action.matched}/${action.total} tracks matched`),
        notification_show: true,
      };

    case "start-spotify-playlist-export":
      return {
        ...state,
        spotify_playlist_id: action.id,
        spotify_playlist_running: true,
        spotify_playlist_checked: 0,
        spotify_playlist_matched: 0,
        spotify_playlist_total: action.total,
      };

    case "spotify-playlist-progress":
      if (action.id !== state.spotify_playlist_id) return { ...state };
      return {
        ...state,
        spotify_playlist_checked: action.checked,
        spotify_playlist_matched: action.matched,
        spotify_playlist_total: action.total,
      };

    case "spotify-playlist":
      if (action.id !== state.spotify_playlist_id) return { ...state };
      return {
        ...state,
        spotify_playlist_running: false,
        spotify_playlist_open_url: action.url,
        notification_kind: action.url ? "info" : "error",
        notification_msg: action.url
          ? `Created Spotify playlist with ${action.matched}/${action.total} matched tracks`
          : (action.error_message ?? "Failed to create Spotify playlist"),
        notification_show: true,
      };

    case "clear-spotify-playlist-open-url":
      return { ...state, spotify_playlist_open_url: undefined };

    case "youtube-session":
      return {
        ...state,
        youtube_connected: action.connected,
        youtube_client_id: action.client_id,
      };

    case "start-youtube-playlist-export":
      return {
        ...state,
        youtube_playlist_id: action.id,
        youtube_playlist_running: true,
        youtube_playlist_current: 0,
        youtube_playlist_total: action.total,
      };

    case "youtube-playlist-progress":
      if (action.id !== state.youtube_playlist_id) return { ...state };
      return {
        ...state,
        youtube_playlist_current: action.current,
        youtube_playlist_total: action.total,
      };

    case "fixup-tracks-progress":
      if (action.id !== state.youtube_fixup_id) return { ...state };
      return {
        ...state,
        youtube_fixup_current: action.current,
        youtube_fixup_matched: action.matched,
        youtube_fixup_total: action.total,
      };

    case "fixup-tracks-complete":
      if (action.id !== state.youtube_fixup_id) return { ...state };
      return {
        ...state,
        youtube_fixup_running: false,
        youtube_fixup_current: action.total,
        youtube_fixup_matched: action.matched,
        youtube_fixup_total: action.total,
        youtube_fixup_tracks:
          action.tracks.length > 0 ? action.tracks : undefined,
        notification_kind:
          action.tracks.length > 0 ? state.notification_kind : "error",
        notification_msg:
          action.tracks.length > 0
            ? state.notification_msg
            : "Playlist metadata lookup returned no tracks",
        notification_show:
          action.tracks.length > 0 ? state.notification_show : true,
      };

    case "start-youtube-fixup":
      return {
        ...state,
        youtube_fixup_id: action.id,
        youtube_fixup_name: action.name,
        youtube_fixup_running: true,
        youtube_fixup_current: 0,
        youtube_fixup_matched: 0,
        youtube_fixup_total: action.total,
        youtube_fixup_tracks: undefined,
      };

    case "clear-youtube-fixup-result":
      return { ...state, youtube_fixup_tracks: undefined };

    case "cancel-youtube-fixup":
      return {
        ...state,
        youtube_fixup_id: undefined,
        youtube_fixup_running: false,
        youtube_fixup_tracks: undefined,
      };

    case "youtube-playlist":
      if (action.id !== state.youtube_playlist_id) return { ...state };
      return {
        ...state,
        youtube_playlist_running: false,
        youtube_playlist_open_url: action.url,
        notification_kind: action.url ? "info" : "error",
        notification_msg: action.url
          ? "Created YouTube playlist"
          : (action.error_message ?? "Failed to create YouTube playlist"),
        notification_show: true,
      };

    case "clear-youtube-playlist-open-url":
      return { ...state, youtube_playlist_open_url: undefined };

    case "cancel-youtube-playlist":
      if (!state.youtube_playlist_running) return { ...state };
      return {
        ...state,
        youtube_playlist_running: false,
        youtube_playlist_id: undefined,
        notification_kind: action.reason === "timeout" ? "warning" : "info",
        notification_msg:
          action.reason === "timeout"
            ? "YouTube playlist export stalled (possibly hit the API quota) — cancelled"
            : "Cancelled YouTube playlist export",
        notification_show: true,
      };

    default:
      // Don't take down the whole UI over a message type this build doesn't know.
      logger.warn(`missing handler for action: ${JSON.stringify(action)}`);
      return state;
  }
};

function App() {
  const [state, dispatch] = useReducer(reducer, {
    playlist: [],
    track_selection: [],
    show_player2: false,
    player_state: YTPlayerState.UNSTARTED,
    should_refresh_grid: false,
    should_purge_grid: false,
    search_busy: false,
    search_limit: 100,
    search_kind: "fuzzy",
    search_sources: ["beatport", "volumo", "traxsource"],
    new_releases_days: 7,
    notification_show: false,
    spotify_backfill_running: false,
    spotify_playlist_running: false,
    youtube_connected: false,
    youtube_playlist_running: false,
    youtube_fixup_running: false,
  });

  const [settings, setSettings] = useLocalStorage<Settings>(
    "stereo_app_settings",
    defaultSettings,
  );

  const [filteredRowCount, setFilteredRowCount] = useState<number | undefined>(
    undefined,
  );

  const [statsOpen, setStatsOpen] = useState(false);

  const [newReleasesOpen, setNewReleasesOpen] = useState(false);
  // Keyed by collection path.
  const [newReleasesOverrides, setNewReleasesOverrides] = useLocalStorage<
    Record<string, NewReleasesOverrides>
  >("stereo_new_releases_overrides", {});

  const [beatportGenreId, setBeatportGenreId] = useLocalStorage<number>(
    "stereo_beatport_top100_genre",
    DEFAULT_BEATPORT_GENRE_ID,
  );

  const [gridState, setGridState] = useLocalStorage<GridState | undefined>(
    "stereo_app_grid_state",
    undefined,
  );

  const wsOnMessage = useCallback(
    (msg: ServerMsg | ServerMsg[]) => {
      if (Array.isArray(msg)) {
        msg.forEach(dispatch);
      } else {
        dispatch(msg);
      }
    },
    [dispatch],
  );

  const wsOnError = useCallback(
    () =>
      dispatch({
        type: "notification",
        message: "WebSocket error encountered.",
        kind: "error",
      }),
    [dispatch],
  );

  // The repo selected on the current connection. The server forgets it when the socket drops,
  // so it is reset on close and re-sent once the new connection lists its repos.
  const activeRepo = useRef<string | null>(null);

  const wsOnClose = useCallback(
    (ev: CloseEvent) => {
      activeRepo.current = null;
      dispatch({ type: "repos", repos: [] });
      dispatch({
        type: "notification",
        message: ev.reason
          ? `WebSocket connection closed: ${ev.reason} (code ${ev.code})`
          : `WebSocket connection closed (code ${ev.code})`,
        kind: "warn",
      });
    },
    [dispatch],
  );

  const wsOnOpen = useCallback(() => {
    dispatch({ type: "dismiss-connection-closed-notification" });
  }, [dispatch]);

  const { sendMsg, requestReply } = useWebSocket<ServerMsg, ClientMsg>(
    wsUrl,
    wsProtocols,
    wsOnMessage,
    wsOnError,
    wsOnClose,
    wsOnOpen,
  );

  const playId = useCallback(
    (id: string) => dispatch({ type: "play-id", id }),
    [],
  );

  const gridRef = useRef<AgGridReact<ITrack>>(null);
  const [gridReady, setGridReady] = useState(false);

  const updatePlaylist = useCallback(
    (shuffle: boolean) => {
      if (gridReady && gridRef.current) {
        const api = gridRef.current.api;
        const filterModel = api.getState().filter?.filterModel;
        const sortModel = api.getState().sort?.sortModel;
        requestReply(
          {
            type: "get-rows",
            startRow: 0,
            endRow: 10000,
            filterModel,
            sortModel,
          },
          function (msg: ServerMsg | { type: string }) {
            if ("rows" in msg) {
              let ids = msg.rows.map((t: ITrack) => t.yt_id);
              if (shuffle) {
                ids = shuffleArray(ids);
              }
              dispatch({ type: "set-playlist", ids });
            } else {
              logger.warn("failed to get rows");
            }
          },
        );
      }
    },
    [requestReply, gridReady],
  );

  useEffect(() => {
    if (state.playlist.length === 0) {
      updatePlaylist(settings.shufflePlay);
    }
  }, [state.playlist, updatePlaylist, settings.shufflePlay, gridReady]);

  // Used by the main tracks grid: sets current track and ensures the playlist
  // reflects the current filter+sort order before the next track auto-advances.
  const playIdForGrid = useCallback(
    (id: string) => {
      dispatch({ type: "play-id", id });
      updatePlaylist(settings.shufflePlay);
    },
    [dispatch, updatePlaylist, settings.shufflePlay],
  );

  const searchGridRef = useRef<AgGridReact<ITrack>>(null);

  const queryId = useRef<number>(0);

  const backfillId = useRef<number>(0);
  const backfillSpotifyIds = useCallback(() => {
    const id = ++backfillId.current;
    dispatch({ type: "start-spotify-backfill", id });
    sendMsg({ type: "backfill-spotify-ids", id });
  }, [sendMsg]);

  useEffect(() => {
    if (state.should_refresh_grid) {
      if (gridRef.current) {
        gridRef.current.api.refreshInfiniteCache();
        gridRef.current.api.refreshCells({ force: true, suppressFlash: true });
      }
      dispatch({ type: "grid-refreshed" });
    }
  }, [state.should_refresh_grid]);

  useEffect(() => {
    if (state.should_purge_grid) {
      if (gridRef.current) {
        gridRef.current.api.purgeInfiniteCache();
      }
      dispatch({ type: "grid-purged" });
    }
  }, [state.should_purge_grid]);

  const playerOptions: YTPlayerOptions = useMemo(() => {
    return {
      playerVars: {
        autoplay: 1,
        modestbranding: 1,
        color: "white",
        controls: 0,
        disablekb: 1,
        rel: 0,
        playsinline: 1,
        enablejsapi: 1,
      },
      onError: (event) => {
        dispatch({
          type: "notification",
          message: `YouTube playback error: ${event.data}`,
          kind: "error",
        });
      },
      onStateChange: (event: YT.OnStateChangeEvent) => {
        const videoData = getVideoData(event.target);
        if (!videoData?.video_id) return;

        if (event.data === window.YT.PlayerState.PLAYING) {
          dispatch({
            type: "player-state-change",
            state: YTPlayerState.PLAYING,
            id: videoData.video_id,
          });
        }
        if (event.data === window.YT.PlayerState.PAUSED) {
          dispatch({
            type: "player-state-change",
            state: YTPlayerState.PAUSED,
            id: videoData.video_id,
          });
        }
        if (event.data === window.YT.PlayerState.ENDED) {
          dispatch({
            type: "player-state-change",
            state: YTPlayerState.ENDED,
            id: videoData.video_id,
          });
          dispatch({ type: "play-next-track" });
        }
      },
    };
  }, []);

  const {
    containerRef: playerRef,
    player,
    isReady: playerIsReady,
  } = useYTPlayer(playerOptions);

  const player2Options: YTPlayerOptions = useMemo(() => {
    return {
      playerVars: {
        autoplay: 1,
        color: "white",
        disablekb: 1,
        controls: 0,
        mute: 1,
        modestbranding: 1,
        rel: 0,
      },
      onStateChange: (event: YT.OnStateChangeEvent) => {
        if (
          event.data === window.YT.PlayerState.PLAYING ||
          event.data === window.YT.PlayerState.CUED
        ) {
          dispatch({ type: "set-player2-visible", visible: true });
        }
      },
    };
  }, []);

  const {
    containerRef: player2Ref,
    player: player2,
    isReady: player2IsReady,
  } = useYTPlayer(player2Options);

  const mediaSessionAudioRef = useRef<HTMLAudioElement | null>(null);

  const setMediaSessionAudioPlaying = useCallback((playing: boolean) => {
    const audio = mediaSessionAudioRef.current;
    if (!audio) return;

    if (!playing) {
      audio.pause();
      audio.muted = true;
      return;
    }

    // Start muted so autoplay policy cannot block the media-session anchor.
    // The WAV source contains only silence, so unmuting it produces no sound.
    audio.muted = true;
    void audio.play().then(
      () => {
        audio.muted = false;
      },
      () => {
        // The YouTube player remains usable if the browser rejects the anchor.
      },
    );
  }, []);

  useEffect(() => {
    if (!("mediaSession" in navigator)) return;

    const audio = document.createElement("audio");
    const source = createSilentAudioSource();
    audio.src = source;
    audio.loop = true;
    audio.preload = "auto";
    document.body.append(audio);
    mediaSessionAudioRef.current = audio;

    return () => {
      audio.pause();
      audio.removeAttribute("src");
      audio.load();
      audio.remove();
      URL.revokeObjectURL(source);
      mediaSessionAudioRef.current = null;
    };
  }, []);

  useEffect(() => {
    logger.debug("state:", state);
  }, [state]);

  // Auto-select repo when list first arrives (or changes)
  useEffect(() => {
    if (!state.repos || state.repos.length === 0) return;
    const paths = state.repos.map((r) => r.path);
    const target = paths.includes(settings.repoPath)
      ? settings.repoPath
      : state.repos[0].path;
    if (target !== settings.repoPath) {
      setSettings({ ...settings, repoPath: target });
    }
    // The repo list is re-sent after clones, remote changes etc.; re-selecting the same repo
    // would close the open collection and run another sync for nothing.
    if (activeRepo.current !== target) {
      activeRepo.current = target;
      sendMsg({ type: "set-repo", path: target });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.repos]);

  // Auto-select collection when list changes after a repo switch
  useEffect(() => {
    if (!state.collections || state.collections.length === 0) return;
    const paths = state.collections.map((c) => c.path);
    const target = paths.includes(settings.collectionPath)
      ? settings.collectionPath
      : state.collections[0].path;
    if (target !== settings.collectionPath) {
      setSettings({ ...settings, collectionPath: target });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.collections]);

  // Keep settings.collectionPath in sync when backend switches collection (e.g. after CreateCollection)
  useEffect(() => {
    if (
      state.collection &&
      state.collection_is_valid &&
      state.collection.path !== settings.collectionPath
    ) {
      setSettings({ ...settings, collectionPath: state.collection.path });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.collection, state.collection_is_valid]);

  // Only send set-collection after a repo has been selected (state.collections defined)
  useEffect(() => {
    if (settings.collectionPath && state.collections !== undefined) {
      requestReply(
        { type: "set-collection", path: settings.collectionPath },
        (msg) => {
          if ("collection" in msg) {
            dispatch({ type: "collection-is-valid" });
          }
        },
      );
    }
  }, [settings.collectionPath, state.collections, requestReply]);

  useEffect(() => {
    if (state.current_id && state.current_id !== state.track_info?.yt_id) {
      const searchTrack = state.search_results?.find(
        (t) => t.yt_id === state.current_id,
      );
      if (searchTrack) {
        dispatch({ type: "track-info", track: searchTrack });
      } else {
        sendMsg({ type: "get-track-info", yt_id: state.current_id });
      }
    }
  }, [state.current_id, state.track_info, state.search_results, sendMsg]);

  useEffect(() => {
    let t = null;
    const yt_id = state.player_id;

    if (yt_id && state.player_state === YTPlayerState.PLAYING) {
      if (player && playerIsReady) {
        const title = getVideoData(player)?.title;
        if (title) {
          dispatch({ type: "set-title", title });
        }

        // 1 minute of continuous playback triggers play count increase
        t = setTimeout(() => sendMsg({ type: "inc-play-count", yt_id }), 60000);
      }
    }

    if (state.player_state === YTPlayerState.ENDED) {
      dispatch({ type: "clear-title" });
    }

    return () => {
      if (t !== null) {
        clearTimeout(t);
      }
    };
  }, [sendMsg, state.player_state, state.player_id, player, playerIsReady]);

  useEffect(() => {
    if (!("mediaSession" in navigator)) return;

    const currentTrack =
      state.track_info?.yt_id === state.current_id
        ? state.track_info
        : undefined;
    const videoTitle =
      player && playerIsReady ? getVideoData(player)?.title : undefined;

    navigator.mediaSession.metadata = new MediaMetadata({
      title: state.title ?? currentTrack?.title ?? videoTitle ?? "Stereo",
      artist: currentTrack?.artists.join(", ") ?? "Stereo",
      album: "Stereo",
      artwork: state.current_id
        ? [
            {
              src: `https://i.ytimg.com/vi/${state.current_id}/hqdefault.jpg`,
              sizes: "480x360",
              type: "image/jpeg",
            },
          ]
        : [],
    });
  }, [player, playerIsReady, state.current_id, state.title, state.track_info]);

  useEffect(() => {
    if (!("mediaSession" in navigator)) return;

    if (state.player_state === YTPlayerState.PLAYING) {
      navigator.mediaSession.playbackState = "playing";
      setMediaSessionAudioPlaying(true);
    } else if (state.player_state === YTPlayerState.PAUSED) {
      navigator.mediaSession.playbackState = "paused";
      setMediaSessionAudioPlaying(false);
    } else if (state.player_state === YTPlayerState.ENDED) {
      navigator.mediaSession.playbackState = "none";
      setMediaSessionAudioPlaying(false);
    }
  }, [setMediaSessionAudioPlaying, state.player_state]);

  useEffect(() => {
    if (state.current_id && player && playerIsReady) {
      const id = getVideoData(player)?.video_id;
      if (id !== state.current_id) {
        try {
          player.loadVideoById(state.current_id);
          player2?.loadVideoById(state.current_id);
        } catch {
          // YouTube can reject commands while replacing the iframe's video.
        }
      }
    }
  }, [state.current_id, player, playerIsReady, player2]);

  useEffect(() => {
    searchGridRef.current?.api.refreshCells({
      force: true,
      suppressFlash: true,
    });
  }, [state.collection_last_update]);

  useEffect(() => {
    const i = setInterval(() => {
      if (player && playerIsReady) {
        const t = Math.max(0.0, player.getCurrentTime());
        const d = Math.max(1.0, player.getDuration());
        document.documentElement.style.setProperty(
          "--stereo-playback-progress",
          `${t / d}`,
        );
        dispatch({
          type: "set-playback-progress",
          percent: (100.0 * t) / d,
          duration: d,
          current_time: t,
        });

        if (
          "mediaSession" in navigator &&
          Number.isFinite(t) &&
          Number.isFinite(d) &&
          d > 0
        ) {
          try {
            navigator.mediaSession.setPositionState({
              duration: d,
              playbackRate: 1,
              position: Math.min(Math.max(t, 0), d),
            });
          } catch {
            // Ignore invalid position updates while YouTube is loading a video.
          }
        }
      }

      if (player && playerIsReady && player2 && player2IsReady) {
        const t = player.getCurrentTime();
        const t2 = player2.getCurrentTime();
        if (t > 0 && t2 > 0 && Math.abs(t - t2) > 0.1) {
          player2.seekTo(t, true);
        }
      }
    }, 1000);

    return () => {
      clearInterval(i);
    };
  }, [player, playerIsReady, player2, player2IsReady, dispatch]);

  useEffect(() => {
    const query = state.search_box_input?.trim() ?? "";
    if (query.length === 0) {
      dispatch({ type: "clear-search-results" });
      sendMsg({ type: "search-cancel-all" });
    }
  }, [sendMsg, state.search_box_input]);

  const handleNewReleasesSearch = useCallback(
    (artists: string[], labels: string[], overrides: NewReleasesOverrides) => {
      setNewReleasesOverrides((prev) => ({
        ...prev,
        [settings.collectionPath]: overrides,
      }));
      const query_id = ++queryId.current;
      dispatch({ type: "clear-search-results" });
      dispatch({ type: "set-search-busy", query_id });
      sendMsg({
        type: "search-new-releases",
        query_id,
        days: state.new_releases_days,
        limit: state.search_limit,
        sources: state.search_sources,
        artists,
        labels,
      });
    },
    [
      sendMsg,
      setNewReleasesOverrides,
      settings.collectionPath,
      state.new_releases_days,
      state.search_limit,
      state.search_sources,
    ],
  );

  const handleSearch = useCallback(() => {
    if (state.search_kind === "new-releases") {
      setNewReleasesOpen(true);
      return;
    }

    if (state.search_kind === "beatport-top-100") {
      const query_id = ++queryId.current;
      dispatch({ type: "clear-search-results" });
      dispatch({ type: "set-search-busy", query_id });
      sendMsg({
        type: "search-beatport-top100",
        query_id,
        genre_id: beatportGenreId,
        limit: state.search_limit,
      });
      return;
    }

    const query = state.search_box_input?.trim() ?? "";
    if (query.length > 0) {
      const query_id = ++queryId.current;
      dispatch({ type: "clear-search-results" });
      dispatch({ type: "set-search-busy", query_id });
      sendMsg({
        type: "search",
        query,
        query_id,
        kind: state.search_kind,
        limit: state.search_limit,
        sources: state.search_sources,
      });
    }
  }, [
    sendMsg,
    beatportGenreId,
    state.search_box_input,
    state.search_kind,
    state.search_limit,
    state.search_sources,
  ]);

  const handleExitSearch = useCallback(() => {
    dispatch({ type: "search-box-input", input: "" });
    dispatch({ type: "clear-search-results" });
    sendMsg({ type: "search-cancel-all" });
  }, [dispatch, sendMsg]);

  useEffect(() => {
    let t = null;

    t = setTimeout(() => {
      if (state.import_from) {
        sendMsg({ type: "check-import-from", path: state.import_from });
      }
    }, 1000);

    return () => {
      if (t !== null) {
        clearTimeout(t);
      }
    };
  }, [state.import_from, sendMsg]);

  const updateRating = useCallback(
    (yt_id: string, rating: number | null) => {
      sendMsg({ type: "update-rating", yt_id, rating });
    },
    [sendMsg],
  );

  const onSliderValueChange = useCallback(
    (value: number[]) => {
      if (player && playerIsReady && player2 && player2IsReady) {
        const d = player.getDuration();
        const t = player.getCurrentTime();
        if (d) {
          player.seekTo((d * value[0]) / 100.0, true);
          player2.seekTo((d * value[0]) / 100.0, true);
        }
        dispatch({
          type: "set-playback-progress",
          percent: value[0],
          duration: d,
          current_time: t,
        });
      }
    },
    [player, player2, playerIsReady, player2IsReady],
  );

  const resetColumnState = useCallback(() => {
    gridRef.current?.api.resetColumnState();
    gridRef.current?.api.setFilterModel(null);
  }, []);

  const deselectAll = useCallback(() => {
    gridRef.current?.api.deselectAll();
  }, []);

  // Deletes wait for confirmation in an AlertDialog.
  const [pendingDelete, setPendingDelete] = useState<{
    ids: string[];
    description: string;
    // Deleting the playing track also skips to the next one.
    isNowPlaying: boolean;
  } | null>(null);

  const deleteSelectedTracks = useCallback(() => {
    const selectedRows = gridRef.current?.api.getSelectedRows();
    if (selectedRows && selectedRows.length > 0) {
      const what =
        selectedRows.length === 1
          ? `“${selectedRows[0].title}”`
          : `${selectedRows.length} tracks`;
      setPendingDelete({
        ids: selectedRows.map((t) => t.yt_id),
        description: `This removes ${what} from ${collectionLabel(settings.collectionPath)}, including ratings and play counts.`,
        isNowPlaying: false,
      });
    }
  }, [settings.collectionPath]);

  const confirmDelete = useCallback(() => {
    if (!pendingDelete) return;
    if (pendingDelete.isNowPlaying) {
      dispatch({ type: "play-next-track" });
    } else {
      gridRef.current?.api.deselectAll();
    }
    sendMsg({ type: "delete-tracks", ids: pendingDelete.ids });
    setPendingDelete(null);
  }, [pendingDelete, sendMsg]);

  const exportSelectedTracksToCollection = useCallback(
    (collection: string) => {
      const tracks = gridRef.current?.api.getSelectedRows();
      if (tracks) {
        sendMsg({ type: "export-tracks-to-collection", tracks, collection });
      }
    },
    [sendMsg],
  );

  const openSelectedTracksAsYTMPlaylist = useCallback(() => {
    const selectedRows = gridRef.current?.api.getSelectedRows();
    if (selectedRows) {
      const yt_ids = selectedRows.map((t) => t.yt_id);
      requestReply({ type: "create-yt-anon-playlist", yt_ids }, (r) => {
        if ("url" in r) {
          window.open(r.url ?? undefined, "_blank");
        }
      });
    }
  }, [requestReply]);

  const openTopTracksAsYTMPlaylist = useCallback(() => {
    if (gridRef.current) {
      const api = gridRef.current.api;
      const filterModel = api.getState().filter?.filterModel;
      const sortModel = api.getState().sort?.sortModel;
      requestReply(
        { type: "get-rows", startRow: 0, endRow: 50, filterModel, sortModel },
        (r) => {
          if ("rows" in r) {
            const yt_ids = r.rows.map((t: ITrack) => t.yt_id);
            requestReply({ type: "create-yt-anon-playlist", yt_ids }, (r2) => {
              if ("url" in r2) {
                window.open(r2.url ?? undefined, "_blank");
              }
            });
          }
        },
      );
    }
  }, [requestReply]);

  const [ytPlaylistOpen, setYtPlaylistOpen] = useState(false);
  const [ytPlaylistYtIds, setYtPlaylistYtIds] = useState<string[]>([]);

  const [spotifyPlaylistOpen, setSpotifyPlaylistOpen] = useState(false);
  const [spotifyPlaylistTracks, setSpotifyPlaylistTracks] = useState<
    { artist: string; title: string; sp_id?: string | null }[]
  >([]);

  const [addTracksPending, setAddTracksPending] = useState<{
    conflicts: ITrack[];
    nonConflicting: ITrack[];
  } | null>(null);
  const addTracksPendingId = useRef(0);

  const exportSelectionToSpotifyPlaylist = useCallback(() => {
    const selectedRows = gridRef.current?.api.getSelectedRows();
    if (selectedRows && selectedRows.length > 0) {
      setSpotifyPlaylistTracks(
        selectedRows.map((t: ITrack) => ({
          artist: t.artists.join(", "),
          title: t.title,
          sp_id: t.sp_id,
        })),
      );
      setSpotifyPlaylistOpen(true);
    }
  }, []);

  const spotifyPlaylistReqId = useRef<number>(0);
  const createSpotifyPlaylist = useCallback(
    (target: SpotifyPlaylistTarget) => {
      const id = ++spotifyPlaylistReqId.current;
      dispatch({
        type: "start-spotify-playlist-export",
        id,
        total: spotifyPlaylistTracks.length,
      });
      sendMsg({
        type: "create-spotify-playlist",
        id,
        name: target.mode === "new" ? target.name : "",
        tracks: spotifyPlaylistTracks,
        existing_playlist: target.mode === "existing" ? target.url : null,
      });
    },
    [sendMsg, spotifyPlaylistTracks],
  );

  useEffect(() => {
    if (state.spotify_playlist_open_url) {
      window.open(state.spotify_playlist_open_url, "_blank");
      dispatch({ type: "clear-spotify-playlist-open-url" });
    }
  }, [state.spotify_playlist_open_url]);

  const youtubePlaylistReqId = useRef<number>(0);
  const createYouTubePlaylist = useCallback(
    (name: string) => {
      const id = ++youtubePlaylistReqId.current;
      dispatch({
        type: "start-youtube-playlist-export",
        id,
        total: ytPlaylistYtIds.length,
      });
      sendMsg({
        type: "create-youtube-playlist",
        id,
        name,
        yt_ids: ytPlaylistYtIds,
      });
    },
    [sendMsg, ytPlaylistYtIds],
  );

  useEffect(() => {
    if (state.youtube_playlist_open_url) {
      window.open(state.youtube_playlist_open_url, "_blank");
      dispatch({ type: "clear-youtube-playlist-open-url" });
    }
  }, [state.youtube_playlist_open_url]);

  const cancelYouTubePlaylist = useCallback(
    (reason: "cancelled" | "timeout" = "cancelled") => {
      const id = youtubePlaylistReqId.current;
      dispatch({ type: "cancel-youtube-playlist", reason });
      sendMsg({ type: "cancel-youtube-playlist", id });
    },
    [sendMsg],
  );

  // Auto-cancel if the server goes quiet for 15s (e.g. hitting the YouTube
  // API quota) so the progress dialog never gets stuck open indefinitely.
  useEffect(() => {
    if (!state.youtube_playlist_running) return;
    const timer = setTimeout(() => cancelYouTubePlaylist("timeout"), 15000);
    return () => clearTimeout(timer);
  }, [
    state.youtube_playlist_running,
    state.youtube_playlist_current,
    cancelYouTubePlaylist,
  ]);

  const openSelectedTracksAsYTSavedPlaylist = useCallback(() => {
    const selectedRows = gridRef.current?.api.getSelectedRows();
    if (selectedRows && selectedRows.length > 0) {
      setYtPlaylistYtIds(selectedRows.map((t) => t.yt_id));
      setYtPlaylistOpen(true);
    }
  }, []);

  const openAllTracksAsYTSavedPlaylist = useCallback(() => {
    if (gridRef.current) {
      const api = gridRef.current.api;
      const filterModel = api.getState().filter?.filterModel;
      const sortModel = api.getState().sort?.sortModel;
      requestReply(
        {
          type: "get-rows",
          startRow: 0,
          endRow: 10000,
          filterModel,
          sortModel,
        },
        (r) => {
          if ("rows" in r) {
            const yt_ids = (r.rows as ITrack[]).map((t) => t.yt_id);
            if (yt_ids.length > 0) {
              setYtPlaylistYtIds(yt_ids);
              setYtPlaylistOpen(true);
            }
          }
        },
      );
    }
  }, [requestReply]);

  const exportAllToSpotifyPlaylist = useCallback(() => {
    if (gridRef.current) {
      const api = gridRef.current.api;
      const filterModel = api.getState().filter?.filterModel;
      const sortModel = api.getState().sort?.sortModel;
      requestReply(
        {
          type: "get-rows",
          startRow: 0,
          endRow: 10000,
          filterModel,
          sortModel,
        },
        (r) => {
          if ("rows" in r) {
            const tracks = (r.rows as ITrack[]).map((t) => ({
              artist: t.artists.join(", "),
              title: t.title,
              sp_id: t.sp_id,
            }));
            if (tracks.length > 0) {
              setSpotifyPlaylistTracks(tracks);
              setSpotifyPlaylistOpen(true);
            }
          }
        },
      );
    }
  }, [requestReply]);

  useYouTubeOAuthCallback(requestReply, dispatch);

  const addTracksToCollection = useCallback(
    async (tracks: ITrack[]) => {
      if (tracks.length === 0) return true;

      const contained = await new Promise<Set<string> | null>((resolve) => {
        requestReply(
          {
            type: "collection-contains-ids",
            yt_ids: tracks.map((t) => t.yt_id),
          },
          (msg) => resolve("contained" in msg ? new Set(msg.contained) : null),
        );
      });
      // Without knowing what's already there, adding (which overwrites) isn't safe; the
      // server has already reported why the check failed.
      if (contained === null) return false;

      const conflicts = tracks.filter((t) => contained.has(t.yt_id));
      const nonConflicting = tracks.filter((t) => !contained.has(t.yt_id));

      if (conflicts.length === 0) {
        if (nonConflicting.length > 0) {
          sendMsg({
            type: "add-tracks",
            tracks: nonConflicting,
            overwrite_existing: true,
          });
        }
        return true;
      } else {
        addTracksPendingId.current += 1;
        setAddTracksPending({ conflicts, nonConflicting });
        return false;
      }
    },
    [requestReply, sendMsg],
  );

  const youtubeFixupReqId = useRef(0);
  const fixupYouTubePlaylist = useCallback(
    (name: string, tracks: ITrack[]) => {
      const id = ++youtubeFixupReqId.current;
      dispatch({
        type: "start-youtube-fixup",
        id,
        name,
        total: tracks.length,
      });
      sendMsg({ type: "fixup-tracks", id, tracks });
    },
    [sendMsg],
  );

  const cancelYouTubeFixup = useCallback(() => {
    const id = youtubeFixupReqId.current;
    dispatch({ type: "cancel-youtube-fixup" });
    sendMsg({ type: "cancel-fixup-tracks", id });
  }, [sendMsg]);

  const commitYouTubeStaging = useCallback(
    (tracks: ITrack[]) => {
      const name = state.youtube_fixup_name ?? "YouTube playlist";
      const matched = state.youtube_fixup_matched ?? 0;
      const total = state.youtube_fixup_total ?? tracks.length;
      dispatch({ type: "clear-youtube-fixup-result" });
      void addTracksToCollection(tracks).then((added) => {
        if (added) {
          dispatch({
            type: "notification",
            message: `Imported ${tracks.length} track${tracks.length === 1 ? "" : "s"} from “${name}” (${matched}/${total} enriched)`,
            kind: "info",
          });
        }
      });
    },
    [
      addTracksToCollection,
      state.youtube_fixup_matched,
      state.youtube_fixup_name,
      state.youtube_fixup_total,
    ],
  );

  const addSelectedTracks = useCallback(async () => {
    const selection = state.search_results_selection;
    if (!selection || selection.length === 0) return;

    const added = await addTracksToCollection(selection);
    if (added) {
      searchGridRef.current?.api.deselectAll();
    }
  }, [addTracksToCollection, state.search_results_selection]);

  const importYouTubePlaylist = useCallback(
    (name: string, tracks: ITrack[]) => {
      fixupYouTubePlaylist(name, tracks);
    },
    [fixupYouTubePlaylist],
  );

  const addSearchResultTrack = useCallback(
    (track: ITrack) => {
      void addTracksToCollection([track]);
    },
    [addTracksToCollection],
  );

  const confirmAddTracks = useCallback(
    (overwriteYtIds: Set<string>) => {
      if (!addTracksPending) return;
      const tracks = [
        ...addTracksPending.nonConflicting,
        ...addTracksPending.conflicts.filter((t) =>
          overwriteYtIds.has(t.yt_id),
        ),
      ];
      if (tracks.length > 0) {
        sendMsg({ type: "add-tracks", tracks, overwrite_existing: true });
      }
      searchGridRef.current?.api.deselectAll();
      setAddTracksPending(null);
    },
    [addTracksPending, sendMsg],
  );

  const updateCurrentRating = useCallback(
    (rating: number | null) => {
      if (state.current_id) {
        dispatch({ type: "set-track-rating", yt_id: state.current_id, rating });
        updateRating(state.current_id, rating);
      }
    },
    [dispatch, updateRating, state.current_id],
  );

  const fastForward = useCallback(
    (seconds: number) => {
      if (player && playerIsReady) {
        const t = player.getCurrentTime();
        const d = player.getDuration();
        const target = Math.max(
          0,
          Math.min(Number.isFinite(d) && d > 0 ? d : Infinity, t + seconds),
        );
        player.seekTo(target, true);
        if (player2 && player2IsReady) {
          player2.seekTo(target, true);
        }
      }
    },
    [player, playerIsReady, player2, player2IsReady],
  );

  const pausePlayback = useCallback(() => {
    if (player && playerIsReady && player2 && player2IsReady) {
      player.pauseVideo();
      player2.pauseVideo();
    }
  }, [player, player2, playerIsReady, player2IsReady]);

  const startPlayback = useCallback(() => {
    if (player && playerIsReady && player2 && player2IsReady) {
      player.playVideo();
      player2.playVideo();
    }
  }, [player, player2, playerIsReady, player2IsReady]);

  const stopPlayback = useCallback(() => {
    if (player && playerIsReady && player2 && player2IsReady) {
      player.stopVideo();
      player2.stopVideo();
    }
  }, [player, player2, playerIsReady, player2IsReady]);

  const togglePlayback = useCallback(() => {
    if (player && playerIsReady) {
      const state = player.getPlayerState();
      if (state === window.YT.PlayerState.PLAYING) {
        pausePlayback();
      }
      if (
        state === window.YT.PlayerState.PAUSED ||
        state === window.YT.PlayerState.CUED
      ) {
        startPlayback();
      }
    }
  }, [player, playerIsReady, pausePlayback, startPlayback]);

  const toggleShuffle = useCallback(() => {
    const shufflePlay = !settings.shufflePlay;
    setSettings({ ...settings, shufflePlay });
    updatePlaylist(shufflePlay);
  }, [settings, setSettings, updatePlaylist]);

  const toggleVideo = useCallback(
    () => setSettings({ ...settings, video: !settings.video }),
    [settings, setSettings],
  );

  const toggleBackgroundVideo = useCallback(
    () =>
      setSettings({ ...settings, backgroundVideo: !settings.backgroundVideo }),
    [settings, setSettings],
  );

  const hasSearchInput =
    isQuerylessSearchKind(state.search_kind) || !!state.search_box_input;
  const isSearchGridActive =
    (state.search_results !== undefined || state.search_busy) && hasSearchInput;

  const centerGridAroundCurrentTrack = useCallback(() => {
    if (!state.current_id) {
      return;
    }
    if (isSearchGridActive) {
      const index = state.search_results?.findIndex(
        (track) => track.yt_id === state.current_id,
      );
      if (index !== undefined && index >= 0) {
        searchGridRef.current?.api.ensureIndexVisible(index, "middle");
      }
      return;
    }
    if (gridReady && gridRef.current) {
      const api = gridRef.current.api;
      const filterModel = api.getState().filter?.filterModel;
      const sortModel = api.getState().sort?.sortModel;
      requestReply(
        {
          type: "get-row-index",
          yt_id: state.current_id,
          filterModel,
          sortModel,
        },
        function (msg: ServerMsg | { type: string }) {
          if ("index" in msg) {
            gridRef.current?.api.ensureIndexVisible(
              msg.index as number,
              "middle",
            );
          } else {
            logger.warn("failed to get row index");
          }
        },
      );
    }
  }, [
    gridReady,
    requestReply,
    state.current_id,
    isSearchGridActive,
    state.search_results,
  ]);

  const nextTrack = useCallback(
    () => dispatch({ type: "play-next-track" }),
    [dispatch],
  );

  const prevTrack = useCallback(
    () => dispatch({ type: "play-prev-track" }),
    [dispatch],
  );

  useEffect(() => {
    if (!("mediaSession" in navigator)) return;

    const mediaSession = navigator.mediaSession;
    const handlers: [MediaSessionAction, MediaSessionActionHandler][] = [
      ["play", startPlayback],
      ["pause", pausePlayback],
      ["seekbackward", (details) => fastForward(-(details.seekOffset ?? 10))],
      ["seekforward", (details) => fastForward(details.seekOffset ?? 10)],
      [
        "seekto",
        (details) => {
          const seekTime = details.seekTime;
          if (
            player &&
            playerIsReady &&
            seekTime !== undefined &&
            Number.isFinite(seekTime)
          ) {
            player.seekTo(seekTime, true);
            if (player2 && player2IsReady) {
              player2.seekTo(seekTime, true);
            }
          }
        },
      ],
      ["previoustrack", prevTrack],
      ["nexttrack", nextTrack],
    ];

    const setHandler = (
      action: MediaSessionAction,
      handler: MediaSessionActionHandler | null,
    ) => {
      try {
        mediaSession.setActionHandler(action, handler);
      } catch {
        // Browsers may expose Media Session without supporting every action.
      }
    };

    for (const [action, handler] of handlers) {
      setHandler(action, handler);
    }

    return () => {
      for (const [action] of handlers) {
        setHandler(action, null);
      }
    };
  }, [
    fastForward,
    nextTrack,
    pausePlayback,
    player,
    player2,
    player2IsReady,
    playerIsReady,
    prevTrack,
    startPlayback,
  ]);

  const onTrackGridStateUpdate = useCallback(
    (ev: StateUpdatedEvent<ITrack>) => {
      if (ev.sources.some((s) => s === "filter" || s === "sort")) {
        updatePlaylist(settings.shufflePlay);
      }
      setGridState(ev.state);
    },
    [setGridState, updatePlaylist, settings.shufflePlay],
  );

  const appShellRef = useRef<HTMLDivElement>(null);

  const toggleFullscreen = useCallback(() => {
    const entering = !settings.fullScreen;
    setSettings({ ...settings, fullScreen: entering });
    if (entering) {
      appShellRef.current?.requestFullscreen?.().catch(() => {});
    } else if (document.fullscreenElement) {
      document.exitFullscreen().catch(() => {});
    }
  }, [settings, setSettings]);

  // Keep settings in sync when the user exits fullscreen via Esc or browser UI
  useEffect(() => {
    const onFullscreenChange = () => {
      if (!document.fullscreenElement) {
        setSettings((s) => ({ ...s, fullScreen: false }));
      }
    };
    document.addEventListener("fullscreenchange", onFullscreenChange);
    return () =>
      document.removeEventListener("fullscreenchange", onFullscreenChange);
  }, [setSettings]);

  const handleArtistClick = useCallback(
    (name: string) => {
      const query_id = ++queryId.current;
      dispatch({ type: "set-search-kind", kind: "by-artist" });
      dispatch({ type: "search-box-input", input: name });
      dispatch({ type: "clear-search-results" });
      dispatch({ type: "set-search-busy", query_id });
      sendMsg({
        type: "search",
        query: name,
        query_id,
        kind: "by-artist",
        limit: state.search_limit,
        sources: state.search_sources,
      });
    },
    [dispatch, sendMsg, state.search_limit, state.search_sources],
  );

  const handleLabelClick = useCallback(
    (name: string) => {
      const query_id = ++queryId.current;
      dispatch({ type: "set-search-kind", kind: "by-label" });
      dispatch({ type: "search-box-input", input: name });
      dispatch({ type: "clear-search-results" });
      dispatch({ type: "set-search-busy", query_id });
      sendMsg({
        type: "search",
        query: name,
        query_id,
        kind: "by-label",
        limit: state.search_limit,
        sources: state.search_sources,
      });
    },
    [dispatch, sendMsg, state.search_limit, state.search_sources],
  );

  const handleShazamSimilar = useCallback(
    (track: ITrack) => {
      const query_id = ++queryId.current;
      const query = `Shazam similar to: ${track.artists.join(", ")} - ${track.title}`;
      dispatch({ type: "set-search-kind", kind: "fuzzy" });
      dispatch({ type: "search-box-input", input: query });
      dispatch({ type: "clear-search-results" });
      dispatch({ type: "set-search-busy", query_id });
      sendMsg({
        type: "search-shazam-similar",
        query_id,
        track,
        limit: state.search_limit,
      });
    },
    [dispatch, sendMsg, state.search_limit],
  );

  useKeyboardActions({
    "0": () => updateCurrentRating(null),
    "1": () => updateCurrentRating(1),
    "2": () => updateCurrentRating(2),
    "3": () => updateCurrentRating(3),
    "4": () => updateCurrentRating(4),
    "5": () => updateCurrentRating(5),
    s: () => toggleShuffle(),
    v: () => toggleVideo(),
    b: () => toggleBackgroundVideo(),
    c: () => centerGridAroundCurrentTrack(),
    f: () => toggleFullscreen(),
    p: () => togglePlayback(),
    Space: () => togglePlayback(),
    Backspace: () => stopPlayback(),
    ArrowLeft: () => fastForward(-10),
    ArrowRight: () => fastForward(10),
    ArrowUp: () => prevTrack(),
    ArrowDown: () => nextTrack(),
  });

  return (
    <Theme theme={stereoTheme} mode={settings.appearance}>
      <VStack
        ref={appShellRef}
        className={`app-main ${settings.fullScreen ? "fullscreen" : ""}`}
        hAlign="center"
      >
        <HStack vAlign="center" gap={2} className="toolbar">
          <HStack vAlign="center" gap={2} className="toolbar-side">
            <CollectionSelector
              isValid={state.collection_is_valid ?? true}
              collections={state.collections ?? []}
              value={settings.collectionPath}
              setValue={(path) =>
                setSettings({ ...settings, collectionPath: path })
              }
              onCreate={(name) => sendMsg({ type: "create-collection", name })}
              repos={state.repos ?? []}
              repoPath={settings.repoPath}
              setRepoPath={(path) => {
                setSettings({ ...settings, repoPath: path });
                activeRepo.current = path;
                sendMsg({ type: "set-repo", path });
              }}
              onClone={(url) => {
                // The server replies with the activated repo's path before it sends the
                // updated repo list, so the auto-select effect picks the new repo.
                requestReply({ type: "clone-repo", url }, (msg) => {
                  if (msg.type === "clone-repo-reply" && "path" in msg) {
                    const path = msg.path;
                    if (path) {
                      // The server has already switched to (and is syncing) the clone.
                      activeRepo.current = path;
                      setSettings((s) => ({ ...s, repoPath: path }));
                    }
                  }
                });
              }}
              syncState={state.git_sync_state}
              syncMessage={state.git_sync_message}
              onShowStats={
                hasCollectionStats(state.collection_stats)
                  ? () => setStatsOpen(true)
                  : undefined
              }
            />
            <GitSyncStatus
              state={state.git_sync_state}
              message={state.git_sync_message}
              retryable={state.git_sync_retryable}
              onRetry={() => sendMsg({ type: "retry-git-sync" })}
              onResolve={(strategy) =>
                sendMsg({ type: "resolve-git-conflict", strategy })
              }
            />
          </HStack>
          <SearchBox
            value={state.search_box_input ?? ""}
            setValue={(value: string) =>
              dispatch({ type: "search-box-input", input: value })
            }
            onSearch={handleSearch}
            searchLimit={state.search_limit}
            setSearchLimit={(limit: number) =>
              dispatch({ type: "set-search-limit", limit })
            }
            searchKind={state.search_kind}
            setSearchKind={(kind: SearchBoxKind) =>
              dispatch({ type: "set-search-kind", kind })
            }
            searchSources={state.search_sources}
            setSearchSources={(sources: TrackSource[]) =>
              dispatch({ type: "set-search-sources", sources })
            }
            newReleasesDays={state.new_releases_days}
            setNewReleasesDays={(days: number) =>
              dispatch({ type: "set-new-releases-days", days })
            }
            beatportGenreId={beatportGenreId}
            setBeatportGenreId={setBeatportGenreId}
            searchBusy={hasSearchInput && state.search_busy}
            errorMessage={state.search_error_message}
          />
          <HStack vAlign="center" hAlign="end" className="toolbar-side">
            <AppMenu
              version={state.backend_version}
              appearance={settings.appearance}
              setAppearance={(appearance) =>
                setSettings({ ...settings, appearance })
              }
              requestReply={requestReply}
              youtubeConnected={state.youtube_connected}
              youtubeClientId={state.youtube_client_id ?? null}
              collectionOpen={!!state.collection}
              spotifyBackfillRunning={state.spotify_backfill_running}
              spotifyBackfillChecked={state.spotify_backfill_checked}
              spotifyBackfillMatched={state.spotify_backfill_matched}
              spotifyBackfillTotal={state.spotify_backfill_total}
              onSpotifyBackfill={backfillSpotifyIds}
            />
          </HStack>
        </HStack>
        <div
          className={`background-player ${state.show_player2 && settings.backgroundVideo ? "" : "hidden"}`}
        >
          <div ref={player2Ref} />
        </div>
        <HStack width="100%" hAlign="center" vAlign="start" gap={6}>
          <VStack hAlign="center" gap={2}>
            {state.title && (
              <HStack vAlign="center" gap={2}>
                <Title
                  title={state.title}
                  artists={state.track_info?.artists}
                  trackName={state.track_info?.title}
                  label={state.track_info?.label}
                  releaseDate={state.track_info?.release_date}
                  onArtistClick={handleArtistClick}
                  onLabelClick={handleLabelClick}
                />
                {state.track_info && (
                  <NowPlayingMenu
                    track={state.track_info}
                    collections={state.collections ?? []}
                    currentCollectionPath={settings.collectionPath}
                    onCopyToCollection={(path) => {
                      if (state.track_info) {
                        sendMsg({
                          type: "export-tracks-to-collection",
                          tracks: [state.track_info],
                          collection: path,
                        });
                      }
                    }}
                    onMoveToCollection={(path) => {
                      if (state.track_info && state.current_id) {
                        // The server removes the track from the source collection only
                        // once it has been written to the target.
                        sendMsg({
                          type: "export-tracks-to-collection",
                          tracks: [state.track_info],
                          collection: path,
                          source_collection: settings.collectionPath,
                        });
                        dispatch({ type: "play-next-track" });
                      }
                    }}
                    onDeleteFromCollection={() => {
                      if (state.current_id && state.track_info) {
                        setPendingDelete({
                          ids: [state.current_id],
                          description: `This removes “${state.track_info.title}” from ${collectionLabel(settings.collectionPath)}, including its rating and play count.`,
                          isNowPlaying: true,
                        });
                      }
                    }}
                  />
                )}
              </HStack>
            )}

            <VStack hAlign="center" gap={2} className="controls">
              <Rating
                enabled={state.current_id === state.track_info?.yt_id}
                currentRating={
                  state.current_id === state.track_info?.yt_id
                    ? state.track_info?.rating
                    : undefined
                }
                updateRating={updateCurrentRating}
              />
              <HStack
                className="progress-slider"
                width="min(70vw, 900px)"
                vAlign="center"
                gap={2}
              >
                <Text>{formatDuration(state.current_time ?? 0)}</Text>
                <StackItem size="fill">
                  <Slider
                    label="Playback progress"
                    isLabelHidden
                    valueDisplay="none"
                    value={state.playback_progress_pct ?? 0}
                    onChange={(v: number) => onSliderValueChange([v])}
                    style={{ paddingInline: "var(--spacing-3)" }}
                  />
                </StackItem>
                <Text>
                  {state.duration ? formatDuration(state.duration) : "--:--"}
                </Text>
              </HStack>
              <HStack vAlign="center" gap={2} padding={2}>
                <Tooltip
                  content={
                    <>
                      Skip to previous track <Kbd keys="up" />
                    </>
                  }
                >
                  <IconButton
                    variant="secondary"
                    size="lg"
                    label="Skip to previous track"
                    icon={<TrackPreviousIcon />}
                    onClick={prevTrack}
                  />
                </Tooltip>

                <Tooltip
                  content={
                    <>
                      Play / pause <Kbd keys="Space" /> / <Kbd keys="p" />
                    </>
                  }
                >
                  <IconButton
                    variant="secondary"
                    size="lg"
                    className="play-pause-button"
                    label="Play / pause"
                    icon={
                      state.player_state === YTPlayerState.PLAYING ? (
                        <PauseIcon />
                      ) : (
                        <PlayIcon />
                      )
                    }
                    onClick={togglePlayback}
                  />
                </Tooltip>

                <Tooltip
                  content={
                    <>
                      Skip to next track <Kbd keys="down" />
                    </>
                  }
                >
                  <IconButton
                    variant="secondary"
                    size="lg"
                    label="Skip to next track"
                    icon={<TrackNextIcon />}
                    onClick={nextTrack}
                  />
                </Tooltip>

                <Divider orientation="vertical" />

                <Tooltip
                  content={
                    <>
                      Toggle shuffle play <Kbd keys="s" />
                    </>
                  }
                >
                  <IconButton
                    variant={settings.shufflePlay ? "primary" : "secondary"}
                    size="lg"
                    label="Toggle shuffle play"
                    icon={<ShuffleIcon />}
                    onClick={toggleShuffle}
                  />
                </Tooltip>

                <Tooltip
                  content={
                    <>
                      Toggle background video <Kbd keys="b" />
                    </>
                  }
                >
                  <IconButton
                    variant={settings.backgroundVideo ? "primary" : "secondary"}
                    size="lg"
                    label="Toggle background video"
                    icon={<MagicWandIcon />}
                    onClick={toggleBackgroundVideo}
                  />
                </Tooltip>

                <Divider orientation="vertical" />

                <Tooltip
                  content={
                    <>
                      Toggle video <Kbd keys="v" />
                    </>
                  }
                >
                  <IconButton
                    className="toggle-video-button"
                    variant={settings.video ? "primary" : "secondary"}
                    size="lg"
                    label="Toggle video"
                    icon={<VideoIcon />}
                    onClick={toggleVideo}
                  />
                </Tooltip>

                <Tooltip
                  content={
                    <>
                      Toggle fullscreen mode <Kbd keys="f" />
                    </>
                  }
                >
                  <IconButton
                    variant="secondary"
                    size="lg"
                    label="Toggle fullscreen mode"
                    icon={
                      settings.fullScreen ? (
                        <ExitFullScreenIcon />
                      ) : (
                        <EnterFullScreenIcon />
                      )
                    }
                    onClick={toggleFullscreen}
                  />
                </Tooltip>
              </HStack>
            </VStack>
          </VStack>
        </HStack>

        <div
          className={`player ${settings.video || settings.fullScreen ? "" : "hidden"}`}
        >
          <div ref={playerRef} />
        </div>
        {isSearchGridActive ? (
          <HStack width="100%" gap={2} padding={2}>
            <IconButton
              variant="primary"
              label="Add selected tracks"
              tooltip="Add selected tracks"
              icon={<PlusIcon />}
              clickAction={addSelectedTracks}
              isDisabled={
                !(
                  state.search_results_selection &&
                  state.search_results_selection.length > 0
                )
              }
            />

            {state.search_results !== undefined && (
              <Tooltip content={`${state.search_results.length} results`}>
                <HStack
                  vAlign="center"
                  gap={1}
                  paddingInline={2}
                  className="row-count"
                >
                  <Text weight="bold" color="secondary">
                    {state.search_results.length}
                  </Text>
                  <Text color="secondary">results</Text>
                </HStack>
              </Tooltip>
            )}

            <IconButton
              variant="secondary"
              label="Exit search"
              tooltip="Exit search"
              icon={<Cross2Icon />}
              onClick={handleExitSearch}
            />
          </HStack>
        ) : (
          <HStack width="100%" gap={2} padding={2} className="grid-controls">
            <IconButton
              variant="secondary"
              label="Deselect all"
              tooltip="Deselect all"
              icon={<Cross2Icon />}
              onClick={deselectAll}
              isDisabled={state.track_selection.length === 0}
            />

            <IconButton
              variant="destructive"
              label="Delete selected tracks"
              tooltip="Delete selected tracks"
              icon={<TrashIcon />}
              onClick={deleteSelectedTracks}
              isDisabled={state.track_selection.length === 0}
            />

            <ExportDropDown
              selectionCount={state.track_selection.length}
              collections={state.collections ?? []}
              exportToCollection={exportSelectedTracksToCollection}
              exportSelectionToYTM={openSelectedTracksAsYTMPlaylist}
              exportSelectionToYTSavedPlaylist={
                openSelectedTracksAsYTSavedPlaylist
              }
              exportTop50ToYTM={openTopTracksAsYTMPlaylist}
              exportAllToYTSavedPlaylist={openAllTracksAsYTSavedPlaylist}
              exportSelectionToSpotifyPlaylist={
                exportSelectionToSpotifyPlaylist
              }
              exportAllToSpotifyPlaylist={exportAllToSpotifyPlaylist}
            />

            <ImportMenu
              setImportFrom={(path: string) =>
                dispatch({ type: "set-import-from", path })
              }
              keepUserData={state.import_from_keep_user_data ?? false}
              setKeepUserData={(keep: boolean) =>
                dispatch({ type: "set-import-keep-user-data", keep })
              }
              importFrom={state.import_from ?? ""}
              doImport={() => {
                if (state.import_from) {
                  sendMsg({
                    type: "import-from",
                    path: state.import_from,
                    keep_user_data: state.import_from_keep_user_data ?? false,
                  });
                }
              }}
              isValidImportFrom={state.import_from_is_valid}
              requestReply={requestReply}
              onYouTubeImport={importYouTubePlaylist}
            />

            <ColumnsMenu gridRef={gridRef} />

            <IconButton
              variant="secondary"
              label="Reset columns"
              tooltip="Reset columns"
              icon={<ResetIcon />}
              onClick={resetColumnState}
            />

            {filteredRowCount !== undefined && (
              <Tooltip
                content={
                  filteredRowCount !== state.collection?.size
                    ? `${filteredRowCount} of ${state.collection?.size} tracks match active filters`
                    : `${filteredRowCount} tracks`
                }
              >
                <HStack
                  vAlign="center"
                  gap={1}
                  paddingInline={2}
                  className="row-count"
                >
                  <Text
                    weight="bold"
                    color={
                      filteredRowCount !== state.collection?.size
                        ? "accent"
                        : "secondary"
                    }
                  >
                    {filteredRowCount}
                  </Text>
                  {filteredRowCount !== state.collection?.size &&
                    state.collection?.size !== undefined && (
                      <Text color="secondary">/ {state.collection.size}</Text>
                    )}
                  <Text color="secondary">tracks</Text>
                </HStack>
              </Tooltip>
            )}
          </HStack>
        )}
        <StackItem size="fill" className="grid-container">
          {isSearchGridActive ? (
            <SearchResultsGrid
              gridRef={searchGridRef}
              currentId={state.current_id}
              playId={playId}
              requestReply={requestReply}
              dispatch={dispatch}
              tracks={state.search_results}
              lastCollectionUpdate={state.collection_last_update}
              onArtistClick={handleArtistClick}
              onLabelClick={handleLabelClick}
              addTrack={addSearchResultTrack}
            />
          ) : (
            <TracksGrid
              gridRef={gridRef}
              currentId={state.current_id}
              playId={playIdForGrid}
              updateRating={updateRating}
              requestReply={requestReply}
              dispatch={dispatch}
              sendMsg={sendMsg}
              setGridReady={setGridReady}
              initialState={gridState}
              onStateUpdate={onTrackGridStateUpdate}
              onRowCountChange={setFilteredRowCount}
              onArtistClick={handleArtistClick}
              onLabelClick={handleLabelClick}
              onShazamSimilar={handleShazamSimilar}
            />
          )}
        </StackItem>
      </VStack>
      <Notification
        open={state.notification_show}
        setOpen={(open: boolean) =>
          dispatch({ type: "set-notification-show", show: open })
        }
        message={state.notification_msg}
        kind={state.notification_kind}
      />
      <YouTubePlaylistDialog
        open={ytPlaylistOpen}
        trackCount={ytPlaylistYtIds.length}
        onOpenChange={setYtPlaylistOpen}
        onCreate={createYouTubePlaylist}
      />
      <YouTubePlaylistProgressDialog
        running={state.youtube_playlist_running}
        current={state.youtube_playlist_current ?? 0}
        total={state.youtube_playlist_total ?? 0}
        onCancel={cancelYouTubePlaylist}
      />
      <YouTubeFixupProgressDialog
        running={state.youtube_fixup_running}
        current={state.youtube_fixup_current ?? 0}
        matched={state.youtube_fixup_matched ?? 0}
        total={state.youtube_fixup_total ?? 0}
        onCancel={cancelYouTubeFixup}
      />
      <YouTubePlaylistStagingDialog
        key={`${state.youtube_fixup_id}-${state.youtube_fixup_tracks ? "staged" : "loading"}`}
        open={state.youtube_fixup_tracks !== undefined}
        name={state.youtube_fixup_name ?? "YouTube playlist"}
        tracks={state.youtube_fixup_tracks ?? []}
        requestReply={requestReply}
        onCancel={() => dispatch({ type: "clear-youtube-fixup-result" })}
        onCommit={commitYouTubeStaging}
      />
      <SpotifyPlaylistDialog
        open={spotifyPlaylistOpen}
        trackCount={spotifyPlaylistTracks.length}
        requestReply={requestReply}
        onOpenChange={setSpotifyPlaylistOpen}
        onCreate={createSpotifyPlaylist}
      />
      <SpotifyPlaylistProgressDialog
        running={state.spotify_playlist_running}
        checked={state.spotify_playlist_checked ?? 0}
        matched={state.spotify_playlist_matched ?? 0}
        total={state.spotify_playlist_total ?? 0}
      />
      <NewReleasesDialog
        isOpen={newReleasesOpen}
        onOpenChange={setNewReleasesOpen}
        days={state.new_releases_days}
        requestReply={requestReply}
        overrides={newReleasesOverrides[settings.collectionPath]}
        onSearch={handleNewReleasesSearch}
      />
      {state.collection_stats && (
        <CollectionSummary
          isOpen={statsOpen}
          onOpenChange={setStatsOpen}
          stats={state.collection_stats}
          totalTracks={state.collection?.size}
          onArtistClick={handleArtistClick}
          onLabelClick={handleLabelClick}
        />
      )}
      <AlertDialog
        isOpen={pendingDelete !== null}
        onOpenChange={(open) => {
          if (!open) setPendingDelete(null);
        }}
        title={
          pendingDelete && pendingDelete.ids.length > 1
            ? "Delete tracks?"
            : "Delete track?"
        }
        description={pendingDelete?.description ?? ""}
        actionLabel={
          pendingDelete && pendingDelete.ids.length > 1
            ? `Delete ${pendingDelete.ids.length} tracks`
            : "Delete track"
        }
        onAction={confirmDelete}
      />
      <AddTracksDialog
        key={addTracksPendingId.current}
        open={addTracksPending !== null}
        conflicts={addTracksPending?.conflicts ?? []}
        onOpenChange={(open) => {
          if (!open) setAddTracksPending(null);
        }}
        onConfirm={confirmAddTracks}
      />
    </Theme>
  );
}

export default App;
