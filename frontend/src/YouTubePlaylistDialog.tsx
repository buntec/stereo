import { useState } from "react";
import { Text } from "@astryxdesign/core/Text";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { ProgressBar } from "@astryxdesign/core/ProgressBar";

type YouTubePlaylistDialogProps = {
  open: boolean;
  trackCount: number;
  onOpenChange: (open: boolean) => void;
  onCreate: (name: string) => void;
};

export function YouTubePlaylistDialog({
  open,
  trackCount,
  onOpenChange,
  onCreate,
}: YouTubePlaylistDialogProps) {
  const [name, setName] = useState("stereo");

  const submit = () => {
    if (!name.trim() || trackCount === 0) return;
    onCreate(name.trim());
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
            title="Export to YouTube playlist"
            subtitle={`Private playlist, ${trackCount} track${trackCount !== 1 ? "s" : ""}`}
            onOpenChange={() => onOpenChange(false)}
          />
        }
        content={
          <LayoutContent>
            <TextInput
              label="Playlist name"
              value={name}
              onChange={setName}
              placeholder="stereo"
              onKeyDown={(ev) => {
                if (ev.key === "Enter") submit();
              }}
            />
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
                label="Export"
                variant="primary"
                isDisabled={!name.trim() || trackCount === 0}
                onClick={submit}
              />
            </HStack>
          </LayoutFooter>
        }
      />
    </Dialog>
  );
}

type YouTubePlaylistProgressDialogProps = {
  running: boolean;
  current: number;
  total: number;
  onCancel: () => void;
};

export function YouTubePlaylistProgressDialog({
  running,
  current,
  total,
  onCancel,
}: YouTubePlaylistProgressDialogProps) {
  const progressValue = total > 0 ? Math.round((current / total) * 100) : 0;

  return (
    <Dialog
      isOpen={running}
      onOpenChange={(open) => {
        if (!open) onCancel();
      }}
      purpose="form"
      width={400}
    >
      <Layout
        header={<DialogHeader title="Creating YouTube playlist" />}
        content={
          <LayoutContent>
            <VStack gap={3}>
              <Text type="body">
                Adding tracks… ({current} / {total})
              </Text>
              <ProgressBar
                label="Adding tracks"
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
