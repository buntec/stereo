import type {
  ITrack,
  Action,
  RequestReply,
  ServerMsg,
  ClientMsg,
} from "./Types.tsx";

import { type CustomCellRendererProps } from "ag-grid-react";

import { IconButton } from "@astryxdesign/core/IconButton";
import { Tooltip } from "@astryxdesign/core/Tooltip";
import { HStack } from "@astryxdesign/core/Stack";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
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

import {
  useState,
  useMemo,
  useCallback,
  useEffect,
  type Dispatch,
} from "react";

import { CheckCircledIcon, PlayIcon, ColumnsIcon } from "@radix-ui/react-icons";

import { EditTrackDialog } from "./EditDialog.tsx";
import logger from "./logger.tsx";
import { MenuCheck } from "./Menu.tsx";

ModuleRegistry.registerModules([AllCommunityModule]);

const ArtistCellRenderer = (params: ICellRendererParams<ITrack, string[]>) => {
  const artists: string[] = params.value ?? [];
  const onArtistClick: ((name: string) => void) | undefined =
    params.context?.onArtistClick;

  return (
    <span>
      {artists.map((artist, i) => (
        <span key={artist}>
          {i > 0 && ", "}
          <span
            className="artist-link"
            onClick={(e) => {
              e.stopPropagation();
              onArtistClick?.(artist);
            }}
          >
            {artist}
          </span>
        </span>
      ))}
    </span>
  );
};

const LabelCellRenderer = (params: ICellRendererParams<ITrack, string>) => {
  const label = params.value;
  const onLabelClick: ((name: string) => void) | undefined =
    params.context?.onLabelClick;

  if (!label) return null;

  return (
    <span
      className="artist-link"
      onClick={(e) => {
        e.stopPropagation();
        onLabelClick?.(label);
      }}
    >
      {label}
    </span>
  );
};

const RatingRenderer = (params: ICellRendererParams<ITrack, number>) => {
  const [rating, setRating] = useState<number | null>(null);

  // very hacky!
  useEffect(() => {
    const t = setInterval(() => {
      if ((params.value ?? null) !== rating) {
        setRating(params.value ?? null);
      }
    }, 250);

    return () => {
      clearInterval(t);
    };
  }, [rating, setRating, params.value]);

  const handleClick = (index: number) => {
    const newRating = index >= 0 ? index + 1 : null;

    setRating(newRating);
    params.context.updateRating(params.data?.yt_id, newRating);
  };

  return (
    <HStack className="rating-stars">
      {[...Array(5)].map((_, i) => (
        <span
          key={i}
          className="rating-star"
          onClick={(ev) => {
            ev.stopPropagation();
            handleClick(i);
          }}
          onDoubleClick={(ev) => {
            ev.stopPropagation();
            handleClick(-1); // reset rating
          }}
          style={{
            color:
              i < (rating ?? 0)
                ? "var(--color-icon-yellow)"
                : "var(--color-icon-gray)",
          }}
        >
          {i < (rating ?? 0) ? "★" : "☆"}
        </span>
      ))}
    </HStack>
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

  const [value, setValue] = useState(data);

  const [isValid, setIsValid] = useState<boolean | null>(null);

  useEffect(() => {
    if (value !== data) {
      context.requestReply({ type: "validate-track", track: value }, (msg) => {
        if ("is_valid" in msg) {
          setIsValid(msg.is_valid);
        }
      });
    }
  }, [value, context, data]);

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
    <HStack gap={3} vAlign="center" className="track-control-cell">
      <Tooltip content="Open in YouTube">
        <a
          className="yt-thumbnail-link"
          href={`https://www.youtube.com/watch?v=${data.yt_id}`}
          target="_blank"
        >
          <img
            className="yt-thumbnail"
            src={`https://i.ytimg.com/vi/${data.yt_id}/default.jpg`}
            alt="YouTube Video Thumbnail"
            width="40"
          />
        </a>
      </Tooltip>
      <IconButton
        variant="ghost"
        size="sm"
        onClick={handlePlay}
        label="Play"
        tooltip="Play"
        icon={<PlayIcon />}
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

  const [exists, setExists] = useState<boolean | null>(null);

  useEffect(() => {
    if (data?.yt_id) {
      context.requestReply(
        { type: "collection-contains-id", yt_id: data.yt_id },
        (msg) => {
          if ("contains_id" in msg) {
            setExists(msg.contains_id);
          } else {
            throw new Error("unexpected response shape");
          }
        },
      );
    }
  }, [
    context,
    data?.yt_id,
    context.lastCollectionUpdate,
    context.requestReply,
  ]);

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
      {exists ? (
        <Tooltip content="Track exists in collection">
          <CheckCircledIcon color="green" />
        </Tooltip>
      ) : null}
    </HStack>
  );
};

interface SearchResultsGridProps {
  gridRef: React.RefObject<AgGridReact<ITrack> | null>;
  currentId?: string;
  playId: (id: string) => void;
  requestReply: RequestReply;
  dispatch: Dispatch<Action>;
  tracks?: ITrack[];
  lastCollectionUpdate?: number;
  onArtistClick: (name: string) => void;
  onLabelClick: (name: string) => void;
  appearance: "light" | "dark";
}

