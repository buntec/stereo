import {
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
} from "react";

/** Width of a bar and of the gap after it, in CSS pixels. */
const BAR_WIDTH = 2;
const BAR_GAP = 1;
/** Bars of silence still show as a line. */
const MIN_BAR_HEIGHT = 1;
/** Height of the bars while there's no waveform (yet), as a fraction. */
const PLACEHOLDER_LEVEL = 0.08;
/** Seek step of the arrow keys, and of Page Up / Page Down, in seconds. */
const KEY_STEP = 5;
const PAGE_STEP = 30;

const formatTime = (seconds: number) => {
  const s = Math.max(0, Math.floor(seconds));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
};

/** The loudest value in each of `n` equal spans of `peaks`, as fractions of 255. */
const resample = (peaks: number[], n: number): number[] =>
  Array.from({ length: n }, (_, i) => {
    const start = Math.floor((i * peaks.length) / n);
    const end = Math.max(start + 1, Math.floor(((i + 1) * peaks.length) / n));
    let max = 0;
    for (let j = start; j < end && j < peaks.length; j++)
      max = Math.max(max, peaks[j]);
    return max / 255;
  });

/** One path of the bars in `[from, to)`, centered vertically in `height`. */
const barsPath = (
  levels: number[],
  from: number,
  to: number,
  height: number,
) => {
  let d = "";
  for (let i = Math.max(0, from); i < Math.min(to, levels.length); i++) {
    const h = Math.max(MIN_BAR_HEIGHT, levels[i] * height);
    const x = i * (BAR_WIDTH + BAR_GAP);
    const y = (height - h) / 2;
    d += `M${x} ${y.toFixed(1)}h${BAR_WIDTH}v${h.toFixed(1)}h${-BAR_WIDTH}z`;
  }
  return d;
};

type WaveformProps = {
  /**
   * Loudness values (0–255); undefined while loading or when there is no
   * waveform, which shows a flat bar.
   */
  peaks: number[] | undefined;
  /** Playback position in seconds. */
  currentTime: number;
  /** Duration in seconds; 0 while unknown, which disables seeking. */
  duration: number;
  onSeek: (seconds: number) => void;
  /** Fraction of the audio downloaded, while it downloads. */
  buffered?: number;
  /** Waiting for the audio (e.g. a download that hasn't started): pulses. */
  isPending?: boolean;
};

/**
 * The playing track's waveform as its seek bar: played bars in the accent
 * color, the rest muted. Hovering previews the position a click seeks to;
 * dragging scrubs, seeking when released. A slider for assistive technology
 * and the keyboard (arrow keys, Page Up/Down, Home/End).
 */
