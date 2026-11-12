import { useState } from "react";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { IconButton } from "@astryxdesign/core/IconButton";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { FormLayout } from "@astryxdesign/core/FormLayout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { HStack } from "@astryxdesign/core/Stack";
import { Selector } from "@astryxdesign/core/Selector";

import { DownloadIcon } from "@radix-ui/react-icons";
import { collectionLabel } from "./CollectionSelector.tsx";
import type { ICollection } from "./Types.tsx";

/** Where tracks are imported from: a collection in the current repo, or any file path/URL. */
export type ImportSource = "repo" | "path";

type ImportDialogProps = {
  setImportFrom: (path: string) => void;
  importFrom: string;
  setKeepUserData: (keep: boolean) => void;
  keepUserData: boolean;
  doImport: () => void;
  isValidImportFrom?: boolean;
  source?: ImportSource;
  /** Other collections in the current repo (the open one excluded); used when `source` is "repo". */
  repoCollections?: ICollection[];
  showTrigger?: boolean;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
};

function ImportDialog({
  setImportFrom,
  importFrom,
  setKeepUserData,
  keepUserData,
  doImport,
  isValidImportFrom,
  source = "path",
  repoCollections = [],
  showTrigger = true,
  open,
  onOpenChange,
}: ImportDialogProps) {
  const [internalOpen, setInternalOpen] = useState(false);
  const dialogOpen = open ?? internalOpen;
  const setDialogOpen = onOpenChange ?? setInternalOpen;
  const closeDialog = () => setDialogOpen(false);

  const fromRepo = source === "repo";
  // Repo collections are known to exist, so only a failed validation blocks the import
  // (the backend validates again when importing).
  const canImport = fromRepo
    ? repoCollections.some((c) => c.path === importFrom) &&
      isValidImportFrom !== false
    : !!isValidImportFrom;

  const status =
    isValidImportFrom === undefined || !importFrom
      ? undefined
      : isValidImportFrom
        ? ({ type: "success" } as const)
        : ({
            type: "error",
            message: fromRepo
              ? "Could not read this collection"
              : "No collection found here",
          } as const);

  return (
    <>
      {showTrigger && (
        <IconButton
          variant="secondary"
          label="Import tracks"
          tooltip="Import tracks"
          icon={<DownloadIcon />}
          onClick={() => setDialogOpen(true)}
        />
      )}

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
              <FormLayout>
                {fromRepo ? (
                  <Selector
                    label="Import from"
                    placeholder={
                      repoCollections.length > 0
                        ? "Choose a collection"
                        : "No other collections in this repository"
                    }
                    isDisabled={repoCollections.length === 0}
                    options={repoCollections.map((c) => ({
                      value: c.path,
                      label: collectionLabel(c.path),
                    }))}
                    value={
                      repoCollections.some((c) => c.path === importFrom)
                        ? importFrom
                        : undefined
                    }
                    onChange={setImportFrom}
                    status={isValidImportFrom === false ? status : undefined}
                  />
                ) : (
                  <TextInput
                    label="File path or URL"
                    placeholder="/path/to/collection.db"
                    value={importFrom}
                    onChange={setImportFrom}
                    status={status}
                  />
                )}

                <CheckboxInput
                  label="Keep ratings and play history"
                  value={keepUserData}
                  onChange={setKeepUserData}
                />
              </FormLayout>
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
                  isDisabled={!canImport}
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
