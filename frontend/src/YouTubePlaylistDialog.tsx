import { useState } from "react";
import {
  Text,
  Button,
  Dialog,
  TextField,
  Flex,
  Progress,
} from "@radix-ui/themes";

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

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Content maxWidth="440px">
        <Dialog.Title>Export to YouTube playlist</Dialog.Title>
        <Dialog.Description size="2" mb="4">
          Creates a private playlist on your connected YouTube account with{" "}
          {trackCount} track{trackCount !== 1 ? "s" : ""}.
        </Dialog.Description>

        <label>
          <Text as="div" size="2" mb="1" weight="bold">
            Playlist name
          </Text>
          <TextField.Root
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="stereo"
            onKeyDown={(ev) => {
              if (ev.key === "Enter" && name.trim()) {
                onCreate(name.trim());
                onOpenChange(false);
              }
            }}
          />
        </label>

        <Flex gap="3" mt="4" justify="end">
          <Dialog.Close>
            <Button variant="soft" color="gray">
              Cancel
            </Button>
          </Dialog.Close>
          <Dialog.Close>
            <Button
              color="red"
              disabled={!name.trim() || trackCount === 0}
              onClick={() => onCreate(name.trim())}
            >
              Export
            </Button>
          </Dialog.Close>
        </Flex>
      </Dialog.Content>
    </Dialog.Root>
  );
}

type YouTubePlaylistProgressDialogProps = {
  running: boolean;
  current: number;
  total: number;
};

export function YouTubePlaylistProgressDialog({
  running,
  current,
  total,
}: YouTubePlaylistProgressDialogProps) {
  const progressValue =
    total > 0 ? Math.round((current / total) * 100) : undefined;

  return (
    <Dialog.Root open={running}>
      <Dialog.Content maxWidth="400px">
        <Dialog.Title>Creating YouTube playlist</Dialog.Title>
        <Flex direction="column" gap="3" mt="2">
          <Text size="2">
            Adding tracks… ({current} / {total})
          </Text>
          <Progress value={progressValue} size="2" />
        </Flex>
      </Dialog.Content>
    </Dialog.Root>
  );
}
