import type {
  ITrack,
  Action,
  RequestReply,
  ServerMsg,
  ClientMsg,
} from "./Types.tsx";

import {
  type CustomCellRendererProps,
  type CustomNoRowsOverlayProps,
} from "ag-grid-react";

import { IconButton } from "@astryxdesign/core/IconButton";
import { Tooltip } from "@astryxdesign/core/Tooltip";
import Rating from "./Rating";
import { Button } from "@astryxdesign/core/Button";
import { Text } from "@astryxdesign/core/Text";
import { Divider } from "@astryxdesign/core/Divider";
import { Spinner } from "@astryxdesign/core/Spinner";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { StackItem, HStack } from "@astryxdesign/core/Stack";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuItem,
} from "@astryxdesign/core/DropdownMenu";

import { AgGridReact } from "ag-grid-react";

import {
  type IDatasource,
  type RowClassParams,
  type ICellRendererParams,
  type IGetRowsParams,
  type ValueFormatterParams,
  type GetRowIdParams,
  type ColDef,
  type RowSelectionOptions,
  type SelectionColumnDef,
  type SelectionChangedEvent,
  type CellKeyDownEvent,
  type FullWidthCellKeyDownEvent,
  themeQuartz,
  colorSchemeDark,
  type GridState,
  type StateUpdatedEvent,
} from "ag-grid-community";

import { AllCommunityModule, ModuleRegistry } from "ag-grid-community";
import { useTheme } from "@astryxdesign/core/theme";
import { useMediaQuery } from "@astryxdesign/core/hooks";

import {
  useState,
  useMemo,
  useCallback,
  useEffect,
  useRef,
  type Dispatch,
  type RefObject,
} from "react";

import {
  PlayIcon,
  PlusIcon,
  ColumnsIcon,
  HamburgerMenuIcon,
  Pencil1Icon,
  VideoIcon,
} from "@radix-ui/react-icons";

import ChangeVideoDialog from "./ChangeVideoDialog.tsx";

import { EditTrackDialog } from "./EditDialog.tsx";
import { GridScrollButtons } from "./GridScrollButtons.tsx";
import { scrollGridToTop, useGridScrollState } from "./useGridScrollState.ts";
import { AudioCacheButton } from "./AudioCacheButton.tsx";
import logger from "./logger.tsx";
import { rowProgressStyle } from "./rowProgress.ts";
import ShazamIcon from "./ShazamIcon.tsx";
import SpotifyIcon from "./SpotifyIcon.tsx";
import YouTubeIcon from "./YouTubeIcon.tsx";

ModuleRegistry.registerModules([AllCommunityModule]);

// Not resizable, so a stateful `width` is safe here, and lets the compact layout change it.
const fixedWidth = (width: number): ColDef => ({
  width,
  minWidth: width,
  maxWidth: width,
  resizable: false,
  suppressAutoSize: true,
});

// Columns whose content has a typical length (e.g. "Extended Mix"): start at that width
// instead of growing to the longest value, but stay manually resizable. User-adjustable
// attributes are given as initial* ones throughout: a stateful `width`/`flex`/`hide` is
// re-applied whenever the column definitions change (e.g. crossing the compact
// breakpoint), discarding the user's resizing and column choices.
const compactWidth = (width: number): ColDef => ({
  initialWidth: width,
  minWidth: 60,
  suppressAutoSize: true,
});

// Free-text columns share whatever space the other columns leave. They are resizable;
// resizing one takes space from the flex columns to its right.
const flexWidth = (flex: number): ColDef => ({
  initialFlex: flex,
  minWidth: 120,
});

// Width of the controls column (thumbnail play button and two icon buttons), which is
// fixed by its renderer's content: 104px plus the cell's horizontal padding.
const CONTROLS_WIDTH = 136;
// The search results' controls have a third icon button (28px plus a 4px gap).
const SEARCH_CONTROLS_WIDTH = CONTROLS_WIDTH + 32;

// Narrow screens: App.css halves the cells' horizontal padding there, so the fixed
// columns shrink by as much, and the free-text columns may get narrower before the grid
// scrolls sideways. The selection column fits just its checkbox.
const COMPACT_PADDING_SAVING = CONTROLS_WIDTH - 120;
const COMPACT_FLEX_MIN_WIDTH = 100;
const compactColumns = (colDefs: ColDef[]): ColDef[] =>
  colDefs.map((c) =>
    c.field === "yt_id" && c.width
      ? { ...c, ...fixedWidth(c.width - COMPACT_PADDING_SAVING) }
      : c.initialFlex
        ? { ...c, minWidth: COMPACT_FLEX_MIN_WIDTH }
        : c,
  );
