import { useState } from "react";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Button } from "@astryxdesign/core/Button";
import { Divider } from "@astryxdesign/core/Divider";
import { FormLayout } from "@astryxdesign/core/FormLayout";
import { Link } from "@astryxdesign/core/Link";
import { List, ListItem } from "@astryxdesign/core/List";
import { NumberInput } from "@astryxdesign/core/NumberInput";
import { ProgressBar } from "@astryxdesign/core/ProgressBar";
import { HStack, StackItem, VStack } from "@astryxdesign/core/Stack";
import { StatusDot } from "@astryxdesign/core/StatusDot";
import { Switch } from "@astryxdesign/core/Switch";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { LinkBreak2Icon, TrashIcon, UpdateIcon } from "@radix-ui/react-icons";
import { collectionLabel } from "./CollectionSelector.tsx";
import { describeView } from "./playlists.ts";
import {
  playlistSyncKey,
  type ICollection,
  type IPlaylist,
  type PlaylistEdit,
  type PlaylistService,
  type PlaylistSyncStatus,
  type PlaylistTarget,
} from "./Types.tsx";

/** YouTube refuses longer playlists; the backend caps every sync at this. */
const MAX_SYNCED_TRACKS = 5000;

const serviceLabels: Record<PlaylistService, string> = {
  spotify: "Spotify",
  youtube: "YouTube",
};

