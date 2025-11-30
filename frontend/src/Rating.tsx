import { useState, useEffect, useCallback } from "react";
import { Flex } from "@radix-ui/themes";

interface RatingProps {
  enabled: boolean;
  currentRating?: number;
  updateRating: (rating: number | null) => void;
}

export function Rating({ enabled, currentRating, updateRating }: RatingProps) {
  const [rating, setRating] = useState<number | null>(null);

  useEffect(() => {
    setRating(currentRating ?? null);
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
    <Flex
      align="center"
      style={{
        cursor: enabled ? "pointer" : "not-allowed",
      }}
    >
      {[...Array(5)].map((_, i) => (
        <span
          key={i}
          onClick={(ev) => {
            ev.stopPropagation();
            handleClick(i);
          }}
          onDoubleClick={(ev) => {
            ev.stopPropagation();
            handleClick(-1); // reset rating
          }}
          style={{
            color: i < (rating ?? 0) ? "var(--yellow-8)" : "var(--gray-8)",
            fontSize: "1.2rem",
          }}
        >
          {i < (rating ?? 0) ? "★" : "☆"}
        </span>
      ))}
    </Flex>
  );
}
