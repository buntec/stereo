import { useState } from "react";
import {
  Select,
  Dialog,
  Button,
  IconButton,
  TextField,
  Flex,
  Text,
  Tooltip,
} from "@radix-ui/themes";
import { PlusCircledIcon } from "@radix-ui/react-icons";
import type { IRepo } from "./Types.tsx";

function repoLabel(repo: IRepo): string {
  return repo.remote ?? repo.path;
}

type RepoSelectorProps = {
  repos: IRepo[];
  value: string;
  setValue: (path: string) => void;
  onClone: (url: string) => void;
};

function RepoSelector({ repos, value, setValue, onClone }: RepoSelectorProps) {
  const [dialogOpen, setDialogOpen] = useState(false);
  const [url, setUrl] = useState("");

  const selectedValue = repos.some((r) => r.path === value) ? value : undefined;

  return (
    <Flex align="center" gap="2" m="2">
      <Select.Root value={selectedValue} onValueChange={setValue}>
        <Select.Trigger
          variant="soft"
          placeholder={repos.length === 0 ? "No repos — clone one →" : "Select repo…"}
        />
        <Select.Content>
          {repos.map((r) => (
            <Select.Item key={r.path} value={r.path}>
              {repoLabel(r)}
            </Select.Item>
          ))}
        </Select.Content>
      </Select.Root>

      <Dialog.Root open={dialogOpen} onOpenChange={setDialogOpen}>
        <Tooltip content="Clone a git repository">
          <Dialog.Trigger>
            <IconButton variant="soft">
              <PlusCircledIcon />
            </IconButton>
          </Dialog.Trigger>
        </Tooltip>

        <Dialog.Content maxWidth="440px">
          <Dialog.Title>Clone a repository</Dialog.Title>
          <Dialog.Description size="2" mb="4">
            Enter a git URL. The repository will be cloned into the stereo home
            directory and its collections will appear in the dropdown.
          </Dialog.Description>

          <label>
            <Text as="div" size="2" mb="1" weight="bold">
              Git URL
            </Text>
            <TextField.Root
              value={url}
              placeholder="https://github.com/user/my-sets.git"
              onChange={(ev) => setUrl(ev.target.value.trim())}
              onKeyDown={(ev) => {
                if (ev.key === "Enter" && url) {
                  onClone(url);
                  setUrl("");
                  setDialogOpen(false);
                }
              }}
            />
          </label>

          <Flex gap="3" mt="4" justify="end">
            <Dialog.Close>
              <Button variant="soft" color="gray" onClick={() => setUrl("")}>
                Cancel
              </Button>
            </Dialog.Close>
            <Dialog.Close>
              <Button
                color="green"
                disabled={!url}
                onClick={() => {
                  onClone(url);
                  setUrl("");
                }}
              >
                Clone
              </Button>
            </Dialog.Close>
          </Flex>
        </Dialog.Content>
      </Dialog.Root>
    </Flex>
  );
}

export default RepoSelector;
