import { useState } from "react";
import { Text } from "@astryxdesign/core/Text";
import { Button } from "@astryxdesign/core/Button";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Code } from "@astryxdesign/core/Code";
import { Badge } from "@astryxdesign/core/Badge";
import { Divider } from "@astryxdesign/core/Divider";

import type { RequestReply } from "./Types.tsx";
import SpotifyIcon from "./SpotifyIcon.tsx";

type SpotifySessionDialogProps = {
  requestReply: RequestReply;
  collectionOpen: boolean;
  backfillRunning: boolean;
  backfillChecked?: number;
  backfillMatched?: number;
  backfillTotal?: number;
  onBackfill: () => void;
};

function SpotifySessionDialog({
  requestReply,
  collectionOpen,
  backfillRunning,
  backfillChecked,
  backfillMatched,
  backfillTotal,
  onBackfill,
}: SpotifySessionDialogProps) {
  const [dialogOpen, setDialogOpen] = useState(false);
  const [connected, setConnected] = useState(false);
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

  const closeDialog = () => setDialogOpen(false);

  return (
    <>
      <IconButton
        variant={connected ? "primary" : "secondary"}
        label="Connect a Spotify account (for creating playlists)"
        tooltip="Connect a Spotify account (for creating playlists)"
        icon={<SpotifyIcon />}
        onClick={() => {
          setDialogOpen(true);
          requestReply({ type: "get-spotify-session" }, (r) => {
            if ("connected" in r) setConnected(r.connected);
          });
          setCookie("");
        }}
      />

      <Dialog isOpen={dialogOpen} onOpenChange={setDialogOpen} width={480}>
        <Layout
          header={
            <DialogHeader
              title="Spotify Account"
              onOpenChange={closeDialog}
              endContent={
                connected ? (
                  <Badge variant="success" label="Connected" />
                ) : undefined
              }
            />
          }
          content={
            <LayoutContent>
              <VStack gap={4}>
                <Text as="div" type="body">
                  Stereo needs your browser&rsquo;s Spotify session to create
                  playlists on your behalf. Your password never touches stereo
                  &mdash; either let stereo read the session directly from a
                  browser you&rsquo;re already logged into, or copy it yourself:
                  log into <Code>open.spotify.com</Code>, open devtools →
                  Network, pick any request to spotify.com, and copy the value
                  of its <Code>Cookie</Code> request header.
                </Text>

                <VStack gap={3}>
                  <Button
                    label="Auto-detect from a logged-in browser"
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
                    Matches every track in the current collection that&rsquo;s
                    missing a Spotify ID by artist/title (doesn&rsquo;t require
                    a connected account).
                    {backfillRunning &&
                      ` Matching… ${backfillChecked ?? 0}/${backfillTotal ?? 0} checked, ${backfillMatched ?? 0} matched.`}
                  </Text>
                  <Button
                    label="Backfill missing Spotify IDs for this collection"
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
    </>
  );
}

export default SpotifySessionDialog;
