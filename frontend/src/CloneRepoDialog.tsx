import { useState } from "react";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { Button } from "@astryxdesign/core/Button";
import { TextInput } from "@astryxdesign/core/TextInput";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";

type CloneRepoDialogProps = {
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  onClone: (url: string) => void;
};

function CloneRepoDialog({
  isOpen,
  onOpenChange,
  onClone,
}: CloneRepoDialogProps) {
  const [url, setUrl] = useState("");

  const close = () => {
    onOpenChange(false);
    setUrl("");
  };

  const clone = () => {
    if (!url) return;
    onClone(url);
    close();
  };

  return (
    <Dialog
      isOpen={isOpen}
      onOpenChange={(open) => (open ? onOpenChange(true) : close())}
      purpose="form"
      width={440}
    >
      <Layout
        header={
          <DialogHeader title="Clone a repository" onOpenChange={close} />
        }
        content={
          <LayoutContent>
            <VStack gap={3}>
              <Text as="div" type="body">
                Enter a git URL. The repository will be cloned into the stereo
                home directory and its collections will appear in the collection
                menu.
              </Text>
              <TextInput
                label="Git URL"
                value={url}
                placeholder="https://github.com/user/my-sets.git"
                onChange={(v) => setUrl(v.trim())}
                onKeyDown={(ev) => {
                  if (ev.key === "Enter") clone();
                }}
              />
            </VStack>
          </LayoutContent>
        }
        footer={
          <LayoutFooter>
            <HStack gap={2} hAlign="end">
              <Button label="Cancel" variant="secondary" onClick={close} />
              <Button
                label="Clone"
                variant="primary"
                isDisabled={!url}
                onClick={clone}
              />
            </HStack>
          </LayoutFooter>
        }
      />
    </Dialog>
  );
}

export default CloneRepoDialog;
