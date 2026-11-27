import {
  DropdownMenu,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
} from "@astryxdesign/core/DropdownMenu";
import { Divider } from "@astryxdesign/core/Divider";
import { StatusDot } from "@astryxdesign/core/StatusDot";
import {
  BookmarkIcon,
  GearIcon,
  ListBulletIcon,
  ResetIcon,
} from "@radix-ui/react-icons";
import { MenuHeading } from "./Menu.tsx";
import { collectionLabel } from "./CollectionSelector.tsx";
import type { IPlaylist } from "./Types.tsx";

type PlaylistMenuProps = {
  playlists: IPlaylist[];
  /** The playlist whose view is shown, if any. */
  active?: IPlaylist;
  /** Whether the view's filters or sorting differ from the active playlist's. */
  isModified: boolean;
  /** Whether a playlist's last sync failed. */
  hasSyncErrors: boolean;
  canSave: boolean;
  onSelect: (playlist: IPlaylist) => void;
  onSaveAs: () => void;
  onSaveChanges: () => void;
  onRevert: () => void;
  onManage: () => void;
};

/** Opens, saves and manages dynamic playlists: saved views of a collection. */
function PlaylistMenu({
  playlists,
  active,
  isModified,
  hasSyncErrors,
  canSave,
  onSelect,
  onSaveAs,
  onSaveChanges,
  onRevert,
  onManage,
}: PlaylistMenuProps) {
  const label = active
    ? `${active.name}${isModified ? " (modified)" : ""}`
    : "Playlists";
  const viewTooltip = active
    ? `Playlist ${active.name}${isModified ? ", with changed filters or sorting" : ""}`
    : "Saved views of your collections, which can be kept in sync with Spotify and YouTube";
  const tooltip = hasSyncErrors
    ? `${active ? `${viewTooltip}. ` : ""}A playlist failed to sync: see Manage playlists.`
    : viewTooltip;
  return (
    <DropdownMenu
      menuWidth={300}
      button={{
        label,
        variant: "ghost",
        style: { maxWidth: "100%" },
        tooltip,
        icon: hasSyncErrors ? (
          <StatusDot variant="error" label="A playlist failed to sync" />
        ) : (
          <ListBulletIcon />
        ),
      }}
    >
      {playlists.length > 0 && (
        <>
          <MenuHeading>Playlists</MenuHeading>
          <DropdownMenuRadioGroup
            label="Playlists"
            value={active?.id}
            onChange={(id) => {
              const playlist = playlists.find((p) => p.id === id);
              if (playlist) onSelect(playlist);
            }}
          >
            {playlists.map((p) => (
              <DropdownMenuRadioItem
                key={p.id}
                value={p.id}
                label={p.name}
                description={collectionLabel(p.collection)}
              />
            ))}
          </DropdownMenuRadioGroup>
          <Divider />
        </>
      )}
      {active && isModified && (
        <>
          <DropdownMenuItem
            icon={<BookmarkIcon />}
            label={`Save changes to ${active.name}`}
            onClick={onSaveChanges}
          />
          <DropdownMenuItem
            icon={<ResetIcon />}
            label={`Revert to ${active.name}`}
            onClick={onRevert}
          />
        </>
      )}
      <DropdownMenuItem
        icon={<BookmarkIcon />}
        label="Save current view as playlist…"
        isDisabled={!canSave}
        onClick={onSaveAs}
      />
      <DropdownMenuItem
        icon={<GearIcon />}
        label="Manage playlists…"
        description={hasSyncErrors ? "A playlist failed to sync" : undefined}
        endContent={
          hasSyncErrors && <StatusDot variant="error" label="Sync failed" />
        }
        isDisabled={playlists.length === 0}
        onClick={onManage}
      />
    </DropdownMenu>
  );
}

export default PlaylistMenu;
