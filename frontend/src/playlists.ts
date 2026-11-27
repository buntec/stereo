import {
  playlistSyncKey,
  type IPlaylist,
  type PlaylistService,
  type PlaylistSyncStatus,
} from "./Types.tsx";

const services: PlaylistService[] = ["spotify", "youtube"];

/** The error of a playlist's export to a service, if its last sync failed:
    the live one from this session, else the saved one unless a sync is running.
    An export that is turned off has none, even if it failed before. */
export function exportError(
  p: IPlaylist,
  service: PlaylistService,
  live: PlaylistSyncStatus | undefined,
): string | null {
  if (!p[service]) return null;
  if (live?.state === "failed") return live.error ?? "Sync failed";
  if (live?.state === "running") return null;
  return p[`${service}_status`]?.error ?? null;
}

/** Whether any of a playlist's exports failed its last sync. */
export const hasExportError = (
  p: IPlaylist,
  syncStatus: Record<string, PlaylistSyncStatus>,
) =>
  services.some(
    (s) => exportError(p, s, syncStatus[playlistSyncKey(p.id, s)]) !== null,
  );

/** What a playlist's view shows, in words. */
export function describeView(p: IPlaylist): string {
  const filters = Object.keys(p.filter_model);
  const parts = [
    filters.length ? `filtered by ${filters.join(", ")}` : "all tracks",
    p.sort_model.length
      ? `sorted by ${p.sort_model.map((s) => `${s.colId} ${s.sort === "asc" ? "↑" : "↓"}`).join(", ")}`
      : undefined,
    p.limit ? `first ${p.limit}` : undefined,
  ];
  return parts.filter(Boolean).join(" · ");
}

/** A filter model with keys sorted and empty values dropped, for comparisons. */
const normalizeModel = (value: unknown): string =>
  JSON.stringify(value ?? {}, (_key, v: unknown) =>
    v && typeof v === "object" && !Array.isArray(v)
      ? Object.fromEntries(
          Object.entries(v)
            .filter(([, x]) => x !== null && x !== undefined)
            .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)),
        )
      : v,
  );

type View = Pick<IPlaylist, "collection" | "filter_model" | "sort_model">;

/** Whether two views show the same: collection, filters and sorting. */
export const sameView = (a: View, b: View) =>
  a.collection === b.collection &&
  normalizeModel(a.filter_model) === normalizeModel(b.filter_model) &&
  a.sort_model.map((s) => `${s.colId}:${s.sort}`).join(",") ===
    b.sort_model.map((s) => `${s.colId}:${s.sort}`).join(",");