export function Waveform({
  peaks,
  currentTime,
  duration,
  onSeek,
  buffered,
  isPending = false,
}: WaveformProps) {
  const ref = useRef<SVGSVGElement>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  /** Fraction under the mouse pointer, when hovering. */
  const [hover, setHover] = useState<number>();
  /** Fraction being dragged to, while dragging. */
  const [scrub, setScrub] = useState<number>();

  useLayoutEffect(() => {
    const svg = ref.current;
    if (!svg) return;
    // The rendered box: an <svg>'s content rect isn't its CSS box in every browser.
    const measure = () => {
      const { width, height } = svg.getBoundingClientRect();
      setSize({ width, height });
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(svg);
    return () => observer.disconnect();
  }, []);

  const count = Math.max(
    0,
    Math.floor((size.width + BAR_GAP) / (BAR_WIDTH + BAR_GAP)),
  );
  const levels = useMemo(
    () =>
      peaks
        ? resample(peaks, count)
        : new Array<number>(count).fill(PLACEHOLDER_LEVEL),
    [peaks, count],
  );

  const enabled = duration > 0;
  const progress = enabled ? Math.min(1, currentTime / duration) : 0;
  const shown = scrub ?? progress;
  const played = Math.round(shown * count);
  const hovered = hover === undefined ? played : Math.round(hover * count);
  const ahead = Math.max(played, hovered);
  const downloaded = Math.max(ahead, Math.round((buffered ?? 0) * count));
  const { height } = size;

  const fractionAt = (event: PointerEvent) => {
    const rect = event.currentTarget.getBoundingClientRect();
    return Math.min(
      1,
      Math.max(0, (event.clientX - rect.left) / (rect.width || 1)),
    );
  };

  const onPointerDown = (event: PointerEvent<SVGSVGElement>) => {
    if (!enabled || event.button !== 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    setScrub(fractionAt(event));
  };
  const onPointerMove = (event: PointerEvent<SVGSVGElement>) => {
    if (!enabled) return;
    const fraction = fractionAt(event);
    if (scrub !== undefined) setScrub(fraction);
    if (event.pointerType === "mouse") setHover(fraction);
  };
  const onPointerUp = (event: PointerEvent<SVGSVGElement>) => {
    if (scrub === undefined) return;
    onSeek(fractionAt(event) * duration);
    setScrub(undefined);
  };
  const onKeyDown = (event: KeyboardEvent<SVGSVGElement>) => {
    if (!enabled) return;
    const steps: Record<string, number> = {
      ArrowLeft: -KEY_STEP,
      ArrowDown: -KEY_STEP,
      ArrowRight: KEY_STEP,
      ArrowUp: KEY_STEP,
      PageDown: -PAGE_STEP,
      PageUp: PAGE_STEP,
      Home: -duration,
      End: duration,
    };
    const step = steps[event.key];
    if (step === undefined) return;
    event.preventDefault();
    onSeek(Math.min(duration, Math.max(0, currentTime + step)));
  };

  // The time a click would seek to, next to the pointer.
  const label =
    enabled && (scrub ?? hover) !== undefined
      ? {
          text: formatTime((scrub ?? hover)! * duration),
          x: (scrub ?? hover)! * size.width,
        }
      : undefined;
  const labelWidth = 40;
  const labelX = label
    ? Math.min(Math.max(0, label.x - labelWidth / 2), size.width - labelWidth)
    : 0;

  return (
    <svg
      ref={ref}
      className="waveform"
      data-enabled={enabled}
      data-pending={isPending}
      viewBox={`0 0 ${size.width || 1} ${height || 1}`}
      role="slider"
      tabIndex={enabled ? 0 : -1}
      aria-label="Playback position"
      aria-disabled={!enabled}
      aria-valuemin={0}
      aria-valuemax={Math.round(duration)}
      aria-valuenow={Math.round(currentTime)}
      aria-valuetext={
        enabled
          ? `${formatTime(currentTime)} of ${formatTime(duration)}`
          : undefined
      }
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={() => setScrub(undefined)}
      onPointerLeave={() => setHover(undefined)}
      onKeyDown={onKeyDown}
    >
      <path
        className="waveform-played"
        d={barsPath(levels, 0, Math.min(played, hovered), height)}
      />
      {/* Between the position and the pointer: played up to it, or what a click skips. */}
      <path
        className={
          hovered < played ? "waveform-rewind" : "waveform-hover-preview"
        }
        d={barsPath(
          levels,
          Math.min(played, hovered),
          Math.max(played, hovered),
          height,
        )}
      />
      <path
        className="waveform-buffered"
        d={barsPath(levels, ahead, downloaded, height)}
      />
      <path
        className="waveform-unplayed"
        d={barsPath(levels, downloaded, count, height)}
      />
      {label && height > 0 ? (
        <g className="waveform-label" aria-hidden>
          <rect
            x={labelX}
            y={0}
            width={labelWidth}
            height={Math.min(height, 16)}
            rx={3}
          />
          <text
            x={labelX + labelWidth / 2}
            y={Math.min(height, 16) / 2}
            textAnchor="middle"
            dominantBaseline="central"
          >
            {label.text}
          </text>
        </g>
      ) : null}
    </svg>
  );
}
