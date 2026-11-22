import { useState } from "react";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { Button } from "@astryxdesign/core/Button";
import { FormLayout } from "@astryxdesign/core/FormLayout";
import { FileInput } from "@astryxdesign/core/FileInput";
import { TextInput } from "@astryxdesign/core/TextInput";
import { HStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { Code } from "@astryxdesign/core/Code";

// The file travels base64-encoded in one WebSocket message, which the backend takes up to
// 64 MiB (`MAX_MESSAGE_SIZE` in server.rs).
const MAX_SIZE = 45 * 1024 * 1024;

type ImportCollectionDialogProps = {
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  // Takes the collection's name and the file's contents, base64-encoded.
  onImport: (name: string, data: string) => void;
};

function readBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const url = reader.result as string;
      resolve(url.slice(url.indexOf(",") + 1));
    };
    reader.onerror = () => reject(reader.error ?? new Error("Read failed"));
    reader.readAsDataURL(file);
  });
}

function fileLabel(file: File): string {
  return file.name.replace(/\.(db|sqlite3?)$/i, "").replace(/[/\\]/g, "");
}

function ImportCollectionDialog({
  isOpen,
  onOpenChange,
  onImport,
}: ImportCollectionDialogProps) {
  const [file, setFile] = useState<File | null>(null);
  const [name, setName] = useState("");
  const [reading, setReading] = useState(false);
  const [error, setError] = useState<string>();

  const close = () => {
    onOpenChange(false);
    setFile(null);
    setName("");
    setError(undefined);
  };

  const canImport = !!file && !!name.trim() && !reading;

  const doImport = async () => {
    if (!file || !name.trim()) return;
    setReading(true);
    try {
      onImport(name.trim(), await readBase64(file));
      close();
    } catch (e) {
      setError(`Could not read ${file.name}: ${String(e)}`);
    } finally {
      setReading(false);
    }
  };

  return (
    <Dialog
      isOpen={isOpen}
      onOpenChange={(open) => (open ? onOpenChange(true) : close())}
      purpose="form"
      width={420}
    >
      <Layout
        header={<DialogHeader title="Import Collection" onOpenChange={close} />}
        content={
          <LayoutContent>
            <FormLayout>
              <Text as="div" type="body">
                Adds a collection file to the current repo as{" "}
                <Code>{name || "name"}.db</Code>, with its ratings and play
                counts.
              </Text>
              <FileInput
                label="Collection file"
                mode="dropzone"
                accept=".db,.sqlite,.sqlite3"
                maxSize={MAX_SIZE}
                value={file}
                isLoading={reading}
                status={error ? { type: "error", message: error } : undefined}
                onChange={(value) => {
                  const next = Array.isArray(value) ? value[0] : value;
                  setError(undefined);
                  // Follows the file's name unless one was typed.
                  if (!name || (file && name === fileLabel(file)))
                    setName(next ? fileLabel(next) : "");
                  setFile(next ?? null);
                }}
              />
              <TextInput
                label="Collection name"
                value={name}
                placeholder="my-sets"
                onChange={(v) => setName(v.replace(/[/\\]/g, ""))}
                onKeyDown={(ev) => {
                  if (ev.key === "Enter" && canImport) void doImport();
                }}
              />
            </FormLayout>
          </LayoutContent>
        }
        footer={
          <LayoutFooter>
            <HStack gap={2} hAlign="end">
              <Button label="Cancel" variant="secondary" onClick={close} />
              <Button
                label="Import"
                variant="primary"
                isDisabled={!canImport}
                onClick={() => void doImport()}
              />
            </HStack>
          </LayoutFooter>
        }
      />
    </Dialog>
  );
}

export default ImportCollectionDialog;
