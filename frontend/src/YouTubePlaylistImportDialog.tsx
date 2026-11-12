import { useState } from "react";
import { Text } from "@astryxdesign/core/Text";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { IconButton } from "@astryxdesign/core/IconButton";
import { TextInput } from "@astryxdesign/core/TextInput";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { ProgressBar } from "@astryxdesign/core/ProgressBar";
import { DownloadIcon } from "@radix-ui/react-icons";
import type { ITrack, RequestReply, ServerMsg } from "./Types.tsx";

type YouTubePlaylistImportDialogProps = {
  requestReply: RequestReply;
  onImport: (name: string, tracks: ITrack[]) => void;
  showTrigger?: boolean;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
};

function YouTubePlaylistImportDialog({
  requestReply,
  onImport,
  showTrigger = true,
  open,
  onOpenChange,
}: YouTubePlaylistImportDialogProps) {
  const [internalOpen, setInternalOpen] = useState(false);
  const [url, setUrl] = useState("");
  const [error, setError] = useState<string>();
  const [loading, setLoading] = useState(false);
  const dialogOpen = open ?? internalOpen;
  const setDialogOpen = onOpenChange ?? setInternalOpen;

  const close = () => {
    setDialogOpen(false);
    setLoading(false);
    setError(undefined);
  };

  const submit = () => {
    const value = url.trim();
    if (!value || loading) return;

    setLoading(true);
    setError(undefined);
    requestReply(
      { type: "get-youtube-playlist", url: value },
      (response: ServerMsg | { type: string }) => {
        setLoading(false);
        if (
          response.type !== "youtube-playlist-info" ||
          !("tracks" in response)
        ) {
          setError("The playlist could not be loaded. Please try again.");
          return;
        }
        if (response.error_message) {
          setError(response.error_message);
          return;
        }
        if (response.tracks.length === 0) {
          setError("The playlist does not contain any importable videos.");
          return;
        }
        onImport(response.name ?? "YouTube playlist", response.tracks);
        close();
      },
      60000,
    );
  };

  return (
    <>
      {showTrigger && (
        <IconButton
          variant="secondary"
          label="Import tracks from a YouTube playlist"
          tooltip="Import tracks from a YouTube playlist"
          icon={<DownloadIcon />}
          onClick={() => setDialogOpen(true)}
        />
      )}

      <Dialog
        isOpen={dialogOpen}
        onOpenChange={(nextOpen) => {
          if (nextOpen) setDialogOpen(true);
          else close();
        }}
        purpose="form"
        width={450}
      >
        <Layout
          header={
            <DialogHeader
              title="Import YouTube playlist"
              subtitle="Public playlists only"
              onOpenChange={close}
            />
          }
          content={
            <LayoutContent>
              <VStack gap={3}>
                <TextInput
                  label="Playlist URL"
                  value={url}
                  onChange={(value) => {
                    setUrl(value);
                    setError(undefined);
                  }}
                  placeholder="https://music.youtube.com/playlist?list=…"
                  status={error ? { type: "error", message: error } : undefined}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") submit();
                  }}
                />
              </VStack>
            </LayoutContent>
          }
          footer={
            <LayoutFooter>
              <HStack gap={2} hAlign="end">
                <Button label="Cancel" variant="secondary" onClick={close} />
                <Button
                  label="Import"
                  variant="primary"
                  isLoading={loading}
                  isDisabled={!url.trim() || loading}
                  onClick={submit}
                />
              </HStack>
            </LayoutFooter>
          }
        />
      </Dialog>
    </>
  );
}

export default YouTubePlaylistImportDialog;

type YouTubeFixupProgressDialogProps = {
  running: boolean;
  current: number;
  matched: number;
  total: number;
  onCancel: () => void;
};

export function YouTubeFixupProgressDialog({
  running,
  current,
  matched,
  total,
  onCancel,
}: YouTubeFixupProgressDialogProps) {
  const progressValue = total > 0 ? Math.round((current / total) * 100) : 0;

  return (
    <Dialog
      isOpen={running}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) onCancel();
      }}
      purpose="form"
      width={420}
    >
      <Layout
        header={<DialogHeader title="Enriching playlist metadata" />}
        content={
          <LayoutContent>
            <VStack gap={3}>
              <Text type="body">
                Looking up track metadata… ({current} / {total})
              </Text>
              <Text type="supporting">
                {matched} confident {matched === 1 ? "match" : "matches"} found
              </Text>
              <ProgressBar
                label="Enriching track metadata"
                isLabelHidden
                value={progressValue}
                isIndeterminate={total === 0}
              />
            </VStack>
          </LayoutContent>
        }
        footer={
          <LayoutFooter>
            <HStack gap={2} hAlign="end">
              <Button label="Cancel" variant="secondary" onClick={onCancel} />
            </HStack>
          </LayoutFooter>
        }
      />
    </Dialog>
  );
}
