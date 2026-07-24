import { useState, useEffect, useCallback } from "react";
import { HStack } from "@astryxdesign/core/Stack";

interface RatingProps {
  enabled: boolean;
  currentRating?: number;
  updateRating: (rating: number | null) => void;
}

function Rating({ enabled, currentRating, updateRating }: RatingProps) {
  const [rating, setRating] = useState<number | null>(null);

  useEffect(() => {
    setTimeout(() => setRating(currentRating ?? null));
  }, [currentRating]);

  const handleClick = useCallback(
    (index: number) => {
      if (enabled) {
        const newRating = index >= 0 ? index + 1 : null;
        setRating(newRating);
        updateRating(newRating);
      }
    },
    [enabled, updateRating],
  );

  return (
    <HStack
      vAlign="center"
      style={{
        cursor: enabled ? "pointer" : "not-allowed",
      }}
    >
      {[...Array(5)].map((_, i) => (
        <span
          key={i}
          className="rating-star"
          onClick={(ev) => {
            ev.stopPropagation();
            handleClick(i);
          }}
          onDoubleClick={(ev) => {
            ev.stopPropagation();
            handleClick(-1); // reset rating
          }}
          style={{
            color:
              i < (rating ?? 0)
                ? "var(--color-icon-yellow)"
                : "var(--color-icon-gray)",
          }}
        >
          {i < (rating ?? 0) ? "★" : "☆"}
        </span>
      ))}
    </HStack>
  );
}

export default Rating;
