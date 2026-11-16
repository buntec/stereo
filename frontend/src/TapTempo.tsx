import { useEffect, useState, type ReactNode } from "react";
import { Button, type ButtonProps } from "@astryxdesign/core/Button";
import { Kbd } from "@astryxdesign/core/Kbd";
import { Text } from "@astryxdesign/core/Text";
import { Tooltip } from "@astryxdesign/core/Tooltip";
import { VStack } from "@astryxdesign/core/Stack";
// Radix has no metronome glyph. Lucide is the icon set Astryx's Neutral theme draws its own
// icons with, so it matches the buttons around it.
import { Metronome } from "lucide-react";
import type { Tempo } from "./Types.tsx";
import { atOctaveOf, formatTempo } from "./tempo.ts";
import {
  formatTapBpm,
  TAP_RESET_MS,
  tapTempo,
  useTapTempo,
} from "./tapTempo.ts";

/** The global shortcut for a tap (`App.tsx`'s keyboard actions). */
export const TAP_TEMPO_KEY = "Shift+T";

/** How far the tapped BPM is from `reference`, e.g. "0.3 faster", "half time, matches". */
const comparison = (tapped: number, reference: number): string => {
  const value = atOctaveOf(reference, tapped);
  const octave =
    value > reference
      ? "double time, "
      : value < reference
        ? "half time, "
        : "";
  const difference = tapped - value;
  if (Math.abs(difference) < 0.05) return `${octave}matches`;
  return `${octave}tapped ${Math.abs(difference).toFixed(1)} ${difference > 0 ? "faster" : "slower"}`;
};

/**
 * A button to tap along with the beat, showing the tempo of the taps, to check
 * the tempo measured from the audio and the metadata's BPM against: its tooltip
 * compares them. A pause of two seconds starts a new count; the last count stays
 * on show (dimmed) until then.
 */
export default function TapTempoButton({
  tempo,
  bpm,
  size,
}: {
  /** The playing track's measured tempo. */
  tempo?: Tempo;
  /** The BPM in its metadata. */
  bpm?: number | null;
  size?: ButtonProps["size"];
}) {
  const tapped = useTapTempo();
  const tappedBpm = formatTapBpm(tapped);

  // Whether the user is still tapping, i.e. the next tap continues the count.
  // The latest tap once the pause after it has been long enough to end the count.
  const [settledTap, setSettledTap] = useState<number>();
  const isActive = tapped.last !== undefined && tapped.last !== settledTap;
  useEffect(() => {
    const last = tapped.last;
    if (last === undefined) return;
    const timer = window.setTimeout(
      () => setSettledTap(last),
      last + TAP_RESET_MS - performance.now(),
    );
    return () => window.clearTimeout(timer);
  }, [tapped.last]);

  let details: ReactNode;
  if (tapped.bpm === undefined) {
    details = (
      <>
        Tap along with the beat to check the tempo <Kbd keys={TAP_TEMPO_KEY} />
      </>
    );
  } else {
    const lines = [`Tapped ${tappedBpm} BPM, ${tapped.taps} taps`];
    if (tempo) {
      lines.push(
        `Measured ${formatTempo(tempo, bpm)}: ${comparison(tapped.bpm, tempo.bpm)}`,
      );
    }
    if (bpm) {
      lines.push(`Metadata ${bpm} BPM: ${comparison(tapped.bpm, bpm)}`);
    }
    details = (
      <VStack gap={0} hAlign="start">
        {lines.map((line) => (
          <Text key={line} type="supporting" color="inherit">
            {line}
          </Text>
        ))}
        <Text type="supporting" color="inherit">
          Pause for 2 seconds to start over <Kbd keys={TAP_TEMPO_KEY} />
        </Text>
      </VStack>
    );
  }

  return (
    <Tooltip content={details}>
      <Button
        size={size}
        variant="ghost"
        className="tap-tempo-button"
        label={tappedBpm ? `Tap tempo: ${tappedBpm} BPM` : "Tap tempo"}
        icon={<Metronome size="1em" aria-hidden />}
        // Counted when the finger or button goes down, not on release: that's
        // when the tap is on the beat.
        onPointerDown={(event) => {
          if (event.button === 0) tapTempo.tap(event.timeStamp);
        }}
        // Enter or Space on the focused button; a pointer's click has a detail.
        onClick={(event) => {
          if (event.detail === 0) tapTempo.tap(event.timeStamp);
        }}
      >
        <Text
          type="supporting"
          hasTabularNumbers
          color={isActive || !tappedBpm ? "inherit" : "secondary"}
          className="tap-tempo-value"
        >
          {tappedBpm ?? "Tap"}
        </Text>
      </Button>
    </Tooltip>
  );
}
