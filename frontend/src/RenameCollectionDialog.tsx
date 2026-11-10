import { useState } from "react";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { Button } from "@astryxdesign/core/Button";
import { FormLayout } from "@astryxdesign/core/FormLayout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { HStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { Code } from "@astryxdesign/core/Code";

type RenameCollectionDialogProps = {
  // The current name of the collection being renamed; null when closed.
  currentName: string | null;
  onClose: () => void;
  onRename: (name: string) => void;
  hasRemote: boolean;
};

function RenameCollectionDialog({
  currentName,
  onClose,
  onRename,
  hasRemote,
}: RenameCollectionDialogProps) {
  const [name, setName] = useState("");
  // Reset the input whenever the dialog opens for a (different) collection.
  const [openedFor, setOpenedFor] = useState<string | null>(null);
  if (currentName !== openedFor) {
    setOpenedFor(currentName);
    setName(currentName ?? "");
  }

  const trimmed = name.trim();
  const canRename = trimmed !== "" && trimmed !== currentName;

  const rename = () => {
    if (!canRename) return;
    onRename(trimmed);
    onClose();
  };

  return (
    <Dialog
      isOpen={currentName !== null}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      purpose="form"
      width={380}
    >
      <Layout
        header={
          <DialogHeader title="Rename Collection" onOpenChange={onClose} />
        }
        content={
          <LayoutContent>
            <FormLayout>
              <Text as="div" type="body">
                The collection file will be renamed to{" "}
                <Code>{trimmed || "name"}.db</Code>.
                {hasRemote &&
                  " The rename is synced to the repository's remote, so it also applies on your other machines."}
              </Text>
              <TextInput
                label="Collection name"
                value={name}
                onChange={(v) => setName(v.replace(/[/\\]/g, ""))}
                onKeyDown={(ev) => {
                  if (ev.key === "Enter") rename();
                }}
              />
            </FormLayout>
          </LayoutContent>
        }
        footer={
          <LayoutFooter>
            <HStack gap={2} hAlign="end">
              <Button label="Cancel" variant="secondary" onClick={onClose} />
              <Button
                label="Rename"
                variant="primary"
                isDisabled={!canRename}
                onClick={rename}
              />
            </HStack>
          </LayoutFooter>
        }
      />
    </Dialog>
  );
}

export default RenameCollectionDialog;
