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
import { Link } from "@astryxdesign/core/Link";
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
  type SelectionChangedEvent,
  themeQuartz,
  colorSchemeDark,
  type SizeColumnsToContentStrategy,
  type GridState,
  type StateUpdatedEvent,
} from "ag-grid-community";

import { AllCommunityModule, ModuleRegistry } from "ag-grid-community";
import { useTheme } from "@astryxdesign/core/theme";

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
  MagnifyingGlassIcon,
} from "@radix-ui/react-icons";

import { EditTrackDialog } from "./EditDialog.tsx";
import logger from "./logger.tsx";
import { MenuCheck } from "./Menu.tsx";

ModuleRegistry.registerModules([AllCommunityModule]);

const fixedWidth = (width: number): ColDef => ({
  width,
  minWidth: width,
  maxWidth: width,
  resizable: false,
  suppressAutoSize: true,
});

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
    description="Try another title, artist, label, or search source."
    actions={<Button label="Back to collection" onClick={onExitSearch} />}
  />
);

const NoFilterMatchesOverlay = ({ api }: CustomNoRowsOverlayProps) => (
  <EmptyState
    isCompact
    title="No tracks match these filters"
    description="Clear the filters to see your collection again."
    actions={
      <Button label="Clear filters" onClick={() => api.setFilterModel(null)} />
    }
  />
);

const PlayControlRenderer: React.FC<
  CustomCellRendererProps<ITrack, unknown, TracksGridContext>
