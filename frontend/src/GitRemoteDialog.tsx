import { useState, useEffect } from "react";
import {
  Text,
  Button,
  Dialog,
  IconButton,
  Tooltip,
  TextField,
  Flex,
  Code,
} from "@radix-ui/themes";
import { GitHubLogoIcon } from "@radix-ui/react-icons";

type GitRemoteDialogProps = {
  currentRemote: string | null | undefined;
  onOpen: () => void;
  onSave: (url: string | null) => void;
};

function GitRemoteDialog({ currentRemote, onOpen, onSave }: GitRemoteDialogProps) {
  const [value, setValue] = useState("");

  useEffect(() => {
    setValue(currentRemote ?? "");
  }, [currentRemote]);

  return (
    <Dialog.Root onOpenChange={(open) => { if (open) onOpen(); }}>
      <Tooltip content="Configure git remote for collection backups">
        <Dialog.Trigger>
          <IconButton variant={currentRemote ? "solid" : "soft"} color={currentRemote ? "green" : "gray"}>
            <GitHubLogoIcon />
          </IconButton>
        </Dialog.Trigger>
      </Tooltip>

      <Dialog.Content maxWidth="480px">
        <Dialog.Title>Git Remote</Dialog.Title>
        <Dialog.Description size="2" mb="4">
          Set a remote URL to automatically push collection snapshots after each
          change. Create an empty repo on GitHub or GitLab first, then paste its
          URL here. Example:{" "}
          <Code>git@github.com:you/stereo-collections.git</Code>
        </Dialog.Description>

        <Flex direction="column" gap="3">
          <label>
            <Text as="div" size="2" mb="1" weight="bold">
              Remote URL (origin)
            </Text>
            <TextField.Root
              placeholder="git@github.com:you/stereo-collections.git"
              value={value}
              onChange={(ev) => setValue(ev.target.value)}
            />
          </label>
        </Flex>

        <Flex gap="3" mt="4" justify="end">
          {currentRemote && (
            <Dialog.Close>
              <Button
                variant="soft"
                color="red"
                onClick={() => onSave(null)}
              >
                Remove
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
              onClick={() => onSave(value.trim() || null)}
              disabled={value.trim() === (currentRemote ?? "")}
            >
              Save
            </Button>
          </Dialog.Close>
        </Flex>
      </Dialog.Content>
    </Dialog.Root>
  );
}

export default GitRemoteDialog;
