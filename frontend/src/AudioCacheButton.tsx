import { IconButton } from "@astryxdesign/core/IconButton";
import {
  CheckCircledIcon,
  DownloadIcon,
  ExclamationTriangleIcon,
} from "@radix-ui/react-icons";
import { isDownloading, useTrackAudio } from "./AudioCache.ts";

/** Matches the 15×15 Radix icons. */
const SIZE = 15;
const RADIUS = 5.5;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;

/** A ring filling up with the download; spinning while the size isn't known. */
function DownloadRing({ progress }: { progress?: number }) {
  const fraction = progress ?? 0.25;
  return (
    <svg
      width={SIZE}
      height={SIZE}
      viewBox={`0 0 ${SIZE} ${SIZE}`}
      fill="none"
      className={progress === undefined ? "download-ring-spinning" : undefined}
      aria-hidden
    >
      <circle
        cx={SIZE / 2}
        cy={SIZE / 2}
        r={RADIUS}
        stroke="currentColor"
        strokeOpacity={0.3}
        strokeWidth={2}
      />
      <circle
        cx={SIZE / 2}
        cy={SIZE / 2}
        r={RADIUS}
        stroke="currentColor"
        strokeWidth={2}
        strokeLinecap="round"
        strokeDasharray={`${fraction * CIRCUMFERENCE} ${CIRCUMFERENCE}`}
        transform={`rotate(-90 ${SIZE / 2} ${SIZE / 2})`}
      />
    </svg>
  );
}

/**
 * Whether a track's audio is downloaded (green), being downloaded (a progress
 * ring), failed to download (red) or not downloaded yet (gray). Clicking it
 * downloads the track, retrying one that failed.
 */
export function AudioCacheButton({
  id,
  onDownload,
}: {
  id: string;
  onDownload: (id: string) => void;
}) {
  const { cached, download } = useTrackAudio(id);
  const downloading = isDownloading(download);
  const failed = !cached && download?.status === "failed";

  const percent =
    download?.progress === undefined
      ? undefined
      : Math.round(download.progress * 100);
  const [state, label, icon] = cached
    ? (["cached", "Downloaded", <CheckCircledIcon />] as const)
    : downloading
      ? ([
          "downloading",
          download?.status === "downloading"
            ? `Downloading${percent === undefined ? "…" : ` (${percent}%)`}`
            : download?.status === "preparing"
              ? "Preparing download…"
              : "Waiting to download…",
          <DownloadRing
            progress={
              download?.status === "downloading" ? download.progress : undefined
            }
          />,
        ] as const)
      : failed
        ? ([
            "failed",
            `Retry download${download?.error ? ` (${download.error})` : ""}`,
            <ExclamationTriangleIcon />,
          ] as const)
        : (["missing", "Download", <DownloadIcon />] as const);

  return (
    <IconButton
      variant="ghost"
      size="sm"
      className="audio-cache-button"
      data-state={state}
      label={label}
      tooltip={label}
      icon={icon}
      onClick={(event) => {
        event.stopPropagation();
        if (!cached && !downloading) onDownload(id);
      }}
    />
  );
}