> = (params) => {
  const { data, context } = params;

  const handlePlay = useCallback(() => {
    if (data) {
      context.playId(data.yt_id);
    }
  }, [context, data]);

  const [value, setValue] = useState(data);

  const [isValid, setIsValid] = useState<boolean | null>(null);

  // Compare by content: after every grid refresh `data` is a new but usually identical object,
  // and validating then would send a request for every visible row.
  const edited = useMemo(
    () => JSON.stringify(value) !== JSON.stringify(data),
    [value, data],
  );

  useEffect(() => {
    if (edited) {
      context.requestReply({ type: "validate-track", track: value }, (msg) => {
        if ("is_valid" in msg) {
          setIsValid(msg.is_valid);
        }
      });
    }
  }, [edited, value, context]);

  const commit = useCallback(() => {
    if (data && value) {
      context.sendMsg({ type: "update-track", old: data, new: value });
    }
  }, [context, data, value]);

  const resetValue = useCallback(() => {
    setValue(data);
  }, [data]);

  if (!data || !value) return null;

  return (
    <HStack gap={1} vAlign="center" className="track-control-cell">
      <Tooltip content="Open in YouTube">
        <Link
          label={`Open ${data.title} in YouTube`}
          href={`https://www.youtube.com/watch?v=${data.yt_id}`}
          isExternalLink
        >
          <img
            className="yt-thumbnail"
            src={`https://i.ytimg.com/vi/${data.yt_id}/default.jpg`}
            alt="YouTube Video Thumbnail"
            width="40"
          />
        </Link>
      </Tooltip>
      <IconButton
        variant="ghost"
        size="sm"
        onClick={handlePlay}
        label="Play"
        tooltip="Play"
        icon={<PlayIcon />}
      />
      <IconButton
        variant="ghost"
        size="sm"
        onClick={() => context.onShazamSimilar(data)}
        label="Find similar songs on Shazam"
        tooltip="Find similar songs on Shazam"
        icon={<MagnifyingGlassIcon />}
      />

      <EditTrackDialog
        isValid={isValid}
        value={value}
        setValue={setValue}
        commit={commit}
        resetValue={resetValue}
        requestReply={context.requestReply}
      />
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
      const idx = context.tracks.findIndex((t) => t.yt_id === data.yt_id);
      const ids = context.tracks.slice(idx >= 0 ? idx : 0).map((t) => t.yt_id);
      context.dispatch({ type: "set-playlist", ids });
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
    <HStack gap={2} vAlign="center">
      <img
        src={`https://i.ytimg.com/vi/${data.yt_id}/default.jpg`}
        alt="YouTube Video Thumbnail"
        width="40"
      />
      <IconButton
        variant="secondary"
        size="sm"
        onClick={handlePlay}
        label="Play"
        tooltip="Play"
        icon={<PlayIcon />}
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
  // Chart results (Beatport top 100) get a chart-position column and are sorted by it.
  // Read on mount only — remount the grid (via `key`) when it changes.
  isChart?: boolean;
  isSearching: boolean;
  onExitSearch: () => void;
  gridRef: React.RefObject<AgGridReact<ITrack> | null>;
  currentId?: string;
  playId: (id: string) => void;
  requestReply: RequestReply;
  dispatch: Dispatch<Action>;
  tracks?: ITrack[];
  lastCollectionUpdate?: number;
  onArtistClick: (name: string) => void;
  onLabelClick: (name: string) => void;
  addTrack: (track: ITrack) => void;
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
  tracks: ITrack[];
};

export const SearchResultsGrid = ({
  gridRef,
  currentId,
  playId,
  requestReply,
  dispatch,
  tracks,
  lastCollectionUpdate,
  onArtistClick,
  onLabelClick,
  addTrack,
  isChart = false,
  isSearching,
  onExitSearch,
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
      width: 180,
    },
    {
      field: "title",
      headerName: "Title",
      filter: true,
      width: 150,
      suppressAutoSize: true,
    },
    { field: "mix_name", headerName: "Mix", filter: true },
    {
      field: "artists",
      headerName: "Artist",
      filter: true,
      type: "artists",
      cellRenderer: ArtistCellRenderer,
      width: 150,
      suppressAutoSize: true,
    },
    {
      field: "label",
      headerName: "Label",
      filter: true,
      cellRenderer: LabelCellRenderer,
      width: 150,
      suppressAutoSize: true,
    },
    {
      field: "release_date",
      headerName: "Release",
      sortable: true,
      initialSort: isChart ? undefined : "desc",
      filter: true,
      ...fixedWidth(120),
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
      width: 110,
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

  const autoSizeStrategy = useMemo<SizeColumnsToContentStrategy>(() => {
    return {
      type: "fitCellContents",
      scaleUpToFitGridWidth: true,
    };
  }, []);

  const defaultColDef = useMemo<ColDef>(() => {
    return {
      // flex: 1,
    };
  }, []);

  const theme = useGridTheme();

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

  const rowClassRules = useMemo(() => {
    return {
      "progress-bar": (params: RowClassParams<ITrack>) => {
        return Boolean(currentId && params.data?.yt_id === currentId);
      },
    };
  }, [currentId]);

  useEffect(() => {
    gridRef.current?.api?.redrawRows();
  }, [currentId, gridRef]);

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

  const context: SearchResultsGridContext = useMemo(() => {
    return {
      requestReply,
      playId,
      inCollection,
      dispatch,
      onArtistClick,
      onLabelClick,
      addTrack,
      tracks: tracks ?? [],
    };
  }, [
    requestReply,
    playId,
    dispatch,
    onArtistClick,
    onLabelClick,
    addTrack,
    tracks,
  ]);

  const getRowId = useCallback(
    (params: GetRowIdParams<ITrack>) =>
      `${params.data.yt_id}:${params.data.bp_id}:${params.data.sp_id}:${params.data.mb_id}`,
    [],
  );

  const noRowsOverlayParams = useMemo(() => ({ onExitSearch }), [onExitSearch]);

  return (
    <StackItem size="fill" className="grid">
      <AgGridReact<ITrack>
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
        columnDefs={colDefs}
        defaultColDef={defaultColDef}
        rowSelection={rowSelection}
        rowClassRules={rowClassRules}
        autoSizeStrategy={autoSizeStrategy}
        rowHeight={32}
        context={context}
        onSelectionChanged={onSelectionChanged}
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
};

export const ColumnsMenu = ({ gridRef, onReset }: ColumnsMenuProps) => {
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
          endContent={<MenuCheck isChecked={c.visible} />}
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
  dispatch: Dispatch<Action>;
  sendMsg: (msg: ClientMsg) => void;
  setGridReady: (ready: boolean) => void;
  initialState?: GridState;
  onStateUpdate: (event: StateUpdatedEvent<ITrack>) => void;
  onRowCountChange?: (count: number) => void;
  onArtistClick: (name: string) => void;
  onLabelClick: (name: string) => void;
  onShazamSimilar: (track: ITrack) => void;
}

type TracksGridContext = {
  playId: (id: string) => void;
  requestReply: RequestReply;
  updateRating: (yt_id: string, rating: number | null) => void;
  sendMsg: (msg: ClientMsg) => void;
  onArtistClick: (name: string) => void;
  onLabelClick: (name: string) => void;
  onShazamSimilar: (track: ITrack) => void;
};

export const TracksGrid = ({
  gridRef,
  currentId,
  playId,
  updateRating,
  requestReply,
  dispatch,
  sendMsg,
  setGridReady,
  initialState,
  onStateUpdate,
  onRowCountChange,
  onArtistClick,
  onLabelClick,
  onShazamSimilar,
}: TracksGridProps) => {
  const [colDefs] = useState<ColDef[]>([
    {
      field: "yt_id",
      headerName: "",
      cellRenderer: PlayControlRenderer,
      width: 180,
    },
    {
      field: "title",
      headerName: "Title",
      filter: true,
      width: 150,
      suppressAutoSize: true,
    },
    {
      field: "artists",
      headerName: "Artist",
      filter: true,
      type: "artists",
      cellRenderer: ArtistCellRenderer,
      width: 150,
      suppressAutoSize: true,
    },
    {
      field: "label",
      headerName: "Label",
      filter: true,
      cellRenderer: LabelCellRenderer,
      width: 150,
      suppressAutoSize: true,
    },
    {
      field: "release_date",
      headerName: "Release",
      sortable: true,
      initialSort: "desc",
      filter: "agDateColumnFilter",
      ...fixedWidth(120),
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
      width: 80,
      cellStyle: { textAlign: "right" },
    },
    {
      field: "last_played",
      headerName: "Last played",
      filter: true,
      ...fixedWidth(200),
    },
    {
      field: "bp_id",
      headerName: "Beatport ID",
      filter: "agNumberColumnFilter",
      width: 120,
      hide: true,
    },
    {
      field: "sp_id",
      headerName: "Spotify ID",
      filter: true,
      width: 120,
      hide: true,
    },
    {
      field: "mb_id",
      headerName: "MusicBrainz ID",
      filter: true,
      width: 150,
      hide: true,
    },
  ]);

  const rowSelection = useMemo<RowSelectionOptions>(() => {
    return {
      mode: "multiRow",
      headerCheckbox: false,
    };
  }, []);

  const autoSizeStrategy = useMemo<SizeColumnsToContentStrategy>(() => {
    return {
      type: "fitCellContents",
      scaleUpToFitGridWidth: true,
    };
  }, []);

  const defaultColDef = useMemo<ColDef>(() => {
    return {
      filterParams: {
        buttons: ["clear"],
      },
      // flex: 1,
    };
  }, []);

  const theme = useGridTheme();

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
  }, [requestReply, onRowCountChange, gridRef]);

  const rowClassRules = useMemo(() => {
    return {
      "progress-bar": (params: RowClassParams<ITrack>) => {
        return Boolean(currentId && params.data?.yt_id === currentId);
      },
    };
  }, [currentId]);

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
    };
  }, [
    updateRatingAndRefresh,
    requestReply,
    playId,
    sendMsg,
    onArtistClick,
    onLabelClick,
    onShazamSimilar,
  ]);

  const onSelectionChanged = useCallback(
    (ev: SelectionChangedEvent<ITrack>) => {
      dispatch({
        type: "selection-changed",
        selected:
          ev.selectedNodes?.flatMap((node) =>
            node.data ? [node.data.yt_id] : [],
          ) ?? [],
      });
    },
    [dispatch],
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

  return (
    <StackItem size="fill" className="grid">
      <AgGridReact<ITrack>
        theme={theme}
        onGridReady={onGridReady}
        ref={gridRef}
        columnTypes={columnTypes}
        rowClassRules={rowClassRules}
        initialState={initialState}
        onStateUpdated={onStateUpdate0}
        columnDefs={colDefs}
        defaultColDef={defaultColDef}
        rowSelection={rowSelection}
        autoSizeStrategy={autoSizeStrategy}
        rowHeight={32}
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
      />
    </StackItem>
  );
};
