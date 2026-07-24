import { useState } from "react";
import { Text } from "@astryxdesign/core/Text";
import { Code } from "@astryxdesign/core/Code";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { IconButton } from "@astryxdesign/core/IconButton";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { TextInput } from "@astryxdesign/core/TextInput";
import { HStack, VStack } from "@astryxdesign/core/Stack";

import { DownloadIcon } from "@radix-ui/react-icons";

type ImportDialogProps = {
  setImportFrom: (path: string) => void;
  importFrom: string;
  setKeepUserData: (keep: boolean) => void;
  keepUserData: boolean;
  doImport: () => void;
  isValidImportFrom?: boolean;
};

function ImportDialog({
  setImportFrom,
  importFrom,
  setKeepUserData,
  keepUserData,
  doImport,
  isValidImportFrom,
}: ImportDialogProps) {
  const [dialogOpen, setDialogOpen] = useState(false);
  const closeDialog = () => setDialogOpen(false);

  const status =
    isValidImportFrom === undefined || !importFrom
      ? undefined
      : isValidImportFrom
        ? ({ type: "success" } as const)
        : ({ type: "warning" } as const);

  return (
    <>
      <IconButton
        variant="secondary"
        label="Import tracks from another collection"
        tooltip="Import tracks from another collection"
        icon={<DownloadIcon />}
        onClick={() => setDialogOpen(true)}
      />

      <Dialog
        isOpen={dialogOpen}
        onOpenChange={setDialogOpen}
        purpose="form"
        width={450}
      >
        <Layout
          header={<DialogHeader title="Import" onOpenChange={closeDialog} />}
          content={
            <LayoutContent>
              <VStack gap={3}>
                <Text as="div" type="body">
                  Import tracks from another collection. You can use an absolute
                  file path like <Code>/path/to/collection.db</Code> or a URL
                  like <Code>https://example.com/collection.db</Code>. In any
                  case, the import source is validated before the import button
                  is enabled.
                </Text>

                <TextInput
                  label="Import from"
                  value={importFrom}
                  onChange={setImportFrom}
                  status={status}
                />

                <CheckboxInput
                  label="Keep user data (ratings, play count, etc.)"
                  value={keepUserData}
                  onChange={setKeepUserData}
                />
              </VStack>
            </LayoutContent>
          }
          footer={
            <LayoutFooter>
              <HStack gap={2} hAlign="end">
                <Button
                  label="Cancel"
                  variant="secondary"
                  onClick={closeDialog}
                />
                <Button
                  label="Import"
                  variant="primary"
                  isDisabled={!isValidImportFrom}
                  onClick={() => {
                    doImport();
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

export default ImportDialog;
