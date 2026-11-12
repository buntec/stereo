import { IconButton } from "@astryxdesign/core/IconButton";
import { Popover } from "@astryxdesign/core/Popover";
import { Slider } from "@astryxdesign/core/Slider";
import { VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { GearIcon } from "@radix-ui/react-icons";

/** How strongly the background pulses with the beats by default, in %. */
export const DEFAULT_BACKGROUND_PULSE = 50;

type SettingsMenuProps = {
  /** Whether the background artwork is shown at all (the footer's toggle). */
  backgroundShown: boolean;
  backgroundPulse: number;
  setBackgroundPulse: (pulse: number) => void;
};

// A popover rather than a dialog, so the background stays visible while
// adjusting how it pulses.
function SettingsMenu({
  backgroundShown,
  backgroundPulse,
  setBackgroundPulse,
}: SettingsMenuProps) {
  return (
    <Popover
      label="Settings"
      placement="below"
      alignment="end"
      width={280}
      content={
        <VStack gap={3}>
          <Text type="label" weight="bold">
            Settings
          </Text>
          <Slider
            label="Background pulse"
            labelTooltip="How strongly the background pulses with the beats."
            value={backgroundPulse}
            onChange={(v: number) => setBackgroundPulse(v)}
            min={0}
            max={100}
            step={5}
            formatValue={(v) => (v === 0 ? "Off" : `${v}%`)}
            valueDisplay="text"
            isDisabled={!backgroundShown}
            disabledMessage="Background is off"
          />
        </VStack>
      }
    >
      <IconButton
        label="Settings"
        tooltip="Settings"
        icon={<GearIcon />}
        variant="ghost"
      />
    </Popover>
  );
}

export default SettingsMenu;
