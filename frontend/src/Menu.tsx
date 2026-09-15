import type { ReactNode } from "react";
import { HStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { Icon } from "@astryxdesign/core/Icon";

// Visual section title inside a compound-mode DropdownMenu. Hidden from
// assistive tech: the group that follows carries its own aria-label.
export function MenuHeading({ children }: { children: ReactNode }) {
  return (
    <HStack paddingInline={2} paddingBlock={1} aria-hidden="true">
      <Text type="supporting" color="secondary" weight="bold">
        {children}
      </Text>
    </HStack>
  );
}

// Selection indicator for DropdownMenuRadioItem / DropdownMenuCheckboxItem,
// passed as `endContent`. The theme hides the built-in radio and checkbox
// controls (see theme.ts), so this is the only visual cue.
export function MenuCheck({ isChecked }: { isChecked: boolean }) {
  return isChecked ? <Icon icon="check" size="sm" color="accent" /> : null;
}
