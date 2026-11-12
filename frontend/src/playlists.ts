import type { IPlaylist, PlaylistInput } from "./Types.tsx";

/** The editable part of a playlist, to send back with changes in `save-playlist`. */
export function playlistInput(p: IPlaylist): PlaylistInput {
  return {
    id: p.id,
    name: p.name,
    collection: p.collection,
    filter_model: p.filter_model,
    sort_model: p.sort_model,
    limit: p.limit,
    spotify: p.spotify,
    youtube: p.youtube,
  };
}

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
