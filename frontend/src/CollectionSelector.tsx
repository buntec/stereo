import { useState } from "react";
import {
  DropdownMenu,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
} from "@astryxdesign/core/DropdownMenu";
import { Divider } from "@astryxdesign/core/Divider";
import { StatusDot } from "@astryxdesign/core/StatusDot";
import { ButtonGroup } from "@astryxdesign/core/ButtonGroup";
import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import {
  BarChartIcon,
  DownloadIcon,
  Link2Icon,
  Pencil1Icon,
  PlusIcon,
  TrashIcon,
} from "@radix-ui/react-icons";
import { MenuHeading } from "./Menu.tsx";
import NewCollectionDialog from "./NewCollectionDialog.tsx";
import RenameCollectionDialog from "./RenameCollectionDialog.tsx";
import CloneRepoDialog from "./CloneRepoDialog.tsx";
import GitRemoteDialog from "./GitRemoteDialog.tsx";
import { GitConflictDialog, GitSyncStatusButton } from "./GitSyncStatus.tsx";
import { gitSyncLabels, gitSyncVariant, showGitSyncStatus } from "./gitSync.ts";
import type { GitSyncState, ICollection, IRepo } from "./Types.tsx";

function basename(path: string): string {
  return path.split("/").pop() ?? path;
}

export function collectionLabel(path: string): string {
  return basename(path).replace(/\.db$/, "");
}

export type CollectionSelectorProps = {
  collections: ICollection[];
  value: string;
  setValue: (path: string) => void;
  onCreate: (name: string) => void;
  onDelete: (path: string) => void;
  onRename: (path: string, name: string) => void;
  isValid: boolean;
  repos: IRepo[];
  repoPath: string;
  setRepoPath: (path: string) => void;
  onClone: (url: string) => void;
  // Sets (or with null, removes) a repository's remote; resolves once it is set.
  onSetRemote: (repoPath: string, url: string | null) => Promise<void>;
  syncState?: GitSyncState;
  syncMessage?: string;
  syncRetryable?: boolean;
  onRetrySync: () => void;
  onResolveConflict: (strategy: "keep-local" | "keep-upstream") => void;
  // Undefined when there are no stats to show.
  onShowStats?: () => void;
};

