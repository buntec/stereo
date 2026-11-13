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
  visible: ITrack[];
};

const sameState = (a: ScrollState, b: ScrollState) =>
  a.scrolledDown === b.scrolledDown &&
  a.visible.length === b.visible.length &&
  a.visible.every((track, i) => track === b.visible[i]);

/**
 * Tracks whether a grid is scrolled down far enough to offer "back to top", and whether the
 * playing track's row is on screen. Spread `gridEvents` onto the `AgGridReact`.
 */
export const useGridScrollState = (
  gridRef: RefObject<AgGridReact<ITrack> | null>,
  // Whether a row is the playing track's; unset when nothing plays.
  isCurrent: ((track: ITrack) => boolean) | undefined,
) => {
  const [state, setState] = useState<ScrollState>({
    scrolledDown: false,
    visible: [],
  });

  const update = useCallback(() => {
    const api = gridRef.current?.api;
    if (!api || api.isDestroyed()) return;
    const { top, bottom } = api.getVerticalPixelRange();
    const visible = api.getRenderedNodes().flatMap((node) => {
      if (!node.data || node.rowTop == null || node.rowHeight == null)
        return [];
      // Rendered rows include a buffer outside the viewport; count a row as visible when its
      // middle is inside it.
      const middle = node.rowTop + node.rowHeight / 2;
      return middle >= top && middle <= bottom ? [node.data] : [];
    });
    const next = { scrolledDown: top > bottom - top, visible };
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
    currentVisible: !!isCurrent && state.visible.some(isCurrent),
    gridEvents,
  };
};
