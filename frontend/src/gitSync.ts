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

/** Whether the sync state is worth a button beside the collection menu: when the user
    can or should act on it. Routine states are conveyed by the menu's status dot. */
export function showGitSyncStatus(
  state: GitSyncState | undefined,
  retryable: boolean | undefined,
): boolean {
  if (!state) return false;
  const variant = gitSyncVariant(state);
  return (
    !!retryable ||
    state === "conflict-requires-attention" ||
    variant === "warning" ||
    variant === "error"
  );
}
