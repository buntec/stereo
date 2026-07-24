import { useState } from "react";
import {
  Text,
  Button,
  Dialog,
  IconButton,
  Tooltip,
  TextField,
  Flex,
  Code,
  Badge,
  Separator,
} from "@radix-ui/themes";

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

  return (
    <Dialog.Root
      onOpenChange={(open) => {
        if (open) {
          requestReply({ type: "get-spotify-session" }, (r) => {
            if ("connected" in r) setConnected(r.connected);
          });
          setCookie("");
        }
      }}
    >
      <Tooltip content="Connect a Spotify account (for creating playlists)">
        <Dialog.Trigger>
          <IconButton variant={connected ? "solid" : "soft"} color="green">
            <SpotifyIcon />
          </IconButton>
        </Dialog.Trigger>
      </Tooltip>

      <Dialog.Content maxWidth="480px">
        <Dialog.Title>
          Spotify Account{" "}
          {connected && (
            <Badge color="green" ml="2">
              Connected
            </Badge>
          )}
        </Dialog.Title>
        <Dialog.Description size="2" mb="4">
          Stereo needs your browser's Spotify session to create playlists on
          your behalf. Your password never touches stereo &mdash; either let
          stereo read the session directly from a browser you're already logged
          into, or copy it yourself: log into <Code>open.spotify.com</Code>,
          open devtools → Network, pick any request to spotify.com, and copy the
          value of its <Code>Cookie</Code> request header.
        </Dialog.Description>

        <Flex direction="column" gap="3">
          <Button
            variant="soft"
            onClick={detect}
            loading={detecting}
            disabled={detecting}
          >
            Auto-detect from a logged-in browser
          </Button>

          <label>
            <Text as="div" size="2" mb="1" weight="bold">
              Cookie header
            </Text>
            <TextField.Root
              type="password"
              placeholder="sp_t=...; sp_dc=...; ..."
              value={cookie}
              onChange={(ev) => setCookie(ev.target.value)}
            />
          </label>
        </Flex>

        <Separator size="4" my="4" />

        <Flex direction="column" gap="2">
          <Text as="div" size="2" weight="bold">
            Backfill Spotify IDs
          </Text>
          <Text as="div" size="2" color="gray">
            Matches every track in the current collection that's missing a
            Spotify ID by artist/title (doesn't require a connected account).
            {backfillRunning &&
              ` Matching… ${backfillChecked ?? 0}/${backfillTotal ?? 0} checked, ${backfillMatched ?? 0} matched.`}
          </Text>
          <Button
            variant="soft"
            onClick={onBackfill}
            loading={backfillRunning}
            disabled={backfillRunning || !collectionOpen}
          >
            Backfill missing Spotify IDs for this collection
          </Button>
        </Flex>

        <Flex gap="3" mt="4" justify="end">
          {connected && (
            <Dialog.Close>
              <Button
                variant="soft"
                color="red"
                onClick={() =>
                  requestReply(
                    { type: "set-spotify-session", cookie: null },
                    (r) => {
                      if ("connected" in r) setConnected(r.connected);
                    },
                  )
                }
              >
                Disconnect
              </Button>
            </Dialog.Close>
          )}
          <Dialog.Close>
            <Button variant="soft" color="gray">
              Cancel
            </Button>
          </Dialog.Close>
          <Dialog.Close>
            <Button
              color="green"
              disabled={!cookie.trim()}
              onClick={() =>
                requestReply(
                  { type: "set-spotify-session", cookie: cookie.trim() },
                  (r) => {
                    if ("connected" in r) setConnected(r.connected);
                  },
                )
              }
            >
              Connect
            </Button>
          </Dialog.Close>
        </Flex>
      </Dialog.Content>
    </Dialog.Root>
  );
}

export default SpotifySessionDialog;
