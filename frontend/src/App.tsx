import {
  useState,
  useReducer,
  useEffect,
  useRef,
  useCallback,
  useMemo,
} from "react";
import "./App.css";
import { useWebSocket } from "./WebSocket.tsx";
import { useYouTube, type YouTubeOptions } from "./YT.tsx";
import {
  ExclamationTriangleIcon,
  PlusCircledIcon,
  InfoCircledIcon,
  VideoIcon,
  ShuffleIcon,
  DoubleArrowLeftIcon,
  DoubleArrowRightIcon,
  MoonIcon,
  Crosshair2Icon,
  PlayIcon,
  TrackNextIcon,
  StopIcon,
  PauseIcon,
  TrackPreviousIcon,
} from "@radix-ui/react-icons";
import { Toast } from "radix-ui";

import { AgGridReact } from "ag-grid-react";
import { type IRowNode } from "ag-grid-community";

import {
  Callout,
  Slider,
  Tooltip,
  Theme,
  Switch,
  Button,
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
  AppearanceType,
  Settings,
} from "./Types.tsx";
import { TracksGrid, SearchResultsGrid } from "./Grid.tsx";
import { SearchBox } from "./SearchBox.tsx";
import { formatDuration, useLocalStorage } from "./Utils.tsx";
import { Rating } from "./Rating.tsx";
import { CollectionSelector } from "./CollectionSelector.tsx";

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
  current_id?: string;
  duration?: number;
  current_time?: number;
  title?: string;
  playlist: string[];
  player_state: number;
  playback_progress_pct?: number;
  shuffle_play: boolean;
  show_player: boolean;
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

  collection_is_valid?: boolean;
  collection_path_completions?: string[];
  collection_last_update?: number;

  track_selection: string[];
  track_info?: ITrack;

  notification_msg?: string;
  notification_show: boolean;
  notification_kind?: "info" | "warn" | "warning" | "error";
};

