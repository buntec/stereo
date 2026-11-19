import { Divider } from "@astryxdesign/core/Divider";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Popover } from "@astryxdesign/core/Popover";
import { Slider } from "@astryxdesign/core/Slider";
import { Switch } from "@astryxdesign/core/Switch";
import { VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { GearIcon } from "@radix-ui/react-icons";
import type { ReactNode } from "react";
import type { AirSettings, BackgroundSettings } from "./Types.tsx";

/** How strongly the background pulses with the beats by default, in %. */
export const DEFAULT_BACKGROUND_PULSE = 50;

type SettingsMenuProps = {
  /** Whether the background artwork is shown at all (the footer's toggle). */
  backgroundShown: boolean;
  backgroundPulse: number;
  setBackgroundPulse: (pulse: number) => void;
  /** The background artwork's saturation, opacity and blur. */
  background: BackgroundSettings;
  setBackground: (background: BackgroundSettings) => void;
  /** The air the header's dance stacks push with the beats. */
  air: AirSettings;
  setAir: (air: AirSettings) => void;
  /** Controls that live in the nav bar on wide screens (e.g. the appearance switch). */
  header?: ReactNode;
  footer?: ReactNode;
};

// A popover rather than a dialog, so the background stays visible while
// adjusting how it pulses.
function SettingsMenu({
  backgroundShown,
  backgroundPulse,
  setBackgroundPulse,
  background,
  setBackground,
  air,
  setAir,
  header,
  footer,
}: SettingsMenuProps) {
  const airSlider = (
    key: Exclude<keyof AirSettings, "isOn">,
    label: string,
    tooltip: string,
    min: number,
    max: number,
    step: number,
    formatValue: (v: number) => string,
  ) => (
    <Slider
      label={label}
      labelTooltip={tooltip}
      value={air[key]}
      onChange={(v: number) => setAir({ ...air, [key]: v })}
      min={min}
      max={max}
      step={step}
      formatValue={formatValue}
      valueDisplay="text"
      isDisabled={!air.isOn}
      disabledMessage="Speaker air is off"
    />
  );
  const backgroundSlider = (
    key: keyof BackgroundSettings,
    label: string,
    tooltip: string,
    max: number,
    formatValue: (v: number) => string,
  ) => (
    <Slider
      label={label}
      labelTooltip={tooltip}
      value={background[key]}
      onChange={(v: number) => setBackground({ ...background, [key]: v })}
      min={0}
      max={max}
      step={5}
      formatValue={formatValue}
      valueDisplay="text"
      isDisabled={!backgroundShown}
      disabledMessage="Background is off"
    />
  );
  const percent = (v: number) => `${v}%`;
  const ms = (v: number) => `${v} ms`;
  const px = (v: number) => (v === 0 ? "Off" : v.toFixed(1));

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
          {header}
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
          {backgroundSlider(
            "saturation",
            "Background saturation",
            "How colourful the background is; 0% is greyscale.",
            200,
            percent,
          )}
          {backgroundSlider(
            "opacity",
            "Background opacity",
            "How strongly the background shows between beats.",
            100,
            percent,
          )}
          {backgroundSlider(
            "blur",
            "Background blur",
            "How strongly the background is blurred.",
            200,
            (v) => (v === 0 ? "Off" : `${v}%`),
          )}
          <Divider />
          <Switch
            label="Speaker air"
            labelTooltip="Whether the speaker stacks in the header push air out with the beats."
            value={air.isOn}
            onChange={(isOn) => setAir({ ...air, isOn })}
            labelPosition="start"
            labelSpacing="spread"
          />
          {airSlider(
            "waveDuration",
            "Wave duration",
            "How long each beat's pressure wave travels before it fades.",
            200,
            1500,
            50,
            ms,
          )}
          {airSlider(
            "waveTravel",
            "Wave travel",
            "How far each wave travels from the speakers, in cabinet widths.",
            0.2,
            3,
            0.1,
            (v) => `${v.toFixed(1)}×`,
          )}
          {airSlider(
            "calmTurbulence",
            "Calm turbulence",
            "How much the air wobbles the waves between beats.",
            0,
            10,
            0.5,
            px,
          )}
          {airSlider(
            "beatTurbulence",
            "Beat turbulence",
            "How much the air wobbles the waves on a full beat.",
            0,
            20,
            0.5,
            px,
          )}
          {airSlider(
            "settleTime",
            "Settle time",
            "How quickly the air calms down after a beat.",
            50,
            600,
            10,
            ms,
          )}
          {footer}
        </VStack>
      }
    >
      <IconButton
        label="Settings"
        tooltip="Settings"
        icon={<GearIcon />}
        variant="ghost"
        className="settings-button"
      />
    </Popover>
  );
}

export default SettingsMenu;
