import { useState } from "react";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { Button } from "@astryxdesign/core/Button";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { FormLayout } from "@astryxdesign/core/FormLayout";
import { TextInput } from "@astryxdesign/core/TextInput";
import { HStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { Code } from "@astryxdesign/core/Code";
import type { ITrack } from "./Types.tsx";

export type JsonExport = { tracks: ITrack[]; name: string };

type ExportJsonDialogProps = {
  /** The tracks to export and the suggested file name; the dialog is open while set. */
  pending: JsonExport | null;
  onClose: () => void;
  onExport: (name: string, tracks: ITrack[], includeUserData: boolean) => void;
};

/** Exports tracks to a JSON file that someone else can import into their collection
 * (`tracks_file.rs`). Keyed by the export in App.tsx, so each starts from its name. */
function ExportJsonDialog({
  pending,
  onClose,
  onExport,
}: ExportJsonDialogProps) {
  const [name, setName] = useState(pending?.name ?? "");
  const [includeUserData, setIncludeUserData] = useState(false);
  const count = pending?.tracks.length ?? 0;
  const canExport = !!pending && !!name.trim();

  const doExport = () => {
    if (!pending || !canExport) return;
    onExport(name.trim(), pending.tracks, includeUserData);
    onClose();
  };

  return (
    <Dialog
      isOpen={pending !== null}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      purpose="form"
      width={420}
    >
      <Layout
        header={<DialogHeader title="Export to JSON" onOpenChange={onClose} />}
        content={
          <LayoutContent>
            <FormLayout>
              <Text as="div" type="body">
                Saves {count} {count === 1 ? "track" : "tracks"} as{" "}
                <Code>{name.trim() || "name"}.json</Code>, which can be imported
                into any collection.
              </Text>
              {/* A native form (Astryx has none), so that Enter submits only once an input
                  method editor has committed its text. */}
              <form
                onSubmit={(ev) => {
                  ev.preventDefault();
                  doExport();
                }}
              >
                <TextInput
                  label="File name"
                  value={name}
                  placeholder="tracks"
                  onChange={(v) => setName(v.replace(/[/\\]/g, ""))}
                />
              </form>
              <CheckboxInput
                label="Include ratings and play history"
                value={includeUserData}
                onChange={setIncludeUserData}
              />
            </FormLayout>
          </LayoutContent>
        }
        footer={
          <LayoutFooter>
            <HStack gap={2} hAlign="end">
              <Button label="Cancel" variant="secondary" onClick={onClose} />
              <Button
                label="Export"
                variant="primary"
                isDisabled={!canExport}
                onClick={doExport}
              />
            </HStack>
          </LayoutFooter>
        }
      />
    </Dialog>
  );
}

export default ExportJsonDialog;
