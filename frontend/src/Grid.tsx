import type {
  ITrack,
  Action,
  RequestReply,
  ServerMsg,
  ClientMsg,
} from "./Types.tsx";
import { type CustomCellRendererProps } from "ag-grid-react";
import { IconButton, Tooltip, Flex } from "@radix-ui/themes";
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
  type SizeColumnsToContentStrategy,
} from "ag-grid-community";

import { AllCommunityModule, ModuleRegistry } from "ag-grid-community";

import {
  useState,
  useMemo,
  useCallback,
  useEffect,
  type Dispatch,
} from "react";

import { CheckCircledIcon, PlayIcon, ResumeIcon } from "@radix-ui/react-icons";

import { EditTrackDialog } from "./EditDialog.tsx";

ModuleRegistry.registerModules([AllCommunityModule]);

const RatingRenderer = (params: ICellRendererParams<ITrack, number>) => {
  const [rating, setRating] = useState<number | null>(null);

  useEffect(() => {
    setRating(params.value ?? null);
  }, [params.value]);

  const handleClick = (index: number) => {
    const newRating = index >= 0 ? index + 1 : null;

    setRating(newRating);
    if (params.context?.updateRating) {
      params.context.updateRating(params.data?.yt_id, newRating);
    }
  };

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        height: "100%",
        cursor: "pointer",
      }}
    >
      {[...Array(5)].map((_, i) => (
        <span
          key={i}
          onClick={(ev) => {
            ev.stopPropagation();
            handleClick(i);
          }}
          onDoubleClick={(ev) => {
            ev.stopPropagation();
            handleClick(-1); // reset rating
          }}
          style={{
            color: i < (rating ?? 0) ? "var(--yellow-8)" : "var(--gray-8)",
            fontSize: "1.2rem",
          }}
        >
          {i < (rating ?? 0) ? "★" : "☆"}
        </span>
      ))}
    </div>
  );
};

const PlayControlRenderer: React.FC<
  CustomCellRendererProps<ITrack, any, TracksGridContext>
> = (params) => {
  const { data, node, api, context } = params;

  if (!data) return null;

  const handlePlayFromHere = useCallback(() => {
    const startRow = node.rowIndex ?? 0;
    const endRow = startRow + 200; // 200 seems to be the max supported by YT
    const filterModel = api.getState().filter?.filterModel;
    const sortModel = api.getState().sort?.sortModel;
    context.requestReply(
      { type: "get-rows", startRow, endRow, filterModel, sortModel },
      function (msg: ServerMsg | { type: string }) {
        if ("rows" in msg) {
          context.playIds(msg.rows.map((t: ITrack) => t.yt_id));
        } else {
          console.log("failed to get rows");
        }
      },
    );
  }, [params]);

  const handlePlay = useCallback(() => {
    context.playIds([data.yt_id]);
  }, [params]);

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
  }, [value]);

  const commit = useCallback(() => {
    context.sendMsg({ type: "update-track", old: data, new: value });
  }, [context, data, value]);

  const resetValue = useCallback(() => {
    setValue(data);
  }, [data]);

  return (
    <Flex gap="3" align="center">
      <img
        src={`https://i.ytimg.com/vi/${data.yt_id}/default.jpg`}
        alt="YouTube Video Thumbnail"
        width="40"
      />
      <Tooltip content="Play">
        <IconButton
          variant="ghost"
          size="1"
          onClick={handlePlay}
          aria-label="Play"
        >
          <PlayIcon />
        </IconButton>
      </Tooltip>

      <Tooltip content="Play from here">
        <IconButton
          variant="ghost"
          size="1"
          onClick={handlePlayFromHere}
          aria-label="Play from here"
        >
          <ResumeIcon />
        </IconButton>
      </Tooltip>

      <EditTrackDialog
        isValid={isValid}
        value={value}
        setValue={setValue}
        commit={commit}
        resetValue={resetValue}
      />
    </Flex>
  );
};

const SearchPlayControlRenderer: React.FC<
  CustomCellRendererProps<ITrack, any, SearchResultsGridContext>
> = (params) => {
  const { data, context } = params;

  if (!data) return null;

  const handlePlay = useCallback(() => {
    context.playIds([data.yt_id]);
  }, [params]);

  const [exists, setExists] = useState<boolean | null>(null);

  useEffect(() => {
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
  }, [data.yt_id, context.lastCollectionUpdate, context.requestReply]);

  return (
    <Flex gap="2" align="center">
      <img
        src={`https://i.ytimg.com/vi/${data.yt_id}/default.jpg`}
        alt="YouTube Video Thumbnail"
        width="40"
      />
      <Tooltip content="Play">
        <IconButton
          variant="soft"
          size="1"
          onClick={handlePlay}
          aria-label="Play"
        >
          <PlayIcon />
        </IconButton>
      </Tooltip>
      {exists ? (
        <Tooltip content="Track exists in collection">
          <CheckCircledIcon color="green" />
        </Tooltip>
      ) : null}
    </Flex>
  );
};

interface SearchResultsGridProps {
  gridRef: React.RefObject<AgGridReact<ITrack> | null>;
  currentId?: string;
  playIds: (ids: string[]) => void;
  requestReply: RequestReply;
  dispatch: Dispatch<Action>;
  tracks?: ITrack[];
  lastCollectionUpdate?: number;
}

