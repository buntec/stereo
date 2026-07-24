import { DropdownMenu } from "@astryxdesign/core/DropdownMenu";
import { DotsHorizontalIcon, TrashIcon } from "@radix-ui/react-icons";
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
    <DropdownMenu
      hasChevron={false}
      button={{
        label: "Track options",
        icon: <DotsHorizontalIcon />,
        isIconOnly: true,
        variant: "secondary",
        size: "sm",
        tooltip: "Track options",
      }}
      items={[
        {
          type: "section",
          title: "Copy to collection",
          items: collections.map((c) => ({
            label: collectionLabel(c.path),
            onClick: () => onCopyToCollection(c.path),
          })),
        },
        {
          type: "section",
          title: "Move to collection",
          items: otherCollections.map((c) => ({
            label: collectionLabel(c.path),
            onClick: () => onMoveToCollection(c.path),
          })),
        },
        { type: "divider" },
        {
          label: "Delete from current collection",
          icon: <TrashIcon />,
          onClick: onDeleteFromCollection,
        },
      ]}
    />
  );
}

export default NowPlayingMenu;
