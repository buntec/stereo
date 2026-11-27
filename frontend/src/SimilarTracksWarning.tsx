import { useHoverCard } from "@astryxdesign/core/HoverCard";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Icon } from "@astryxdesign/core/Icon";
import { List, ListItem } from "@astryxdesign/core/List";
import type { ITrack } from "./Types.tsx";

// Ratings outside the five-star scale (from an import, say) shouldn't break the grid.
const stars = (rating?: number) => {
  const n = Math.min(5, Math.max(0, Math.round(Number(rating) || 0)));
  return n > 0 ? "★".repeat(n) : undefined;
};

const describe = (track: ITrack) =>
  [
    track.mix_name,
    track.bpm ? `${track.bpm} BPM` : undefined,
    track.key,
    track.label,
    stars(track.rating),
  ]
    .filter(Boolean)
    .join(" · ");

/** Warns that the collection has what looks like the same track as a search result, usually
 * under another YouTube video, and lists those tracks in a card. A hover card rather than a
 * tooltip, for the list; through the hook so that pressing or tapping the button opens it too
 * (as in `UpdateIndicator`). */
function SimilarTracksWarning({ similar }: { similar: ITrack[] }) {
  const label =
    similar.length === 1
      ? "A similar track is in the collection"
      : `${similar.length} similar tracks are in the collection`;
  const {
    ref: triggerRef,
    isOpen,
    id: cardId,
    show,
    renderHoverCard,
  } = useHoverCard({ label, placement: "below", touchTrigger: "tap" });
  return (
    <>
      <IconButton
        ref={triggerRef}
        label={label}
        variant="ghost"
        size="sm"
        icon={<Icon icon="warning" color="warning" />}
        aria-haspopup="dialog"
        aria-expanded={isOpen}
        aria-controls={isOpen ? cardId : undefined}
        onClick={show}
      />
      {renderHoverCard(
        <List header={label} density="compact" edge="flush">
          {similar.map((track) => (
            <ListItem
              key={track.yt_id}
              label={`${track.artists.join(", ")} – ${track.title}`}
              description={describe(track) || undefined}
            />
          ))}
        </List>,
      )}
    </>
  );
}

export default SimilarTracksWarning;
