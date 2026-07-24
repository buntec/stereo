import { useState } from "react";
import type { ReactNode } from "react";
import { InputGroup, InputGroupText } from "@astryxdesign/core/InputGroup";
import { Selector } from "@astryxdesign/core/Selector";
import { Divider } from "@astryxdesign/core/Divider";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { Button } from "@astryxdesign/core/Button";
import { TextInput } from "@astryxdesign/core/TextInput";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { Code } from "@astryxdesign/core/Code";
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
  children?: ReactNode;
};

function CollectionSelector({
  collections,
  value,
  setValue,
  onCreate,
  isValid,
  repoVisible,
  onToggleRepo,
  children,
}: CollectionSelectorProps) {
  const [dialogOpen, setDialogOpen] = useState(false);
  const [newName, setNewName] = useState("");

  const selectedValue = collections.some((c) => c.path === value)
    ? value
    : undefined;

  const closeDialog = () => {
    setDialogOpen(false);
    setNewName("");
  };

  return (
    <HStack className="collection-selector-box" gap={0} vAlign="center">
      <InputGroup label="Collection" isLabelHidden>
        <InputGroupText>
          <CubeIcon color={isValid ? "green" : "red"} height={14} width={14} />
        </InputGroupText>
        <Selector
          label="Collection"
          isLabelHidden
          placeholder="Select collection…"
          options={collections.map((c) => ({
            value: c.path,
            label: collectionLabel(c.path),
          }))}
          value={selectedValue}
          onChange={setValue}
        />
      </InputGroup>

      <HStack className="collection-selector-actions" gap={2} vAlign="center">
        <Divider orientation="vertical" style={{ height: "14px" }} />

        <IconButton
          variant="ghost"
          size="sm"
          label="Create new collection"
          tooltip="Create new collection"
          icon={<PlusCircledIcon />}
          onClick={() => setDialogOpen(true)}
        />

        <Dialog
          isOpen={dialogOpen}
          onOpenChange={(open) => (open ? setDialogOpen(true) : closeDialog())}
          purpose="form"
          width={380}
        >
          <Layout
            header={
              <DialogHeader
                title="New Collection"
                onOpenChange={() => closeDialog()}
              />
            }
            content={
              <LayoutContent>
                <VStack gap={3}>
                  <Text as="div" type="body">
                    Enter a name for the new collection. It will be created as{" "}
                    <Code>{newName || "name"}.db</Code> in the current repo.
                  </Text>
                  <TextInput
                    label="Collection name"
                    value={newName}
                    placeholder="my-sets"
                    onChange={(v) => setNewName(v.replace(/[/\\]/g, ""))}
                  />
                </VStack>
              </LayoutContent>
            }
            footer={
              <LayoutFooter>
                <HStack gap={2} hAlign="end">
                  <Button
                    label="Cancel"
                    variant="secondary"
                    onClick={closeDialog}
                  />
                  <Button
                    label="Create"
                    variant="primary"
                    isDisabled={!newName.trim()}
                    onClick={() => {
                      onCreate(newName.trim());
                      closeDialog();
                    }}
                  />
                </HStack>
              </LayoutFooter>
            }
          />
        </Dialog>

        <IconButton
          variant={repoVisible ? "secondary" : "ghost"}
          size="sm"
          label="Git repository"
          tooltip="Git repository"
          icon={<Link2Icon />}
          onClick={onToggleRepo}
        />

        {children}
      </HStack>
    </HStack>
  );
}

export default CollectionSelector;
