/**
 * The playing track's progress (0–1) for its grid row's progress bar
 * (`--stereo-playback-progress`, read by `.grid .progress-bar` in App.css).
 * Set on the playing row's elements themselves, which restyles only them:
 * - not on the document, where it's inherited by every element, and
 *   recomputing all of their styles takes some 20ms on a full grid, every
 *   second, which visibly stalls the animations timed by the beats;
 * - nor in a stylesheet rule of its own: WebKit (the desktop app) restyles the
 *   whole document for any change to a stylesheet, some 90ms on a full grid,
 *   which stalls the disco ball's rotation every second.
 *
 * Rows the grid renders in between get it from `rowProgressStyle`.
 */
const PROPERTY = "--stereo-playback-progress";

let progress = 0;

export const setRowProgress = (fraction: number) => {
  progress = fraction;
  // A row is one element per pinned section of the grid.
  for (const row of document.querySelectorAll<HTMLElement>(
    ".ag-row.progress-bar",
  ))
    row.style.setProperty(PROPERTY, `${fraction}`);
};

/** The playing row's style (ag-Grid's `getRowStyle`), for when it's rendered. */
export const rowProgressStyle = () => ({ [PROPERTY]: `${progress}` });
