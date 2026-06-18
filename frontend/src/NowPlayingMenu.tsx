import { DropdownMenu, IconButton, Tooltip } from "@radix-ui/themes";
import { DotsHorizontalIcon } from "@radix-ui/react-icons";
import { collectionLabel } from "./CollectionSelector.tsx";
import type { ICollection, ITrack } from "./Types.tsx";

type NowPlayingMenuProps = {
  track: ITrack;
  collections: ICollection[];
  currentCollectionPath: string;
  onCopyToCollection: (collectionPath: string) => void;
  onMoveToCollection: (collectionPath: string) => void;
  onDeleteFromCollection: () => void;
};

function NowPlayingMenu({
  collections,
  currentCollectionPath,
  onCopyToCollection,
  onMoveToCollection,
  onDeleteFromCollection,
}: NowPlayingMenuProps) {
  const otherCollections = collections.filter(
    (c) => c.path !== currentCollectionPath,
  );

  return (
    <DropdownMenu.Root>
      <Tooltip content="Track options">
        <DropdownMenu.Trigger>
          <IconButton variant="soft" size="1">
            <DotsHorizontalIcon />
          </IconButton>
        </DropdownMenu.Trigger>
      </Tooltip>
      <DropdownMenu.Content>
        <DropdownMenu.Sub>
          <DropdownMenu.SubTrigger disabled={collections.length === 0}>
            Copy to collection
          </DropdownMenu.SubTrigger>
          <DropdownMenu.SubContent>
            {collections.map((c) => (
              <DropdownMenu.Item
                key={c.path}
                onSelect={() => onCopyToCollection(c.path)}
              >
                {collectionLabel(c.path)}
              </DropdownMenu.Item>
            ))}
          </DropdownMenu.SubContent>
        </DropdownMenu.Sub>
        <DropdownMenu.Sub>
          <DropdownMenu.SubTrigger disabled={otherCollections.length === 0}>
            Move to collection
          </DropdownMenu.SubTrigger>
          <DropdownMenu.SubContent>
            {otherCollections.map((c) => (
              <DropdownMenu.Item
                key={c.path}
                onSelect={() => onMoveToCollection(c.path)}
              >
                {collectionLabel(c.path)}
              </DropdownMenu.Item>
            ))}
          </DropdownMenu.SubContent>
        </DropdownMenu.Sub>
        <DropdownMenu.Separator />
        <DropdownMenu.Item color="red" onSelect={onDeleteFromCollection}>
          Delete from current collection
        </DropdownMenu.Item>
      </DropdownMenu.Content>
    </DropdownMenu.Root>
  );
}

export default NowPlayingMenu;
