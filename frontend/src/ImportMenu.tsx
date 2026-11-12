import { useState } from "react";
import { DropdownMenu } from "@astryxdesign/core/DropdownMenu";
import { DownloadIcon } from "@radix-ui/react-icons";
import ImportDialog from "./ImportDialog.tsx";
import YouTubePlaylistImportDialog from "./YouTubePlaylistImportDialog.tsx";
import type { ImportSource } from "./ImportDialog.tsx";
import type { ICollection, ITrack, RequestReply } from "./Types.tsx";

type ImportMenuProps = {
  setImportFrom: (path: string) => void;
  importFrom: string;
  setKeepUserData: (keep: boolean) => void;
  keepUserData: boolean;
  doImport: () => void;
  isValidImportFrom?: boolean;
  /** Other collections in the current repo (the open one excluded). */
  repoCollections: ICollection[];
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
  repoCollections,
  requestReply,
  onYouTubeImport,
}: ImportMenuProps) {
  const [collectionOpen, setCollectionOpen] = useState(false);
  const [source, setSource] = useState<ImportSource>("path");
  const [youtubeOpen, setYoutubeOpen] = useState(false);

  const openCollectionImport = (src: ImportSource) => {
    // Start from a clean source so a path typed in one mode doesn't leak into the other.
    if (src !== source) setImportFrom("");
    setSource(src);
    setCollectionOpen(true);
  };

  return (
    <>
      <DropdownMenu
        hasChevron={false}
        button={{
          label: "Import tracks",
          icon: <DownloadIcon />,
          isIconOnly: false,
          variant: "ghost",
          tooltip: "Import tracks",
        }}
        items={[
          {
            label: "Collection in this repo",
            onClick: () => openCollectionImport("repo"),
            isDisabled: repoCollections.length === 0,
          },
          {
            label: "Collection file or URL",
            onClick: () => openCollectionImport("path"),
          },
          {
            label: "YouTube playlist",
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
        source={source}
        repoCollections={repoCollections}
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
