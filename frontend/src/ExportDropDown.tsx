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
  exportSelectionToJson: () => void;
  exportAllToJson: () => void;
  isIconOnly?: boolean;
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
  exportSelectionToJson,
  exportAllToJson,
  isIconOnly = false,
}: ExportDropDownProps) {
  const hasSelection = selectionCount > 0;

  return (
    <DropdownMenu
      hasChevron={false}
      button={{
        label: "Export tracks",
        icon: <Share2Icon />,
        isIconOnly,
        variant: "ghost",
        tooltip: "Export tracks",
      }}
      items={[
        {
          type: "section",
          title: "Selection to collection",
          items: collections.map((c) => ({
            label: collectionLabel(c.path),
            onClick: () => exportToCollection(c.path),
            isDisabled: !hasSelection,
          })),
        },
        { type: "divider" },
        {
          type: "section",
          title: "Selection",
          items: [
            {
              label: "YouTube Music (temporary, max 50)",
              onClick: exportSelectionToYTM,
              isDisabled: !hasSelection,
            },
            {
              label: "YouTube playlist",
              onClick: exportSelectionToYTSavedPlaylist,
              isDisabled: !hasSelection,
            },
            {
              label: "Spotify playlist",
              onClick: exportSelectionToSpotifyPlaylist,
              isDisabled: !hasSelection,
            },
            {
              label: "JSON file",
              onClick: exportSelectionToJson,
              isDisabled: !hasSelection,
            },
          ],
        },
        { type: "divider" },
        {
          type: "section",
          title: "Current view",
          items: [
            {
              label: "YouTube Music (temporary, first 50)",
              onClick: exportTop50ToYTM,
            },
            {
              label: "YouTube playlist",
              onClick: exportAllToYTSavedPlaylist,
            },
            {
              label: "Spotify playlist",
              onClick: exportAllToSpotifyPlaylist,
            },
            {
              label: "JSON file",
              onClick: exportAllToJson,
            },
          ],
        },
      ]}
    />
  );
}

export default ExportDropDown;
