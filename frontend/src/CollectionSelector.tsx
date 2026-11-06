import { useState } from "react";
import {
  DropdownMenu,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
} from "@astryxdesign/core/DropdownMenu";
import { Divider } from "@astryxdesign/core/Divider";
import { StatusDot } from "@astryxdesign/core/StatusDot";
import { BarChartIcon, DownloadIcon, PlusIcon } from "@radix-ui/react-icons";
import { MenuCheck, MenuHeading } from "./Menu.tsx";
import NewCollectionDialog from "./NewCollectionDialog.tsx";
import CloneRepoDialog from "./CloneRepoDialog.tsx";
import { gitSyncLabels, gitSyncVariant } from "./gitSync.ts";
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
  isValid: boolean;
  repos: IRepo[];
  repoPath: string;
  setRepoPath: (path: string) => void;
  onClone: (url: string) => void;
  syncState?: GitSyncState;
  syncMessage?: string;
  // Undefined when there are no stats to show.
  onShowStats?: () => void;
};

function CollectionSelector({
  collections,
  value,
  setValue,
  onCreate,
  isValid,
  repos,
  repoPath,
  setRepoPath,
  onClone,
  syncState,
  syncMessage,
  onShowStats,
}: CollectionSelectorProps) {
  const [createOpen, setCreateOpen] = useState(false);
  const [cloneOpen, setCloneOpen] = useState(false);

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

  return (
    <>
      <DropdownMenu
        menuWidth={300}
        button={{
          label: selectedCollection
            ? collectionLabel(selectedCollection)
            : "Select collection",
          variant: "ghost",
          width: 220,
          style: { maxWidth: "100%" },
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
          aria-label="Collections"
          value={selectedCollection}
          onChange={setValue}
        >
          {collections.map((c) => (
            <DropdownMenuRadioItem
              key={c.path}
              value={c.path}
              label={collectionLabel(c.path)}
              endContent={
                <MenuCheck isChecked={c.path === selectedCollection} />
              }
            />
          ))}
        </DropdownMenuRadioGroup>
        <DropdownMenuItem
          icon={<PlusIcon />}
          label="New collection…"
          onClick={() => setCreateOpen(true)}
        />

        <Divider />

        <MenuHeading>Repository</MenuHeading>
        <DropdownMenuRadioGroup
          aria-label="Repository"
          value={selectedRepo?.path}
          onChange={setRepoPath}
        >
          {repos.map((r) => (
            <DropdownMenuRadioItem
              key={r.path}
              value={r.path}
              label={basename(r.path)}
              description={r.remote ?? "No remote"}
              endContent={
                <MenuCheck isChecked={r.path === selectedRepo?.path} />
              }
            />
          ))}
        </DropdownMenuRadioGroup>
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

      <NewCollectionDialog
        isOpen={createOpen}
        onOpenChange={setCreateOpen}
        onCreate={onCreate}
      />
      <CloneRepoDialog
        isOpen={cloneOpen}
        onOpenChange={setCloneOpen}
        onClone={onClone}
      />
    </>
  );
}

export default CollectionSelector;
