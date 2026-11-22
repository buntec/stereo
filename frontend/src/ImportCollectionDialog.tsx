import { useRef, useState } from "react";
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
  // Takes the collection's name and the file's contents, base64-encoded. Resolves once the
  // backend has imported it; the dialog keeps the file and name if it rejects, showing why.
  onImport: (name: string, data: string) => Promise<void>;
  // Import waits for the backend's connection, as a message queued meanwhile could reach a
  // new session before it has the repo.
  isConnected: boolean;
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
  isConnected,
}: ImportCollectionDialogProps) {
  const [file, setFile] = useState<File | null>(null);
  const [name, setName] = useState("");
  // Reading the file, then waiting for the backend.
  const [pending, setPending] = useState<"reading" | "importing" | null>(null);
  const [error, setError] = useState<string>();
  // Bumped when the dialog closes or the selection changes, so that a read finishing after
  // that doesn't import.
  const attempt = useRef(0);

  const close = () => {
    // Once sent, the import can't be called off, so the dialog waits for its outcome.
    if (pending === "importing") return;
    attempt.current++;
    onOpenChange(false);
    setFile(null);
    setName("");
    setPending(null);
    setError(undefined);
  };

  const canImport = !!file && !!name.trim() && !pending && isConnected;

  const doImport = async () => {
    if (!file || !canImport) return;
    const current = ++attempt.current;
    setPending("reading");
    setError(undefined);
    let data: string;
    try {
      data = await readBase64(file);
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
      await onImport(name.trim(), data);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setPending(null);
      return;
    }
    attempt.current++;
    onOpenChange(false);
    setFile(null);
    setName("");
    setPending(null);
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
                isDisabled={!!pending}
                isLoading={!!pending}
                status={error ? { type: "error", message: error } : undefined}
                onChange={(value) => {
                  const next = Array.isArray(value) ? value[0] : value;
                  attempt.current++;
                  setPending(null);
                  setError(undefined);
                  // Follows the file's name unless one was typed.
                  if (!name || (file && name === fileLabel(file)))
                    setName(next ? fileLabel(next) : "");
                  setFile(next ?? null);
                }}
              />
              {/* A native form (Astryx has none), so that Enter submits only once an input
                  method editor has committed its text. */}
              <form
                onSubmit={(ev) => {
                  ev.preventDefault();
                  void doImport();
                }}
              >
                <TextInput
                  label="Collection name"
                  value={name}
                  placeholder="my-sets"
                  isDisabled={!!pending}
                  onChange={(v) => setName(v.replace(/[/\\]/g, ""))}
                />
              </form>
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

export default ImportCollectionDialog;
