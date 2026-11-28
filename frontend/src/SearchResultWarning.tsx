import { useHoverCard } from "@astryxdesign/core/HoverCard";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Icon } from "@astryxdesign/core/Icon";
import { List, ListItem } from "@astryxdesign/core/List";
import { VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
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

/** Why a result's video may be another version of the track than its own. */
const versionWarning = (track: ITrack) =>
  track.mix_name?.trim()
    ? `This video may not be the ${track.mix_name.trim()}`
    : "This video may be another version";

/** Warns about a search result: that its video may be another version of the track (no video
 * was found that is clearly its version, so the backend settled for one that may not be), or that the
 * collection has what looks like the same track, usually under another YouTube video, which a
 * card lists. One icon for both, so that a row doesn't show two of them. A hover card rather
 * than a tooltip, for the list; through the hook so that pressing or tapping the button opens
 * it too (as in `UpdateIndicator`). */
function SearchResultWarning({
  track,
  similar,
}: {
  track: ITrack;
  similar?: ITrack[];
}) {
  const version = track.version_unconfirmed ? versionWarning(track) : undefined;
  const similarLabel = !similar?.length
    ? undefined
    : similar.length === 1
      ? "A similar track is in the collection"
      : `${similar.length} similar tracks are in the collection`;
  const label = [version, similarLabel].filter(Boolean).join(". ");
  const {
    ref: triggerRef,
    isOpen,
    id: cardId,
    show,
    renderHoverCard,
  } = useHoverCard({ label, placement: "below", touchTrigger: "tap" });
  if (!label) return null;
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
        <VStack gap={2}>
          {version && (
            <Text as="p" type="body">
              {version}. Change the video if you find the right one.
            </Text>
          )}
          {similar?.length ? (
            <List header={similarLabel} density="compact" edge="flush">
              {similar.map((t) => (
                <ListItem
                  key={t.yt_id}
                  label={`${t.artists.join(", ")} – ${t.title}`}
                  description={describe(t) || undefined}
                />
              ))}
            </List>
          ) : null}
        </VStack>,
      )}
    </>
  );
}

export default SearchResultWarning;
