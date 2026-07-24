import { useState } from "react";
import { Text, Button, Dialog, TextField, Flex } from "@radix-ui/themes";

type SpotifyPlaylistDialogProps = {
  open: boolean;
  trackCount: number;
  onOpenChange: (open: boolean) => void;
  onCreate: (name: string) => void;
};

function SpotifyPlaylistDialog({
  open,
  trackCount,
  onOpenChange,
  onCreate,
}: SpotifyPlaylistDialogProps) {
  const [name, setName] = useState("stereo");

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Content maxWidth="440px">
        <Dialog.Title>Create Spotify playlist</Dialog.Title>
        <Dialog.Description size="2" mb="4">
          Creates a playlist on your connected Spotify account with up to{" "}
          {trackCount} track{trackCount !== 1 ? "s" : ""}. Tracks are matched on
          Spotify by artist/title, so some may not be found.
        </Dialog.Description>

        <label>
          <Text as="div" size="2" mb="1" weight="bold">
            Playlist name
          </Text>
          <TextField.Root
            value={name}
            onChange={(ev) => setName(ev.target.value)}
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
              color="green"
              disabled={!name.trim() || trackCount === 0}
              onClick={() => onCreate(name.trim())}
            >
              Create
            </Button>
          </Dialog.Close>
        </Flex>
      </Dialog.Content>
    </Dialog.Root>
  );
}

export default SpotifyPlaylistDialog;