const reducer = (state: State, action: Action): State => {
  switch (action.type) {
    case "play-ids":
      return { ...state, playlist: action.ids };
    case "player-state-change":
      return { ...state, player_state: action.state, current_id: action.id };
    case "heartbeat":
      return { ...state };
    case "backend-info":
      return { ...state, backend_version: action.version };
    case "set-title":
      return { ...state, title: action.title };
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
    case "tracks":
      return { ...state };
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
    case "collection-contains-id-response":
      return { ...state };

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
      if (action.query_id === state.search_query_id) {
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
    case "rows":
      return { ...state };
    case "toggle-shuffle":
      return { ...state, shuffle_play: !state.shuffle_play };
    case "toggle-show-player":
      return { ...state, show_player: !state.show_player };
    case "track-info":
      return { ...state, track_info: action.track };
    case "track-found":
      return { ...state };
    case "set-notification-show":
      return { ...state, notification_show: action.show };
    case "notification":
      return {
        ...state,
        notification_kind: action.kind,
        notification_msg: action.message,
        notification_show: true,
      };
  }

  throw new Error(`should be unreachable - action: ${JSON.stringify(action)}`);
};

type AppearanceSwitchProps = {
  appearance: AppearanceType;
  setAppearance: (t: AppearanceType) => void;
};

function AppearanceSwitch({
  appearance,
  setAppearance,
}: AppearanceSwitchProps) {
  return (
    <Box m="4">
      <Text as="label" size="2">
        <Flex gap="2" align="center">
          <Switch
            color="gold"
            size="1"
            checked={appearance === "dark"}
            onCheckedChange={(checked) =>
              setAppearance(checked ? "dark" : "light")
            }
          />{" "}
          <MoonIcon />
        </Flex>
      </Text>
    </Box>
  );
}

function Title({ title }: { title: string }) {
  return (
    <Box className="title" m="4">
      <Text size="5" weight="light">
        {" "}
        {title}{" "}
      </Text>
    </Box>
  );
}

type NotificationProps = {
  open: boolean;
  setOpen: (open: boolean) => void;
  message?: string;
  kind?: "info" | "warn" | "warning" | "error";
};

function Notification({ open, setOpen, message, kind }: NotificationProps) {
  const color = useMemo(() => {
    switch (kind) {
      case "info":
        return "blue";
      case "warn":
      case "warning":
        return "orange";
      case "error":
        return "red";
    }
  }, [kind]);

  const icon = useMemo(() => {
    switch (kind) {
      case "info":
        return <InfoCircledIcon />;
      case "warn":
      case "warning":
      case "error":
        return <ExclamationTriangleIcon />;
    }
  }, [kind]);

  return (
    <Toast.Root className="ToastRoot" open={open} onOpenChange={setOpen}>
      <Toast.Description asChild>
        <Callout.Root variant="soft" color={color}>
          <Callout.Icon>{icon}</Callout.Icon>
          <Callout.Text>{message}</Callout.Text>
        </Callout.Root>
      </Toast.Description>
    </Toast.Root>
  );
}

function App() {
  const [_, setWsError] = useState<boolean>(false);

  const [state, dispatch] = useReducer(reducer, {
    current_id: "",
    playlist: [],
    track_selection: [],
    show_player: false,
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

  const gridRef = useRef<AgGridReact<ITrack>>(null);
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
          console.log(`Playing ${id}`);
          dispatch({
            type: "player-state-change",
            state: YTPlayerState.PLAYING,
            id,
          });
        }
        if (event.data === window.YT.PlayerState.ENDED) {
          const id = event.target.getVideoData().video_id;
          console.log(`${id} ended`);
          dispatch({
            type: "player-state-change",
            state: YTPlayerState.ENDED,
            id,
          });
        }
      },
    };
  }, []);

  const { containerRef: playerRef, player } = useYouTube(playerOptions);

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
    };
  }, []);

  const { containerRef: player2Ref, player: player2 } =
    useYouTube(player2Options);

  useEffect(() => {
    console.log(state);
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
      // 1 minute of continuous playback trigger play count increase
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
    player?.loadPlaylist(state.playlist);
  }, [state.playlist]);

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

  const playIds = useCallback(
    (ids: string[]) => dispatch({ type: "play-ids", ids }),
    [dispatch],
  );

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
        }
        dispatch({
          type: "set-playback-progress",
          percent: value[0],
          duration: d,
          current_time: t,
        });
      }
    },
    [player],
  );

  const deleteSelectedTracks = useCallback(() => {
    const selectedRows = gridRef.current?.api.getSelectedRows();
    if (selectedRows) {
      const ids = selectedRows.map((t) => t.yt_id);
      sendMsg({ type: "delete-tracks", ids });
      gridRef.current?.api.deselectAll();
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
        <Flex gap="2" justify="between">
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
          <AppearanceSwitch
            appearance={settings.appearance}
            setAppearance={(appearance) =>
              setSettings({ ...settings, appearance })
            }
          />
        </Flex>
        <div className="background-player">
          <div ref={player2Ref} />
        </div>
        <Flex className="app-main" direction="column" align="center" gap="2">
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
                onClick={() => player?.playVideo()}
              >
                <PlayIcon />
              </IconButton>
            </Tooltip>
            <Tooltip content="Pause playback">
              <IconButton
                variant="soft"
                size="4"
                onClick={() => player?.pauseVideo()}
              >
                <PauseIcon />
              </IconButton>
            </Tooltip>
            <Tooltip content="Stop playback">
              <IconButton
                variant="soft"
                size="4"
                onClick={() => player?.stopVideo()}
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
          <Flex width="100%" gap="2" px="2">
            {state.search_results && !!state.search_box_input ? (
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
            ) : (
              <Tooltip content="Delete selected tracks">
                <Button
                  color="red"
                  onClick={deleteSelectedTracks}
                  disabled={state.track_selection.length === 0}
                >
                  Delete
                </Button>
              </Tooltip>
            )}
          </Flex>
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