function CollectionSelector({
  collections,
  value,
  setValue,
  onCreate,
  onDelete,
  onRename,
  isValid,
  repos,
  repoPath,
  setRepoPath,
  onClone,
  onSetRemote,
  syncState,
  syncMessage,
  syncRetryable,
  onRetrySync,
  onResolveConflict,
  onShowStats,
}: CollectionSelectorProps) {
  const [createOpen, setCreateOpen] = useState(false);
  const [cloneOpen, setCloneOpen] = useState(false);
  const [remoteOpen, setRemoteOpen] = useState(false);
  // The repository whose remote the dialog edits: the one selected when it opened, even if
  // the selection changes meanwhile.
  const [remoteRepoPath, setRemoteRepoPath] = useState<string>();
  // Remounts the remote dialog on every opening, so it starts from the current remote.
  const [remoteKey, setRemoteKey] = useState(0);
  // The collection awaiting delete confirmation.
  const [pendingDelete, setPendingDelete] = useState<string | null>(null);
  // The collection being renamed.
  const [pendingRename, setPendingRename] = useState<string | null>(null);
  const [resolveOpen, setResolveOpen] = useState(false);

  const selectedCollection = collections.some((c) => c.path === value)
    ? value
    : undefined;
  const selectedRepo = repos.find((r) => r.path === repoPath);

  const status = !isValid
    ? { variant: "error" as const, label: "Invalid collection" }
    : syncState
      ? { variant: gitSyncVariant(syncState), label: gitSyncLabels[syncState] }
      : { variant: "neutral" as const, label: "Not synchronized" };

  const tooltip = [
    selectedRepo && basename(selectedRepo.path),
    syncMessage ?? status.label,
  ]
    .filter(Boolean)
    .join(" · ");

  // An actionable sync status joins the menu's button, so it reads as the collection's.
  const showSync = showGitSyncStatus(syncState, syncRetryable);

  const menu = (
    <DropdownMenu
      menuWidth={300}
      button={{
        label: selectedCollection
          ? collectionLabel(selectedCollection)
          : "Select collection",
        // Grouped with the sync status, both are outlined to show the group's outline.
        variant: showSync ? "secondary" : "ghost",
        width: showSync ? undefined : 220,
        // Shrinks (truncating the name) to stay beside the logo on narrow screens.
        style: { maxWidth: "100%", minWidth: 0 },
        tooltip,
        icon: (
          <StatusDot
            variant={status.variant}
            label={status.label}
            isPulsing={syncState === "syncing"}
          />
        ),
      }}
    >
      <MenuHeading>Collections</MenuHeading>
      <DropdownMenuRadioGroup
        label="Collections"
        value={selectedCollection}
        onChange={setValue}
      >
        {collections.map((c) => (
          <DropdownMenuRadioItem
            key={c.path}
            value={c.path}
            label={collectionLabel(c.path)}
          />
        ))}
      </DropdownMenuRadioGroup>
      <DropdownMenuItem
        icon={<PlusIcon />}
        label="New collection…"
        onClick={() => setCreateOpen(true)}
      />
      <DropdownMenuItem
        icon={<Pencil1Icon />}
        label={
          selectedCollection
            ? `Rename ${collectionLabel(selectedCollection)}…`
            : "Rename collection…"
        }
        isDisabled={!selectedCollection}
        onClick={() => setPendingRename(selectedCollection ?? null)}
      />
      <DropdownMenuItem
        icon={<TrashIcon />}
        label={
          selectedCollection
            ? `Delete ${collectionLabel(selectedCollection)}…`
            : "Delete collection…"
        }
        isDisabled={!selectedCollection}
        onClick={() => setPendingDelete(selectedCollection ?? null)}
      />

      <Divider />

      <MenuHeading>Repository</MenuHeading>
      <DropdownMenuRadioGroup
        label="Repository"
        value={selectedRepo?.path}
        onChange={setRepoPath}
      >
        {repos.map((r) => (
          <DropdownMenuRadioItem
            key={r.path}
            value={r.path}
            label={basename(r.path)}
            description={r.remote ?? "No remote"}
          />
        ))}
      </DropdownMenuRadioGroup>
      <DropdownMenuItem
        icon={<Link2Icon />}
        label={selectedRepo?.remote ? "Change remote…" : "Set remote…"}
        isDisabled={!selectedRepo}
        onClick={() => {
          setRemoteRepoPath(selectedRepo?.path);
          setRemoteKey((key) => key + 1);
          setRemoteOpen(true);
        }}
      />
      <DropdownMenuItem
        icon={<DownloadIcon />}
        label="Clone repository…"
        onClick={() => setCloneOpen(true)}
      />

      <Divider />

      <DropdownMenuItem
        icon={<BarChartIcon />}
        label="Library stats"
        isDisabled={!onShowStats}
        onClick={onShowStats}
      />
    </DropdownMenu>
  );

  return (
    <>
      {/* Always the group, if only of the menu, so the menu isn't remounted (closing it)
          when the status comes and goes. */}
      <ButtonGroup label="Collection" className="collection-group">
        {menu}
        {showSync && (
          <GitSyncStatusButton
            state={syncState}
            message={syncMessage}
            retryable={syncRetryable}
            onRetry={onRetrySync}
            onResolve={() => setResolveOpen(true)}
          />
        )}
      </ButtonGroup>
      <GitConflictDialog
        isOpen={resolveOpen}
        onOpenChange={setResolveOpen}
        message={syncMessage}
        onResolve={onResolveConflict}
      />
      <NewCollectionDialog
        isOpen={createOpen}
        onOpenChange={setCreateOpen}
        onCreate={onCreate}
      />
      <RenameCollectionDialog
        currentName={pendingRename && collectionLabel(pendingRename)}
        onClose={() => setPendingRename(null)}
        onRename={(name) => {
          if (pendingRename) onRename(pendingRename, name);
        }}
        hasRemote={!!selectedRepo?.remote}
      />
      <CloneRepoDialog
        isOpen={cloneOpen}
        onOpenChange={setCloneOpen}
        onClone={onClone}
      />
      <GitRemoteDialog
        key={remoteKey}
        isOpen={remoteOpen}
        onOpenChange={setRemoteOpen}
        repoName={remoteRepoPath && basename(remoteRepoPath)}
        currentRemote={repos.find((r) => r.path === remoteRepoPath)?.remote}
        onSave={(url) =>
          remoteRepoPath
            ? onSetRemote(remoteRepoPath, url)
            : Promise.reject(new Error("No repository"))
        }
      />
      <AlertDialog
        isOpen={pendingDelete !== null}
        onOpenChange={(open) => {
          if (!open) setPendingDelete(null);
        }}
        title="Delete collection?"
        description={
          pendingDelete
            ? `Deletes ${collectionLabel(pendingDelete)} with all its tracks, ratings and play counts` +
              (selectedRepo?.remote ? ", on all your machines." : ".")
            : ""
        }
        actionLabel="Delete collection"
        onAction={() => {
          if (pendingDelete) onDelete(pendingDelete);
          setPendingDelete(null);
        }}
      />
    </>
  );
}

export default CollectionSelector;
