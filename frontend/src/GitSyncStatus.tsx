import { ReloadIcon } from "@radix-ui/react-icons";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import type { GitSyncState } from "./Types.tsx";
import { gitSyncLabels } from "./gitSync.ts";

type Props = {
  state: GitSyncState;
  message?: string;
  retryable?: boolean;
  onRetry: () => void;
  onResolve: () => void;
};

/** The sync status as the second half of the collection menu's button group (see
    CollectionSelector.tsx): it retries the sync, or for a conflict opens
    GitConflictDialog. A state there's nothing to do about is shown disabled, its
    message in the tooltip. Pending local changes are routine, synced on a timer, so
    they get just a "sync now" icon, saying what it's about in its tooltip; the states
    that need attention say so in the button, and a conflict to resolve says what to
    do about it, which has no fitting icon. A background sync's diverged histories are
    a conflict a retry reconciles (`retryable`), so that one offers the retry. */
export function GitSyncStatusButton({
  state,
  message,
  retryable,
  onRetry,
  onResolve,
}: Props) {
  const label = gitSyncLabels[state];
  const conflict = state === "conflict-requires-attention";
  const localChanges = state === "local-changes";
  const action = retryable
    ? localChanges
      ? "Sync now"
      : "Retry sync"
    : undefined;
  const icon = retryable ? <ReloadIcon /> : undefined;
  const tooltip = [localChanges && label, message ?? label, action]
    .filter(Boolean)
    .join(" · ");
  if (conflict && !retryable) {
    return (
      <Button
        className="git-sync-status"
        variant="secondary"
        label="Resolve conflict…"
        tooltip={message ?? label}
        onClick={onResolve}
      />
    );
  }
  if (localChanges && icon) {
    return (
      <Button
        className="git-sync-status"
        variant="secondary"
        label={`${label}: ${action}`}
        tooltip={tooltip}
        icon={icon}
        isIconOnly
        onClick={onRetry}
      />
    );
  }
  return (
    <Button
      className="git-sync-status"
      variant="secondary"
      label={action ? `${label}: ${action}` : label}
      tooltip={tooltip}
      endContent={icon}
      isDisabled={!action}
      onClick={onRetry}
    >
      {label}
    </Button>
  );
}

export function GitConflictDialog({
  isOpen,
  onOpenChange,
  message,
  onResolve,
}: {
  isOpen: boolean;
  onOpenChange: (isOpen: boolean) => void;
  message?: string;
  onResolve: (strategy: "keep-local" | "keep-upstream") => void;
}) {
  return (
    <Dialog
      isOpen={isOpen}
      onOpenChange={onOpenChange}
      purpose="required"
      width={480}
    >
      <Layout
        header={<DialogHeader title="Resolve collection conflict" />}
        content={
          <LayoutContent>
            <VStack gap={3}>
              <Text type="body">
                Local and upstream changes conflict. Backups of both were saved.
                Which version wins?
              </Text>
              {message && <Text type="supporting">{message}</Text>}
            </VStack>
          </LayoutContent>
        }
        footer={
          <LayoutFooter>
            <HStack gap={2} hAlign="end">
              <Button
                label="Cancel"
                variant="secondary"
                onClick={() => onOpenChange(false)}
              />
              <Button
                label="Use upstream"
                variant="secondary"
                onClick={() => {
                  onResolve("keep-upstream");
                  onOpenChange(false);
                }}
              />
              <Button
                label="Keep local"
                variant="primary"
                onClick={() => {
                  onResolve("keep-local");
                  onOpenChange(false);
                }}
              />
            </HStack>
          </LayoutFooter>
        }
      />
    </Dialog>
  );
}