// Touch screens get taller rows, spacing out the rows' small buttons so a tap doesn't
// land on the neighboring row's.
const ROW_HEIGHT = 32;
const TOUCH_ROW_HEIGHT = 40;
const useRowHeight = () =>
  useMediaQuery("(pointer: coarse)") ? TOUCH_ROW_HEIGHT : ROW_HEIGHT;

const compactSelectionColumn: SelectionColumnDef = {
  width: 32,
  minWidth: 32,
  maxWidth: 32,
  resizable: false,
};

const ArtistCellRenderer = (params: ICellRendererParams<ITrack, string[]>) => (
  <HStack as="span" gap={0} vAlign="center" height="100%">
    {(params.value ?? []).map((artist, index) => (
      <HStack as="span" key={artist} gap={0}>
        {index > 0 && <Text as="span">, </Text>}
        <Button
          label={artist}
          variant="ghost"
          size="sm"
          onClick={(event) => {
            event.stopPropagation();
            params.context?.onArtistClick?.(artist);
          }}
        />
      </HStack>
    ))}
  </HStack>
);

const LabelCellRenderer = (params: ICellRendererParams<ITrack, string>) =>
  params.value ? (
    <Button
      label={params.value}
      variant="ghost"
      size="sm"
      onClick={(event) => {
        event.stopPropagation();
        params.context?.onLabelClick?.(params.value);
      }}
    />
  ) : null;

const RatingRenderer = (params: ICellRendererParams<ITrack, number>) =>
  params.data ? (
    <Rating
      enabled
      currentRating={params.value ?? undefined}
      label={`Rating for ${params.data.title}`}
      updateRating={(rating) => {
        params.node.setDataValue("rating", rating);
        params.context.updateRating(params.data?.yt_id, rating);
      }}
    />
  ) : null;

// Overlays are module-level components so AG Grid does not remount them on every render.
const SearchingOverlay = () => <Spinner label="Searching for tracks…" />;

const NoSearchResultsOverlay = ({
  onExitSearch,
}: CustomNoRowsOverlayProps & { onExitSearch: () => void }) => (
  <EmptyState
    isCompact
    title="No tracks found"
    actions={<Button label="Back to collection" onClick={onExitSearch} />}
  />
);

const NoFilterMatchesOverlay = ({ api }: CustomNoRowsOverlayProps) => (
  <EmptyState
    isCompact
    title="No tracks match these filters"
    actions={
      <Button label="Clear filters" onClick={() => api.setFilterModel(null)} />
    }
  />
);

const openInYouTube = (id: string) =>
  window.open(
    `https://www.youtube.com/watch?v=${id}`,
    "_blank",
    "noopener,noreferrer",
  );

const openInSpotify = (id: string) =>
  window.open(
    `https://open.spotify.com/track/${encodeURIComponent(id)}`,
    "_blank",
    "noopener,noreferrer",
  );

// The track's YouTube thumbnail with a play triangle on it, which plays the track.
const ThumbnailPlayButton = ({
  track,
  onPlay,
}: {
  track: ITrack;
  onPlay: () => void;
}) => (
  <Tooltip content="Play">
    <button
      type="button"
      className="thumbnail-play-button"
      aria-label={`Play ${track.title}`}
      onClick={onPlay}
    >
      <img
        src={`https://i.ytimg.com/vi/${track.yt_id}/default.jpg`}
        alt=""
        loading="lazy"
      />
      <PlayIcon aria-hidden />
    </button>
  </Tooltip>
);

// A row's edit dialog, with the edited copy of its track. Each opening starts from the
// row's current track.
const RowEditDialog = ({
  data,
  context,
  isOpen,
  onOpenChange,
}: {
  data: ITrack;
  context: TracksGridContext;
  isOpen: boolean;
  onOpenChange: (isOpen: boolean) => void;
}) => {
  const [value, setValue] = useState(data);
  const [wasOpen, setWasOpen] = useState(isOpen);
  if (isOpen !== wasOpen) {
    setWasOpen(isOpen);
    if (isOpen) setValue(data);
  }

  // Compare by content: after every grid refresh `data` is a new but usually identical object,
  // and validating then would send a request for every visible row.
  const edited = useMemo(
    () => JSON.stringify(value) !== JSON.stringify(data),
    [value, data],
  );

  // The backend's verdict on a draft. It only counts for the draft it was given: any other
  // (a newer edit, or the track again after reopening) is unvalidated until its own reply.
  const [validation, setValidation] = useState<{
    draft: ITrack;
    isValid: boolean;
  }>();
  const isValid =
    edited && validation?.draft === value ? validation.isValid : null;

  useEffect(() => {
    if (!isOpen || !edited) return;
    // A reply that arrives after the dialog closed or the draft changed is ignored.
    let current = true;
    context.requestReply({ type: "validate-track", track: value }, (msg) => {
      if (current && "is_valid" in msg) {
        setValidation({ draft: value, isValid: msg.is_valid });
      }
    });
    return () => {
      current = false;
    };
  }, [isOpen, edited, value, context]);

  const commit = useCallback(() => {
    context.sendMsg({ type: "update-track", old: data, new: value });
  }, [context, data, value]);

  const resetValue = useCallback(() => {
    setValue(data);
  }, [data]);

  return (
    <EditTrackDialog
      isOpen={isOpen}
      onOpenChange={onOpenChange}
      isValid={isValid}
      value={value}
      setValue={setValue}
      commit={commit}
      resetValue={resetValue}
      requestReply={context.requestReply}
    />
  );
};

