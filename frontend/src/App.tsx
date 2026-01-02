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
import { type IRowNode } from "ag-grid-community";

import {
  PlusCircledIcon,
  Cross2Icon,
  VideoIcon,
  ResetIcon,
  TrashIcon,
  OpenInNewWindowIcon,
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
  IconButton,
  Box,
  Flex,
  Text,
  Code,
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
import { useYouTube, type YouTubeOptions } from "./YT.tsx";
import { formatDuration, useLocalStorage } from "./Utils.tsx";
import { TracksGrid, SearchResultsGrid } from "./Grid.tsx";
import SearchBox from "./SearchBox.tsx";
import Rating from "./Rating.tsx";
import CollectionSelector from "./CollectionSelector.tsx";
import ImportDialog from "./ImportDialog.tsx";
import AppearanceSwitch from "./AppearanceSwitch.tsx";
import Notification from "./Notification.tsx";
import Title from "./Title.tsx";

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
};

type State = {
  backend_version?: string;

  collection?: ICollection;
  default_collection?: ICollection;
  collection_is_valid?: boolean;
  collection_path_completions?: string[];
  collection_last_update?: number;

  current_id?: string;
  duration?: number;
  current_time?: number;
  title?: string;
  playlist: string[];
  queue: string[];
  player_state: number;
  playback_progress_pct?: number;
  shuffle_play: boolean;
  show_player: boolean;
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
    case "play-ids":
      return { ...state, playlist: action.ids };

    case "cue-ids":
      return { ...state, queue: action.ids };

    case "player-state-change":
      return { ...state, player_state: action.state, current_id: action.id };

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

    case "toggle-shuffle":
      return { ...state, shuffle_play: !state.shuffle_play };

    case "toggle-show-player":
      return { ...state, show_player: !state.show_player };

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
    case "track-not-found":
    case "track-found":
    case "tracks":
    case "play-id":
    case "heartbeat":
    case "collection-contains-id-response":
    case "path-completions":
    case "validate-track-reply":
    case "pong":
      return { ...state };

    default:
      throw new Error(`missing handler for action: ${JSON.stringify(action)}`);
  }
};

