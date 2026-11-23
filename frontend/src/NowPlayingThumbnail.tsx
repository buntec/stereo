import { useState, type CSSProperties } from "react";
import { HoverCard } from "@astryxdesign/core/HoverCard";

const thumbnailUrl = (ytId: string, size: string) =>
  `https://i.ytimg.com/vi/${encodeURIComponent(ytId)}/${size}.jpg`;

// Centers the card in the viewport instead of beside its trigger. The card is a
// popover, which the browser centers with these, once its anchor position is
// switched off; HoverCard applies `style` after its own position styles.
const CENTERED: CSSProperties = {
  positionArea: "none",
  positionTryFallbacks: "none",
  inset: 0,
  margin: "auto",
};
// The largest of YouTube's thumbnails; not every video has one.
const LARGE_SIZES = ["maxresdefault", "hqdefault"];

// The playing track's thumbnail, in the footer. Hovering it (or tapping it, on touch
// screens) shows the video's largest thumbnail at its natural size, centered in and
// fitting the window. Meant to be keyed by the video id, so the fallback starts over
// for every track.
export const NowPlayingThumbnail = ({ ytId }: { ytId: string }) => {
  const [sizeIndex, setSizeIndex] = useState(0);
  const nextSize = () =>
    setSizeIndex((i) => Math.min(i + 1, LARGE_SIZES.length - 1));

  return (
    <HoverCard
      label="Thumbnail of the playing track"
      className="now-playing-thumbnail-card"
      style={CENTERED}
      content={
        <img
          className="now-playing-thumbnail-large"
          src={thumbnailUrl(ytId, LARGE_SIZES[sizeIndex])}
          alt=""
          onError={nextSize}
          // A missing maxresdefault can also come back as a 120×90 placeholder.
          onLoad={(e) => {
            if (e.currentTarget.naturalWidth <= 120) nextSize();
          }}
        />
      }
    >
      <img
        className="now-playing-thumbnail"
        // 16:9 without the letterboxing of the 4:3 sizes.
        src={thumbnailUrl(ytId, "mqdefault")}
        alt="Thumbnail of the playing track"
        tabIndex={0}
      />
    </HoverCard>
  );
};
