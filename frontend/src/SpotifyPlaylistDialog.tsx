import { useState, useEffect, useRef } from "react";
import { Text } from "@astryxdesign/core/Text";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { RadioList, RadioListItem } from "@astryxdesign/core/RadioList";
import { ProgressBar } from "@astryxdesign/core/ProgressBar";
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
    <Dialog
      isOpen={open}
      onOpenChange={onOpenChange}
      purpose="form"
      width={440}
    >
      <Layout
        header={
          <DialogHeader
            title="Export to Spotify playlist"
            subtitle={`Exports up to ${trackCount} track${trackCount !== 1 ? "s" : ""} to Spotify. Tracks are matched by artist/title, so some may not be found.`}
            onOpenChange={() => onOpenChange(false)}
          />
        }
        content={
          <LayoutContent>
            <VStack gap={3}>
              <RadioList
                label="Playlist target"
                isLabelHidden
                value={mode}
                onChange={(v) => setMode(v as "new" | "existing")}
              >
                <RadioListItem value="new" label="Create a new playlist" />
                <RadioListItem
                  value="existing"
                  label="Replace tracks in an existing playlist"
                />
              </RadioList>

              {mode === "new" && (
                <TextInput
                  label="Playlist name"
                  isLabelHidden
                  value={name}
                  onChange={setName}
                  placeholder="stereo"
                  onKeyDown={(ev) => {
                    if (ev.key === "Enter") submit();
                  }}
                />
              )}

              {mode === "existing" && (
                <VStack gap={2}>
                  <TextInput
                    label="Existing playlist"
                    isLabelHidden
                    value={existingUrl}
                    onChange={setExistingUrl}
                    placeholder="Spotify playlist link"
                    onKeyDown={(ev) => {
                      if (ev.key === "Enter") submit();
                    }}
                    isLoading={lookupStatus === "checking"}
                    status={
                      lookupStatus === "found"
                        ? {
                            type: "warning",
                            message: `This will remove all existing tracks from "${lookupName}" and replace them with this export. Its link stays the same.`,
                          }
                        : lookupStatus === "not-found"
                          ? {
                              type: "error",
                              message:
                                "Couldn't find that playlist. Paste the playlist's share link or its spotify:playlist:... URI.",
                            }
                          : undefined
                    }
                  />
                  {lookupStatus === "checking" && (
                    <Text type="supporting">Looking up playlist…</Text>
                  )}
                </VStack>
              )}
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
                label={mode === "new" ? "Create" : "Replace"}
                variant="primary"
                isDisabled={!canCreate}
                onClick={submit}
              />
            </HStack>
          </LayoutFooter>
        }
      />
    </Dialog>
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
  const progressValue = total > 0 ? Math.round((checked / total) * 100) : 0;

  return (
    <Dialog
      isOpen={running}
      onOpenChange={() => {}}
      purpose="required"
      width={400}
    >
      <Layout
        header={<DialogHeader title="Creating Spotify playlist" />}
        content={
          <LayoutContent>
            <VStack gap={3}>
              <Text type="body">
                Matching tracks… ({checked} / {total})
              </Text>
              <ProgressBar
                label="Matching tracks"
                isLabelHidden
                value={progressValue}
                isIndeterminate={total === 0}
              />
              <Text type="supporting">
                {matched} of {checked} checked tracks matched
              </Text>
            </VStack>
          </LayoutContent>
        }
      />
    </Dialog>
  );
}

export default SpotifyPlaylistDialog;
