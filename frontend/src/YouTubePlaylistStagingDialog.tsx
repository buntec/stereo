import { useState } from "react";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { List, ListItem } from "@astryxdesign/core/List";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import type { ITrack, RequestReply } from "./Types.tsx";
import { EditTrackDialog } from "./EditDialog.tsx";

type StagedTrackRowProps = {
  track: ITrack;
  requestReply: RequestReply;
  onChange: (track: ITrack) => void;
};

function StagedTrackRow({
  track,
  requestReply,
  onChange,
}: StagedTrackRowProps) {
  const [value, setValue] = useState(track);
  const [prevTrack, setPrevTrack] = useState(track);
  if (track !== prevTrack) {
    setPrevTrack(track);
    setValue(track);
  }

  const isValid = Boolean(value.yt_id.trim() && value.title.trim());

  return (
    <ListItem
      label={value.title || "Untitled track"}
      description={value.artists.join(", ") || "No artist"}
      endContent={
        <EditTrackDialog
          value={value}
          isValid={isValid}
          setValue={setValue}
          resetValue={() => setValue(track)}
          commit={() => onChange(value)}
          requestReply={requestReply}
        />
      }
    />
  );
}

type YouTubePlaylistStagingDialogProps = {
  open: boolean;
  name: string;
  tracks: ITrack[];
  requestReply: RequestReply;
  onCancel: () => void;
  onCommit: (tracks: ITrack[]) => void;
};

export default function YouTubePlaylistStagingDialog({
  open,
  name,
  tracks,
  requestReply,
  onCancel,
  onCommit,
}: YouTubePlaylistStagingDialogProps) {
  const [stagedTracks, setStagedTracks] = useState(() => tracks);

  const updateTrack = (index: number, track: ITrack) => {
    setStagedTracks((current) =>
      current.map((existing, currentIndex) =>
        currentIndex === index ? track : existing,
      ),
    );
  };

  return (
    <Dialog
      isOpen={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) onCancel();
      }}
      purpose="form"
      width={760}
    >
      <Layout
        header={
          <DialogHeader
            title="Review imported tracks"
            subtitle={`${stagedTracks.length} tracks from “${name}”`}
            onOpenChange={onCancel}
          />
        }
        content={
          <LayoutContent>
            <VStack gap={3}>
              <List hasDividers density="compact">
                {stagedTracks.map((track, index) => (
                  <StagedTrackRow
                    key={`${track.yt_id}-${index}`}
                    track={track}
                    requestReply={requestReply}
                    onChange={(nextTrack) => updateTrack(index, nextTrack)}
                  />
                ))}
              </List>
            </VStack>
          </LayoutContent>
        }
        footer={
          <LayoutFooter>
            <HStack gap={2} hAlign="end">
              <Button label="Discard" variant="secondary" onClick={onCancel} />
              <Button
                label="Add to collection"
                variant="primary"
                isDisabled={stagedTracks.some(
                  (track) => !track.yt_id.trim() || !track.title.trim(),
                )}
                onClick={() => onCommit(stagedTracks)}
              />
            </HStack>
          </LayoutFooter>
        }
      />
    </Dialog>
  );
}