const PlayControlRenderer: React.FC<
  CustomCellRendererProps<ITrack, unknown, TracksGridContext>
> = (params) => {
  const { data, context } = params;

  const handlePlay = useCallback(() => {
    if (data) {
      context.playId(data.yt_id);
    }
  }, [context, data]);

  // The edit dialog is mounted on its first opening, not with the row: a closed Astryx
  // Dialog still renders its whole form (some 430 components), which made every row
  // mount slowly, e.g. when scrolling or making the window taller.
  const [editOpen, setEditOpen] = useState<boolean>();

  if (!data) return null;

  return (
    <HStack gap={1} vAlign="center">
      <ThumbnailPlayButton track={data} onPlay={handlePlay} />
      <AudioCacheButton id={data.yt_id} onDownload={context.downloadAudio} />
      <DropdownMenu
        hasChevron={false}
        button={{
          label: `Options for ${data.title}`,
          icon: <HamburgerMenuIcon />,
          isIconOnly: true,
          variant: "ghost",
          size: "sm",
          tooltip: "Track options",
        }}
        items={[
          {
            label: "Edit",
            icon: <Pencil1Icon />,
            onClick: () => setEditOpen(true),
          },
          {
            label: "Find similar songs on Shazam",
            icon: <ShazamIcon size={15} />,
            onClick: () => context.onShazamSimilar(data),
          },
          {
            label: "Open in YouTube",
            icon: <YouTubeIcon size={15} />,
            onClick: () => openInYouTube(data.yt_id),
          },
          ...(data.sp_id
            ? [
                {
                  label: "Open in Spotify",
                  icon: <SpotifyIcon size={15} />,
                  onClick: () => openInSpotify(data.sp_id!),
                },
              ]
            : []),
        ]}
      />
      {editOpen !== undefined && (
        <RowEditDialog
          data={data}
          context={context}
          isOpen={editOpen}
          onOpenChange={setEditOpen}
        />
      )}
    </HStack>
  );
};

const SearchPlayControlRenderer: React.FC<
  CustomCellRendererProps<ITrack, ITrack, SearchResultsGridContext>
> = (params) => {
  const { data, context } = params;

  const handlePlay = useCallback(() => {
    if (data) {
      context.dispatch({ type: "track-info", track: data });
      // By result, not video: results can share one (see `set-search-result-video`).
      const idx = context.tracks.findIndex(
        (t) => t.result_id === data.result_id,
      );
      const queue = context.tracks.slice(idx >= 0 ? idx : 0);
      context.dispatch({
        type: "set-playlist",
        ids: queue.map((t) => t.yt_id),
        // One per entry, so the two stay aligned.
        result_ids: queue.map((t) => t.result_id),
      });
      context.playId(data.yt_id);
    }
  }, [data, context]);

  const exists = !!data && context.inCollection.current.has(data.yt_id);

  const handleAdd = useCallback(() => {
    if (data) {
      context.addTrack(data);
    }
  }, [data, context]);

  if (!data) return null;

  return (
    <HStack gap={1} vAlign="center">
      <ThumbnailPlayButton track={data} onPlay={handlePlay} />
      <AudioCacheButton id={data.yt_id} onDownload={context.downloadAudio} />
      <DropdownMenu
        hasChevron={false}
        button={{
          label: `Options for ${data.title}`,
          icon: <HamburgerMenuIcon />,
          isIconOnly: true,
          variant: "ghost",
          size: "sm",
          tooltip: "Track options",
        }}
        items={[
          {
            label: "Change YouTube video…",
            icon: <VideoIcon />,
            onClick: () => context.onChangeVideo(data),
          },
          {
            label: "Open in YouTube",
            icon: <YouTubeIcon size={15} />,
            onClick: () => openInYouTube(data.yt_id),
          },
          ...(data.sp_id
            ? [
                {
                  label: "Open in Spotify",
                  icon: <SpotifyIcon size={15} />,
                  onClick: () => openInSpotify(data.sp_id!),
                },
              ]
            : []),
        ]}
      />
      <IconButton
        variant="secondary"
        size="sm"
        onClick={handleAdd}
        isDisabled={exists}
        label={exists ? "Track exists in collection" : "Add to collection"}
        tooltip={exists ? "Track exists in collection" : "Add to collection"}
        icon={<PlusIcon />}
      />
    </HStack>
  );
};

