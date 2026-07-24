import { useState } from "react";
import { Selector } from "@astryxdesign/core/Selector";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { Layout, LayoutContent, LayoutFooter } from "@astryxdesign/core/Layout";
import { Button } from "@astryxdesign/core/Button";
import { TextInput } from "@astryxdesign/core/TextInput";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { PlusCircledIcon } from "@radix-ui/react-icons";
import type { IRepo } from "./Types.tsx";

function repoLabel(repo: IRepo): string {
  return repo.remote ?? repo.path;
}

type RepoSelectorProps = {
  repos: IRepo[];
  value: string;
  setValue: (path: string) => void;
  onClone: (url: string) => void;
};

function RepoSelector({ repos, value, setValue, onClone }: RepoSelectorProps) {
  const [dialogOpen, setDialogOpen] = useState(false);
  const [url, setUrl] = useState("");

  const selectedValue = repos.some((r) => r.path === value) ? value : undefined;

  const closeDialog = () => {
    setDialogOpen(false);
    setUrl("");
  };

  return (
    <HStack vAlign="center" gap={2} padding={2}>
      <Selector
        label="Repository"
        isLabelHidden
        placeholder={
          repos.length === 0 ? "No repos — clone one →" : "Select repo…"
        }
        options={repos.map((r) => ({ value: r.path, label: repoLabel(r) }))}
        value={selectedValue}
        onChange={setValue}
      />

      <IconButton
        variant="secondary"
        label="Clone a git repository"
        tooltip="Clone a git repository"
        icon={<PlusCircledIcon />}
        onClick={() => setDialogOpen(true)}
      />

      <Dialog
        isOpen={dialogOpen}
        onOpenChange={(open) => (open ? setDialogOpen(true) : closeDialog())}
        purpose="form"
        width={440}
      >
        <Layout
          header={
            <DialogHeader
              title="Clone a repository"
              onOpenChange={() => closeDialog()}
            />
          }
          content={
            <LayoutContent>
              <VStack gap={3}>
                <Text as="div" type="body">
                  Enter a git URL. The repository will be cloned into the stereo
                  home directory and its collections will appear in the
                  dropdown.
                </Text>
                <TextInput
                  label="Git URL"
                  value={url}
                  placeholder="https://github.com/user/my-sets.git"
                  onChange={(v) => setUrl(v.trim())}
                  onKeyDown={(ev) => {
                    if (ev.key === "Enter" && url) {
                      onClone(url);
                      closeDialog();
                    }
                  }}
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
                  label="Clone"
                  variant="primary"
                  isDisabled={!url}
                  onClick={() => {
                    onClone(url);
                    closeDialog();
                  }}
                />
              </HStack>
            </LayoutFooter>
          }
        />
      </Dialog>
    </HStack>
  );
}

export default RepoSelector;
