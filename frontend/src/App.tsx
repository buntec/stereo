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
import { type GridState } from "ag-grid-community";
import logger from "./logger.tsx";

import {
  ReloadIcon,
  PlusIcon,
  Cross2Icon,
  VideoIcon,
  MagicWandIcon,
  ResetIcon,
  EnterFullScreenIcon,
  ExitFullScreenIcon,
  TrashIcon,
  ShuffleIcon,
  DoubleArrowLeftIcon,
  DoubleArrowRightIcon,
  Crosshair2Icon,
  PlayIcon,
  TrackNextIcon,
  StopIcon,
  PauseIcon,
  TrackPreviousIcon,
} from "@radix-ui/react-icons";

import { Toast } from "radix-ui";

import {
  Slider,
  Tooltip,
  Theme,
  Skeleton,
  Kbd,
  IconButton,
  Box,
  Flex,
  Text,
} from "@radix-ui/themes";

import type {
  ITrack,
  ICollection,
  SearchKind,
  Action,
  ClientMsg,
  ServerMsg,
  Settings,
} from "./Types.tsx";

import { useWebSocket } from "./WebSocket.tsx";
import { useYTPlayer, type YTPlayerOptions } from "./YT.tsx";
import { formatDuration, useLocalStorage, shuffleArray } from "./Utils.tsx";
import { TracksGrid, SearchResultsGrid } from "./Grid.tsx";
import SearchBox from "./SearchBox.tsx";
import Rating from "./Rating.tsx";
import CollectionSelector from "./CollectionSelector.tsx";
import ImportDialog from "./ImportDialog.tsx";
import ExportDropDown from "./ExportDropDown.tsx";
import AppearanceSwitch from "./AppearanceSwitch.tsx";
import Notification from "./Notification.tsx";
import Title from "./Title.tsx";
import { useKeyboardActions } from "./Keyboard.tsx";

const wsUrl = `${window.location.protocol === "https:" ? "wss" : "ws"}://${window.location.host}/ws`;

const YTPlayerState = {
  UNSTARTED: -1,
  ENDED: 0,
  PLAYING: 1,
  PAUSED: 2,
  BUFFERING: 3,
  CUED: 5,
};

const defaultSettings: Settings = {
  appearance: "dark",
  collectionPath: "",
  useDefaultCollection: true,
  video: false,
  backgroundVideo: true,
  shufflePlay: false,
  fullScreen: false,
};

