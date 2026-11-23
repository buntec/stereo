import { useRef, useState } from "react";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { Button } from "@astryxdesign/core/Button";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { FormLayout } from "@astryxdesign/core/FormLayout";
import { FileInput } from "@astryxdesign/core/FileInput";
import { HStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";

// The file's text travels in one WebSocket message, which the backend takes up to 64 MiB
// (`MAX_MESSAGE_SIZE` in server.rs).
const MAX_SIZE = 60 * 1024 * 1024;

type ImportJsonDialogProps = {
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  /** Imports the file's text. Resolves once the backend has added the tracks; the dialog keeps
   * the file if it rejects, showing why. */
  onImport: (data: string, keepUserData: boolean) => Promise<void>;
  isConnected: boolean;
};

/** Adds the tracks of a JSON file exported from Stereo (`ExportJsonDialog.tsx`) to the open
 * collection, skipping those already in it. */
function ImportJsonDialog({
  isOpen,
  onOpenChange,
  onImport,
  isConnected,
}: ImportJsonDialogProps) {
  const [file, setFile] = useState<File | null>(null);
  const [keepUserData, setKeepUserData] = useState(false);
  const [pending, setPending] = useState<"reading" | "importing" | null>(null);
  const [error, setError] = useState<string>();
  // Bumped when the dialog closes or the selection changes, so that a read finishing after
  // that doesn't import.
  const attempt = useRef(0);

  const reset = () => {
    attempt.current++;
    onOpenChange(false);
    setFile(null);
    setPending(null);
    setError(undefined);
  };

  const close = () => {
    // Once sent, the import can't be called off, so the dialog waits for its outcome.
    if (pending !== "importing") reset();
  };

  const canImport = !!file && !pending && isConnected;

  const doImport = async () => {
    if (!file || !canImport) return;
    const current = ++attempt.current;
    setPending("reading");
    setError(undefined);
    let data: string;
    try {
      data = await file.text();
    } catch (e) {
      if (attempt.current === current) {
        setError(`Could not read ${file.name}: ${String(e)}`);
        setPending(null);
      }
      return;
    }
    if (attempt.current !== current) return;
    setPending("importing");
    try {
      await onImport(data, keepUserData);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setPending(null);
      return;
    }
    reset();
  };

  return (
    <Dialog
      isOpen={isOpen}
      onOpenChange={(open) => (open ? onOpenChange(true) : close())}
      purpose="form"
      width={420}
    >
      <Layout
        header={<DialogHeader title="Import from JSON" onOpenChange={close} />}
        content={
          <LayoutContent>
            <FormLayout>
              <Text as="div" type="body">
                Adds the tracks of a JSON file exported from Stereo to this
                collection. Tracks already in it are left as they are.
              </Text>
              <FileInput
                label="JSON file"
                mode="dropzone"
                accept=".json,application/json"
                maxSize={MAX_SIZE}
                value={file}
                isDisabled={!!pending}
                isLoading={!!pending}
                status={error ? { type: "error", message: error } : undefined}
                onChange={(value) => {
                  const next = Array.isArray(value) ? value[0] : value;
                  attempt.current++;
                  setPending(null);
                  setError(undefined);
                  setFile(next ?? null);
                }}
              />
              <CheckboxInput
                label="Keep ratings and play history"
                value={keepUserData}
                isDisabled={!!pending}
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
                isDisabled={pending === "importing"}
                onClick={close}
              />
              <Button
                label="Import"
                variant="primary"
                isLoading={!!pending}
                isDisabled={!canImport && !pending}
                tooltip={isConnected ? undefined : "Waiting for the connection"}
                onClick={() => void doImport()}
              />
            </HStack>
          </LayoutFooter>
        }
      />
    </Dialog>
  );
}

export default ImportJsonDialog;