type SearchResultsGridContext = {
  playId: (id: string) => void;
  requestReply: RequestReply;
  lastCollectionUpdate?: number;
  dispatch: Dispatch<Action>;
  onArtistClick: (name: string) => void;
  onLabelClick: (name: string) => void;
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
  appearance,
}: SearchResultsGridProps) => {
  const [colDefs] = useState<ColDef[]>([
    {
      field: "yt_id",
      headerName: "",
      cellRenderer: SearchPlayControlRenderer,
      width: 140,
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
      initialSort: "desc",
      filter: true,
      width: 120,
    },
    {
      field: "genre",
      headerName: "Genre",
      filter: true,
    },
    {
      field: "bpm",
      headerName: "BPM",
      filter: true,
      width: 80,
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

  const theme = useMemo(() => {
    return appearance === "dark"
      ? themeQuartz.withPart(colorSchemeDark)
      : themeQuartz;
  }, [appearance]);

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
        return params.data?.yt_id === currentId;
      },
    };
  }, [currentId]);

  useEffect(() => {
    gridRef.current?.api?.redrawRows();
  }, [currentId, gridRef]);

  const context: SearchResultsGridContext = useMemo(() => {
    return {
      requestReply,
      playId,
      lastCollectionUpdate,
      dispatch,
      onArtistClick,
      onLabelClick,
      tracks: tracks ?? [],
    };
  }, [
    requestReply,
    playId,
    lastCollectionUpdate,
    dispatch,
    onArtistClick,
    onLabelClick,
    tracks,
  ]);

  const getRowId = useCallback(
    (params: GetRowIdParams<ITrack>) =>
      `${params.data.yt_id}:${params.data.bp_id}:${params.data.sp_id}:${params.data.mb_id}`,
    [],
  );

  return (
    <div
      key="search-results-grid"
      className="grid"
      style={{ flex: "1 1 auto", minHeight: 0, width: "100%" }}
    >
      <AgGridReact<ITrack>
        rowData={tracks}
        theme={theme}
        ref={gridRef}
        rowModelType={"clientSide"}
        getRowId={getRowId}
        columnTypes={columnTypes}
        columnDefs={colDefs}
        defaultColDef={defaultColDef}
        rowSelection={rowSelection}
        suppressCellFocus
        rowClassRules={rowClassRules}
        autoSizeStrategy={autoSizeStrategy}
        rowHeight={32}
        context={context}
        onSelectionChanged={onSelectionChanged}
        overlayComponentParams={{ loading: { overlayText: "Searching..." } }}
      />
    </div>
  );
};

type ToggleableColumn = {
  colId: string;
  headerName: string;
  visible: boolean;
};

type ColumnsMenuProps = {
  gridRef: React.RefObject<AgGridReact<ITrack> | null>;
};

export const ColumnsMenu = ({ gridRef }: ColumnsMenuProps) => {
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
        label: "Show/hide columns",
        icon: <ColumnsIcon />,
        isIconOnly: true,
        variant: "secondary",
        tooltip: "Show/hide columns",
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
  appearance: "light" | "dark";
}

type TracksGridContext = {
  playId: (id: string) => void;
  requestReply: RequestReply;
  updateRating: (yt_id: string, rating: number) => void;
  sendMsg: (msg: ClientMsg) => void;
  onArtistClick: (name: string) => void;
  onLabelClick: (name: string) => void;
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
  appearance,
}: TracksGridProps) => {
  const [colDefs] = useState<ColDef[]>([
    {
      field: "yt_id",
      headerName: "",
      cellRenderer: PlayControlRenderer,
      width: 140,
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
      width: 120,
    },
    {
      field: "genre",
      headerName: "Genre",
      filter: true,
    },
    {
      field: "bpm",
      headerName: "BPM",
      filter: "agNumberColumnFilter",
      width: 80,
      cellStyle: { textAlign: "right" },
    },
    {
      field: "key",
      headerName: "Key",
      filter: true,
      width: 80,
    },
    {
      field: "rating",
      headerName: "Rating",
      filter: "agNumberColumnFilter",
      cellRenderer: RatingRenderer,
    },
    {
      field: "play_count",
      headerName: "Plays",
      filter: "agNumberColumnFilter",
      width: 80,
      cellStyle: { textAlign: "right" },
    },
    { field: "last_played", headerName: "Last played", filter: true },
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

  const theme = useMemo(() => {
    return appearance === "dark"
      ? themeQuartz.withPart(colorSchemeDark)
      : themeQuartz;
  }, [appearance]);

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
              }
            } else {
              params.failCallback();
            }
          },
          3000,
        );
      },
    };
  }, [requestReply, onRowCountChange]);

  const rowClassRules = useMemo(() => {
    return {
      "progress-bar": (params: RowClassParams<ITrack>) => {
        return params.data?.yt_id === currentId;
      },
    };
  }, [currentId]);

  const updateRatingAndRefresh = useCallback(
    (yt_id: string, rating: number) => {
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
    };
  }, [
    updateRatingAndRefresh,
    requestReply,
    playId,
    sendMsg,
    onArtistClick,
    onLabelClick,
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
    <div
      key="tracks-grid"
      className="grid"
      style={{ flex: "1 1 auto", minHeight: 0, width: "100%" }}
    >
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
        suppressCellFocus
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
        datasource={dataSource}
        onSelectionChanged={onSelectionChanged}
      />
    </div>
  );
};
