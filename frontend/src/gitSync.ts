import type { StatusDotVariant } from "@astryxdesign/core/StatusDot";
import type { GitSyncState } from "./Types.tsx";

export const gitSyncLabels: Record<GitSyncState, string> = {
  "local-changes": "Local changes",
  syncing: "Syncing",
  synced: "Synced",
  "local-only": "Local only",
  offline: "Offline",
  "authentication-required": "Authentication required",
  "remote-ahead": "Upstream changes",
  "conflict-requires-attention": "Sync conflict",
  "corrupt-collection": "Invalid collection",
  error: "Sync error",
};

export function gitSyncVariant(state: GitSyncState): StatusDotVariant {
  if (state === "synced") return "success";
  if (state === "syncing") return "accent";
  if (state === "error" || state === "corrupt-collection") return "error";
  if (
    state === "offline" ||
    state === "authentication-required" ||
    state === "remote-ahead" ||
    state === "conflict-requires-attention"
  ) {
    return "warning";
  }
  return "neutral";
}