function App() {
  const [_, setWsError] = useState<boolean>(false);

  const [state, dispatch] = useReducer(reducer, {
    current_id: "",
    playlist: [],
    queue: [],
    track_selection: [],
    show_player: false,
    show_player2: false,
    shuffle_play: false,
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

  const { sendMsg, requestReply } = useWebSocket<ServerMsg, ClientMsg>(
    wsUrl,
    (msg: ServerMsg | ServerMsg[]) => {
      if (Array.isArray(msg)) {
        msg.forEach(dispatch);
      } else {
        dispatch(msg);
      }
    },
    () => setWsError(true),
    () => setWsError(true),
  );

  const playIds = useCallback(
    (ids: string[]) => dispatch({ type: "play-ids", ids }),
    [dispatch],
  );

  const cueIds = useCallback(
    (ids: string[]) => dispatch({ type: "cue-ids", ids }),
    [dispatch],
  );

  const gridRef = useRef<AgGridReact<ITrack>>(null);
  const [gridReady, setGridReady] = useState(false);

  useEffect(() => {
    if (
      gridReady &&
      state.playlist.length === 0 &&
      state.queue.length === 0 &&
      gridRef.current
    ) {
      const api = gridRef.current.api;
      const filterModel = api.getState().filter?.filterModel;
      const sortModel = api.getState().sort?.sortModel;
      requestReply(
        { type: "get-rows", startRow: 0, endRow: 200, filterModel, sortModel },
        function (msg: ServerMsg | { type: string }) {
          if ("rows" in msg) {
            cueIds(msg.rows.map((t: ITrack) => t.yt_id));
          } else {
            console.warn("failed to get rows");
          }
        },
      );
    }
  }, [
    gridReady,
    gridRef.current,
    cueIds,
    requestReply,
    state.playlist,
    state.queue,
  ]);

  const searchGridRef = useRef<AgGridReact<ITrack>>(null);

  const queryId = useRef<number>(0);

  useEffect(() => {
    if (state.should_refresh_grid && gridRef.current) {
      gridRef.current.api.refreshInfiniteCache();
      dispatch({ type: "grid-refreshed" });
    }
  }, [state.should_refresh_grid]);

  useEffect(() => {
    if (state.should_purge_grid && gridRef.current) {
      gridRef.current.api.purgeInfiniteCache();
      dispatch({ type: "grid-purged" });
    }
  }, [state.should_purge_grid]);

  const playerOptions: YouTubeOptions = useMemo(() => {
    return {
      playerVars: {
        autoplay: 1,
        modestbranding: 1,
        color: "white",
        controls: 0,
        rel: 0,
        playsinline: 1,
        enablejsapi: 1,
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
        }
      },
    };
  }, []);

  const {
    containerRef: playerRef,
    player,
    isReady: playerIsReady,
  } = useYouTube(playerOptions);

  const player2Options: YouTubeOptions = useMemo(() => {
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
  } = useYouTube(player2Options);

  // useEffect(() => { console.log(state); }, [state]);

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
  }, [settings.collectionPath]);

  useEffect(() => {
    if (state.current_id) {
      sendMsg({ type: "get-track-info", yt_id: state.current_id });
    }
  }, [state.current_id]);

  useEffect(() => {
    let t = null;
    let yt_id = state.current_id;

    if (yt_id && state.player_state === YTPlayerState.PLAYING) {
      const title = player?.getVideoData().title;
      dispatch({ type: "set-title", title });
      // 1 minute of continuous playback triggers play count increase
      t = setTimeout(() => sendMsg({ type: "inc-play-count", yt_id }), 60000);
    }

    if (state.player_state === YTPlayerState.ENDED) {
      dispatch({ type: "clear-title" });
    }

    return () => {
      if (t !== null) {
        clearTimeout(t);
      }
    };
  }, [state.player_state, state.current_id]);

  useEffect(() => {
    if (state.current_id) {
      player2?.loadVideoById(state.current_id);
    }
  }, [state.current_id]);

  useEffect(() => {
    if (state.playlist && player && playerIsReady) {
      player.loadPlaylist(state.playlist);
    }
  }, [state.playlist, player, playerIsReady]);

  useEffect(() => {
    if (
      state.queue.length > 0 &&
      player &&
      playerIsReady &&
      player2 &&
      player2IsReady
    ) {
      player.cuePlaylist(state.queue);
      player2.cuePlaylist(state.queue);
    }
  }, [state.queue, player, playerIsReady, player2, player2IsReady]);

  useEffect(() => {
    searchGridRef.current?.api.refreshCells({
      force: true,
      suppressFlash: true,
    });
  }, [state.collection_last_update]);

  useEffect(() => {
    const i = setInterval(() => {
      const t = Math.max(0.0, player?.getCurrentTime() ?? 0.0);
      const d = Math.max(1.0, player?.getDuration() ?? 1.0);
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
    }, 1000);

    return () => {
      clearInterval(i);
    };
  }, [player, dispatch]);

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
  }, [state.search_box_input, state.search_limit, state.search_kind]);

  useEffect(() => {
    if (player) {
      player.setShuffle(state.shuffle_play);
    }
  }, [state.shuffle_play]);

  useEffect(() => {
    if (state.import_from) {
      sendMsg({ type: "check-import-from", path: state.import_from });
    }
  }, [state.import_from]);

  const updateRating = useCallback(
    (yt_id: string, rating: number | null) => {
      sendMsg({ type: "update-rating", yt_id, rating });
    },
    [sendMsg],
  );

  const onSliderValueChange = useCallback(
    (value: number[]) => {
      if (player) {
        const d = player.getDuration();
        const t = player.getCurrentTime();
        if (d) {
          player.seekTo((d * value[0]) / 100.0, true);
          player2?.seekTo((d * value[0]) / 100.0, true);
        }
        dispatch({
          type: "set-playback-progress",
          percent: value[0],
          duration: d,
          current_time: t,
        });
      }
    },
    [player, player2],
  );

  const resetColumnState = useCallback(() => {
    gridRef.current?.api.resetColumnState();
    gridRef.current?.api.setFilterModel(null);
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
  }, [sendMsg]);

  const addSelectedTracks = useCallback(() => {
    if (
      state.search_results_selection &&
      state.search_results_selection.length > 0
    ) {
      sendMsg({
        type: "add-tracks",
        tracks: state.search_results_selection,
        overwrite_existing: false,
      });
      searchGridRef.current?.api.deselectAll();
    }
  }, [sendMsg, state.search_results_selection]);

  const updateCurrentRating = useCallback(
    (rating: number | null) => {
      if (state.current_id) {
        updateRating(state.current_id, rating);
      }
    },
    [updateRating, state.current_id],
  );

  return (
    <Theme appearance={settings.appearance} accentColor="gray" grayColor="gray">
      <Toast.Provider>
        <Flex className="app-main" direction="column" align="center">
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
                <Code>Stereo - {state.backend_version}</Code>
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
            className={`background-player ${state.show_player2 ? "" : "hidden"}`}
          >
            <div ref={player2Ref} />
          </div>
          <Title title={state.title ?? ""} />
          <Rating
            enabled={state.current_id === state.track_info?.yt_id}
            currentRating={
              state.current_id === state.track_info?.yt_id
                ? state.track_info?.rating
                : undefined
            }
            updateRating={updateCurrentRating}
          />
          <Flex className="progress-slider" width="30%" align="center" gap="2">
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
            <Tooltip content="Rewind 10 seconds">
              <IconButton
                variant="soft"
                size="4"
                onClick={() => {
                  if (player) {
                    const t = player.getCurrentTime();
                    player.seekTo(t - 10, true);
                  }
                }}
              >
                <DoubleArrowLeftIcon />
              </IconButton>
            </Tooltip>
            <Tooltip content="Skip to previous track">
              <IconButton
                variant="soft"
                size="4"
                onClick={() => player?.previousVideo()}
              >
                <TrackPreviousIcon />
              </IconButton>
            </Tooltip>
            <Tooltip content="Start playback">
              <IconButton
                variant="soft"
                size="4"
                onClick={() => {
                  player?.playVideo();
                  player2?.playVideo();
                }}
              >
                <PlayIcon />
              </IconButton>
            </Tooltip>
            <Tooltip content="Pause playback">
              <IconButton
                variant="soft"
                size="4"
                onClick={() => {
                  player?.pauseVideo();
                  player2?.pauseVideo();
                }}
              >
                <PauseIcon />
              </IconButton>
            </Tooltip>
            <Tooltip content="Stop playback">
              <IconButton
                variant="soft"
                size="4"
                onClick={() => {
                  player?.stopVideo();
                  player2?.stopVideo();
                }}
              >
                <StopIcon />
              </IconButton>
            </Tooltip>
            <Tooltip content="Skip to next track">
              <IconButton
                variant="soft"
                size="4"
                onClick={() => player?.nextVideo()}
              >
                <TrackNextIcon />
              </IconButton>
            </Tooltip>
            <Tooltip content="Fast forward 10 seconds">
              <IconButton
                variant="soft"
                size="4"
                onClick={() => {
                  if (player) {
                    const t = player.getCurrentTime();
                    player.seekTo(t + 10, true);
                  }
                }}
              >
                <DoubleArrowRightIcon />
              </IconButton>
            </Tooltip>
            <Tooltip content="Toggle shuffle play">
              <IconButton
                color={state.shuffle_play ? "cyan" : "gray"}
                variant="soft"
                size="4"
                onClick={() => dispatch({ type: "toggle-shuffle" })}
              >
                <ShuffleIcon />
              </IconButton>
            </Tooltip>
            <Tooltip content="Center grid around current track">
              <IconButton
                variant="soft"
                size="4"
                onClick={() =>
                  gridRef.current?.api.ensureNodeVisible(
                    (row: IRowNode<ITrack>) =>
                      row.data?.yt_id === state.current_id,
                    "middle",
                  )
                }
              >
                <Crosshair2Icon />
              </IconButton>
            </Tooltip>
            <Tooltip content="Toggle video">
              <IconButton
                color={state.show_player ? "cyan" : "gray"}
                variant="soft"
                size="4"
                onClick={() => dispatch({ type: "toggle-show-player" })}
              >
                <VideoIcon />
              </IconButton>
            </Tooltip>
          </Flex>
          <div className={`player ${state.show_player ? "" : "hidden"}`}>
            <div ref={playerRef} />
          </div>
          {state.search_results && !!state.search_box_input ? (
            <Flex width="100%" gap="2" px="2">
              <Tooltip content="Add selected tracks">
                <IconButton
                  color="green"
                  onClick={addSelectedTracks}
                  disabled={
                    !(
                      state.search_results_selection &&
                      state.search_results_selection.length > 0
                    )
                  }
                >
                  <PlusCircledIcon />
                </IconButton>
              </Tooltip>
            </Flex>
          ) : (
            <Flex width="100%" gap="2" p="2">
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
              <Tooltip content="Open selection as YouTube Music playlist (max 50)">
                <IconButton
                  variant="soft"
                  onClick={openSelectedTracksAsYTMPlaylist}
                  disabled={state.track_selection.length === 0}
                >
                  <OpenInNewWindowIcon />
                </IconButton>
              </Tooltip>
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
                isValidImportFrom={state.import_from_is_valid ?? false}
              />
              <Tooltip content="Reset column state">
                <IconButton variant="soft" onClick={resetColumnState}>
                  <ResetIcon />
                </IconButton>
              </Tooltip>
            </Flex>
          )}
          <Box className="grid-container">
            {state.search_results && !!state.search_box_input ? (
              <SearchResultsGrid
                gridRef={searchGridRef}
                currentId={state.current_id}
                playIds={playIds}
                requestReply={requestReply}
                dispatch={dispatch}
                tracks={state.search_results}
                lastCollectionUpdate={state.collection_last_update}
              />
            ) : (
              <TracksGrid
                gridRef={gridRef}
                currentId={state.current_id}
                playIds={playIds}
                updateRating={updateRating}
                requestReply={requestReply}
                dispatch={dispatch}
                sendMsg={sendMsg}
                setGridReady={setGridReady}
              />
            )}
          </Box>
          <Notification
            open={state.notification_show}
            setOpen={(open: boolean) =>
              dispatch({ type: "set-notification-show", show: open })
            }
            message={state.notification_msg}
            kind={state.notification_kind}
          />
          <Toast.Viewport className="ToastViewport" />
        </Flex>
      </Toast.Provider>
    </Theme>
  );
}

export default App;
