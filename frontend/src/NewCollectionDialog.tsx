import { useState } from "react";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { Button } from "@astryxdesign/core/Button";
import { FormLayout } from "@astryxdesign/core/FormLayout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { HStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { Code } from "@astryxdesign/core/Code";

type NewCollectionDialogProps = {
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  onCreate: (name: string) => void;
};

function NewCollectionDialog({
  isOpen,
  onOpenChange,
  onCreate,
}: NewCollectionDialogProps) {
  const [name, setName] = useState("");

  const close = () => {
    onOpenChange(false);
    setName("");
  };

  const create = () => {
    if (!name.trim()) return;
    onCreate(name.trim());
    close();
  };

  return (
    <Dialog
      isOpen={isOpen}
      onOpenChange={(open) => (open ? onOpenChange(true) : close())}
      purpose="form"
      width={380}
    >
      <Layout
        header={<DialogHeader title="New Collection" onOpenChange={close} />}
        content={
          <LayoutContent>
            <FormLayout>
              <Text as="div" type="body">
                Enter a name for the new collection. It will be created as{" "}
                <Code>{name || "name"}.db</Code> in the current repo.
              </Text>
              <TextInput
                label="Collection name"
                value={name}
                placeholder="my-sets"
                onChange={(v) => setName(v.replace(/[/\\]/g, ""))}
                onKeyDown={(ev) => {
                  if (ev.key === "Enter") create();
                }}
              />
            </FormLayout>
          </LayoutContent>
        }
        footer={
          <LayoutFooter>
            <HStack gap={2} hAlign="end">
              <Button label="Cancel" variant="secondary" onClick={close} />
              <Button
                label="Create"
                variant="primary"
                isDisabled={!name.trim()}
                onClick={create}
              />
            </HStack>
          </LayoutFooter>
        }
      />
    </Dialog>
  );
}

export default NewCollectionDialog;
