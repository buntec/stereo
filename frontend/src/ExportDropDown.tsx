import { Tooltip, DropdownMenu, IconButton, Flex } from "@radix-ui/themes";
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
    <DropdownMenu.Root>
      <Tooltip content="Export tracks">
        <DropdownMenu.Trigger>
          <IconButton variant="soft">
            <Share2Icon />
          </IconButton>
        </DropdownMenu.Trigger>
      </Tooltip>
      <DropdownMenu.Content>
        <DropdownMenu.Sub>
          <DropdownMenu.SubTrigger disabled={!hasSelection}>
            Export selection to collection
          </DropdownMenu.SubTrigger>
          <DropdownMenu.SubContent>
            {collections.map((c) => (
              <DropdownMenu.Item
                key={c.path}
                onSelect={() => exportToCollection(c.path)}
              >
                <Flex justify="between" align="center" gapX="4" width="100%">
                  {collectionLabel(c.path)}
                </Flex>
              </DropdownMenu.Item>
            ))}
          </DropdownMenu.SubContent>
        </DropdownMenu.Sub>
        <DropdownMenu.Item
          disabled={!hasSelection}
          onSelect={exportSelectionToYTM}
        >
          Export selection to YouTube Music playlist (50 tracks max)
        </DropdownMenu.Item>
        <DropdownMenu.Item
          disabled={!hasSelection}
          onSelect={exportSelectionToYTSavedPlaylist}
        >
          Export selection to YouTube playlist (login required)
        </DropdownMenu.Item>
        <DropdownMenu.Item
          disabled={!hasSelection}
          onSelect={exportSelectionToSpotifyPlaylist}
        >
          Create Spotify playlist from selection
        </DropdownMenu.Item>
        <DropdownMenu.Separator />
        <DropdownMenu.Item onSelect={exportTop50ToYTM}>
          Export first 50 tracks to temporary YouTube Music playlist (no login
          required)
        </DropdownMenu.Item>
        <DropdownMenu.Item onSelect={exportAllToYTSavedPlaylist}>
          Export current view (filtered and sorted) to YouTube playlist (login
          required)
        </DropdownMenu.Item>
        <DropdownMenu.Item onSelect={exportAllToSpotifyPlaylist}>
          Create Spotify playlist from current view (filtered and sorted)
        </DropdownMenu.Item>
      </DropdownMenu.Content>
    </DropdownMenu.Root>
  );
}

export default ExportDropDown;
