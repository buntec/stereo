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
import { useMediaQuery } from "@astryxdesign/core/hooks";
import { LayerProvider } from "@astryxdesign/core/Layer";
import { AppShell } from "@astryxdesign/core/AppShell";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { Toolbar } from "@astryxdesign/core/Toolbar";
import { Button } from "@astryxdesign/core/Button";
import { Banner } from "@astryxdesign/core/Banner";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Spinner } from "@astryxdesign/core/Spinner";
import NewCollectionDialog from "./NewCollectionDialog";
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
  AppNotification,
} from "./Types.tsx";
import { isChartSearchKind, isQuerylessSearchKind } from "./Types.tsx";
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
import SpotifyPlaylistDialog, {
  SpotifyPlaylistProgressDialog,
  type SpotifyPlaylistTarget,
} from "./SpotifyPlaylistDialog.tsx";
import AddTracksDialog from "./AddTracksDialog.tsx";
import NavBarControls from "./NavBarControls.tsx";
import StereoLogo from "./StereoLogo.tsx";
import Notification from "./Notification.tsx";
import Title from "./Title.tsx";
import { useKeyboardActions } from "./Keyboard.tsx";
import NowPlayingMenu from "./NowPlayingMenu.tsx";
import CollectionSummary from "./CollectionSummary.tsx";
import NewReleasesDialog, {
  type NewReleasesOverrides,
} from "./NewReleasesDialog.tsx";
import GitSyncStatus from "./GitSyncStatus.tsx";
import { useTrackSelection } from "./TrackSelection.tsx";

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
  search_is_chart: boolean;
  search_query_id?: number;
  search_results?: ITrack[];
  search_results_selection?: ITrack[];
  search_error_message?: string;
  search_box_input?: string;
  search_limit: number;
  search_kind: SearchBoxKind;
  search_sources: TrackSource[];
  new_releases_days: number;

  track_info?: ITrack;

  notification_queue: AppNotification[];
  notification_next_id: number;

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

// Queues a toast; the Notification component shows and then consumes queued entries.
const notify = (
  state: State,
  message: string,
  kind?: AppNotification["kind"],
): State => ({
  ...state,
  notification_next_id: state.notification_next_id + 1,
  notification_queue: [
    ...state.notification_queue,
    { id: state.notification_next_id, message, kind },
  ],
});

const NO_SEARCH_SOURCES_MESSAGE =
  "Choose at least one source in Search options before searching.";

const isConnectionClosedMessage = (message: string) =>
  message.startsWith("WebSocket connection closed");

