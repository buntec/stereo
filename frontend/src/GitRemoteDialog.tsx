import { useState } from "react";
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
  // The URL starts out as this; remount the dialog (by its key) to start over.
  currentRemote: string | null | undefined;
  // Resolves once the backend has set the remote; the dialog stays open, with the draft, if
  // it rejects (the backend's notification says why).
  onSave: (url: string | null) => Promise<void>;
};

function GitRemoteDialog({
  isOpen,
  onOpenChange,
  repoName,
  currentRemote,
  onSave,
}: GitRemoteDialogProps) {
  const [value, setValue] = useState(currentRemote ?? "");
  const [pending, setPending] = useState<"save" | "remove" | null>(null);

  const close = () => onOpenChange(false);
  const url = value.trim();
  const canSave = url !== (currentRemote ?? "");

  const save = (action: "save" | "remove") => {
    if (pending || (action === "save" && !canSave)) return;
    setPending(action);
    onSave(action === "save" ? url || null : null).then(close, () =>
      setPending(null),
    );
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
            {/* A native form (Astryx has none), so that Enter submits only once an input
                method editor has committed its text. */}
            <form
              onSubmit={(ev) => {
                ev.preventDefault();
                save("save");
              }}
            >
              <FormLayout>
                <TextInput
                  label="Remote URL"
                  labelTooltip="Collections are pushed here after changes. Use an empty repository."
                  placeholder="git@github.com:you/stereo-collections.git"
                  value={value}
                  onChange={setValue}
                />
              </FormLayout>
            </form>
          </LayoutContent>
        }
        footer={
          <LayoutFooter>
            <HStack gap={2} hAlign="end">
              {currentRemote && (
                <Button
                  label="Remove"
                  variant="destructive"
                  isLoading={pending === "remove"}
                  isDisabled={pending === "save"}
                  onClick={() => save("remove")}
                />
              )}
              <Button label="Cancel" variant="secondary" onClick={close} />
              <Button
                label="Save"
                variant="primary"
                isLoading={pending === "save"}
                isDisabled={!canSave || pending === "remove"}
                onClick={() => save("save")}
              />
            </HStack>
          </LayoutFooter>
        }
      />
    </Dialog>
  );
}

export default GitRemoteDialog;
