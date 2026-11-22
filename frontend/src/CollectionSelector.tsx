import { useState } from "react";
import {
  DropdownMenu,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
} from "@astryxdesign/core/DropdownMenu";
import { Divider } from "@astryxdesign/core/Divider";
import { StatusDot } from "@astryxdesign/core/StatusDot";
import { Button } from "@astryxdesign/core/Button";
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
import { FileDown, FileUp } from "lucide-react";
import { MenuHeading } from "./Menu.tsx";
import NewCollectionDialog from "./NewCollectionDialog.tsx";
import ImportCollectionDialog from "./ImportCollectionDialog.tsx";
import RenameCollectionDialog from "./RenameCollectionDialog.tsx";
import CloneRepoDialog from "./CloneRepoDialog.tsx";
import GitRemoteDialog from "./GitRemoteDialog.tsx";
import { GitConflictDialog, GitSyncStatusButton } from "./GitSyncStatus.tsx";
import { gitSyncLabels, gitSyncVariant, showGitSyncStatus } from "./gitSync.ts";
import type { GitSyncState, ICollection, IRepo } from "./Types.tsx";

// Backend paths are absolute and use the server's rules: a Unix path starts with `/`, and a
// backslash in it is part of a name; a Windows path (`C:\…`, `\\server\…`) never does, and
// separates with either.
function basename(path: string): string {
  return path.split(path.startsWith("/") ? "/" : /[\\/]/).pop() ?? path;
}

export function collectionLabel(path: string): string {
  return basename(path).replace(/\.db$/, "");
}

export type CollectionSelectorProps = {
  collections: ICollection[];
  value: string;
  setValue: (path: string) => void;
  onCreate: (name: string) => void;
  // Adds a collection file (base64-encoded) to a repository as is; resolves once it is
  // imported, rejects with the reason it isn't.
  onImport: (repoPath: string, name: string, data: string) => Promise<void>;
  isConnected: boolean;
  onDelete: (path: string) => void;
  onRename: (path: string, name: string) => void;
  // Downloads the collection's SQLite file.
  onDownload: (path: string) => void;
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
  onImport,
  isConnected,
  onDelete,
  onRename,
  onDownload,
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
  const [importOpen, setImportOpen] = useState(false);
  // The repository the import dialog adds to: the one selected when it opened.
  const [importRepoPath, setImportRepoPath] = useState<string>();
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
  // The conflict the resolve dialog was opened for: its repository and message. The
  // backend applies a choice to whatever conflict it finds when it gets it, so the
  // dialog only stays open while that's still the conflict reported (or a sync is
  // finding out, which may report it again): a sync that ends it, another
  // repository's or a new conflict closes it, and its return doesn't reopen it. Only a
  // conflict the backend can't sort out by retrying is resolved here (not a background
  // sync's diverged histories, which a retry reconciles).
  const [resolveFor, setResolveFor] = useState<{
    repo: string;
    message: string | undefined;
  } | null>(null);
  const resolvable =
    syncState === "conflict-requires-attention" && !syncRetryable;
  const isCurrentConflict =
    resolvable &&
    resolveFor?.repo === repoPath &&
    resolveFor.message === syncMessage;
  const resolveOpen =
    resolveFor !== null &&
    (isCurrentConflict ||
      (syncState === "syncing" && resolveFor.repo === repoPath));
  if (resolveFor !== null && !resolveOpen) setResolveFor(null);

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

  const statsLabel = selectedCollection
    ? `${collectionLabel(selectedCollection)} stats`
    : "Collection stats";

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
        // With the stats button, the group keeps the 220 px the header is laid out for.
        width: 188,
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
        icon={<FileUp />}
        label="Import collection file…"
        isDisabled={!selectedRepo}
        onClick={() => {
          setImportRepoPath(selectedRepo?.path);
          setImportOpen(true);
        }}
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
        icon={<FileDown />}
        label={
          selectedCollection
            ? `Download ${collectionLabel(selectedCollection)}.db`
            : "Download collection file"
        }
        isDisabled={!selectedCollection || !isValid}
        onClick={() => selectedCollection && onDownload(selectedCollection)}
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
    </DropdownMenu>
  );

  return (
    <>
      {/* Always the group, if only of the menu, so the menu isn't remounted (closing it)
          when the status comes and goes. */}
      <ButtonGroup label="Collection" className="collection-group">
        {menu}
        <Button
          className="collection-stats"
          label={statsLabel}
          tooltip={statsLabel}
          icon={<BarChartIcon />}
          isIconOnly
          variant={showSync ? "secondary" : "ghost"}
          isDisabled={!onShowStats}
          onClick={onShowStats}
        />
        {showSync && syncState && (
          <GitSyncStatusButton
            state={syncState}
            message={syncMessage}
            retryable={syncRetryable}
            onRetry={onRetrySync}
            onResolve={() =>
              setResolveFor({ repo: repoPath, message: syncMessage })
            }
          />
        )}
      </ButtonGroup>
      <GitConflictDialog
        isOpen={resolveOpen}
        onOpenChange={(isOpen) => {
          if (!isOpen) setResolveFor(null);
        }}
        message={resolveFor?.message}
        onResolve={(strategy) => {
          if (isCurrentConflict) onResolveConflict(strategy);
        }}
      />
      <NewCollectionDialog
        isOpen={createOpen}
        onOpenChange={setCreateOpen}
        onCreate={onCreate}
      />
      <ImportCollectionDialog
        isOpen={importOpen}
        onOpenChange={setImportOpen}
        onImport={(name, data) =>
          importRepoPath
            ? onImport(importRepoPath, name, data)
            : Promise.reject(new Error("No repository"))
        }
        isConnected={isConnected}
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
