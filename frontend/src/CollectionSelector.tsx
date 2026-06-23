import { useState } from "react";
import {
  Select,
  Dialog,
  Button,
  IconButton,
  TextField,
  Separator,
  Flex,
  Text,
  Tooltip,
  Code,
} from "@radix-ui/themes";
import { CubeIcon, PlusCircledIcon, Link2Icon } from "@radix-ui/react-icons";
import type { ICollection } from "./Types.tsx";

export function collectionLabel(path: string): string {
  return path.split("/").pop() ?? path;
}

export type CollectionSelectorProps = {
  collections: ICollection[];
  value: string;
  setValue: (path: string) => void;
  onCreate: (name: string) => void;
  isValid: boolean;
  repoVisible: boolean;
  onToggleRepo: () => void;
};

function CollectionSelector({
  collections,
  value,
  setValue,
  onCreate,
  isValid,
  repoVisible,
  onToggleRepo,
}: CollectionSelectorProps) {
  const [dialogOpen, setDialogOpen] = useState(false);
  const [newName, setNewName] = useState("");

  const selectedValue = collections.some((c) => c.path === value)
    ? value
    : undefined;

  return (
    <div className="rt-TextFieldRoot rt-r-size-2 rt-variant-soft collection-selector-box">
      <TextField.Slot>
        <CubeIcon color={isValid ? "green" : "red"} height="14" width="14" />
      </TextField.Slot>

      <Select.Root value={selectedValue} onValueChange={setValue}>
        <Select.Trigger variant="ghost" placeholder="Select collection…" />
        <Select.Content>
          {collections.map((c) => (
            <Select.Item key={c.path} value={c.path}>
              {collectionLabel(c.path)}
            </Select.Item>
          ))}
        </Select.Content>
      </Select.Root>

      <TextField.Slot side="right" className="collection-selector-actions">
        <Separator orientation="vertical" style={{ height: "14px" }} />

        <Dialog.Root open={dialogOpen} onOpenChange={setDialogOpen}>
          <Tooltip content="Create new collection">
            <Dialog.Trigger>
              <IconButton variant="ghost" size="1" color="gray">
                <PlusCircledIcon />
              </IconButton>
            </Dialog.Trigger>
          </Tooltip>

          <Dialog.Content maxWidth="380px">
            <Dialog.Title>New Collection</Dialog.Title>
            <Dialog.Description size="2" mb="4">
              Enter a name for the new collection. It will be created as{" "}
              <Code>{newName || "name"}.db</Code> in the current repo.
            </Dialog.Description>

            <label>
              <Text as="div" size="2" mb="1" weight="bold">
                Collection name
              </Text>
              <TextField.Root
                value={newName}
                placeholder="my-sets"
                onChange={(ev) =>
                  setNewName(ev.target.value.replace(/[/\\]/g, ""))
                }
              />
            </label>

            <Flex gap="3" mt="4" justify="end">
              <Dialog.Close>
                <Button
                  variant="soft"
                  color="gray"
                  onClick={() => setNewName("")}
                >
                  Cancel
                </Button>
              </Dialog.Close>
              <Dialog.Close>
                <Button
                  color="green"
                  disabled={!newName.trim()}
                  onClick={() => {
                    onCreate(newName.trim());
                    setNewName("");
                  }}
                >
                  Create
                </Button>
              </Dialog.Close>
            </Flex>
          </Dialog.Content>
        </Dialog.Root>

        <Tooltip content="Git repository">
          <IconButton
            variant="ghost"
            size="1"
            color={repoVisible ? "cyan" : "gray"}
            onClick={onToggleRepo}
          >
            <Link2Icon />
          </IconButton>
        </Tooltip>
      </TextField.Slot>
    </div>
  );
}

export default CollectionSelector;
