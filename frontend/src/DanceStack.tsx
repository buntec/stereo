import { useEffect, useMemo, useRef } from "react";
import {
  airWave,
  danceStack,
  EDGE_COLOR,
  type Front,
  type Point,
} from "./danceStack.ts";
import type { PlaybackEngine } from "./Playback.ts";
import { DEFAULT_AIR, type AirSettings, type Beat } from "./Types.tsx";
import {
  lastBeatIndex,
  playbackClock,
  usePrefersReducedMotion,
} from "./useBeatPulse.ts";

/** The logo's size in the header's layout, in px. */
const LOGO_SIZE = 28;
/**
 * How much larger than its box the stack is drawn, the logo with it: centered
 * on the box, it overflows into the header's padding and gap a little, and the
 * rest of the stack overflows into the space above the table.
 */
const STACK_SCALE = 1.2;
/** Px per logo unit (64 to the logo), and where the logo's origin is in its box. */
const SCALE = (LOGO_SIZE * STACK_SCALE) / 64;
const INSET = (LOGO_SIZE * (1 - STACK_SCALE)) / 2;

/** A wave's stroke width (logo units) as it leaves, and as it fades out. */
const WAVE_WIDTH = 5;
const WAVE_WIDTH_FADED = 1;

const path = (points: Point[]) =>
  `M${points.map(([x, y]) => `${x.toFixed(2)} ${y.toFixed(2)}`).join("L")}`;

/** The track playing, whose beats push air out of the speakers. */
export type DanceStackBeats = {
  playback: PlaybackEngine | null;
  id: string | undefined;
  beats: Beat[] | undefined;
  /** 0–1: how hard the speakers push, 0 while paused or muted. */
  strength: number;
  air: AirSettings;
};

type DanceStackProps = {
  /** 0–1: the volume, from the logo alone to a full stack. */
  level: number;
  /**
   * The stack's mirror image, for the header's other end: a stereo pair. It is
   * only decoration.
   */
  isMirrored?: boolean;
  beats?: DanceStackBeats;
};

/**
 * The header's logo, growing into a dance stack with the volume (geometry in
 * `danceStack.ts`). It takes up the logo's place in the layout and draws the
 * rest of the stack over its edges, behind the rest of the UI (App.css), so
 * its size never moves the header. With every beat of the playing track each
 * cabinet pushes a pressure wave out of its front, through churning air: drawn
 * on a canvas over the stack, since changing anything in the stack's SVG
 * every frame makes the browser repaint all of it (a core's worth of CPU).
 * Hand-drawn SVG and canvas: Astryx has nothing like it.
 */
function DanceStack({ level, isMirrored = false, beats }: DanceStackProps) {
  const drawing = useMemo(() => danceStack(level), [level]);
  const canvasRef = useAirWaves(drawing.fronts, beats);

  const points = drawing.cabinets.flatMap((c) =>
    c.shapes.flatMap((s) => s.points),
  );
  // The logo's viewBox is 0 0 64 64 and the stack is drawn in the same units,
  // at the same scale, so the logo stays put and the rest overflows its box
  // (the cabinets beside it rise a little, being farther from the camera).
  const xs = points.map(([x]) => x);
  const ys = points.map(([, y]) => y);
  const left = Math.min(0, ...xs) - 2;
  const right = Math.max(64, ...xs) + 2;
  const top = Math.min(0, ...ys) - 2;
  const bottom = Math.max(64, ...ys) + 2;

  return (
    <span
      className={
        isMirrored ? "dance-stack dance-stack-mirrored" : "dance-stack"
      }
      {...(isMirrored
        ? { "aria-hidden": true }
        : { role: "img", "aria-label": "Stereo" })}
    >
      <svg width={LOGO_SIZE} height={LOGO_SIZE} overflow="visible">
        <svg
          x={INSET + left * SCALE}
          y={INSET + top * SCALE}
          viewBox={`${left} ${top} ${right - left} ${bottom - top}`}
          width={(right - left) * SCALE}
          height={(bottom - top) * SCALE}
        >
          {drawing.cabinets.map((cabinet, i) => (
            <g key={i} opacity={cabinet.opacity}>
              {cabinet.shapes.map((shape, j) => (
                <path key={j} d={`${path(shape.points)}Z`} fill={shape.fill} />
              ))}
              <path
                d={path(cabinet.edges)}
                stroke={EDGE_COLOR}
                strokeWidth={0.6}
                fill="none"
              />
            </g>
          ))}
        </svg>
      </svg>
      <canvas ref={canvasRef} className="dance-stack-air" />
    </span>
  );
}

/**
 * Smooth noise in about [-1, 1], fixed in the image (logo units): a few
 * sines at odd angles and frequencies, two octaves of wiggles ~20 units
 * across. Cheap enough to sample every outline point every frame, which an
 * SVG turbulence filter is not (WebKit renders those in software).
 */
const noise = (x: number, y: number, phase: number) =>
  (Math.sin(0.29 * x + 0.17 * y + phase) +
    Math.sin(-0.13 * x + 0.31 * y + 2.1 * phase + 1.3) +
    0.5 * Math.sin(0.61 * x - 0.47 * y + 3.7 * phase + 0.4) +
    0.5 * Math.sin(0.43 * x + 0.67 * y + 5.3 * phase + 2.2)) /
  3;

