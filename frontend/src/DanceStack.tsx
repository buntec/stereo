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
   * The stack's mirror image, for the header's other end: a stereo pair. It is
   * only decoration.
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
  // The logo's viewBox is 0 0 64 64 and the stack is drawn in the same units,
  // at the same scale, so the logo stays put and the rest overflows its box
  // (the cabinets beside it rise a little, being farther from the camera).
  const xs = points.map(([x]) => x);
  const ys = points.map(([, y]) => y);
  const left = Math.min(0, ...xs) - 2;
  const right = Math.max(64, ...xs) + 2;
  const top = Math.min(0, ...ys) - 2;
  const bottom = Math.max(64, ...ys) + 2;
  const scale = LOGO_SIZE / 64;

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
        y={top * scale}
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
