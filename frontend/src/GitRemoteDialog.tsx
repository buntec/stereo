import { useState, useEffect } from "react";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { IconButton } from "@astryxdesign/core/IconButton";
import { FormLayout } from "@astryxdesign/core/FormLayout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { HStack } from "@astryxdesign/core/Stack";
import { GitHubLogoIcon } from "@radix-ui/react-icons";

type GitRemoteDialogProps = {
  currentRemote: string | null | undefined;
  onOpen: () => void;
  onSave: (url: string | null) => void;
};

function GitRemoteDialog({
  currentRemote,
  onOpen,
  onSave,
}: GitRemoteDialogProps) {
  const [dialogOpen, setDialogOpen] = useState(false);
  const [value, setValue] = useState("");

  useEffect(() => {
    setValue(currentRemote ?? "");
  }, [currentRemote]);

  const closeDialog = () => setDialogOpen(false);

  return (
    <>
      <IconButton
        variant={currentRemote ? "primary" : "secondary"}
        label="Git remote"
        tooltip="Git remote"
        icon={<GitHubLogoIcon />}
        onClick={() => {
          setDialogOpen(true);
          onOpen();
        }}
      />

      <Dialog
        isOpen={dialogOpen}
        onOpenChange={(open) => {
          if (open) {
            setDialogOpen(true);
            onOpen();
          } else {
            closeDialog();
          }
        }}
        purpose="form"
        width={480}
      >
        <Layout
          header={
            <DialogHeader title="Git Remote" onOpenChange={closeDialog} />
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
                      closeDialog();
                    }}
                  />
                )}
                <Button
                  label="Cancel"
                  variant="secondary"
                  onClick={closeDialog}
                />
                <Button
                  label="Save"
                  variant="primary"
                  isDisabled={value.trim() === (currentRemote ?? "")}
                  onClick={() => {
                    onSave(value.trim() || null);
                    closeDialog();
                  }}
                />
              </HStack>
            </LayoutFooter>
          }
        />
      </Dialog>
    </>
  );
}

export default GitRemoteDialog;
