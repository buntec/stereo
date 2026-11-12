import { HStack } from "@astryxdesign/core/Stack";
import { Slider } from "@astryxdesign/core/Slider";
import { ToggleButton } from "@astryxdesign/core/ToggleButton";
import { Tooltip } from "@astryxdesign/core/Tooltip";
import { Kbd } from "@astryxdesign/core/Kbd";
import {
  SpeakerLoudIcon,
  SpeakerModerateIcon,
  SpeakerOffIcon,
  SpeakerQuietIcon,
} from "@radix-ui/react-icons";

/** Playback volume by default, in %. */
export const DEFAULT_VOLUME = 100;

type VolumeControlProps = {
  /** 0–100 (%). */
  volume: number;
  muted: boolean;
  setVolume: (volume: number) => void;
  toggleMuted: () => void;
  /** False where the page can't set the volume (iOS): then only mute is offered. */
  showSlider: boolean;
  size: "sm" | "lg";
};

const volumeIcon = (volume: number, muted: boolean) => {
  if (muted || volume === 0) return <SpeakerOffIcon />;
  if (volume < 34) return <SpeakerQuietIcon />;
  if (volume < 67) return <SpeakerModerateIcon />;
  return <SpeakerLoudIcon />;
};

/** The footer's mute toggle and volume slider. Only this page's audio, not the OS's. */
function VolumeControl({
  volume,
  muted,
  setVolume,
  toggleMuted,
  showSlider,
  size,
}: VolumeControlProps) {
  return (
    <HStack vAlign="center" gap={2}>
      <Tooltip
        content={
          <>
            Mute <Kbd keys="m" />
          </>
        }
      >
        <ToggleButton
          isIconOnly
          size={size}
          label="Mute"
          isPressed={muted}
          icon={volumeIcon(volume, muted)}
          onPressedChange={toggleMuted}
        />
      </Tooltip>
      {showSlider ? (
        <Slider
          label="Volume"
          isLabelHidden
          width={96}
          value={muted ? 0 : volume}
          onChange={(v: number) => setVolume(v)}
          min={0}
          max={100}
          step={1}
          formatValue={(v) => `${v}%`}
          valueDisplay="tooltip"
        />
      ) : null}
    </HStack>
  );
}

export default VolumeControl;
