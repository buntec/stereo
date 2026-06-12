import { Tooltip, DropdownMenu, IconButton, Flex } from "@radix-ui/themes";
import { Share2Icon } from "@radix-ui/react-icons";
import { collectionLabel } from "./CollectionSelector.tsx";
import type { ICollection } from "./Types.tsx";

type ExportDropDownProps = {
  collections: ICollection[];
  exportToCollection: (coll: string) => void;
  exportToYTM: () => void;
  exportToYTSavedPlaylist: () => void;
  disabled: boolean;
};

function ExportDropDown({
  collections,
  exportToCollection,
  exportToYTM,
  exportToYTSavedPlaylist,
  disabled,
}: ExportDropDownProps) {
  return (
    <DropdownMenu.Root>
      <Tooltip content="Export selected tracks">
        <DropdownMenu.Trigger disabled={disabled}>
          <IconButton variant="soft">
            <Share2Icon />
          </IconButton>
        </DropdownMenu.Trigger>
      </Tooltip>
      <DropdownMenu.Content>
        <DropdownMenu.Sub>
          <DropdownMenu.SubTrigger>
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
        <DropdownMenu.Separator />
        <DropdownMenu.Item onSelect={exportToYTM}>
          Export selection to YouTube Music playlist (50 tracks max)
        </DropdownMenu.Item>
        <DropdownMenu.Item onSelect={exportToYTSavedPlaylist}>
          Export selection to YouTube playlist
        </DropdownMenu.Item>
      </DropdownMenu.Content>
    </DropdownMenu.Root>
  );
}

export default ExportDropDown;
