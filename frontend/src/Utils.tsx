import { useState, useEffect, useRef } from "react";
import type { ICollectionStats } from "./Types.tsx";
import { currentWsToken } from "./wsToken.ts";

declare global {
  interface Window {
    __STEREO_DESKTOP__?: boolean;
  }
}

/** Whether this page runs in Stereo's desktop app (which sets the flag, see
 * `desktop/src-tauri/src/lib.rs`) rather than in a browser tab. */
export const inDesktopApp = window.__STEREO_DESKTOP__ === true;

/** Opens this Stereo in the user's web browser; the desktop app opens new
 * windows there. For what needs the browser's cookies together with its
 * User-Agent, which the desktop app's webview doesn't share. With this page's
 * WebSocket token, which the browser may not know (`App.tsx` picks it up). */
export const openInBrowser = () => {
  const token = currentWsToken();
  const fragment = token ? `#stereo-token=${encodeURIComponent(token)}` : "";
  window.open(
    `${window.location.origin}/${fragment}`,
    "_blank",
    "noopener,noreferrer",
  );
};

/** Downloads `url`, which the server sends as an attachment. Not in the desktop
 * app, whose webview has no downloads (see `CollectionDownloadDialog.tsx`). */
export const downloadUrl = (url: string) => {
  const a = document.createElement("a");
  a.href = url;
  a.download = "";
  a.click();
};

export function shuffleArray<T>(array: T[]): T[] {
  for (let i = array.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));

    [array[i], array[j]] = [array[j], array[i]];
  }
  return array;
}

/** A time as m:ss, e.g. 3:07 (whole seconds, never negative). */
export const formatTime = (seconds: number) => {
  const s = Math.max(0, Math.floor(seconds));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
};

export function formatDuration(totalSeconds: number): string {
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = Math.floor(totalSeconds % 60);

  const paddedMinutes = String(minutes).padStart(2, "0");
  const paddedSeconds = String(seconds).padStart(2, "0");

  return `${paddedMinutes}:${paddedSeconds}`;
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/** The value stored at `key`, parsed; undefined if there's none. */
const readStored = (key: string): unknown => {
  const item = window.localStorage.getItem(key);
  // "undefined": an unset value, as stored by earlier versions.
  return item && item !== "undefined" ? JSON.parse(item) : undefined;
};

/** The value stored at `key`, parsed; undefined if there's none or it doesn't parse. */
const readStoredOrUndefined = (key: string): unknown => {
  try {
    return readStored(key);
  } catch {
    return undefined;
  }
};

/**
 * State kept in localStorage at `key`, `initialValue` until something is
 * stored there; `migrate` brings a stored value up to date as it's read.
 *
 * Other tabs share the storage but each keeps its own state, so an object's
 * changes are stored as changes to the latest stored object, field by field:
 * a tab changing one field doesn't overwrite fields another tab has changed
 * since (say, the visual effects one reset) with what it last saw.
 */
export function useLocalStorage<T>(
  key: string,
  initialValue: T | (() => T),
  migrate?: (stored: T) => T,
) {
  const [storedValue, setStoredValue] = useState<T>(() => {
    const initial = () =>
      initialValue instanceof Function ? initialValue() : initialValue;
    try {
      const stored = readStored(key) as T | undefined;
      if (stored === undefined) return initial();
      return migrate ? migrate(stored) : stored;
    } catch (error) {
      console.error(`Error reading local storage at "${key}":`, error);
      return initial();
    }
  });
  // What this tab last stored: its changes since are what it stores next.
  const last = useRef<T | undefined>(undefined);

  useEffect(() => {
    const previous = last.current;
    last.current = storedValue;
    try {
      // JSON has no undefined (`JSON.stringify` returns undefined, which
      // `setItem` would store as "undefined"): an unset value is no entry.
      if (storedValue === undefined) {
        window.localStorage.removeItem(key);
        return;
      }
      let value: unknown = storedValue;
      // Only objects merge with what's stored; anything else, or a stored
      // value that doesn't parse, is replaced outright.
      const latest =
        isRecord(previous) && isRecord(storedValue)
          ? readStoredOrUndefined(key)
          : undefined;
      if (isRecord(previous) && isRecord(storedValue) && isRecord(latest)) {
        const merged: Record<string, unknown> = { ...latest };
        for (const field of new Set([
          ...Object.keys(previous),
          ...Object.keys(storedValue),
        ])) {
          if (Object.is(previous[field], storedValue[field])) continue;
          if (storedValue[field] === undefined) delete merged[field];
          else merged[field] = storedValue[field];
        }
        value = merged;
      }
      window.localStorage.setItem(key, JSON.stringify(value));
    } catch (error) {
      console.error(`Error setting local storage at "${key}":`, error);
    }
  }, [key, storedValue]);

  return [storedValue, setStoredValue] as const;
}

export function hasCollectionStats(stats?: ICollectionStats): boolean {
  return (
    !!stats &&
    (stats.top_artists_five_star.length > 0 ||
      stats.top_artists_four_plus.length > 0 ||
      stats.top_labels_five_star.length > 0 ||
      stats.top_labels_four_plus.length > 0)
  );
}

/** How long ago `ms` was, to follow a verb: "just now", "5 min ago", "3 h ago"
 * or, from a day on, "on 11/21/2026". */
export function timeAgo(ms: number): string {
  const minutes = Math.round((Date.now() - ms) / 60000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  return `on ${new Date(ms).toLocaleDateString()}`;
}
