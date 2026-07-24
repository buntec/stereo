import { useState, useEffect, useRef } from "react";
import {
  Text,
  Button,
  Dialog,
  TextField,
  Flex,
  Progress,
  RadioGroup,
  Spinner,
} from "@radix-ui/themes";
import type { RequestReply } from "./Types";

const PLAYLIST_LOOKUP_DEBOUNCE_MS = 500;

export type SpotifyPlaylistTarget =
  | { mode: "new"; name: string }
  | { mode: "existing"; url: string };

type SpotifyPlaylistDialogProps = {
  open: boolean;
  trackCount: number;
  requestReply: RequestReply;
  onOpenChange: (open: boolean) => void;
  onCreate: (target: SpotifyPlaylistTarget) => void;
};

function SpotifyPlaylistDialog({
  open,
  trackCount,
  requestReply,
  onOpenChange,
  onCreate,
}: SpotifyPlaylistDialogProps) {
  const [mode, setMode] = useState<"new" | "existing">("new");
  const [name, setName] = useState("stereo");
  const [existingUrl, setExistingUrl] = useState("");
  const [lookupStatus, setLookupStatus] = useState<
    "idle" | "checking" | "found" | "not-found"
  >("idle");
  const [lookupName, setLookupName] = useState<string | null>(null);
  const lookupTimer = useRef<ReturnType<typeof setTimeout> | undefined>(
    undefined,
  );

  useEffect(() => {
    clearTimeout(lookupTimer.current);
    const url = existingUrl.trim();
    if (!url) {
      setLookupStatus("idle");
      setLookupName(null);
      return;
    }
    setLookupStatus("checking");
    lookupTimer.current = setTimeout(() => {
      requestReply({ type: "get-spotify-playlist-info", url }, (msg) => {
        if ("name" in msg) {
          setLookupStatus(msg.name ? "found" : "not-found");
          setLookupName(msg.name ?? null);
        }
      });
    }, PLAYLIST_LOOKUP_DEBOUNCE_MS);
    return () => clearTimeout(lookupTimer.current);
  }, [existingUrl, requestReply]);

  const canCreate =
    trackCount > 0 &&
    (mode === "new" ? !!name.trim() : lookupStatus === "found");

  const submit = () => {
    if (!canCreate) return;
    onCreate(
      mode === "new"
        ? { mode: "new", name: name.trim() }
        : { mode: "existing", url: existingUrl.trim() },
    );
    onOpenChange(false);
  };

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Content maxWidth="440px">
        <Dialog.Title>Export to Spotify playlist</Dialog.Title>
        <Dialog.Description size="2" mb="4">
          Exports up to {trackCount} track{trackCount !== 1 ? "s" : ""} to
          Spotify. Tracks are matched by artist/title, so some may not be found.
        </Dialog.Description>

        <RadioGroup.Root
          value={mode}
          onValueChange={(v) => setMode(v as "new" | "existing")}
        >
          <Flex direction="column" gap="3">
            <label>
              <Flex gap="2" align="center">
                <RadioGroup.Item value="new" />
                <Text size="2" weight="bold">
                  Create a new playlist
                </Text>
              </Flex>
            </label>
            {mode === "new" && (
              <TextField.Root
                ml="5"
                value={name}
                onChange={(ev) => setName(ev.target.value)}
                placeholder="stereo"
                onKeyDown={(ev) => {
                  if (ev.key === "Enter") submit();
                }}
              />
            )}

            <label>
              <Flex gap="2" align="center">
                <RadioGroup.Item value="existing" />
                <Text size="2" weight="bold">
                  Replace tracks in an existing playlist
                </Text>
              </Flex>
            </label>
            {mode === "existing" && (
              <Flex direction="column" gap="2" ml="5">
                <TextField.Root
                  value={existingUrl}
                  onChange={(ev) => setExistingUrl(ev.target.value)}
                  placeholder="Spotify playlist link"
                  onKeyDown={(ev) => {
                    if (ev.key === "Enter") submit();
                  }}
                />
                {lookupStatus === "checking" && (
                  <Flex gap="2" align="center">
                    <Spinner size="1" />
                    <Text size="1" color="gray">
                      Looking up playlist…
                    </Text>
                  </Flex>
                )}
                {lookupStatus === "found" && (
                  <Text size="1" color="amber">
                    This will remove all existing tracks from “{lookupName}” and
                    replace them with this export. Its link stays the same.
                  </Text>
                )}
                {lookupStatus === "not-found" && (
                  <Text size="1" color="red">
                    Couldn't find that playlist. Paste the playlist's share link
                    or its spotify:playlist:... URI.
                  </Text>
                )}
              </Flex>
            )}
          </Flex>
        </RadioGroup.Root>

        <Flex gap="3" mt="4" justify="end">
          <Dialog.Close>
            <Button variant="soft" color="gray">
              Cancel
            </Button>
          </Dialog.Close>
          <Button color="green" disabled={!canCreate} onClick={submit}>
            {mode === "new" ? "Create" : "Replace"}
          </Button>
        </Flex>
      </Dialog.Content>
    </Dialog.Root>
  );
}

type SpotifyPlaylistProgressDialogProps = {
  running: boolean;
  checked: number;
  matched: number;
  total: number;
};

export function SpotifyPlaylistProgressDialog({
  running,
  checked,
  matched,
  total,
}: SpotifyPlaylistProgressDialogProps) {
  const progressValue =
    total > 0 ? Math.round((checked / total) * 100) : undefined;

  return (
    <Dialog.Root open={running}>
      <Dialog.Content maxWidth="400px">
        <Dialog.Title>Creating Spotify playlist</Dialog.Title>
        <Flex direction="column" gap="3" mt="2">
          <Text size="2">
            Matching tracks… ({checked} / {total})
          </Text>
          <Progress value={progressValue} size="2" />
          <Text size="1" color="gray">
            {matched} of {checked} checked tracks matched
          </Text>
        </Flex>
      </Dialog.Content>
    </Dialog.Root>
  );
}

export default SpotifyPlaylistDialog;
