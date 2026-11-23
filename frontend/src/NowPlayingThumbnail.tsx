import { useEffect, useRef, useState, type CSSProperties } from "react";
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
  const thumbnail = useRef<HTMLImageElement>(null);
  const [isOpen, setIsOpen] = useState(false);
  const [dismissed, setDismissed] = useState(false);

  // The large thumbnail can cover the small one. Closing it with Escape then
  // uncovers the small one under a pointer that hasn't moved, and the browser's
  // new hover on it would open the card again. So after Escape the card stays
  // closed until the pointer is next seen outside the small thumbnail. By its
  // position, not the event's target: while the card fades out, it's the target.
  useEffect(() => {
    if (!isOpen) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") setDismissed(true);
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [isOpen]);
  useEffect(() => {
    if (!dismissed) return;
    const onPointer = (e: PointerEvent) => {
      const rect = thumbnail.current?.getBoundingClientRect();
      const inside =
        rect &&
        e.clientX >= rect.left &&
        e.clientX <= rect.right &&
        e.clientY >= rect.top &&
        e.clientY <= rect.bottom;
      if (!inside) setDismissed(false);
    };
    window.addEventListener("pointermove", onPointer, true);
    window.addEventListener("pointerdown", onPointer, true);
    return () => {
      window.removeEventListener("pointermove", onPointer, true);
      window.removeEventListener("pointerdown", onPointer, true);
    };
  }, [dismissed]);

  return (
    <HoverCard
      label="Thumbnail of the playing track"
      className="now-playing-thumbnail-card"
      style={CENTERED}
      isEnabled={!dismissed}
      onOpenChange={setIsOpen}
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
        ref={thumbnail}
        className="now-playing-thumbnail"
        // 16:9 without the letterboxing of the 4:3 sizes.
        src={thumbnailUrl(ytId, "mqdefault")}
        alt="Thumbnail of the playing track"
        tabIndex={0}
      />
    </HoverCard>
  );
};
