import { useState } from "react";
import { ReloadIcon } from "@radix-ui/react-icons";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Button } from "@astryxdesign/core/Button";
import { Tooltip } from "@astryxdesign/core/Tooltip";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import type { GitSyncState } from "./Types.tsx";
import { gitSyncLabels, gitSyncVariant } from "./gitSync.ts";

type Props = {
  state?: GitSyncState;
  message?: string;
  retryable?: boolean;
  onRetry: () => void;
  onResolve: (strategy: "keep-local" | "keep-upstream") => void;
};

// Only surfaces in the toolbar when the user can or should act on the sync
// state; routine states are conveyed by the dot on the collection menu.
export default function GitSyncStatus({
  state,
  message,
  retryable,
  onRetry,
  onResolve,
}: Props) {
  const [resolveOpen, setResolveOpen] = useState(false);
  if (!state) return null;
  const variant = gitSyncVariant(state);
  const conflict = state === "conflict-requires-attention";
  if (!retryable && !conflict && variant !== "warning" && variant !== "error") {
    return null;
  }
  const label = gitSyncLabels[state];
  // Pending local changes are synced on a timer; the button syncs them right away.
  const retryLabel = state === "local-changes" ? "Sync now" : "Retry sync";

  return (
    <>
      <HStack gap={1} vAlign="center" style={{ minWidth: 0 }}>
        <Tooltip content={message ?? label}>
          <Text type="supporting" color="secondary" maxLines={1}>
            {label}
          </Text>
        </Tooltip>
        {retryable && !conflict && (
          <IconButton
            size="sm"
            variant="ghost"
            label={retryLabel}
            tooltip={retryLabel}
            icon={<ReloadIcon />}
            onClick={onRetry}
          />
        )}
        {conflict && (
          <Button
            size="sm"
            variant="secondary"
            label="Resolve"
            onClick={() => setResolveOpen(true)}
          />
        )}
      </HStack>

      <Dialog
        isOpen={resolveOpen}
        onOpenChange={setResolveOpen}
        purpose="required"
        width={480}
      >
        <Layout
          header={<DialogHeader title="Resolve collection conflict" />}
          content={
            <LayoutContent>
              <VStack gap={3}>
                <Text type="body">
                  Local and upstream changes conflict. Backups of both were
                  saved. Which version wins?
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
                  onClick={() => setResolveOpen(false)}
                />
                <Button
                  label="Use upstream"
                  variant="secondary"
                  onClick={() => {
                    onResolve("keep-upstream");
                    setResolveOpen(false);
                  }}
                />
                <Button
                  label="Keep local"
                  variant="primary"
                  onClick={() => {
                    onResolve("keep-local");
                    setResolveOpen(false);
                  }}
                />
              </HStack>
            </LayoutFooter>
          }
        />
      </Dialog>
    </>
  );
}