function timeAgo(ms: number): string {
  const minutes = Math.round((Date.now() - ms) / 60000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  return new Date(ms).toLocaleDateString();
}

type ServiceSectionProps = {
  service: PlaylistService;
  playlist: IPlaylist;
  live?: PlaylistSyncStatus;
  onChange: (target: PlaylistTarget | null) => void;
  onSync: () => void;
  onUnlink: () => void;
};

function ServiceSection({
  service,
  playlist,
  live,
  onChange,
  onSync,
  onUnlink,
}: ServiceSectionProps) {
  const label = serviceLabels[service];
  const target = playlist[service];
  const status = playlist[`${service}_status`];
  const isRunning = live?.state === "running";
  const error =
    live?.state === "failed" ? live.error : isRunning ? null : status?.error;

  return (
    <VStack gap={2}>
      <Switch
        label={`Export to ${label}`}
        labelTooltip={`Turning it off forgets the ${label} playlist.`}
        value={!!target}
        onChange={(on) => onChange(on ? { auto_sync: true } : null)}
      />
      {target && (
        <VStack gap={2}>
          <Switch
            label="Update automatically"
            labelTooltip="When its tracks change, at most hourly, while Stereo is open"
            value={target.auto_sync}
            onChange={(auto_sync) => onChange({ ...target, auto_sync })}
          />
          {isRunning ? (
            <ProgressBar
              label="Updating"
              value={live.current ?? 0}
              max={live.total ?? 1}
              isIndeterminate={live.total === null}
              hasValueLabel={live.total !== null}
              formatValueLabel={(v, max) => `${v} / ${max}`}
            />
          ) : error ? (
            <HStack gap={2} vAlign="center">
              <StatusDot variant="error" label="Last sync failed" />
              <Text type="supporting" color="secondary">
                {error}
              </Text>
            </HStack>
          ) : (
            <Text type="supporting" color="secondary">
              {status?.synced_at
                ? `Last updated ${timeAgo(status.synced_at)}` +
                  (live?.state === "synced" &&
                  live.matched !== null &&
                  live.total !== null &&
                  live.matched < live.total
                    ? ` · ${live.matched} of ${live.total} found`
                    : "")
                : status?.url
                  ? "Not updated from this machine yet"
                  : "Not created yet"}
            </Text>
          )}
          <HStack gap={2} vAlign="center" wrap="wrap">
            <Button
              label="Sync now"
              size="sm"
              icon={<UpdateIcon />}
              isLoading={isRunning}
              onClick={onSync}
            />
            {status?.url && (
              <>
                <Link href={status.url} isExternalLink>
                  Open in {label}
                </Link>
                <Button
                  label="Unlink"
                  size="sm"
                  variant="ghost"
                  icon={<LinkBreak2Icon />}
                  tooltip={`Forget this ${label} playlist without deleting it`}
                  onClick={onUnlink}
                />
              </>
            )}
          </HStack>
        </VStack>
      )}
    </VStack>
  );
}

type DetailsProps = {
  playlist: IPlaylist;
  collections: ICollection[];
  syncStatus: Record<string, PlaylistSyncStatus>;
  existingNames: string[];
  onEdit: (edit: PlaylistEdit) => void;
  onSync: (service: PlaylistService) => void;
  onUnlink: (service: PlaylistService) => void;
  onOpen: () => void;
  onReplaceWithView?: () => void;
};

function PlaylistDetails({
  playlist,
  collections,
  syncStatus,
  existingNames,
  onEdit,
  onSync,
  onUnlink,
  onOpen,
  onReplaceWithView,
}: DetailsProps) {
  // The name is saved when editing ends, not on every keystroke.
  const [name, setName] = useState(playlist.name);
  // Follows renames that arrive from the server (e.g. from another machine).
  const [savedName, setSavedName] = useState(playlist.name);
  if (savedName !== playlist.name) {
    setSavedName(playlist.name);
    setName(playlist.name);
  }
  const trimmed = name.trim();
  const nameTaken =
    trimmed !== playlist.name && existingNames.includes(trimmed);
  const commitName = () => {
    if (trimmed && trimmed !== playlist.name && !nameTaken) {
      onEdit({ kind: "rename", name: trimmed });
    } else {
      setName(playlist.name);
    }
  };
  const collectionExists = collections.some(
    (c) => c.path === playlist.collection,
  );

  return (
    <FormLayout>
      <TextInput
        label="Name"
        value={name}
        onChange={setName}
        onBlur={commitName}
        onEnter={commitName}
        status={
          nameTaken
            ? {
                type: "error",
                message: "Name already taken",
              }
            : undefined
        }
      />
      <VStack gap={1}>
        <Text type="body" weight="bold">
          {collectionLabel(playlist.collection)}
        </Text>
        <Text type="supporting" color="secondary">
          {collectionExists ? describeView(playlist) : "Collection not found"}
        </Text>
        <HStack gap={2} wrap="wrap">
          <Button
            label="Show in collection"
            size="sm"
            variant="secondary"
            isDisabled={!collectionExists}
            onClick={onOpen}
          />
          {onReplaceWithView && (
            <Button
              label="Replace with current view"
              size="sm"
              variant="ghost"
              onClick={onReplaceWithView}
            />
          )}
        </HStack>
      </VStack>
      <NumberInput
        label="Track limit"
        labelTooltip={`Exports take at most ${MAX_SYNCED_TRACKS} tracks.`}
        placeholder="All tracks"
        value={playlist.limit}
        min={1}
        max={MAX_SYNCED_TRACKS}
        isIntegerOnly
        hasClear
        onChange={(limit: number | null) =>
          onEdit({ kind: "set-limit", limit: limit || null })
        }
      />
      {(["spotify", "youtube"] as const).map((service) => (
        <VStack key={service} gap={3}>
          <Divider />
          <ServiceSection
            service={service}
            playlist={playlist}
            live={syncStatus[playlistSyncKey(playlist.id, service)]}
            onChange={(target) =>
              onEdit({ kind: "set-export", service, target })
            }
            onSync={() => onSync(service)}
            onUnlink={() => onUnlink(service)}
          />
        </VStack>
      ))}
    </FormLayout>
  );
}

type PlaylistsDialogProps = {
  isOpen: boolean;
  onClose: () => void;
  playlists: IPlaylist[];
  /** The playlist to show first, e.g. the active one. */
  initialId?: string;
  collections: ICollection[];
  syncStatus: Record<string, PlaylistSyncStatus>;
  onEdit: (playlist: IPlaylist, edit: PlaylistEdit) => void;
  onDelete: (playlist: IPlaylist) => void;
  onSync: (playlist: IPlaylist, service: PlaylistService) => void;
  onUnlink: (playlist: IPlaylist, service: PlaylistService) => void;
  onOpen: (playlist: IPlaylist) => void;
  /** Undefined when the current view can't be saved (e.g. while searching). */
  onReplaceWithView?: (playlist: IPlaylist) => void;
};

/** Lists the playlists, with each one's settings and its Spotify/YouTube exports. */
function PlaylistsDialog({
  isOpen,
  onClose,
  playlists,
  initialId,
  collections,
  syncStatus,
  onEdit,
  onDelete,
  onSync,
  onUnlink,
  onOpen,
  onReplaceWithView,
}: PlaylistsDialogProps) {
  const [selectedId, setSelectedId] = useState<string | undefined>();
  const [wasOpen, setWasOpen] = useState(false);
  if (isOpen !== wasOpen) {
    setWasOpen(isOpen);
    if (isOpen) setSelectedId(initialId);
  }
  const [pendingDelete, setPendingDelete] = useState<IPlaylist | null>(null);
  const selected =
    playlists.find((p) => p.id === selectedId) ?? playlists.at(0);

  return (
    <>
      <Dialog
        isOpen={isOpen}
        onOpenChange={(open) => {
          if (!open) onClose();
        }}
        width={760}
        maxHeight="85vh"
      >
        <Layout
          header={<DialogHeader title="Playlists" onOpenChange={onClose} />}
          content={
            <LayoutContent>
              <HStack gap={4} wrap="wrap" vAlign="start">
                <StackItem>
                  <List density="compact">
                    {playlists.map((p) => {
                      const failed = (["spotify", "youtube"] as const).some(
                        (s) =>
                          syncStatus[playlistSyncKey(p.id, s)]?.state ===
                            "failed" || !!p[`${s}_status`]?.error,
                      );
                      return (
                        <ListItem
                          key={p.id}
                          label={p.name}
                          description={collectionLabel(p.collection)}
                          isSelected={p.id === selected?.id}
                          onClick={() => setSelectedId(p.id)}
                          endContent={
                            failed ? (
                              <StatusDot
                                variant="error"
                                label="Last sync failed"
                              />
                            ) : undefined
                          }
                        />
                      );
                    })}
                  </List>
                </StackItem>
                <StackItem size="fill">
                  {selected && (
                    <PlaylistDetails
                      key={selected.id}
                      playlist={selected}
                      collections={collections}
                      syncStatus={syncStatus}
                      existingNames={playlists
                        .filter((p) => p.id !== selected.id)
                        .map((p) => p.name)}
                      onEdit={(edit) => onEdit(selected, edit)}
                      onSync={(service) => onSync(selected, service)}
                      onUnlink={(service) => onUnlink(selected, service)}
                      onOpen={() => {
                        onOpen(selected);
                        onClose();
                      }}
                      onReplaceWithView={
                        onReplaceWithView && (() => onReplaceWithView(selected))
                      }
                    />
                  )}
                </StackItem>
              </HStack>
            </LayoutContent>
          }
          footer={
            <LayoutFooter>
              <HStack gap={2} hAlign="between">
                <Button
                  label="Delete playlist"
                  variant="destructive"
                  icon={<TrashIcon />}
                  isDisabled={!selected}
                  onClick={() => setPendingDelete(selected ?? null)}
                />
                <Button label="Done" variant="primary" onClick={onClose} />
              </HStack>
            </LayoutFooter>
          }
        />
      </Dialog>
      <AlertDialog
        isOpen={pendingDelete !== null}
        onOpenChange={(open) => {
          if (!open) setPendingDelete(null);
        }}
        title="Delete playlist?"
        description={
          pendingDelete
            ? `This deletes the playlist ${pendingDelete.name}, not its tracks. Playlists it was exported to on Spotify or YouTube stay there, but are no longer updated.`
            : ""
        }
        actionLabel="Delete playlist"
        onAction={() => {
          if (pendingDelete) onDelete(pendingDelete);
          setPendingDelete(null);
        }}
      />
    </>
  );
}

export default PlaylistsDialog;
