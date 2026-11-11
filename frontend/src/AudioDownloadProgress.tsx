import { HStack, StackItem } from "@astryxdesign/core/Stack";
import { ProgressBar } from "@astryxdesign/core/ProgressBar";
import { Text } from "@astryxdesign/core/Text";
import type { AudioDownload } from "./Types.tsx";

const statusLabels: Record<AudioDownload["status"], string> = {
  queued: "Waiting to download",
  preparing: "Preparing download",
  downloading: "Downloading",
  done: "Downloaded",
  failed: "Download failed",
};

/**
 * Stands in for the playback slider while the playing track's audio is being
 * downloaded: there's nothing to seek in yet.
 */
export function AudioDownloadProgress({
  download,
}: {
  download: AudioDownload;
}) {
  const percent =
    download.progress === undefined
      ? undefined
      : Math.round(download.progress * 100);
  return (
    <HStack
      className="progress-slider"
      width="100%"
      vAlign="center"
      gap={2}
      aria-live="polite"
    >
      <Text type="supporting">{statusLabels[download.status]}</Text>
      <StackItem size="fill">
        <ProgressBar
          label="Audio download progress"
          isLabelHidden
          value={percent ?? 0}
          isIndeterminate={percent === undefined}
          style={{ paddingInline: "var(--spacing-3)" }}
        />
      </StackItem>
      <Text type="supporting" hasTabularNumbers>
        {percent === undefined ? "--%" : `${percent}%`}
      </Text>
    </HStack>
  );
}