const reducer = (state: State, action: Action): State => {
  switch (action.type) {
    case "set-playlist": {
      if (
        (state.current_id || action.ids.length === 0) &&
        action.ids.length === state.playlist.length &&
        action.ids.every((id, index) => id === state.playlist[index])
      )
        return state;
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
      // Drop the previous source's validity; a fresh check-import-from follows.
      return {
        ...state,
        import_from: action.path,
        import_from_is_valid:
          action.path === state.import_from
            ? state.import_from_is_valid
            : undefined,
      };

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
        search_is_chart: false,
      };

    // Results and completions are only accepted for the search that is currently active
    // (set by "set-search-busy"); anything else is a straggler from a cancelled search.
    case "search-result": {
      if (action.query_id !== state.search_query_id) return state;
      const track = {
        ...action.track,
        source: action.source,
        chart_position: action.chart_position,
      };
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
        search_error_message:
          "No tracks found. Try another title, artist, or label.",
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
      return {
        ...state,
        search_busy: true,
        search_query_id: action.query_id,
        search_is_chart: !!action.is_chart,
      };

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

    case "consume-notifications":
      return {
        ...state,
        notification_queue: state.notification_queue.filter(
          (item) => !action.ids.includes(item.id),
        ),
      };

    case "dismiss-connection-closed-notification":
      // A shown connection toast is dismissed by Notification once the socket reconnects;
      // this drops one that has not been shown yet.
      if (
        !state.notification_queue.some((item) =>
          isConnectionClosedMessage(item.message),
        )
      )
        return state;
      return {
        ...state,
        notification_queue: state.notification_queue.filter(
          (item) => !isConnectionClosedMessage(item.message),
        ),
      };

    case "notification":
      return notify(state, action.message, action.kind);

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
    case "beatport-session":
    case "youtube-session":
    case "youtube-logins":
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
      return notify(
        { ...state, spotify_backfill_running: false },
        action.error_message ??
          (action.total === 0
            ? "No tracks needed a Spotify ID"
            : `Spotify backfill complete: ${action.matched}/${action.total} tracks matched`),
        action.error_message ? "error" : "info",
      );

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
      return notify(
        {
          ...state,
          spotify_playlist_running: false,
          spotify_playlist_open_url: action.url,
        },
        action.url
          ? `Created Spotify playlist with ${action.matched}/${action.total} matched tracks`
          : (action.error_message ?? "Failed to create Spotify playlist"),
        action.url ? "info" : "error",
      );

    case "clear-spotify-playlist-open-url":
      return { ...state, spotify_playlist_open_url: undefined };

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

    case "fixup-tracks-complete": {
      if (action.id !== state.youtube_fixup_id) return { ...state };
      const next = {
        ...state,
        youtube_fixup_running: false,
        youtube_fixup_current: action.total,
        youtube_fixup_matched: action.matched,
        youtube_fixup_total: action.total,
        youtube_fixup_tracks:
          action.tracks.length > 0 ? action.tracks : undefined,
      };
      return action.tracks.length > 0
        ? next
        : notify(next, "Playlist metadata lookup returned no tracks", "error");
    }

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
      return notify(
        {
          ...state,
          youtube_playlist_running: false,
          youtube_playlist_open_url: action.url,
        },
        action.url
          ? "Created YouTube playlist"
          : (action.error_message ?? "Failed to create YouTube playlist"),
        action.url ? "info" : "error",
      );

    case "clear-youtube-playlist-open-url":
      return { ...state, youtube_playlist_open_url: undefined };

    case "cancel-youtube-playlist":
      if (!state.youtube_playlist_running) return { ...state };
      return notify(
        {
          ...state,
          youtube_playlist_running: false,
          youtube_playlist_id: undefined,
        },
        action.reason === "timeout"
          ? "YouTube playlist export stalled — cancelled"
          : "Cancelled YouTube playlist export",
        action.reason === "timeout" ? "warning" : "info",
      );

    default:
      // Don't take down the whole UI over a message type this build doesn't know.
      logger.warn(`missing handler for action: ${JSON.stringify(action)}`);
      return state;
  }
};

// The tracks grid state saved before column sizing was reworked, minus its column widths:
// those were saved without flex and would pin every column to its old width.
const LEGACY_GRID_STATE_KEY = "stereo_app_grid_state";
const legacyGridState = (): GridState | undefined => {
  try {
    const item = window.localStorage.getItem(LEGACY_GRID_STATE_KEY);
    if (!item) return undefined;
    const state = JSON.parse(item) as GridState;
    delete state.columnSizing;
    return state;
  } catch {
    return undefined;
  }
};

function App() {
  const [state, dispatch] = useReducer(reducer, {
    playlist: [],
    show_player2: false,
    player_state: YTPlayerState.UNSTARTED,
    should_refresh_grid: false,
    should_purge_grid: false,
    search_busy: false,
    search_is_chart: false,
    search_limit: 100,
    search_kind: "fuzzy",
    search_sources: ["beatport", "volumo", "traxsource"],
    new_releases_days: 7,
    notification_queue: [],
    notification_next_id: 1,
    spotify_backfill_running: false,
    spotify_playlist_running: false,
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

  const reducedMotion = useMediaQuery("(prefers-reduced-motion: reduce)");
  const consumeNotifications = useCallback(
    (ids: number[]) => dispatch({ type: "consume-notifications", ids }),
    [],
  );
  const [createCollectionOpen, setCreateCollectionOpen] = useState(false);
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
    "stereo_app_grid_state_v2",
    legacyGridState,
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

  const {
    sendMsg,
    requestReply,
    isReady: backendReady,
  } = useWebSocket<ServerMsg, ClientMsg>(
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
  const trackSelection = useTrackSelection(gridRef, requestReply);
  const { getSelectedTracks } = trackSelection;

  // A selection only applies to the collection it was made in.
  const deselectTracks = trackSelection.deselectAll;
  const openCollectionPath = state.collection?.path;
  useEffect(() => {
    deselectTracks();
    setFilteredRowCount(undefined);
  }, [openCollectionPath, deselectTracks]);

  const updatePlaylist = useCallback(
    (shuffle: boolean) => {
      if (gridReady && gridRef.current?.api) {
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
  }, [state.playlist.length, updatePlaylist, settings.shufflePlay, gridReady]);

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
        gridRef.current.api?.refreshInfiniteCache();
        gridRef.current.api?.refreshCells({ force: true, suppressFlash: true });
      }
      dispatch({ type: "grid-refreshed" });
    }
  }, [state.should_refresh_grid]);

  useEffect(() => {
    if (state.should_purge_grid) {
      if (gridRef.current) {
        gridRef.current.api?.purgeInfiniteCache();
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

  // Only send set-collection after a repo has been selected (state.collections defined), and
  // only for a collection it has: the list changes before the selection does, e.g. when the
  // selected collection was deleted.
  useEffect(() => {
    if (
      settings.collectionPath &&
      state.collections?.some((c) => c.path === settings.collectionPath)
    ) {
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
    if (!isChartSearchKind(state.search_kind) && !state.search_sources.length)
      return;
    if (state.search_kind === "new-releases") {
      setNewReleasesOpen(true);
      return;
    }

    if (state.search_kind === "beatport-top-100") {
      const query_id = ++queryId.current;
      dispatch({ type: "clear-search-results" });
      dispatch({ type: "set-search-busy", query_id, is_chart: true });
      sendMsg({
        type: "search-beatport-top100",
        query_id,
        genre_id: beatportGenreId,
        limit: state.search_limit,
      });
      return;
    }

    if (state.search_kind === "traxsource-top-100") {
      const query_id = ++queryId.current;
      dispatch({ type: "clear-search-results" });
      dispatch({ type: "set-search-busy", query_id, is_chart: true });
      sendMsg({
        type: "search-traxsource-top100",
        query_id,
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
    gridRef.current?.api?.resetColumnState();
    gridRef.current?.api?.setFilterModel(null);
  }, []);

  // Deletes wait for confirmation in an AlertDialog.
  const [pendingDelete, setPendingDelete] = useState<{
    ids: string[];
    // The collection the ids came from: the server refuses the delete if another one is open.
    collection: string;
    description: string;
    // Deleting the playing track also skips to the next one.
    isNowPlaying: boolean;
  } | null>(null);

  const deleteSelectedTracks = useCallback(() => {
    if (!openCollectionPath) return;
    const collection = openCollectionPath;
    getSelectedTracks((selectedRows) => {
      if (selectedRows.length > 0) {
        const what =
          selectedRows.length === 1
            ? `“${selectedRows[0].title}”`
            : `${selectedRows.length} tracks`;
        setPendingDelete({
          ids: selectedRows.map((t) => t.yt_id),
          collection,
          description: `This removes ${what} from ${collectionLabel(collection)}, including ratings and play counts.`,
          isNowPlaying: false,
        });
      }
    });
  }, [openCollectionPath, getSelectedTracks]);

  const confirmDelete = useCallback(() => {
    if (!pendingDelete) return;
    if (pendingDelete.isNowPlaying) {
      dispatch({ type: "play-next-track" });
    } else {
      deselectTracks();
    }
    sendMsg({
      type: "delete-tracks",
      ids: pendingDelete.ids,
      collection: pendingDelete.collection,
    });
    setPendingDelete(null);
  }, [pendingDelete, sendMsg, deselectTracks]);

  const exportSelectedTracksToCollection = useCallback(
    (collection: string) => {
      getSelectedTracks((tracks) => {
        if (tracks.length > 0) {
          sendMsg({ type: "export-tracks-to-collection", tracks, collection });
        }
      });
    },
    [sendMsg, getSelectedTracks],
  );

  const openSelectedTracksAsYTMPlaylist = useCallback(() => {
    getSelectedTracks((selectedRows) => {
      const yt_ids = selectedRows.map((t) => t.yt_id);
      requestReply({ type: "create-yt-anon-playlist", yt_ids }, (r) => {
        if ("url" in r) {
          window.open(r.url ?? undefined, "_blank");
        }
      });
    });
  }, [requestReply, getSelectedTracks]);

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
    getSelectedTracks((selectedRows) => {
      if (selectedRows.length > 0) {
        setSpotifyPlaylistTracks(
          selectedRows.map((t: ITrack) => ({
            artist: t.artists.join(", "),
            title: t.title,
            sp_id: t.sp_id,
          })),
        );
        setSpotifyPlaylistOpen(true);
      }
    });
  }, [getSelectedTracks]);

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

  // Auto-cancel if the server goes quiet so the progress dialog never gets
  // stuck open indefinitely. Generous, since before the first batch the server
  // reads the YouTube login from the browser, which may wait on a keychain
  // prompt; every request after that times out on its own within 30s.
  useEffect(() => {
    if (!state.youtube_playlist_running) return;
    const timer = setTimeout(() => cancelYouTubePlaylist("timeout"), 120000);
    return () => clearTimeout(timer);
  }, [
    state.youtube_playlist_running,
    state.youtube_playlist_current,
    cancelYouTubePlaylist,
  ]);

  const openSelectedTracksAsYTSavedPlaylist = useCallback(() => {
    getSelectedTracks((selectedRows) => {
      if (selectedRows.length > 0) {
        setYtPlaylistYtIds(selectedRows.map((t) => t.yt_id));
        setYtPlaylistOpen(true);
      }
    });
  }, [getSelectedTracks]);

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
    if (gridReady && gridRef.current?.api) {
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
            gridRef.current?.api?.ensureIndexVisible(
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
      if (!state.search_sources.length) {
        dispatch({
          type: "notification",
          message: NO_SEARCH_SOURCES_MESSAGE,
          kind: "warning",
        });
        return;
      }
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
      if (!state.search_sources.length) {
        dispatch({
          type: "notification",
          message: NO_SEARCH_SOURCES_MESSAGE,
          kind: "warning",
        });
        return;
      }
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

  const importMenu = (
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
      repoCollections={(state.collections ?? []).filter(
        (c) => c.path !== state.collection?.path,
      )}
      requestReply={requestReply}
      onYouTubeImport={importYouTubePlaylist}
    />
  );

  const visibleTrackCount = filteredRowCount ?? state.collection?.size ?? 0;
  const canSelectAllTracks =
    !!state.collection &&
    visibleTrackCount > 0 &&
    trackSelection.selectionCount < visibleTrackCount;

  const collectionBody = isSearchGridActive ? (
    <SearchResultsGrid
      // Chart results get different columns and default sort; remounting applies them.
      key={state.search_is_chart ? "chart" : "search"}
      isChart={state.search_is_chart}
      isSearching={state.search_busy}
      onExitSearch={handleExitSearch}
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
  ) : !backendReady ? (
    <HStack height="100%" hAlign="center" vAlign="center">
      <Spinner label="Connecting to your collection…" />
    </HStack>
  ) : state.collections?.length === 0 ? (
    <EmptyState
      title="Create your first collection"
      description="A collection keeps track metadata and ratings in a local database file."
      actions={
        <Button
          label="Create collection"
          variant="primary"
          onClick={() => setCreateCollectionOpen(true)}
        />
      }
    />
  ) : !state.collection ? (
    <HStack height="100%" hAlign="center" vAlign="center">
      <Spinner label="Opening collection…" />
    </HStack>
  ) : state.collection.size === 0 ? (
    <EmptyState
      title="Your collection is ready"
      description="Search for music or import tracks to start curating."
      actions={
        <HStack gap={2} wrap="wrap">
          <Button
            label="Search for music"
            variant="primary"
            onClick={() =>
              document
                .querySelector<HTMLInputElement>(".search-box input")
                ?.focus()
            }
          />
          {importMenu}
        </HStack>
      }
    />
  ) : (
    <TracksGrid
      gridRef={gridRef}
      currentId={state.current_id}
      playId={playIdForGrid}
      updateRating={updateRating}
      requestReply={requestReply}
      sendMsg={sendMsg}
      setGridReady={setGridReady}
      initialState={gridState}
      onStateUpdate={onTrackGridStateUpdate}
      onRowCountChange={setFilteredRowCount}
      onSelectionChanged={trackSelection.onSelectionChanged}
      onRowsLoaded={trackSelection.selectLoadedRows}
      onFilterChanged={trackSelection.onFilterChanged}
      onSelectAll={trackSelection.selectAll}
      onArtistClick={handleArtistClick}
      onLabelClick={handleLabelClick}
      onShazamSimilar={handleShazamSimilar}
    />
  );

  const nowPlayingMenu = state.track_info && (
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
        if (state.current_id && state.track_info && openCollectionPath) {
          setPendingDelete({
            ids: [state.current_id],
            collection: openCollectionPath,
            description: `This removes “${state.track_info.title}” from ${collectionLabel(openCollectionPath)}, including its rating and play count.`,
            isNowPlaying: true,
          });
        }
      }}
    />
  );

  // Title and track menu: compact in the footer, large and centered above the video in
  // fullscreen.
  const nowPlaying = (expanded: boolean) =>
    state.title || state.track_info ? (
      <HStack
        width="100%"
        vAlign={expanded ? "start" : "center"}
        hAlign={expanded ? "center" : "start"}
        gap={2}
      >
        <StackItem className="now-playing-text">
          <Title
            title={state.title ?? state.track_info?.title ?? "Nothing playing"}
            expanded={expanded}
            artists={state.track_info?.artists}
            trackName={state.track_info?.title}
            label={state.track_info?.label}
            releaseDate={state.track_info?.release_date}
            onArtistClick={handleArtistClick}
            onLabelClick={handleLabelClick}
          />
        </StackItem>
        {nowPlayingMenu}
      </HStack>
    ) : (
      <Text type="supporting" color="secondary">
        Choose a track to start listening.
      </Text>
    );

  // Layout: header | toolbar | optional video stage (16:9, at most 35dvh tall) | grid
  // fills the rest | one-row transport footer (wraps on narrow screens). Fullscreen hides
  // the header, toolbar and grid and lets the video fill the stage under a large title.
  return (
    <Theme theme={stereoTheme} mode={settings.appearance}>
      <LayerProvider toast={{ position: "topEnd" }}>
        <AppShell
          ref={appShellRef}
          className={`stereo-app ${settings.fullScreen ? "fullscreen" : ""}`}
          variant="surface"
          height="fill"
          contentPadding={0}
          mobileNav={false}
          topNav={
            <HStack
              as="header"
              width="100%"
              padding={2}
              vAlign="center"
              gap={2}
              wrap="wrap"
              className="app-header"
            >
              <HStack vAlign="center" gap={2} className="collection-controls">
                <StereoLogo size={28} />
                <CollectionSelector
                  isValid={state.collection_is_valid ?? true}
                  collections={state.collections ?? []}
                  value={settings.collectionPath}
                  setValue={(path) =>
                    setSettings({ ...settings, collectionPath: path })
                  }
                  onCreate={(name) =>
                    sendMsg({ type: "create-collection", name })
                  }
                  onDelete={(path) =>
                    sendMsg({ type: "delete-collection", path })
                  }
                  onRename={(path, name) =>
                    sendMsg({ type: "rename-collection", path, name })
                  }
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
              <StackItem size="fill" className="header-search">
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
              </StackItem>
              <HStack vAlign="center" hAlign="end">
                <NavBarControls
                  version={state.backend_version}
                  appearance={settings.appearance}
                  setAppearance={(appearance) =>
                    setSettings({ ...settings, appearance })
                  }
                  isConnected={backendReady}
                  requestReply={requestReply}
                  collectionOpen={!!state.collection}
                  spotifyBackfillRunning={state.spotify_backfill_running}
                  spotifyBackfillChecked={state.spotify_backfill_checked}
                  spotifyBackfillMatched={state.spotify_backfill_matched}
                  spotifyBackfillTotal={state.spotify_backfill_total}
                  onSpotifyBackfill={backfillSpotifyIds}
                />
              </HStack>
            </HStack>
          }
          banner={
            !backendReady ? (
              <Banner
                container="section"
                status="warning"
                title="Connecting to Stereo"
                description="Your collection will be available when the connection is restored."
              />
            ) : state.collection_is_valid === false ? (
              <Banner
                container="section"
                status="error"
                title="This collection could not be opened"
                description="Select another collection or create a new one."
              />
            ) : undefined
          }
        >
          <Layout
            className="workbench"
            header={
              <Toolbar
                className="collection-toolbar"
                label={
                  isSearchGridActive
                    ? "Search results actions"
                    : "Collection actions"
                }
                size="sm"
                variant={
                  (
                    isSearchGridActive
                      ? state.search_results_selection?.length
                      : trackSelection.selectionCount
                  )
                    ? "muted"
                    : "transparent"
                }
                startContent={
                  <HStack width="100%" gap={2} vAlign="center" wrap="wrap">
                    {isSearchGridActive ? (
                      <>
                        <Text type="supporting">
                          {state.search_results?.length ?? 0} results ·{" "}
                          {state.search_results_selection?.length ?? 0} selected
                        </Text>
                        <Button
                          variant="primary"
                          label={`Add selected (${state.search_results_selection?.length ?? 0})`}
                          icon={<PlusIcon />}
                          clickAction={addSelectedTracks}
                          isDisabled={
                            !state.search_results_selection?.length ||
                            !state.collection
                          }
                        />
                        <Button
                          label="Exit search"
                          variant="ghost"
                          icon={<Cross2Icon />}
                          onClick={handleExitSearch}
                        />
                      </>
                    ) : (
                      <>
                        <Text type="supporting">
                          {visibleTrackCount}
                          {filteredRowCount !== undefined &&
                          state.collection &&
                          filteredRowCount !== state.collection.size
                            ? ` / ${state.collection.size}`
                            : ""}{" "}
                          tracks
                        </Text>
                        {canSelectAllTracks && (
                          <Tooltip
                            content={
                              <>
                                Select all tracks matching the column filters{" "}
                                <Kbd keys="mod+a" />
                              </>
                            }
                          >
                            <Button
                              label="Select all"
                              variant="ghost"
                              onClick={trackSelection.selectAll}
                            />
                          </Tooltip>
                        )}
                        {trackSelection.selectionCount > 0 && (
                          <>
                            <Text type="supporting" weight="bold">
                              {trackSelection.selectionCount} selected
                            </Text>
                            <Button
                              label="Deselect all"
                              variant="ghost"
                              onClick={trackSelection.deselectAll}
                            />
                            <Button
                              label="Delete selected"
                              variant="destructive"
                              icon={<TrashIcon />}
                              onClick={deleteSelectedTracks}
                            />
                          </>
                        )}
                        <ExportDropDown
                          selectionCount={trackSelection.selectionCount}
                          collections={state.collections ?? []}
                          exportToCollection={exportSelectedTracksToCollection}
                          exportSelectionToYTM={openSelectedTracksAsYTMPlaylist}
                          exportSelectionToYTSavedPlaylist={
                            openSelectedTracksAsYTSavedPlaylist
                          }
                          exportTop50ToYTM={openTopTracksAsYTMPlaylist}
                          exportAllToYTSavedPlaylist={
                            openAllTracksAsYTSavedPlaylist
                          }
                          exportSelectionToSpotifyPlaylist={
                            exportSelectionToSpotifyPlaylist
                          }
                          exportAllToSpotifyPlaylist={
                            exportAllToSpotifyPlaylist
                          }
                        />
                        {importMenu}
                        {state.collection && state.collection.size > 0 && (
                          <ColumnsMenu
                            gridRef={gridRef}
                            onReset={resetColumnState}
                          />
                        )}
                      </>
                    )}
                  </HStack>
                }
              />
            }
            content={
              <LayoutContent
                className="collection-content"
                padding={0}
                isScrollable={false}
              >
                <VStack width="100%" height="100%">
                  {settings.fullScreen ? (
                    <HStack
                      key="fullscreen-now-playing"
                      width="100%"
                      padding={4}
                      hAlign="center"
                      className="fullscreen-now-playing"
                    >
                      {nowPlaying(true)}
                    </HStack>
                  ) : null}
                  {/* Rendered unconditionally and always at this position: YouTube swaps the
                      mount node for its iframe, and re-parenting an iframe reloads it. */}
                  <HStack
                    key="player-stage"
                    width="100%"
                    padding={2}
                    hAlign="center"
                    vAlign="center"
                    className={`player-stage ${settings.video || settings.fullScreen ? "" : "hidden"}`}
                  >
                    <VStack className="player">
                      <div ref={playerRef} />
                    </VStack>
                  </HStack>
                  <StackItem
                    key="collection-body"
                    size="fill"
                    className="collection-body"
                  >
                    <VStack width="100%" height="100%">
                      {collectionBody}
                    </VStack>
                  </StackItem>
                </VStack>
              </LayoutContent>
            }
            footer={
              <LayoutFooter padding={2} hasDivider className="playback-footer">
                <HStack
                  width="100%"
                  gap={4}
                  vAlign="center"
                  wrap="wrap"
                  className="playback-bar"
                >
                  <StackItem className="playback-now-playing">
                    {settings.fullScreen ? null : nowPlaying(false)}
                  </StackItem>
                  <StackItem className="playback-transport">
                    <VStack width="100%" hAlign="center" gap={1}>
                      <HStack vAlign="center" hAlign="center" gap={1}>
                        <Tooltip
                          content={
                            <>
                              Skip to previous track <Kbd keys="up" />
                            </>
                          }
                        >
                          <IconButton
                            variant="secondary"
                            size="sm"
                            label="Skip to previous track"
                            icon={<TrackPreviousIcon />}
                            onClick={prevTrack}
                          />
                        </Tooltip>

                        <Tooltip
                          content={
                            <>
                              Play / pause <Kbd keys="Space" /> /{" "}
                              <Kbd keys="p" />
                            </>
                          }
                        >
                          <IconButton
                            variant="primary"
                            size="md"
                            className="play-pause-button"
                            label={
                              state.player_state === YTPlayerState.PLAYING
                                ? "Pause"
                                : "Play"
                            }
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
                            size="sm"
                            label="Skip to next track"
                            icon={<TrackNextIcon />}
                            onClick={nextTrack}
                          />
                        </Tooltip>
                      </HStack>
                      <HStack
                        className="progress-slider"
                        width="100%"
                        vAlign="center"
                        gap={2}
                      >
                        <Text type="supporting" hasTabularNumbers>
                          {formatDuration(state.current_time ?? 0)}
                        </Text>
                        <StackItem size="fill">
                          <Slider
                            label="Playback progress"
                            isLabelHidden
                            valueDisplay="none"
                            value={state.playback_progress_pct ?? 0}
                            onChange={(v: number) => onSliderValueChange([v])}
                            isDisabled={!state.duration}
                            style={{ paddingInline: "var(--spacing-3)" }}
                          />
                        </StackItem>
                        <Text type="supporting" hasTabularNumbers>
                          {state.duration
                            ? formatDuration(state.duration)
                            : "--:--"}
                        </Text>
                      </HStack>
                    </VStack>
                  </StackItem>
                  <StackItem className="playback-options">
                    <HStack vAlign="center" hAlign="end" gap={1} wrap="wrap">
                      <Rating
                        label="Rating for the playing track"
                        enabled={Boolean(
                          state.current_id &&
                          state.current_id === state.track_info?.yt_id,
                        )}
                        currentRating={
                          state.current_id === state.track_info?.yt_id
                            ? state.track_info?.rating
                            : undefined
                        }
                        updateRating={updateCurrentRating}
                      />

                      <Divider orientation="vertical" />

                      <Tooltip
                        content={
                          <>
                            Toggle shuffle play <Kbd keys="s" />
                          </>
                        }
                      >
                        <IconButton
                          variant={settings.shufflePlay ? "primary" : "ghost"}
                          size="sm"
                          label="Toggle shuffle play"
                          aria-pressed={settings.shufflePlay}
                          icon={<ShuffleIcon />}
                          onClick={toggleShuffle}
                        />
                      </Tooltip>

                      <Tooltip
                        content={
                          reducedMotion ? (
                            "Background video is disabled by your reduced motion preference"
                          ) : (
                            <>
                              Toggle background video <Kbd keys="b" />
                            </>
                          )
                        }
                      >
                        <IconButton
                          variant={
                            settings.backgroundVideo && !reducedMotion
                              ? "primary"
                              : "ghost"
                          }
                          size="sm"
                          label="Toggle background video"
                          aria-pressed={
                            settings.backgroundVideo && !reducedMotion
                          }
                          isDisabled={reducedMotion}
                          icon={<MagicWandIcon />}
                          onClick={toggleBackgroundVideo}
                        />
                      </Tooltip>

                      <Tooltip
                        content={
                          <>
                            Toggle video <Kbd keys="v" />
                          </>
                        }
                      >
                        <IconButton
                          className="toggle-video-button"
                          variant={settings.video ? "primary" : "ghost"}
                          size="sm"
                          label="Toggle video"
                          aria-pressed={settings.video}
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
                          variant={settings.fullScreen ? "primary" : "ghost"}
                          size="sm"
                          label="Toggle fullscreen mode"
                          aria-pressed={settings.fullScreen}
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
                  </StackItem>
                </HStack>
              </LayoutFooter>
            }
          />
          <VStack
            className={`background-player ${state.show_player2 && settings.backgroundVideo && !reducedMotion ? "" : "hidden"}`}
          >
            {/* YouTube replaces this DOM mount node. */}
            <div ref={player2Ref} />
          </VStack>
          <Notification
            notifications={state.notification_queue}
            onConsumed={consumeNotifications}
            isConnected={backendReady}
          />
          <NewCollectionDialog
            isOpen={createCollectionOpen}
            onOpenChange={setCreateCollectionOpen}
            onCreate={(name) => sendMsg({ type: "create-collection", name })}
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
        </AppShell>
      </LayerProvider>
    </Theme>
  );
}

export default App;
