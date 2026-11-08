import type { MouseEvent } from "react";
import { HStack } from "@astryxdesign/core/Stack";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Cross2Icon, StarIcon, StarFilledIcon } from "@radix-ui/react-icons";

// Ratings change only via the mouse or the global 0-5 shortcuts, never via other keys.
// The buttons are kept out of the tab order and don't take focus when clicked, so Space,
// Enter and arrow keys keep going to the grid / global shortcuts instead of a star.
const pointerOnly = {
  tabIndex: -1,
  onMouseDown: (event: MouseEvent<HTMLButtonElement>) => event.preventDefault(),
};

// Keyboard-activated clicks (Enter/Space on a focused button) have detail 0.
const isPointerClick = (event: MouseEvent<HTMLButtonElement>) =>
  event.detail > 0;

interface RatingProps {
  enabled: boolean;
  currentRating?: number;
  updateRating: (rating: number | null) => void;
  label?: string;
}

export default function Rating({
  enabled,
  currentRating,
  updateRating,
  label = "Track rating",
}: RatingProps) {
  return (
    <HStack
      vAlign="center"
      gap={0}
      onClick={(event) => event.stopPropagation()}
    >
      <HStack role="group" aria-label={label} gap={0}>
        {[1, 2, 3, 4, 5].map((value) => (
          <IconButton
            key={value}
            aria-pressed={currentRating === value}
            {...pointerOnly}
            label={`${value} of 5 stars`}
            tooltip={`${value} of 5 stars`}
            variant="ghost"
            size="sm"
            isDisabled={!enabled}
            style={{
              color:
                value <= (currentRating ?? 0)
                  ? "var(--color-icon-yellow)"
                  : "var(--color-icon-secondary)",
            }}
            icon={
              value <= (currentRating ?? 0) ? <StarFilledIcon /> : <StarIcon />
            }
            onClick={(event) => {
              if (isPointerClick(event)) updateRating(value);
            }}
          />
        ))}
      </HStack>
      <IconButton
        label="Clear rating"
        tooltip="Clear rating"
        variant="ghost"
        size="sm"
        icon={<Cross2Icon />}
        isDisabled={!enabled || !currentRating}
        {...pointerOnly}
        onClick={(event) => {
          if (isPointerClick(event)) updateRating(null);
        }}
      />
    </HStack>
  );
}
