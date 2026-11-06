import { HStack } from "@astryxdesign/core/Stack";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Cross2Icon, StarIcon, StarFilledIcon } from "@radix-ui/react-icons";

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
      <HStack
        role="radiogroup"
        aria-label={label}
        aria-disabled={!enabled}
        gap={0}
        onKeyDown={(event) => {
          const keys: Record<string, number> = {
            ArrowRight: Math.min(5, (currentRating ?? 0) + 1),
            ArrowUp: Math.min(5, (currentRating ?? 0) + 1),
            ArrowLeft: Math.max(1, (currentRating ?? 2) - 1),
            ArrowDown: Math.max(1, (currentRating ?? 2) - 1),
            Home: 1,
            End: 5,
          };
          const value = keys[event.key];
          if (!enabled || value === undefined) return;
          event.preventDefault();
          event.stopPropagation();
          updateRating(value);
          const radios =
            event.currentTarget.querySelectorAll<HTMLButtonElement>(
              '[role="radio"]',
            );
          radios[value - 1]?.focus();
        }}
      >
        {[1, 2, 3, 4, 5].map((value) => (
          <IconButton
            key={value}
            role="radio"
            aria-checked={currentRating === value}
            tabIndex={value === (currentRating || 1) ? 0 : -1}
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
            onClick={() => updateRating(value)}
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
        onClick={() => updateRating(null)}
      />
    </HStack>
  );
}