/** `points` displaced by the turbulence, by up to `amount / 2` on each axis. */
const churn = (points: Point[], amount: number): Point[] =>
  points.map(([x, y]) => [
    x + (amount / 2) * noise(x, y, 0),
    y + (amount / 2) * noise(x, y, 1.9),
  ]);

/** Where the waves can go, in px from the stack's box: the canvas's bounds. */
const airBounds = (fronts: Front[], air: AirSettings) => {
  const reach = air.beatTurbulence / 2 + WAVE_WIDTH / 2 + 1;
  const points = fronts.flatMap((front) =>
    [0, 0.5, 1].flatMap((d) => airWave(front, d * air.waveTravel)),
  );
  const xs = points.map(([x]) => INSET + x * SCALE);
  const ys = points.map(([, y]) => INSET + y * SCALE);
  const pad = reach * SCALE;
  const left = Math.floor(Math.min(...xs) - pad);
  const top = Math.floor(Math.min(...ys) - pad);
  return {
    left,
    top,
    width: Math.ceil(Math.max(...xs) + pad) - left,
    height: Math.ceil(Math.max(...ys) + pad) - top,
  };
};

/**
 * The air in front of the stack's `fronts`: pressure waves leaving them with
 * the beats, wobbled by turbulence that flares up with each beat and settles,
 * drawn on the canvas of the returned ref. Updated on every animation frame
 * from the playback position, outside React; nothing while paused, muted, in
 * the background or for those who prefer reduced motion.
 */
function useAirWaves(fronts: Front[], source: DanceStackBeats | undefined) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const still = usePrefersReducedMotion();

  const { playback, id, beats, strength = 0, air = DEFAULT_AIR } = source ?? {};
  const active =
    !!playback &&
    !!id &&
    !!beats?.length &&
    strength > 0 &&
    fronts.length > 0 &&
    !still;

  useEffect(() => {
    const canvas = canvasRef.current;
    const context = canvas?.getContext("2d");
    if (!active || !playback || !id || !beats || !canvas || !context) return;
    const bounds = airBounds(fronts, air);
    const ratio = window.devicePixelRatio || 1;
    Object.assign(canvas.style, {
      left: `${bounds.left}px`,
      top: `${bounds.top}px`,
      width: `${bounds.width}px`,
      height: `${bounds.height}px`,
      display: "block",
    });
    canvas.width = Math.round(bounds.width * ratio);
    canvas.height = Math.round(bounds.height * ratio);
    // Logo units to the canvas's pixels.
    const transform = [
      SCALE * ratio,
      0,
      0,
      SCALE * ratio,
      (INSET - bounds.left) * ratio,
      (INSET - bounds.top) * ratio,
    ] as const;
    context.strokeStyle = getComputedStyle(canvas).color;
    context.lineJoin = "round";
    const clock = playbackClock(playback);
    let drawn = false;
    let frame = requestAnimationFrame(function tick(now) {
      frame = requestAnimationFrame(tick);
      // The beats are those of `id`; the engine may already be loading another track.
      const ms = playback.currentId() === id ? clock(now) : -Infinity;
      const last = lastBeatIndex(beats, ms);
      const latest = beats[last];
      const inFlight = !!latest && ms - latest[0] < air.waveDuration;
      if (!inFlight && !drawn) return;
      context.setTransform(1, 0, 0, 1, 0, 0);
      context.clearRect(0, 0, canvas.width, canvas.height);
      drawn = inFlight;
      if (!inFlight) return;
      context.setTransform(...transform);
      const pulse =
        (latest[1] / 255) * Math.exp(-(ms - latest[0]) / air.settleTime);
      const turbulence =
        air.calmTurbulence +
        (air.beatTurbulence - air.calmTurbulence) * pulse * strength;
      // The beats whose waves are still in flight, newest first.
      for (let k = last; k >= 0; k--) {
        const beat = beats[k];
        const age = (ms - beat[0]) / air.waveDuration;
        if (age >= 1) break;
        // Eases out: it leaves with the beat's punch and slows as it spreads.
        const distance = air.waveTravel * (1 - (1 - age) ** 2);
        const fade = (1 - age) ** 1.5;
        context.lineWidth =
          WAVE_WIDTH_FADED + (WAVE_WIDTH - WAVE_WIDTH_FADED) * fade;
        for (const front of fronts) {
          context.globalAlpha =
            0.6 * (beat[1] / 255) * fade * strength * front.opacity;
          context.beginPath();
          churn(airWave(front, distance), turbulence).forEach(([x, y], i) =>
            i ? context.lineTo(x, y) : context.moveTo(x, y),
          );
          context.closePath();
          context.stroke();
        }
      }
    });
    return () => {
      cancelAnimationFrame(frame);
      canvas.style.display = "none";
    };
  }, [active, playback, id, beats, strength, air, fronts]);

  return canvasRef;
}

export default DanceStack;