type State = {
  backend_version?: string;

  collection?: ICollection;
  default_collection?: ICollection;
  collection_is_valid?: boolean;
  collection_path_completions?: string[];
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
  search_kind: SearchKind;

  track_selection: string[];
  track_info?: ITrack;

  notification_msg?: string;
  notification_show: boolean;
  notification_kind?: "info" | "warn" | "warning" | "error";

  import_from?: string;
  import_from_keep_user_data?: boolean;
  import_from_is_valid?: boolean;
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
      return { ...state, backend_version: action.version };

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

    case "collection-path-completions":
      return { ...state, collection_path_completions: action.paths };

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

    case "default-collection":
      return { ...state, default_collection: action.collection };

    case "grid-refreshed":
      return { ...state, should_refresh_grid: false };

    case "grid-purged":
      return { ...state, should_purge_grid: false };

    case "search-box-input":
      return { ...state, search_box_input: action.input };

    case "selection-changed":
      return { ...state, track_selection: action.selected };

    case "search-results-selection-changed":
      return { ...state, search_results_selection: action.selected };

    case "clear-search-results":
      return { ...state, search_results: undefined };

    case "search-result":
      return {
        ...state,
        search_results:
          action.query_id === state.search_query_id
            ? [...(state.search_results ?? []), action.track]
            : [action.track],
        search_query_id: action.query_id,
      };

    case "search-results":
      return {
        ...state,
        search_results:
          action.query_id === state.search_query_id
            ? [...(state.search_results ?? []), ...action.tracks]
            : action.tracks,
        search_query_id: action.query_id,
      };

    case "set-search-failure":
      return {
        ...state,
        search_error_message: "no tracks founds",
        search_results: undefined,
        search_busy: false,
      };

    case "search-complete":
      if (!state.search_query_id || state.search_query_id === action.query_id) {
        return { ...state, search_busy: false };
      }
      return { ...state };

    case "set-search-busy":
      return { ...state, search_busy: true };

    case "set-search-limit":
      return { ...state, search_limit: action.limit };

    case "set-search-kind":
      return { ...state, search_kind: action.kind };

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

    case "notification":
      return {
        ...state,
        notification_kind: action.kind,
        notification_msg: action.message,
        notification_show: true,
      };

    case "rows":
    case "yt-anon-playlist":
    case "tracks":
    case "row-index":
    case "heartbeat":
    case "collection-contains-id-response":
    case "path-completions":
    case "validate-track-reply":
      return { ...state };

    default:
      throw new Error(`missing handler for action: ${JSON.stringify(action)}`);
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
    search_limit: 25,
    search_kind: "fuzzy",
    notification_show: false,
  });

  const [settings, setSettings] = useLocalStorage<Settings>(
    "stereo_app_settings",
    defaultSettings,
  );

  const [gridState, setGridState] = useLocalStorage<GridState | undefined>(
    "stereo_app_grid_state",
    undefined,
  );

  const [recentCollections, setRecentCollections] = useLocalStorage<string[]>(
    "stereo_app_recent_collections",
    [],
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

  const wsOnClose = useCallback(
    (ev: CloseEvent) =>
      dispatch({
        type: "notification",
        message: `WebSocket connection closed (${ev.reason}${ev.code})`,
        kind: "warn",
      }),
    [dispatch],
  );

  const { sendMsg, requestReply } = useWebSocket<ServerMsg, ClientMsg>(
    wsUrl,
    wsOnMessage,
    wsOnError,
    wsOnClose,
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

  const searchGridRef = useRef<AgGridReact<ITrack>>(null);

  const queryId = useRef<number>(0);

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
        if (event.data === window.YT.PlayerState.PLAYING) {
          const id = event.target.getVideoData().video_id;
          dispatch({
            type: "player-state-change",
            state: YTPlayerState.PLAYING,
            id,
          });
        }
        if (event.data === window.YT.PlayerState.ENDED) {
          const id = event.target.getVideoData().video_id;
          dispatch({
            type: "player-state-change",
            state: YTPlayerState.ENDED,
            id,
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

  useEffect(() => {
    logger.debug("state:", state);
  }, [state]);

  useEffect(() => {
    if (
      settings.useDefaultCollection &&
      state.default_collection &&
      !settings.collectionPath
    ) {
      setSettings({
        ...settings,
        collectionPath: state.default_collection.path,
      });
    }
  }, [
    state.default_collection,
    settings.useDefaultCollection,
    settings.collectionPath,
    settings,
    setSettings,
  ]);

  useEffect(() => {
    if (settings.collectionPath) {
      requestReply(
        { type: "set-collection", path: settings.collectionPath },
        (msg) => {
          if ("collection" in msg) {
            dispatch({ type: "collection-is-valid" });
          }
          if ("path_completions" in msg) {
            dispatch({
              type: "collection-path-completions",
              paths: msg.path_completions,
            });
          }
        },
      );
    }
  }, [settings.collectionPath, requestReply]);

  useEffect(() => {
    if (state.current_id && state.current_id !== state.track_info?.yt_id) {
      sendMsg({ type: "get-track-info", yt_id: state.current_id });
    }
  }, [state.current_id, state.track_info, sendMsg]);

  useEffect(() => {
    let t = null;
    const yt_id = state.player_id;

    if (yt_id && state.player_state === YTPlayerState.PLAYING) {
      if (player && playerIsReady) {
        const title = player.getVideoData().title;
        dispatch({ type: "set-title", title });

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
    if (state.current_id && player && playerIsReady) {
      const id = player?.getVideoData().video_id;
      if (id !== state.current_id) {
        player?.loadVideoById(state.current_id);
        player2?.loadVideoById(state.current_id);
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
    dispatch({ type: "clear-search-results" });

    let t = null;

    const query = state.search_box_input?.trim() ?? "";

    if (query.length > 0) {
      const query_id = ++queryId.current;
      t = setTimeout(() => {
        dispatch({ type: "set-search-busy" });
        sendMsg({
          type: "search",
          query,
          query_id,
          kind: state.search_kind,
          limit: state.search_limit,
        });
      }, 1000); // debounce - search is expensive!
    } else {
      sendMsg({
        type: "search-cancel-all",
      });
    }

    return () => {
      if (t !== null) {
        clearTimeout(t);
      }
    };
  }, [sendMsg, state.search_box_input, state.search_limit, state.search_kind]);

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

  useEffect(() => {
    if (
      state.collection &&
      state.collection_is_valid &&
      !recentCollections.includes(state.collection.path)
    ) {
      setRecentCollections([state.collection.path, ...recentCollections]);
    }
  }, [
    state.collection,
    state.collection_is_valid,
    recentCollections,
    setRecentCollections,
  ]);

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

  const refreshGrid = useCallback(() => {
    gridRef.current?.api.purgeInfiniteCache();
  }, []);

  const deselectAll = useCallback(() => {
    gridRef.current?.api.deselectAll();
  }, []);

  const deleteSelectedTracks = useCallback(() => {
    const selectedRows = gridRef.current?.api.getSelectedRows();
    if (selectedRows) {
      const ids = selectedRows.map((t) => t.yt_id);
      sendMsg({ type: "delete-tracks", ids });
      gridRef.current?.api.deselectAll();
    }
  }, [sendMsg]);

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
          window.open(r.url, "_blank");
        }
      });
    }
  }, [requestReply]);

  const addSelectedTracks = useCallback(
    (overwrite: boolean) => {
      if (
        state.search_results_selection &&
        state.search_results_selection.length > 0
      ) {
        sendMsg({
          type: "add-tracks",
          tracks: state.search_results_selection,
          overwrite_existing: overwrite,
        });
        searchGridRef.current?.api.deselectAll();
      }
    },
    [sendMsg, state.search_results_selection],
  );

  const updateCurrentRating = useCallback(
    (rating: number | null) => {
      if (state.current_id) {
        updateRating(state.current_id, rating);
      }
    },
    [updateRating, state.current_id],
  );

  const fastForward = useCallback(
    (seconds: number) => {
      if (player && playerIsReady && player2 && player2IsReady) {
        const t = player.getCurrentTime();
        player.seekTo(t + seconds, true);
        player2.seekTo(t + seconds, true);
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

  const centerGridAroundCurrentTrack = useCallback(() => {
    if (state.current_id && gridReady && gridRef.current) {
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
  }, [gridReady, requestReply, state.current_id]);

  const nextTrack = useCallback(
    () => dispatch({ type: "play-next-track" }),
    [dispatch],
  );

  const prevTrack = useCallback(
    () => dispatch({ type: "play-prev-track" }),
    [dispatch],
  );

  const onTrackGridStateUpdate = useCallback(
    (state: GridState) => {
      updatePlaylist(settings.shufflePlay); // update playlist according to sort/filter models
      setGridState(state); // save to local storage
    },
    [setGridState, updatePlaylist, settings.shufflePlay],
  );

  const toggleFullscreen = useCallback(() => {
    setSettings({ ...settings, fullScreen: !settings.fullScreen });
  }, [settings, setSettings]);

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
    <Theme appearance={settings.appearance} accentColor="gray" grayColor="gray">
      <Toast.Provider>
        <Flex
          className={`app-main ${settings.fullScreen ? "fullscreen" : ""}`}
          direction="column"
          align="center"
        >
          <Flex
            gapX="2"
            justify="between"
            wrap="wrap-reverse"
            className="toolbar"
          >
            <CollectionSelector
              isValid={state.collection_is_valid ?? true}
              createCollection={(path: string) =>
                sendMsg({ type: "create-collection", path })
              }
              resetToDefault={() => {
                if (state.default_collection) {
                  setSettings({
                    ...settings,
                    collectionPath: state.default_collection?.path,
                    useDefaultCollection: true,
                  });
                }
              }}
              value={settings.collectionPath}
              setValue={(value: string) =>
                setSettings({
                  ...settings,
                  collectionPath: value,
                  useDefaultCollection: false,
                })
              }
              suggestions={state.collection_path_completions ?? []}
              size={state.collection?.size}
              recentCollections={recentCollections}
              clearRecentCollections={() => setRecentCollections([])}
              removeRecentCollection={(coll) => {
                setRecentCollections(
                  recentCollections.filter((c) => c !== coll),
                );
              }}
            />
            <SearchBox
              value={state.search_box_input ?? ""}
              setValue={(value: string) =>
                dispatch({ type: "search-box-input", input: value })
              }
              searchLimit={state.search_limit}
              setSearchLimit={(limit: number) =>
                dispatch({ type: "set-search-limit", limit })
              }
              searchKind={state.search_kind}
              setSearchKind={(kind: SearchKind) =>
                dispatch({ type: "set-search-kind", kind })
              }
              searchBusy={!!state.search_box_input && state.search_busy}
              errorMessage={state.search_error_message}
            />

            <Flex align="center" m="2" gap="2">
              {state.backend_version ? (
                <Text size="2" className="version-string">
                  Stereo - {state.backend_version}
                </Text>
              ) : (
                <Skeleton>Stereo</Skeleton>
              )}
            </Flex>

            <AppearanceSwitch
              appearance={settings.appearance}
              setAppearance={(appearance) =>
                setSettings({ ...settings, appearance })
              }
            />
          </Flex>
          <div
            className={`background-player ${state.show_player2 && settings.backgroundVideo ? "" : "hidden"}`}
          >
            <div ref={player2Ref} />
          </div>
          <Title title={state.title ?? ""} />

          <Flex direction="column" align="center" gap="2" className="controls">
            <Rating
              enabled={state.current_id === state.track_info?.yt_id}
              currentRating={
                state.current_id === state.track_info?.yt_id
                  ? state.track_info?.rating
                  : undefined
              }
              updateRating={updateCurrentRating}
            />
            <Flex
              className="progress-slider"
              width="30%"
              align="center"
              gap="2"
            >
              <Text>{formatDuration(state.current_time ?? 0)}</Text>
              <Slider
                color="pink"
                size="2"
                variant="soft"
                value={[state.playback_progress_pct ?? 0]}
                onValueChange={onSliderValueChange}
              />
              <Text>
                {state.duration ? formatDuration(state.duration) : "--:--"}
              </Text>
            </Flex>
            <Flex direction="row" gap="2" m="2">
              <Tooltip
                content={
                  <>
                    Rewind 10 seconds <Kbd>←</Kbd>
                  </>
                }
              >
                <IconButton
                  variant="soft"
                  size="4"
                  onClick={() => fastForward(-10)}
                >
                  <DoubleArrowLeftIcon />
                </IconButton>
              </Tooltip>

              <Tooltip
                content={
                  <>
                    Skip to previous track <Kbd>↑</Kbd>
                  </>
                }
              >
                <IconButton variant="soft" size="4" onClick={prevTrack}>
                  <TrackPreviousIcon />
                </IconButton>
              </Tooltip>

              <Tooltip
                content={
                  <>
                    Start playback <Kbd>Space</Kbd> / <Kbd>p</Kbd>
                  </>
                }
              >
                <IconButton variant="soft" size="4" onClick={startPlayback}>
                  <PlayIcon />
                </IconButton>
              </Tooltip>

              <Tooltip
                content={
                  <>
                    Pause playback <Kbd>Space</Kbd> / <Kbd>p</Kbd>
                  </>
                }
              >
                <IconButton
                  variant="soft"
                  size="4"
                  onClick={() => pausePlayback()}
                >
                  <PauseIcon />
                </IconButton>
              </Tooltip>

              <Tooltip
                content={
                  <>
                    Stop playback <Kbd>Backspace</Kbd>
                  </>
                }
              >
                <IconButton variant="soft" size="4" onClick={stopPlayback}>
                  <StopIcon />
                </IconButton>
              </Tooltip>

              <Tooltip
                content={
                  <>
                    Skip to next track <Kbd>↓</Kbd>
                  </>
                }
              >
                <IconButton variant="soft" size="4" onClick={nextTrack}>
                  <TrackNextIcon />
                </IconButton>
              </Tooltip>

              <Tooltip
                content={
                  <>
                    Fast-forward 10 seconds <Kbd>→</Kbd>
                  </>
                }
              >
                <IconButton
                  variant="soft"
                  size="4"
                  onClick={() => fastForward(10)}
                >
                  <DoubleArrowRightIcon />
                </IconButton>
              </Tooltip>

              <Tooltip
                content={
                  <>
                    Toggle shuffle play <Kbd>s</Kbd>
                  </>
                }
              >
                <IconButton
                  color={settings.shufflePlay ? "cyan" : "gray"}
                  variant="soft"
                  size="4"
                  onClick={toggleShuffle}
                >
                  <ShuffleIcon />
                </IconButton>
              </Tooltip>

              <Tooltip
                content={
                  <>
                    Center grid around current track <Kbd>c</Kbd>
                  </>
                }
              >
                <IconButton
                  className="center-grid-around-current-track-button"
                  variant="soft"
                  size="4"
                  onClick={centerGridAroundCurrentTrack}
                >
                  <Crosshair2Icon />
                </IconButton>
              </Tooltip>

              <Tooltip
                content={
                  <>
                    Toggle video <Kbd>v</Kbd>
                  </>
                }
              >
                <IconButton
                  className="toggle-video-button"
                  color={settings.video ? "cyan" : "gray"}
                  variant="soft"
                  size="4"
                  onClick={toggleVideo}
                >
                  <VideoIcon />
                </IconButton>
              </Tooltip>

              <Tooltip
                content={
                  <>
                    Toggle background video <Kbd>b</Kbd>
                  </>
                }
              >
                <IconButton
                  color={settings.backgroundVideo ? "cyan" : "gray"}
                  variant="soft"
                  size="4"
                  onClick={toggleBackgroundVideo}
                >
                  <MagicWandIcon />
                </IconButton>
              </Tooltip>

              <Tooltip
                content={
                  <>
                    Toggle fullscreen mode <Kbd>f</Kbd>
                  </>
                }
              >
                <IconButton variant="soft" size="4" onClick={toggleFullscreen}>
                  {settings.fullScreen ? (
                    <ExitFullScreenIcon />
                  ) : (
                    <EnterFullScreenIcon />
                  )}
                </IconButton>
              </Tooltip>
            </Flex>
          </Flex>

          <div
            className={`player ${settings.video || settings.fullScreen ? "" : "hidden"}`}
          >
            <div ref={playerRef} />
          </div>
          {state.search_results && !!state.search_box_input ? (
            <Flex width="100%" gap="2" p="2">
              <Tooltip content="Add selected tracks (don't overwrite existing)">
                <IconButton
                  color="green"
                  onClick={() => addSelectedTracks(false)}
                  disabled={
                    !(
                      state.search_results_selection &&
                      state.search_results_selection.length > 0
                    )
                  }
                >
                  <PlusIcon />
                </IconButton>
              </Tooltip>
              <Tooltip content="Add selected tracks (overwrite existing)">
                <IconButton
                  color="orange"
                  onClick={() => addSelectedTracks(true)}
                  disabled={
                    !(
                      state.search_results_selection &&
                      state.search_results_selection.length > 0
                    )
                  }
                >
                  <PlusIcon />
                </IconButton>
              </Tooltip>
            </Flex>
          ) : (
            <Flex width="100%" gap="2" p="2" className="grid-controls">
              <Tooltip content="Deselect all">
                <IconButton
                  variant="soft"
                  onClick={deselectAll}
                  disabled={state.track_selection.length === 0}
                >
                  <Cross2Icon />
                </IconButton>
              </Tooltip>

              <Tooltip content="Delete selected tracks">
                <IconButton
                  color="red"
                  variant="soft"
                  onClick={deleteSelectedTracks}
                  disabled={state.track_selection.length === 0}
                >
                  <TrashIcon />
                </IconButton>
              </Tooltip>

              <ExportDropDown
                disabled={state.track_selection.length === 0}
                recentCollections={recentCollections}
                exportToCollection={exportSelectedTracksToCollection}
                exportToYTM={openSelectedTracksAsYTMPlaylist}
              />

              <ImportDialog
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
              />

              <Tooltip content="Reset columns">
                <IconButton variant="soft" onClick={resetColumnState}>
                  <ResetIcon />
                </IconButton>
              </Tooltip>

              <Tooltip content="Force refresh">
                <IconButton variant="soft" onClick={refreshGrid}>
                  <ReloadIcon />
                </IconButton>
              </Tooltip>
            </Flex>
          )}
          <Box className="grid-container">
            {state.search_results && !!state.search_box_input ? (
              <SearchResultsGrid
                gridRef={searchGridRef}
                currentId={state.current_id}
                playId={playId}
                requestReply={requestReply}
                dispatch={dispatch}
                tracks={state.search_results}
                lastCollectionUpdate={state.collection_last_update}
              />
            ) : (
              <TracksGrid
                gridRef={gridRef}
                currentId={state.current_id}
                playId={playId}
                updateRating={updateRating}
                requestReply={requestReply}
                dispatch={dispatch}
                sendMsg={sendMsg}
                setGridReady={setGridReady}
                initialState={gridState}
                onStateUpdate={onTrackGridStateUpdate}
              />
            )}
          </Box>
        </Flex>
        <Toast.Viewport className="ToastViewport" />
        <Notification
          open={state.notification_show}
          setOpen={(open: boolean) =>
            dispatch({ type: "set-notification-show", show: open })
          }
          message={state.notification_msg}
          kind={state.notification_kind}
        />
      </Toast.Provider>
    </Theme>
  );
}

export default App;
