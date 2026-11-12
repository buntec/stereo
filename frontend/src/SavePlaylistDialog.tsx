import { useState } from "react";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { Button } from "@astryxdesign/core/Button";
import { FormLayout } from "@astryxdesign/core/FormLayout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { HStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";

type SavePlaylistDialogProps = {
  isOpen: boolean;
  onClose: () => void;
  /** The collection the view shows, for the explanation. */
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
        header={<DialogHeader title="Save playlist" onOpenChange={onClose} />}
        content={
          <LayoutContent>
            <FormLayout>
              <Text as="div" type="body">
                Saves {collectionName} with its current column filters and
                sorting. The playlist follows the collection: tracks you add,
                rate or edit later show up in it whenever they match.
              </Text>
              <TextInput
                label="Playlist name"
                value={name}
                hasAutoFocus
                onChange={setName}
                status={
                  isTaken
                    ? {
                        type: "error",
                        message: "A playlist with this name already exists",
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
