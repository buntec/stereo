import { useState } from "react";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { Button } from "@astryxdesign/core/Button";
import { FormLayout } from "@astryxdesign/core/FormLayout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { HStack } from "@astryxdesign/core/Stack";

type SavePlaylistDialogProps = {
  isOpen: boolean;
  onClose: () => void;
  /** The collection the view shows, for the subtitle. */
  collectionName: string;
  /** Names already taken, which would be confusing to reuse. */
  existingNames: string[];
  onSave: (name: string) => void;
};

/** Asks for the name of a new playlist made from the current view. */
function SavePlaylistDialog({
  isOpen,
  onClose,
  collectionName,
  existingNames,
  onSave,
}: SavePlaylistDialogProps) {
  const [name, setName] = useState("");
  const [wasOpen, setWasOpen] = useState(false);
  if (isOpen !== wasOpen) {
    setWasOpen(isOpen);
    if (isOpen) setName("");
  }

  const trimmed = name.trim();
  const isTaken = existingNames.includes(trimmed);
  const canSave = trimmed !== "" && !isTaken;
  const save = () => {
    if (!canSave) return;
    onSave(trimmed);
    onClose();
  };

  return (
    <Dialog
      isOpen={isOpen}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      purpose="form"
      width={420}
    >
      <Layout
        header={
          <DialogHeader
            title="Save playlist"
            subtitle={`${collectionName} with its current filters and sorting`}
            onOpenChange={onClose}
          />
        }
        content={
          <LayoutContent>
            <FormLayout>
              <TextInput
                label="Playlist name"
                labelTooltip="Tracks you add, rate or edit later show up whenever they match."
                value={name}
                hasAutoFocus
                onChange={setName}
                status={
                  isTaken
                    ? {
                        type: "error",
                        message: "Name already taken",
                      }
                    : undefined
                }
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
              <Button label="Cancel" variant="secondary" onClick={onClose} />
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

export default SavePlaylistDialog;
