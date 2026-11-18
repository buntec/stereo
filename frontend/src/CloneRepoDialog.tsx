import { useState } from "react";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { Button } from "@astryxdesign/core/Button";
import { FormLayout } from "@astryxdesign/core/FormLayout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { HStack } from "@astryxdesign/core/Stack";

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
            {/* A native form (Astryx has none), so that Enter submits only once an input
                method editor has committed its text. */}
            <form
              onSubmit={(ev) => {
                ev.preventDefault();
                clone();
              }}
            >
              <FormLayout>
                <TextInput
                  label="Git URL"
                  value={url}
                  placeholder="https://github.com/user/my-sets.git"
                  onChange={(v) => setUrl(v.trim())}
                />
              </FormLayout>
            </form>
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
