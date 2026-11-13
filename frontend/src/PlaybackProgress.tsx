import { useEffect, useState, type ReactNode } from "react";
import { HStack, StackItem } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { downloadStatusLabels } from "./AudioCache.ts";
import type { PlaybackEngine } from "./Playback.ts";
import type { AudioDownload } from "./Types.tsx";
import { formatDuration } from "./Utils.tsx";
import { Waveform } from "./Waveform.tsx";

/** How often the shown position follows playback, in ms. */
const UPDATE_MS = 1000;

type Progress = { currentTime: number; duration: number };

type PlaybackProgressProps = {
  playback: PlaybackEngine | null;
  /** The playing track's unfinished download, shown instead of its position. */
  download?: AudioDownload;
  peaks?: number[];
  onSeek: (seconds: number) => void;
  /** Floats over the middle of the waveform (the transport buttons). */
  children?: ReactNode;
};

/**
 * The footer's seek bar: the position, the waveform and the duration. It
 * follows playback itself rather than through the app's state, so that the
 * update every second re-renders only this, not the whole app: that takes long
 * enough to visibly stall the animations timed by the beats.
 */
function PlaybackProgress({
  playback,
  download,
  peaks,
  onSeek,
  children,
}: PlaybackProgressProps) {
  const [progress, setProgress] = useState<Progress>({
    currentTime: 0,
    duration: 0,
  });

  useEffect(() => {
    if (!playback) return;
    const update = () => {
      const currentTime = Math.max(0, playback.currentTime());
      // 0 while the track is loading, which shows "--:--" and disables the slider.
      const duration = playback.duration();
      setProgress((p) =>
        p.currentTime === currentTime && p.duration === duration
          ? p
          : { currentTime, duration },
      );
    };
    update();
    const i = setInterval(update, UPDATE_MS);
    return () => clearInterval(i);
  }, [playback]);

  // Shows the new position right away, not after the next update.
  const seek = (seconds: number) => {
    onSeek(seconds);
    setProgress((p) => (p.duration ? { ...p, currentTime: seconds } : p));
  };

  return (
    <HStack className="progress-slider" width="100%" vAlign="center" gap={2}>
      <Text
        type="supporting"
        hasTabularNumbers
        aria-live={download ? "polite" : undefined}
      >
        {download
          ? downloadStatusLabels[download.status]
          : formatDuration(progress.currentTime)}
      </Text>
      <StackItem size="fill" className="transport-stage">
        <Waveform
          peaks={peaks}
          currentTime={download ? 0 : progress.currentTime}
          duration={download ? 0 : progress.duration}
          onSeek={seek}
          buffered={download?.progress}
          isPending={!!download && download.progress === undefined}
        />
        {children}
      </StackItem>
      <Text type="supporting" hasTabularNumbers>
        {download
          ? download.progress === undefined
            ? "--%"
            : `${Math.round(download.progress * 100)}%`
          : progress.duration
            ? formatDuration(progress.duration)
            : "--:--"}
      </Text>
    </HStack>
  );
}

export default PlaybackProgress;