type SearchResultsGridContext = {
  playIds: (ids: string[]) => void;
  requestReply: RequestReply;
  lastCollectionUpdate?: number;
};

export const SearchResultsGrid = ({
  gridRef,
  playIds,
  requestReply,
  dispatch,
  tracks,
  lastCollectionUpdate,
}: SearchResultsGridProps) => {
  const [colDefs] = useState<ColDef[]>([
    {
      field: "yt_id",
      headerName: "",
      cellRenderer: SearchPlayControlRenderer,
      width: 140,
    },
    { field: "title", headerName: "Title", filter: true },
    { field: "mix_name", headerName: "Mix", filter: true },
    { field: "artists", headerName: "Artist", filter: true, type: "artists" },
    { field: "label", headerName: "Label", filter: true },
    {
      field: "release_date",
      headerName: "Release",
      sortable: true,
      initialSort: "desc",
      filter: true,
      width: 120,
    },
    {
      field: "bpm",
      headerName: "BPM",
      filter: true,
      width: 80,
      cellStyle: { "text-align": "right" },
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
    return themeQuartz.withParams({});
  }, []);

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

  const context: SearchResultsGridContext = useMemo(() => {
    return { requestReply, playIds, lastCollectionUpdate };
  }, [requestReply, playIds, lastCollectionUpdate]);

  const getRowId = useCallback(
    (params: GetRowIdParams<ITrack>) =>
      `${params.data.yt_id}:${params.data.bp_id}:${params.data.mb_id}`,
    [],
  );

  return (
    <div
      className="grid search-results"
      style={{ height: "100%", width: "100%" }}
    >
      <AgGridReact<ITrack>
        debug
        rowData={tracks}
        theme={theme}
        ref={gridRef}
        rowModelType={"clientSide"}
        getRowId={getRowId}
        columnTypes={columnTypes}
        columnDefs={colDefs}
        defaultColDef={defaultColDef}
        rowSelection={rowSelection}
        autoSizeStrategy={autoSizeStrategy}
        rowHeight={32}
        context={context}
        onSelectionChanged={onSelectionChanged}
      />
    </div>
  );
};

interface TracksGridProps {
  gridRef: React.RefObject<AgGridReact<ITrack> | null>;
  currentId?: string;
  playIds: (ids: string[]) => void;
  updateRating: (yt_id: string, rating: number | null) => void;
  requestReply: RequestReply;
  dispatch: Dispatch<Action>;
  sendMsg: (msg: ClientMsg) => void;
}

type TracksGridContext = {
  playIds: (ids: string[]) => void;
  requestReply: RequestReply;
  updateRating: (yt_id: string, rating: number) => void;
  sendMsg: (msg: ClientMsg) => void;
};

export const TracksGrid = ({
  gridRef,
  currentId,
  playIds,
  updateRating,
  requestReply,
  dispatch,
  sendMsg,
}: TracksGridProps) => {
  const [colDefs] = useState<ColDef[]>([
    {
      field: "yt_id",
      headerName: "",
      cellRenderer: PlayControlRenderer,
      width: 140,
    },
    { field: "title", headerName: "Title", filter: true },
    { field: "artists", headerName: "Artist", filter: true, type: "artists" },
    { field: "label", headerName: "Label", filter: true },
    {
      field: "release_date",
      headerName: "Release",
      sortable: true,
      initialSort: "desc",
      filter: true,
      width: 120,
    },
    {
      field: "bpm",
      headerName: "BPM",
      filter: true,
      width: 80,
      cellStyle: { "text-align": "right" },
    },
    {
      field: "rating",
      headerName: "Rating",
      filter: true,
      cellRenderer: RatingRenderer,
    },
    {
      field: "play_count",
      headerName: "Plays",
      filter: true,
      width: 80,
      cellStyle: { "text-align": "right" },
    },
    { field: "last_played", headerName: "Last played", filter: true },
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
    return themeQuartz.withParams({});
  }, []);

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
        console.log("asking for " + params.startRow + " to " + params.endRow);
        requestReply(
          { type: "get-rows", ...params },
          function (msg: ServerMsg | { type: string }) {
            if ("rows" in msg) {
              params.successCallback(msg.rows, msg.last_row);
            } else {
              params.failCallback();
            }
          },
          3000,
        );
      },
    };
  }, []);

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
    [updateRating],
  );

  const context: TracksGridContext = useMemo(() => {
    return {
      updateRating: updateRatingAndRefresh,
      requestReply,
      playIds,
      sendMsg,
    };
  }, [updateRatingAndRefresh, requestReply, playIds, sendMsg]);

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
      `${params.data.yt_id}:${params.data.bp_id}:${params.data.mb_id}`,
    [],
  );

  return (
    <div className="grid" style={{ height: "100%", width: "100%" }}>
      <AgGridReact<ITrack>
        debug
        theme={theme}
        ref={gridRef}
        columnTypes={columnTypes}
        rowClassRules={rowClassRules}
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
        cacheOverflowSize={2}
        maxConcurrentDatasourceRequests={4}
        infiniteInitialRowCount={1000}
        maxBlocksInCache={10}
        datasource={dataSource}
        onSelectionChanged={onSelectionChanged}
      />
    </div>
  );
};
