import {
  Text,
  Code,
  Button,
  Dialog,
  IconButton,
  Tooltip,
  Checkbox,
  TextField,
  Flex,
} from "@radix-ui/themes";

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
  return (
    <Dialog.Root>
      <Tooltip content="Import tracks from another collection">
        <Dialog.Trigger>
          <IconButton variant="soft">
            <DownloadIcon />
          </IconButton>
        </Dialog.Trigger>
      </Tooltip>

      <Dialog.Content maxWidth="450px">
        <Dialog.Title>Import</Dialog.Title>
        <Dialog.Description size="2" mb="4">
          Import tracks from another collection. You can use an absolute file
          path like <Code>/path/to/collection.db</Code> or a URL like
          <Code>https://example.com/collection.db</Code>. In any case, the
          import source is validated before the import button is enabled.
        </Dialog.Description>

        <Flex direction="column" gap="3">
          <label>
            <Text as="div" size="2" mb="1" weight="bold">
              Import from
            </Text>
            <TextField.Root
              color={
                isValidImportFrom === undefined || !importFrom
                  ? "gray"
                  : isValidImportFrom
                    ? "green"
                    : "orange"
              }
              onChange={(ev) => setImportFrom(ev.target.value)}
              value={importFrom}
            />
          </label>

          <Text as="label" size="2">
            <Flex gap="2">
              <Checkbox
                checked={keepUserData}
                onCheckedChange={(ev) => setKeepUserData(Boolean(ev))}
              />
              Keep user data (ratings, play count, etc.)
            </Flex>
          </Text>
        </Flex>

        <Flex gap="3" mt="4" justify="end">
          <Dialog.Close>
            <Button variant="soft" color="gray">
              Cancel
            </Button>
          </Dialog.Close>
          <Dialog.Close>
            <Button
              color="green"
              onClick={doImport}
              disabled={!isValidImportFrom}
            >
              Import
            </Button>
          </Dialog.Close>
        </Flex>
      </Dialog.Content>
    </Dialog.Root>
  );
}

export default ImportDialog;
