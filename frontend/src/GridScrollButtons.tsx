import { IconButton } from "@astryxdesign/core/IconButton";
import { Tooltip } from "@astryxdesign/core/Tooltip";
import { Kbd } from "@astryxdesign/core/Kbd";
import { VStack } from "@astryxdesign/core/Stack";
import { ArrowUpIcon, TargetIcon } from "@radix-ui/react-icons";

interface GridScrollButtonsProps {
  showTop: boolean;
  showCurrent: boolean;
  onTop: () => void;
  onShowCurrent: () => void;
}

/** Floating buttons over the grid's bottom-right corner (the grid's container is positioned). */
export const GridScrollButtons = ({
  showTop,
  showCurrent,
  onTop,
  onShowCurrent,
}: GridScrollButtonsProps) => {
  if (!showTop && !showCurrent) return null;
  return (
    <VStack gap={2} className="grid-scroll-buttons">
      {showCurrent && (
        <Tooltip
          placement="start"
          content={
            <>
              Show playing track <Kbd keys="c" />
            </>
          }
        >
          <IconButton
            label="Show playing track"
            icon={<TargetIcon />}
            size="lg"
            onClick={onShowCurrent}
          />
        </Tooltip>
      )}
      {showTop && (
        <Tooltip
          placement="start"
          content={
            <>
              Back to top <Kbd keys="t" />
            </>
          }
        >
          <IconButton
            label="Back to top"
            icon={<ArrowUpIcon />}
            size="lg"
            onClick={onTop}
          />
        </Tooltip>
      )}
    </VStack>
  );
};
