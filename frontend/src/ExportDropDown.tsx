import { DropdownMenu } from "@astryxdesign/core/DropdownMenu";
import { Share2Icon } from "@radix-ui/react-icons";
import { collectionLabel } from "./CollectionSelector.tsx";
import type { ICollection } from "./Types.tsx";

type ExportDropDownProps = {
  collections: ICollection[];
  selectionCount: number;
  exportToCollection: (coll: string) => void;
  exportSelectionToYTM: () => void;
  exportSelectionToYTSavedPlaylist: () => void;
  exportTop50ToYTM: () => void;
  exportAllToYTSavedPlaylist: () => void;
  exportSelectionToSpotifyPlaylist: () => void;
  exportAllToSpotifyPlaylist: () => void;
};

function ExportDropDown({
  collections,
  selectionCount,
  exportToCollection,
  exportSelectionToYTM,
  exportSelectionToYTSavedPlaylist,
  exportTop50ToYTM,
  exportAllToYTSavedPlaylist,
  exportSelectionToSpotifyPlaylist,
  exportAllToSpotifyPlaylist,
}: ExportDropDownProps) {
  const hasSelection = selectionCount > 0;

  return (
    <DropdownMenu
      hasChevron={false}
      button={{
        label: "Export tracks",
        icon: <Share2Icon />,
        isIconOnly: true,
        variant: "secondary",
        tooltip: "Export tracks",
      }}
      items={[
        {
          type: "section",
          title: "Export selection to collection",
          items: collections.map((c) => ({
            label: collectionLabel(c.path),
            onClick: () => exportToCollection(c.path),
            isDisabled: !hasSelection,
          })),
        },
        { type: "divider" },
        {
          label: "Export selection to YouTube Music playlist (50 tracks max)",
          onClick: exportSelectionToYTM,
          isDisabled: !hasSelection,
        },
        {
          label: "Export selection to YouTube playlist (login required)",
          onClick: exportSelectionToYTSavedPlaylist,
          isDisabled: !hasSelection,
        },
        {
          label: "Create Spotify playlist from selection",
          onClick: exportSelectionToSpotifyPlaylist,
          isDisabled: !hasSelection,
        },
        { type: "divider" },
        {
          label:
            "Export first 50 tracks to temporary YouTube Music playlist (no login required)",
          onClick: exportTop50ToYTM,
        },
        {
          label:
            "Export current view (filtered and sorted) to YouTube playlist (login required)",
          onClick: exportAllToYTSavedPlaylist,
        },
        {
          label:
            "Create Spotify playlist from current view (filtered and sorted)",
          onClick: exportAllToSpotifyPlaylist,
        },
      ]}
    />
  );
}

export default ExportDropDown;
