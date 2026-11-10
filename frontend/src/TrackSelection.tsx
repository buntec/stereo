import { useCallback, useRef, useState, type RefObject } from "react";
import type { AgGridReact } from "ag-grid-react";
import type {
  FilterModel,
  IRowNode,
  SelectionChangedEvent,
} from "ag-grid-community";
import type { ITrack, RequestReply } from "./Types.tsx";

// The collection grid uses ag-Grid's infinite row model, which only holds the blocks it has
// loaded and can't select the others. So "select all" is kept here instead: the ids of every
// track matching the grid's filters, minus rows deselected since. Rows are selected in the grid
// as they load, to show it, and `getSelectedTracks` resolves the whole selection.

type SelectAll = {
  ids: Set<string>;
  filterModel: FilterModel;
};

// Fetches every matching row; the server's `get-rows` limit is `endRow - startRow`.
const ALL_ROWS = Number.MAX_SAFE_INTEGER;

const idsOf = (nodes: IRowNode<ITrack>[] | null | undefined) =>
  new Set(nodes?.flatMap((node) => (node.data ? [node.data.yt_id] : [])));

export function useTrackSelection(
  gridRef: RefObject<AgGridReact<ITrack> | null>,
  requestReply: RequestReply,
) {
  const selectAllRef = useRef<SelectAll | null>(null);
  // Bumped whenever the select-all ends, so that a reply still in flight is dropped.
  const generation = useRef(0);
  // The grid's own selection after its last change.
  const gridSelection = useRef<Set<string>>(new Set());
  const [gridCount, setGridCount] = useState(0);
  const [allCount, setAllCount] = useState<number | null>(null);

  const exitSelectAll = useCallback(() => {
    generation.current++;
    selectAllRef.current = null;
    setAllCount(null);
  }, []);

  // Selects the loaded rows that are part of a select-all; called whenever rows load.
  const selectLoadedRows = useCallback(() => {
    const api = gridRef.current?.api;
    const all = selectAllRef.current;
    if (!api || !all) return;
    const nodes: IRowNode<ITrack>[] = [];
    api.forEachNode((node) => {
      if (node.data && all.ids.has(node.data.yt_id) && !node.isSelected()) {
        nodes.push(node);
      }
    });
    if (nodes.length > 0) {
      api.setNodesSelected({ nodes, newValue: true, source: "api" });
    }
  }, [gridRef]);

  const selectAll = useCallback(() => {
    const api = gridRef.current?.api;
    if (!api) return;
    const filterModel = api.getFilterModel();
    const gen = ++generation.current;
    requestReply(
      {
        type: "get-rows",
        startRow: 0,
        endRow: ALL_ROWS,
        filterModel,
        sortModel: [],
      },
      (msg) => {
        if (gen !== generation.current || !("rows" in msg)) return;
        const ids = new Set(msg.rows.map((t) => t.yt_id));
        selectAllRef.current = { ids, filterModel };
        setAllCount(ids.size);
        selectLoadedRows();
      },
    );
  }, [gridRef, requestReply, selectLoadedRows]);

  const deselectAll = useCallback(() => {
    exitSelectAll();
    gridRef.current?.api?.deselectAll();
  }, [exitSelectAll, gridRef]);

  // A select-all covers the filters it was made with, so changing them ends it.
  const onFilterChanged = useCallback(() => {
    if (selectAllRef.current) {
      deselectAll();
    } else {
      generation.current++;
    }
  }, [deselectAll]);

  const onSelectionChanged = useCallback(
    (ev: SelectionChangedEvent<ITrack>) => {
      const previous = gridSelection.current;
      const current = idsOf(ev.selectedNodes);
      gridSelection.current = current;
      setGridCount(current.size);
      const all = selectAllRef.current;
      // "api" is selectLoadedRows extending the grid's selection.
      if (!all || ev.source === "api") return;
      const removed = [...previous].filter((id) => !current.has(id));
      const added = [...current].some((id) => !previous.has(id));
      if (!added && removed.length === 1) {
        // A row toggled off (Cmd/Ctrl-click, checkbox): it leaves the selection.
        all.ids.delete(removed[0]);
        setAllCount(all.ids.size);
      } else {
        // Anything else (a plain click, a range) replaces the selection with the grid's.
        exitSelectAll();
      }
    },
    [exitSelectAll],
  );

  // Resolves the selected tracks with their current data, in the grid's sort order for a
  // select-all.
  const getSelectedTracks = useCallback(
    (cb: (tracks: ITrack[]) => void) => {
      const api = gridRef.current?.api;
      if (!api) return;
      const all = selectAllRef.current;
      if (!all) {
        cb(api.getSelectedRows());
        return;
      }
      requestReply(
        {
          type: "get-rows",
          startRow: 0,
          endRow: ALL_ROWS,
          filterModel: all.filterModel,
          sortModel: api.getState().sort?.sortModel ?? [],
        },
        (msg) => {
          if ("rows" in msg) {
            cb(msg.rows.filter((t) => all.ids.has(t.yt_id)));
          }
        },
      );
    },
    [gridRef, requestReply],
  );

  return {
    selectionCount: allCount ?? gridCount,
    isAllSelected: allCount !== null,
    selectAll,
    deselectAll,
    selectLoadedRows,
    onFilterChanged,
    onSelectionChanged,
    getSelectedTracks,
  };
}
