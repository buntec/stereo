import { useState } from "react";
import { Text } from "@astryxdesign/core/Text";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Code } from "@astryxdesign/core/Code";
import { StatusDot } from "@astryxdesign/core/StatusDot";
import { Divider } from "@astryxdesign/core/Divider";

import type { RequestReply } from "./Types.tsx";

type SpotifySessionDialogProps = {
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  connected: boolean;
  setConnected: (connected: boolean) => void;
  requestReply: RequestReply;
  collectionOpen: boolean;
  backfillRunning: boolean;
  backfillChecked?: number;
  backfillMatched?: number;
  backfillTotal?: number;
  onBackfill: () => void;
};

function SpotifySessionDialog({
  isOpen,
  onOpenChange,
  connected,
  setConnected,
  requestReply,
  collectionOpen,
  backfillRunning,
  backfillChecked,
  backfillMatched,
  backfillTotal,
  onBackfill,
}: SpotifySessionDialogProps) {
  const [cookie, setCookie] = useState("");
  const [detecting, setDetecting] = useState(false);

  const detect = () => {
    setDetecting(true);
    requestReply(
      { type: "detect-spotify-session" },
      (r) => {
        setDetecting(false);
        if ("connected" in r) setConnected(r.connected);
      },
      15000,
    );
  };

  const closeDialog = () => onOpenChange(false);

  return (
    <Dialog isOpen={isOpen} onOpenChange={onOpenChange} width={480}>
      <Layout
        header={
          <DialogHeader
            title="Spotify Account"
            onOpenChange={closeDialog}
            endContent={
              connected ? (
                <HStack gap={1} vAlign="center">
                  <StatusDot variant="success" label="Connected" />
                  <Text type="supporting" color="secondary">
                    Connected
                  </Text>
                </HStack>
              ) : undefined
            }
          />
        }
        content={
          <LayoutContent>
            <VStack gap={4}>
              <Text as="div" type="body">
                Detect the Spotify login from your browser, or paste the{" "}
                <Code>Cookie</Code> header of a request to{" "}
                <Code>open.spotify.com</Code> (devtools → Network).
              </Text>

              <VStack gap={3}>
                <Button
                  label="Detect from browser"
                  variant="secondary"
                  onClick={detect}
                  isLoading={detecting}
                  isDisabled={detecting}
                />

                <TextInput
                  label="Cookie header"
                  type="password"
                  placeholder="sp_t=...; sp_dc=...; ..."
                  value={cookie}
                  onChange={setCookie}
                />
              </VStack>

              <Divider />

              <VStack gap={2}>
                <Text as="div" type="body" weight="bold">
                  Backfill Spotify IDs
                </Text>
                <Text as="div" type="supporting">
                  {backfillRunning
                    ? `${backfillChecked ?? 0}/${backfillTotal ?? 0} checked, ${backfillMatched ?? 0} matched`
                    : "Matches tracks without one by artist and title. No account needed."}
                </Text>
                <Button
                  label="Backfill"
                  variant="secondary"
                  onClick={onBackfill}
                  isLoading={backfillRunning}
                  isDisabled={backfillRunning || !collectionOpen}
                />
              </VStack>
            </VStack>
          </LayoutContent>
        }
        footer={
          <LayoutFooter>
            <HStack gap={2} hAlign="end">
              {connected && (
                <Button
                  label="Disconnect"
                  variant="destructive"
                  onClick={() => {
                    requestReply(
                      { type: "set-spotify-session", cookie: null },
                      (r) => {
                        if ("connected" in r) setConnected(r.connected);
                      },
                    );
                    closeDialog();
                  }}
                />
              )}
              <Button
                label="Cancel"
                variant="secondary"
                onClick={closeDialog}
              />
              <Button
                label="Connect"
                variant="primary"
                isDisabled={!cookie.trim()}
                onClick={() => {
                  requestReply(
                    { type: "set-spotify-session", cookie: cookie.trim() },
                    (r) => {
                      if ("connected" in r) setConnected(r.connected);
                    },
                  );
                  closeDialog();
                }}
              />
            </HStack>
          </LayoutFooter>
        }
      />
    </Dialog>
  );
}

export default SpotifySessionDialog;
