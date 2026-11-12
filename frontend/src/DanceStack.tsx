import { useMemo } from "react";
import { danceStack, EDGE_COLOR, type Point } from "./danceStack.ts";

/** The logo's size in the header, in px: the stack's cabinet at level 0. */
const LOGO_SIZE = 28;

const path = (points: Point[]) =>
  `M${points.map(([x, y]) => `${x.toFixed(2)} ${y.toFixed(2)}`).join("L")}`;

type DanceStackProps = {
  /** 0–1: the volume, from the logo alone to a full stack. */
  level: number;
  /**
   * The stack's mirror image, for the header's other end: a stereo pair. It
   * takes no space in the layout (App.css places it) and is only decoration.
   */
  isMirrored?: boolean;
};

/**
 * The header's logo, growing into a dance stack with the volume (geometry in
 * `danceStack.ts`). It takes up the logo's place in the layout and draws the
 * rest of the stack over its edges, behind the rest of the UI (App.css), so
 * its size never moves the header. Hand-drawn SVG: Astryx has nothing like it.
 */
function DanceStack({ level, isMirrored = false }: DanceStackProps) {
  const drawing = useMemo(() => danceStack(level), [level]);

  const points = drawing.cabinets.flatMap((c) =>
    c.shapes.flatMap((s) => s.points),
  );
  // The logo's viewBox is 0 0 64 64 with its top at 2; the stack keeps that
  // margin above and left, and grows to the right and down.
  const left = Math.min(0, ...points.map(([x]) => x));
  const right = Math.max(64, ...points.map(([x]) => x));
  const top = drawing.top - 2;
  const bottom = Math.max(64, ...points.map(([, y]) => y + 2));
  const scale = (LOGO_SIZE / 64) * drawing.scale;

  return (
    <svg
      className={
        isMirrored ? "dance-stack dance-stack-mirrored" : "dance-stack"
      }
      width={LOGO_SIZE}
      height={LOGO_SIZE}
      {...(isMirrored
        ? { "aria-hidden": true }
        : { role: "img", "aria-label": "Stereo" })}
    >
      <svg
        x={left * scale}
        viewBox={`${left} ${top} ${right - left} ${bottom - top}`}
        width={(right - left) * scale}
        height={(bottom - top) * scale}
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
  );
}

export default DanceStack;
