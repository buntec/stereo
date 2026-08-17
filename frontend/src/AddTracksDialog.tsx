import { useState } from "react";
import { Text } from "@astryxdesign/core/Text";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { List, ListItem } from "@astryxdesign/core/List";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import type { ITrack } from "./Types";

type AddTracksDialogProps = {
  open: boolean;
  conflicts: ITrack[];
  onOpenChange: (open: boolean) => void;
  onConfirm: (overwriteYtIds: Set<string>) => void;
};

function AddTracksDialog({
  open,
  conflicts,
  onOpenChange,
  onConfirm,
}: AddTracksDialogProps) {
  const [overwrite, setOverwrite] = useState<Set<string>>(
    () => new Set(conflicts.map((t) => t.yt_id)),
  );

  const toggle = (yt_id: string, checked: boolean) => {
    setOverwrite((prev) => {
      const next = new Set(prev);
      if (checked) {
        next.add(yt_id);
      } else {
        next.delete(yt_id);
      }
      return next;
    });
  };

  return (
    <Dialog isOpen={open} onOpenChange={onOpenChange} purpose="form" width={480}>
      <Layout
        header={
          <DialogHeader
            title="Some tracks already exist"
            subtitle={`${conflicts.length} already in the collection. Choose which to overwrite.`}
            onOpenChange={() => onOpenChange(false)}
          />
        }
        content={
          <LayoutContent>
            <VStack gap={3}>
              <HStack gap={2}>
                <Button
                  label="Select all"
                  variant="ghost"
                  size="sm"
                  onClick={() =>
                    setOverwrite(new Set(conflicts.map((t) => t.yt_id)))
                  }
                />
                <Button
                  label="Select none"
                  variant="ghost"
                  size="sm"
                  onClick={() => setOverwrite(new Set())}
                />
              </HStack>

              <List hasDividers density="compact">
                {conflicts.map((track) => (
                  <ListItem
                    key={track.yt_id}
                    label={track.title}
                    description={track.artists.join(", ")}
                    endContent={
                      <CheckboxInput
                        label="Overwrite"
                        isLabelHidden
                        value={overwrite.has(track.yt_id)}
                        onChange={(checked) => toggle(track.yt_id, checked)}
                      />
                    }
                  />
                ))}
              </List>

              <Text type="supporting">
                Unchecked tracks are left as-is. New tracks are always added.
              </Text>
            </VStack>
          </LayoutContent>
        }
        footer={
          <LayoutFooter>
            <HStack gap={2} hAlign="end">
              <Button
                label="Cancel"
                variant="secondary"
                onClick={() => onOpenChange(false)}
              />
              <Button
                label="Add tracks"
                variant="primary"
                onClick={() => {
                  onConfirm(overwrite);
                  onOpenChange(false);
                }}
              />
            </HStack>
          </LayoutFooter>
        }
      />
    </Dialog>
  );
}

export default AddTracksDialog;