// ag-Grid themes itself, so follow the effective Astryx mode (which resolves
// "system" to the OS preference and tracks changes to it).
function useGridTheme() {
  const { mode } = useTheme();
  return useMemo(
    () =>
      mode === "dark" ? themeQuartz.withPart(colorSchemeDark) : themeQuartz,
    [mode],
  );
}

interface SearchResultsGridProps {
  // Chart results (Beatport/Traxsource top 100) get a chart-position column and are sorted by it.
  // Read on mount only — remount the grid (via `key`) when it changes.
  isChart?: boolean;
  isSearching: boolean;
  onExitSearch: () => void;
  gridRef: React.RefObject<AgGridReact<ITrack> | null>;
  currentId?: string;
  // The playing result, when playing a queue of search results: results can share a video,
  // so `currentId` alone may match several rows.
  currentResultId?: number;
  playId: (id: string) => void;
  requestReply: RequestReply;
  dispatch: Dispatch<Action>;
  tracks?: ITrack[];
  lastCollectionUpdate?: number;
  onArtistClick: (name: string) => void;
  onLabelClick: (name: string) => void;
  addTrack: (track: ITrack) => void;
  downloadAudio: (id: string) => void;
  // Scrolls the playing track into view (the floating "show playing track" button).
  onShowCurrent: () => void;
  isCompact?: boolean;
}

type SearchResultsGridContext = {
  playId: (id: string) => void;
  requestReply: RequestReply;
  // Result ids already in the collection. A stable ref, so updating it doesn't replace the
  // grid context; cells are refreshed explicitly instead.
  inCollection: RefObject<Set<string>>;
  dispatch: Dispatch<Action>;
  onArtistClick: (name: string) => void;
  onLabelClick: (name: string) => void;
  addTrack: (track: ITrack) => void;
  downloadAudio: (id: string) => void;
  // Opens the dialog to pick another YouTube video for a result.
  onChangeVideo: (track: ITrack) => void;
  tracks: ITrack[];
};

