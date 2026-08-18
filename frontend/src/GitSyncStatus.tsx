import { useState } from "react";
import { ResetIcon } from "@radix-ui/react-icons";
import { StatusDot } from "@astryxdesign/core/StatusDot";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import type { GitSyncState } from "./Types.tsx";

type Props = {
  state?: GitSyncState;
  message?: string;
  retryable?: boolean;
  onRetry: () => void;
  onResolve: (strategy: "keep-local" | "keep-upstream") => void;
};

const labels: Record<GitSyncState, string> = {
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

function variant(state: GitSyncState) {
  if (state === "synced") return "success" as const;
  if (state === "syncing") return "accent" as const;
  if (state === "error" || state === "corrupt-collection")
    return "error" as const;
  if (
    state === "offline" ||
    state === "authentication-required" ||
    state === "remote-ahead" ||
    state === "conflict-requires-attention"
  ) {
    return "warning" as const;
  }
  return "neutral" as const;
}

export default function GitSyncStatus({
  state,
  message,
  retryable,
  onRetry,
  onResolve,
}: Props) {
  const [resolveOpen, setResolveOpen] = useState(false);
  if (!state) return null;
  const label = labels[state];
  const conflict = state === "conflict-requires-attention";

  return (
    <>
      <HStack gap={2} vAlign="center">
        <StatusDot
          variant={variant(state)}
          label={label}
          tooltip={message ?? label}
          isPulsing={state === "syncing"}
        />
        <Text type="supporting">{label}</Text>
        {retryable && !conflict && (
          <IconButton
            size="sm"
            variant="ghost"
            label="Retry repository synchronization"
            tooltip={message ?? "Retry repository synchronization"}
            icon={<ResetIcon />}
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
                  Stereo could not safely combine local and upstream changes.
                  Recovery copies of both versions have been saved. Choose which
                  version should win for conflicting collections.
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
