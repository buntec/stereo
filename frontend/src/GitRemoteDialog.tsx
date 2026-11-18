import { useState, useEffect } from "react";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { FormLayout } from "@astryxdesign/core/FormLayout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { HStack } from "@astryxdesign/core/Stack";

type GitRemoteDialogProps = {
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  // The repository whose remote is edited, for the title.
  repoName: string | undefined;
  currentRemote: string | null | undefined;
  onSave: (url: string | null) => void;
};

function GitRemoteDialog({
  isOpen,
  onOpenChange,
  repoName,
  currentRemote,
  onSave,
}: GitRemoteDialogProps) {
  const [value, setValue] = useState("");

  // Start from the current remote on every opening.
  useEffect(() => {
    if (isOpen) setValue(currentRemote ?? "");
  }, [isOpen, currentRemote]);

  const close = () => onOpenChange(false);
  const url = value.trim();
  const canSave = url !== (currentRemote ?? "");

  const save = () => {
    if (!canSave) return;
    onSave(url || null);
    close();
  };

  return (
    <Dialog
      isOpen={isOpen}
      onOpenChange={onOpenChange}
      purpose="form"
      width={480}
    >
      <Layout
        header={
          <DialogHeader
            title={repoName ? `Remote for ${repoName}` : "Git remote"}
            onOpenChange={close}
          />
        }
        content={
          <LayoutContent>
            <FormLayout>
              <TextInput
                label="Remote URL"
                labelTooltip="Collections are pushed here after changes. Use an empty repository."
                placeholder="git@github.com:you/stereo-collections.git"
                value={value}
                onChange={setValue}
                onKeyDown={(ev) => {
                  if (ev.key === "Enter") save();
                }}
              />
            </FormLayout>
          </LayoutContent>
        }
        footer={
          <LayoutFooter>
            <HStack gap={2} hAlign="end">
              {currentRemote && (
                <Button
                  label="Remove"
                  variant="destructive"
                  onClick={() => {
                    onSave(null);
                    close();
                  }}
                />
              )}
              <Button label="Cancel" variant="secondary" onClick={close} />
              <Button
                label="Save"
                variant="primary"
                isDisabled={!canSave}
                onClick={save}
              />
            </HStack>
          </LayoutFooter>
        }
      />
    </Dialog>
  );
}

export default GitRemoteDialog;
