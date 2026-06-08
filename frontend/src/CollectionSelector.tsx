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
  Code,
} from "@radix-ui/themes";
import { CubeIcon, PlusCircledIcon } from "@radix-ui/react-icons";
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
};

function CollectionSelector({
  collections,
  value,
  setValue,
  onCreate,
  isValid,
}: CollectionSelectorProps) {
  const [dialogOpen, setDialogOpen] = useState(false);
  const [newName, setNewName] = useState("");

  const selectedValue = collections.some((c) => c.path === value)
    ? value
    : undefined;

  return (
    <Flex className="collection-selector-box" align="center" gap="2" m="2">
      <CubeIcon color={isValid ? "green" : "red"} height="16" width="16" />

      <Select.Root value={selectedValue} onValueChange={setValue}>
        <Select.Trigger variant="soft" placeholder="Select collection…" />
        <Select.Content>
          {collections.map((c) => (
            <Select.Item key={c.path} value={c.path}>
              {collectionLabel(c.path)}
            </Select.Item>
          ))}
        </Select.Content>
      </Select.Root>

      <Dialog.Root open={dialogOpen} onOpenChange={setDialogOpen}>
        <Tooltip content="Create new collection">
          <Dialog.Trigger>
            <IconButton variant="soft">
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
    </Flex>
  );
}

export default CollectionSelector;
