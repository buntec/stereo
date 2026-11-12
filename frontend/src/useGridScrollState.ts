import { useCallback, useMemo, useState, type RefObject } from "react";
import type { AgGridReact } from "ag-grid-react";
import type { GridApi } from "ag-grid-community";
import type { ITrack } from "./Types.tsx";

/** Scrolls a grid back to its first row. */
export const scrollGridToTop = (api: GridApi<ITrack> | undefined) => {
  if (!api || api.isDestroyed() || api.getDisplayedRowCount() === 0) return;
  api.ensureIndexVisible(0, "top");
};

type ScrollState = {
  // More than a screenful down.
  scrolledDown: boolean;
  // Tracks whose rows are on screen.
  visibleIds: string[];
};

const sameState = (a: ScrollState, b: ScrollState) =>
  a.scrolledDown === b.scrolledDown &&
  a.visibleIds.length === b.visibleIds.length &&
  a.visibleIds.every((id, i) => id === b.visibleIds[i]);

/**
 * Tracks whether a grid is scrolled down far enough to offer "back to top", and whether the
 * playing track's row is on screen. Spread `gridEvents` onto the `AgGridReact`.
 */
export const useGridScrollState = (
  gridRef: RefObject<AgGridReact<ITrack> | null>,
  currentId: string | undefined,
) => {
  const [state, setState] = useState<ScrollState>({
    scrolledDown: false,
    visibleIds: [],
  });

  const update = useCallback(() => {
    const api = gridRef.current?.api;
    if (!api || api.isDestroyed()) return;
    const { top, bottom } = api.getVerticalPixelRange();
    const visibleIds = api.getRenderedNodes().flatMap((node) => {
      if (!node.data || node.rowTop == null || node.rowHeight == null)
        return [];
      // Rendered rows include a buffer outside the viewport; count a row as visible when its
      // middle is inside it.
      const middle = node.rowTop + node.rowHeight / 2;
      return middle >= top && middle <= bottom ? [node.data.yt_id] : [];
    });
    const next = { scrolledDown: top > bottom - top, visibleIds };
    setState((prev) => (sameState(prev, next) ? prev : next));
  }, [gridRef]);

  const gridEvents = useMemo(
    () => ({
      onBodyScroll: update,
      // Rows rendered (or, in the infinite model, their data loaded), filter/sort changed.
      onViewportChanged: update,
      onModelUpdated: update,
    }),
    [update],
  );

  return {
    scrolledDown: state.scrolledDown,
    currentVisible: !!currentId && state.visibleIds.includes(currentId),
    gridEvents,
  };
};
