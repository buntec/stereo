import { useState } from "react";
import { DropdownMenu } from "@astryxdesign/core/DropdownMenu";
import { DownloadIcon } from "@radix-ui/react-icons";
import ImportDialog from "./ImportDialog.tsx";
import YouTubePlaylistImportDialog from "./YouTubePlaylistImportDialog.tsx";
import type { ITrack, RequestReply } from "./Types.tsx";

type ImportMenuProps = {
  setImportFrom: (path: string) => void;
  importFrom: string;
  setKeepUserData: (keep: boolean) => void;
  keepUserData: boolean;
  doImport: () => void;
  isValidImportFrom?: boolean;
  requestReply: RequestReply;
  onYouTubeImport: (name: string, tracks: ITrack[]) => void;
};

export default function ImportMenu({
  setImportFrom,
  importFrom,
  setKeepUserData,
  keepUserData,
  doImport,
  isValidImportFrom,
  requestReply,
  onYouTubeImport,
}: ImportMenuProps) {
  const [collectionOpen, setCollectionOpen] = useState(false);
  const [youtubeOpen, setYoutubeOpen] = useState(false);

  return (
    <>
      <DropdownMenu
        hasChevron={false}
        button={{
          label: "Import tracks",
          icon: <DownloadIcon />,
          isIconOnly: true,
          variant: "secondary",
          tooltip: "Import tracks",
        }}
        items={[
          {
            label: "Import collection/database",
            onClick: () => setCollectionOpen(true),
          },
          {
            label: "Import YouTube playlist",
            onClick: () => setYoutubeOpen(true),
          },
        ]}
      />

      <ImportDialog
        showTrigger={false}
        open={collectionOpen}
        onOpenChange={setCollectionOpen}
        setImportFrom={setImportFrom}
        importFrom={importFrom}
        setKeepUserData={setKeepUserData}
        keepUserData={keepUserData}
        doImport={doImport}
        isValidImportFrom={isValidImportFrom}
      />
      <YouTubePlaylistImportDialog
        showTrigger={false}
        open={youtubeOpen}
        onOpenChange={setYoutubeOpen}
        requestReply={requestReply}
        onImport={onYouTubeImport}
      />
    </>
  );
}
