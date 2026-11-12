import type { ReactNode } from "react";
import { HStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";

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