export const SearchResultsGrid = ({
  gridRef,
  currentId,
  currentResultId,
  playId,
  requestReply,
  dispatch,
  tracks,
  lastCollectionUpdate,
  onArtistClick,
  onLabelClick,
  addTrack,
  downloadAudio,
  isChart = false,
  isSearching,
  onExitSearch,
  onShowCurrent,
  isCompact = false,
}: SearchResultsGridProps) => {
  const [colDefs] = useState<ColDef[]>(() => [
    ...(isChart
      ? [
          {
            field: "chart_position",
            headerName: "#",
            headerTooltip: "Chart position",
            sortable: true,
            initialSort: "asc",
            ...fixedWidth(72),
            cellStyle: { textAlign: "right" },
          } satisfies ColDef,
        ]
      : []),
    {
      field: "yt_id",
      headerName: "",
      cellRenderer: SearchPlayControlRenderer,
      ...fixedWidth(SEARCH_CONTROLS_WIDTH),
    },
    {
      field: "title",
      headerName: "Title",
      filter: true,
      ...flexWidth(3),
    },
    {
      field: "mix_name",
      headerName: "Mix",
      filter: true,
      ...compactWidth(120),
    },
    {
      field: "artists",
      headerName: "Artist",
      filter: true,
      type: "artists",
      cellRenderer: ArtistCellRenderer,
      ...flexWidth(3),
    },
    {
      field: "label",
      headerName: "Label",
      filter: true,
      cellRenderer: LabelCellRenderer,
      ...flexWidth(2),
    },
    {
      field: "release_date",
      headerName: "Release",
      sortable: true,
      initialSort: isChart ? undefined : "desc",
      filter: true,
      ...fixedWidth(130),
    },
    {
      field: "genre",
      headerName: "Genre",
      filter: true,
      ...fixedWidth(130),
    },
    {
      field: "bpm",
      headerName: "BPM",
      filter: true,
      ...fixedWidth(80),
      cellStyle: { textAlign: "right" },
    },
    {
      field: "source",
      headerName: "Source",
      filter: true,
      ...compactWidth(120),
      valueFormatter: (params: ValueFormatterParams<ITrack, string>) => {
        switch (params.value) {
          case "beatport":
            return "Beatport";
          case "soundcloud":
            return "SoundCloud";
          case "volumo":
            return "Volumo";
          case "traxsource":
            return "Traxsource";
          case "spotify":
            return "Spotify";
          case "apple-music":
            return "Apple Music";
          case "shazam":
            return "Shazam";
          default:
            return params.value ?? "";
        }
      },
    },
  ]);

  const rowSelection = useMemo<RowSelectionOptions>(() => {
    return {
      mode: "multiRow",
    };
  }, []);

  const columnDefs = useMemo(
    () => (isCompact ? compactColumns(colDefs) : colDefs),
    [colDefs, isCompact],
  );

  const theme = useGridTheme();
  const rowHeight = useRowHeight();

  const columnTypes = useMemo(() => {
    return {
      artists: {
        valueFormatter: (params: ValueFormatterParams<ITrack, string[]>) => {
          return params.value?.join(", ") ?? "";
        },
      },
    };
  }, []);

  const onSelectionChanged = useCallback(
    (ev: SelectionChangedEvent<ITrack>) => {
      dispatch({
        type: "search-results-selection-changed",
        selected:
          ev.selectedNodes
            ?.map((node) => node.data)
            .filter((data) => data !== undefined) ?? [],
      });
    },
    [dispatch],
  );

  const isCurrent = useMemo(
    () =>
      currentResultId !== undefined
        ? (track: ITrack) => track.result_id === currentResultId
        : currentId
          ? (track: ITrack) => track.yt_id === currentId
          : undefined,
    [currentId, currentResultId],
  );

  const rowClassRules = useMemo(() => {
    return {
      "progress-bar": (params: RowClassParams<ITrack>) =>
        !!params.data && !!isCurrent?.(params.data),
    };
  }, [isCurrent]);
  const getRowStyle = useCallback(
    (params: RowClassParams<ITrack>) =>
      params.data && isCurrent?.(params.data) ? rowProgressStyle() : undefined,
    [isCurrent],
  );

  useEffect(() => {
    gridRef.current?.api?.redrawRows();
  }, [isCurrent, gridRef]);

  // One batched lookup for all results instead of one request per row. Results stream in one
  // at a time, so the lookup is debounced.
  const inCollection = useRef<Set<string>>(new Set());
  const resultIds = useMemo(() => (tracks ?? []).map((t) => t.yt_id), [tracks]);
  useEffect(() => {
    if (resultIds.length === 0) return;
    let cancelled = false;
    const timer = setTimeout(() => {
      requestReply(
        { type: "collection-contains-ids", yt_ids: resultIds },
        (msg) => {
          if (cancelled || !("contained" in msg)) return;
          inCollection.current = new Set(msg.contained);
          gridRef.current?.api?.refreshCells({
            columns: ["yt_id"],
            force: true,
          });
        },
      );
    }, 250);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [resultIds, lastCollectionUpdate, requestReply, gridRef]);

  const [changingVideo, setChangingVideo] = useState<ITrack | null>(null);
  const changeVideo = useCallback(
    (track: ITrack, yt_id: string) => {
      if (track.result_id === undefined) return;
      dispatch({
        type: "set-search-result-video",
        result_id: track.result_id,
        yt_id,
      });
    },
    [dispatch],
  );

  const context: SearchResultsGridContext = useMemo(() => {
    return {
      requestReply,
      playId,
      inCollection,
      dispatch,
      onArtistClick,
      onLabelClick,
      addTrack,
      downloadAudio,
      onChangeVideo: setChangingVideo,
      tracks: tracks ?? [],
    };
  }, [
    requestReply,
    playId,
    dispatch,
    onArtistClick,
    onLabelClick,
    addTrack,
    downloadAudio,
    tracks,
  ]);

  // Not from `yt_id`, which changing a result's video changes: the row (and its selection)
  // must stay the same.
  const getRowId = useCallback(
    (params: GetRowIdParams<ITrack>) => String(params.data.result_id),
    [],
  );

  const noRowsOverlayParams = useMemo(() => ({ onExitSearch }), [onExitSearch]);

  const { scrolledDown, currentVisible, gridEvents } = useGridScrollState(
    gridRef,
    isCurrent,
  );
  // Only offered when the playing track is one of the results.
  const currentInResults = !!isCurrent && !!tracks?.some(isCurrent);

  return (
    <StackItem size="fill" className="grid">
      <AgGridReact<ITrack>
        {...gridEvents}
        rowData={tracks}
        loading={isSearching && !tracks?.length}
        loadingOverlayComponent={SearchingOverlay}
        noRowsOverlayComponent={NoSearchResultsOverlay}
        noRowsOverlayComponentParams={noRowsOverlayParams}
        theme={theme}
        ref={gridRef}
        rowModelType={"clientSide"}
        getRowId={getRowId}
        columnTypes={columnTypes}
        columnDefs={columnDefs}
        // Keep columns the user moved where they are when the definitions change.
        maintainColumnOrder
        rowSelection={rowSelection}
        selectionColumnDef={isCompact ? compactSelectionColumn : undefined}
        rowClassRules={rowClassRules}
        getRowStyle={getRowStyle}
        rowHeight={rowHeight}
        context={context}
        onSelectionChanged={onSelectionChanged}
      />
      <GridScrollButtons
        showTop={scrolledDown}
        showCurrent={currentInResults && !currentVisible}
        onTop={() => scrollGridToTop(gridRef.current?.api)}
        onShowCurrent={onShowCurrent}
      />
      <ChangeVideoDialog
        track={changingVideo}
        onClose={() => setChangingVideo(null)}
        onChange={changeVideo}
        requestReply={requestReply}
      />
    </StackItem>
  );
};

type ToggleableColumn = {
  colId: string;
  headerName: string;
  visible: boolean;
};

type ColumnsMenuProps = {
  gridRef: React.RefObject<AgGridReact<ITrack> | null>;
  onReset: () => void;
  isIconOnly?: boolean;
};

export const ColumnsMenu = ({
  gridRef,
  onReset,
  isIconOnly = false,
}: ColumnsMenuProps) => {
  const [columns, setColumns] = useState<ToggleableColumn[]>([]);
  const [isOpen, setIsOpen] = useState(false);

  const refreshColumns = useCallback(() => {
    const api = gridRef.current?.api;
    if (!api) return;

    const headerNames = new Map<string, string>();
    for (const def of (api.getColumnDefs() ?? []) as ColDef[]) {
      if (def.field && def.headerName) {
        headerNames.set(def.field, def.headerName);
      }
    }

    setColumns(
      api.getColumnState().flatMap((s) => {
        const headerName = headerNames.get(s.colId);
        return headerName
          ? [{ colId: s.colId, headerName, visible: !s.hide }]
          : [];
      }),
    );
  }, [gridRef]);

  const toggleColumn = useCallback(
    (colId: string, visible: boolean) => {
      gridRef.current?.api.setColumnsVisible([colId], visible);
      refreshColumns();
    },
    [gridRef, refreshColumns],
  );

  // DropdownMenu only fires onOpenChange when its open state is controlled,
  // so keep it controlled to re-read column state each time the menu opens.
  const handleOpenChange = useCallback(
    (open: boolean) => {
      if (open) refreshColumns();
      setIsOpen(open);
    },
    [refreshColumns],
  );

  return (
    <DropdownMenu
      hasChevron={false}
      isMenuOpen={isOpen}
      onOpenChange={handleOpenChange}
      button={{
        label: "View",
        icon: <ColumnsIcon />,
        isIconOnly,
        variant: "ghost",
        tooltip: "View options",
      }}
    >
      {columns.map((c) => (
        <DropdownMenuCheckboxItem
          key={c.colId}
          label={c.headerName}
          value={c.visible}
          onChange={(checked) => toggleColumn(c.colId, checked)}
        />
      ))}
      <Divider />
      <DropdownMenuItem label="Reset columns" onClick={onReset} />
    </DropdownMenu>
  );
};

interface TracksGridProps {
  gridRef: React.RefObject<AgGridReact<ITrack> | null>;
  currentId?: string;
  playId: (id: string) => void;
  updateRating: (yt_id: string, rating: number | null) => void;
  requestReply: RequestReply;
  sendMsg: (msg: ClientMsg) => void;
  setGridReady: (ready: boolean) => void;
  // The state to start from, read when the grid mounts.
  loadInitialState: () => GridState | undefined;
  onStateUpdate: (event: StateUpdatedEvent<ITrack>) => void;
  onRowCountChange?: (count: number) => void;
  onSelectionChanged: (event: SelectionChangedEvent<ITrack>) => void;
  // Called after a block of rows has loaded into the grid.
  onRowsLoaded: () => void;
  onFilterChanged: () => void;
  // Cmd/Ctrl+A, which ag-Grid can't do itself with the infinite row model.
  onSelectAll: () => void;
  onArtistClick: (name: string) => void;
  onLabelClick: (name: string) => void;
  onShazamSimilar: (track: ITrack) => void;
  downloadAudio: (id: string) => void;
  // Scrolls the playing track into view (the floating "show playing track" button).
  onShowCurrent: () => void;
  isCompact?: boolean;
}

type TracksGridContext = {
  playId: (id: string) => void;
  requestReply: RequestReply;
  updateRating: (yt_id: string, rating: number | null) => void;
  sendMsg: (msg: ClientMsg) => void;
  onArtistClick: (name: string) => void;
  onLabelClick: (name: string) => void;
  onShazamSimilar: (track: ITrack) => void;
  downloadAudio: (id: string) => void;
};

export const TracksGrid = ({
  gridRef,
  currentId,
  playId,
  updateRating,
  requestReply,
  sendMsg,
  setGridReady,
  loadInitialState,
  onStateUpdate,
  onRowCountChange,
  onSelectionChanged,
  onRowsLoaded,
  onFilterChanged,
  onSelectAll,
  onArtistClick,
  onLabelClick,
  onShazamSimilar,
  downloadAudio,
  onShowCurrent,
  isCompact = false,
}: TracksGridProps) => {
  const [initialState] = useState(loadInitialState);
  const [colDefs] = useState<ColDef[]>([
    {
      field: "yt_id",
      headerName: "",
      cellRenderer: PlayControlRenderer,
      ...fixedWidth(CONTROLS_WIDTH),
    },
    {
      field: "title",
      headerName: "Title",
      filter: true,
      ...flexWidth(3),
    },
    {
      field: "artists",
      headerName: "Artist",
      filter: true,
      type: "artists",
      cellRenderer: ArtistCellRenderer,
      ...flexWidth(3),
    },
    {
      field: "label",
      headerName: "Label",
      filter: true,
      cellRenderer: LabelCellRenderer,
      ...flexWidth(2),
    },
    {
      field: "release_date",
      headerName: "Release",
      sortable: true,
      initialSort: "desc",
      filter: "agDateColumnFilter",
      ...fixedWidth(130),
    },
    {
      field: "genre",
      headerName: "Genre",
      filter: true,
      ...fixedWidth(200),
    },
    {
      field: "bpm",
      headerName: "BPM",
      filter: "agNumberColumnFilter",
      ...fixedWidth(80),
      cellStyle: { textAlign: "right" },
    },
    {
      field: "key",
      headerName: "Key",
      filter: true,
      ...fixedWidth(100),
    },
    {
      field: "rating",
      headerName: "Rating",
      filter: "agNumberColumnFilter",
      cellRenderer: RatingRenderer,
      ...fixedWidth(200),
    },
    {
      field: "play_count",
      headerName: "Plays",
      filter: "agNumberColumnFilter",
      ...compactWidth(90),
      cellStyle: { textAlign: "right" },
    },
    {
      field: "last_played",
      headerName: "Last played",
      filter: true,
      ...fixedWidth(130),
    },
    {
      field: "bp_id",
      headerName: "Beatport ID",
      filter: "agNumberColumnFilter",
      initialWidth: 120,
      initialHide: true,
    },
    {
      field: "sp_id",
      headerName: "Spotify ID",
      filter: true,
      initialWidth: 120,
      initialHide: true,
    },
    {
      field: "mb_id",
      headerName: "MusicBrainz ID",
      filter: true,
      initialWidth: 150,
      initialHide: true,
    },
  ]);

  // Restoring a saved state clears the flex of every column its sizing doesn't list, so
  // add the flex defaults back for columns the user hasn't resized (resizing a flex
  // column gives it a fixed width, which is then saved).
  const restoredState = useMemo<GridState | undefined>(() => {
    if (!initialState) return undefined;
    const saved = initialState.columnSizing?.columnSizingModel ?? [];
    const savedIds = new Set(saved.map((s) => s.colId));
    const flexDefaults = colDefs.flatMap((c) =>
      c.field && c.initialFlex && !savedIds.has(c.field)
        ? [{ colId: c.field, flex: c.initialFlex }]
        : [],
    );
    return {
      ...initialState,
      columnSizing: { columnSizingModel: [...saved, ...flexDefaults] },
    };
  }, [initialState, colDefs]);

  const rowSelection = useMemo<RowSelectionOptions>(() => {
    return {
      mode: "multiRow",
      headerCheckbox: false,
    };
  }, []);

  const columnDefs = useMemo(
    () => (isCompact ? compactColumns(colDefs) : colDefs),
    [colDefs, isCompact],
  );

  const defaultColDef = useMemo<ColDef>(() => {
    return {
      filterParams: {
        buttons: ["clear"],
      },
    };
  }, []);

  const theme = useGridTheme();
  const rowHeight = useRowHeight();

  const columnTypes = useMemo(() => {
    return {
      artists: {
        valueFormatter: (params: ValueFormatterParams<ITrack, string[]>) => {
          return params.value?.join(", ") ?? "";
        },
      },
    };
  }, []);

  const dataSource = useMemo<IDatasource>(() => {
    return {
      rowCount: undefined,
      getRows: (params: IGetRowsParams<ITrack>) => {
        logger.debug("asking for " + params.startRow + " to " + params.endRow);
        requestReply(
          { type: "get-rows", ...params },
          function (msg: ServerMsg | { type: string }) {
            if ("rows" in msg) {
              params.successCallback(msg.rows, msg.last_row);
              onRowsLoaded();
              if (msg.last_row !== undefined) {
                onRowCountChange?.(msg.last_row);
                if (msg.last_row === 0)
                  gridRef.current?.api.showNoRowsOverlay();
                else gridRef.current?.api.hideOverlay();
              }
            } else {
              params.failCallback();
            }
          },
          3000,
        );
      },
    };
  }, [requestReply, onRowCountChange, onRowsLoaded, gridRef]);

  const rowClassRules = useMemo(() => {
    return {
      "progress-bar": (params: RowClassParams<ITrack>) => {
        return Boolean(currentId && params.data?.yt_id === currentId);
      },
    };
  }, [currentId]);
  const getRowStyle = useCallback(
    (params: RowClassParams<ITrack>) =>
      currentId && params.data?.yt_id === currentId
        ? rowProgressStyle()
        : undefined,
    [currentId],
  );

  const updateRatingAndRefresh = useCallback(
    (yt_id: string, rating: number | null) => {
      updateRating(yt_id, rating);
      gridRef.current?.api.refreshInfiniteCache();
    },
    [updateRating, gridRef],
  );

  const context: TracksGridContext = useMemo(() => {
    return {
      updateRating: updateRatingAndRefresh,
      requestReply,
      playId,
      sendMsg,
      onArtistClick,
      onLabelClick,
      onShazamSimilar,
      downloadAudio,
    };
  }, [
    updateRatingAndRefresh,
    requestReply,
    playId,
    sendMsg,
    onArtistClick,
    onLabelClick,
    onShazamSimilar,
    downloadAudio,
  ]);

  const onCellKeyDown = useCallback(
    (ev: CellKeyDownEvent<ITrack> | FullWidthCellKeyDownEvent<ITrack>) => {
      const key = ev.event as KeyboardEvent | null | undefined;
      if (
        key &&
        (key.metaKey || key.ctrlKey) &&
        !key.altKey &&
        !key.shiftKey &&
        key.key.toLowerCase() === "a"
      ) {
        onSelectAll();
      }
    },
    [onSelectAll],
  );

  const getRowId = useCallback(
    (params: GetRowIdParams<ITrack>) =>
      `${params.data.yt_id}:${params.data.bp_id}:${params.data.sp_id}:${params.data.mb_id}`,
    [],
  );

  const onGridReady = useCallback(() => setGridReady(true), [setGridReady]);

  const onStateUpdate0 = useCallback(
    (ev: StateUpdatedEvent<ITrack>) => onStateUpdate(ev),
    [onStateUpdate],
  );

  const isCurrent = useMemo(
    () =>
      currentId ? (track: ITrack) => track.yt_id === currentId : undefined,
    [currentId],
  );
  const { scrolledDown, currentVisible, gridEvents } = useGridScrollState(
    gridRef,
    isCurrent,
  );

  return (
    <StackItem size="fill" className="grid">
      <AgGridReact<ITrack>
        {...gridEvents}
        theme={theme}
        onGridReady={onGridReady}
        ref={gridRef}
        columnTypes={columnTypes}
        rowClassRules={rowClassRules}
        getRowStyle={getRowStyle}
        initialState={restoredState}
        onStateUpdated={onStateUpdate0}
        columnDefs={columnDefs}
        // Keep columns the user moved where they are when the definitions change.
        maintainColumnOrder
        defaultColDef={defaultColDef}
        rowSelection={rowSelection}
        selectionColumnDef={isCompact ? compactSelectionColumn : undefined}
        rowHeight={rowHeight}
        getRowId={getRowId}
        context={context}
        rowBuffer={10}
        rowModelType={"infinite"}
        cacheBlockSize={100}
        maxConcurrentDatasourceRequests={4}
        infiniteInitialRowCount={1000}
        maxBlocksInCache={10}
        noRowsOverlayComponent={NoFilterMatchesOverlay}
        datasource={dataSource}
        onSelectionChanged={onSelectionChanged}
        onFilterChanged={onFilterChanged}
        onCellKeyDown={onCellKeyDown}
      />
      <GridScrollButtons
        showTop={scrolledDown}
        showCurrent={!!currentId && !currentVisible}
        onTop={() => scrollGridToTop(gridRef.current?.api)}
        onShowCurrent={onShowCurrent}
      />
    </StackItem>
  );
};
