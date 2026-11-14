/**
 * The header's speaker, as a dance stack that builds up with the volume: the
 * logo's cabinet alone when quiet, then two more beside it and two bass bins
 * under the three. Pure geometry: `danceStack(level)` returns the SVG
 * polygons, drawn by `DanceStack.tsx`.
 *
 * Every cabinet is a box in 3D, seen through the camera the logo was drawn with
 * (recovered from its corners: a level camera, so vertical edges stay vertical,
 * above the cabinet and to its left, the cabinet's front turned right toward the
 * app). Decorations (horns, mouths, handles) are polygons in a face's own
 * coordinates, (u, v) in [0, 1]², u left to right as seen and v downward;
 * the logo's are exactly those of `public/favicon.svg`, so at level 0 the
 * stack is the logo, path for path.
 */

/** A point in the image: the logo's 64 × 64 viewBox. */
export type Point = [number, number];

export type Shape = { points: Point[]; fill: string };

/** One cabinet: its faces and decorations, back to front, and its highlighted edges. */
export type CabinetDrawing = {
  shapes: Shape[];
  edges: Point[];
  /** 0–1: a cabinet fades in as a whole. */
  opacity: number;
};

/** Lengths are in cabinet (front) widths. */
const CAMERA = {
  /** Focal length and principal point, in image units. */
  f: 222.634,
  cx: 32,
  cy: -35.986,
  /** The stack's yaw, in radians; negative turns its front to the right. */
  yaw: -0.68519,
  /** Where the middle of the cabinet's front top edge is, relative to the camera. */
  x: 0.1644,
  y: 1.1785,
  z: 6.0959,
};

const sinYaw = Math.sin(CAMERA.yaw);
const cosYaw = Math.cos(CAMERA.yaw);

/**
 * Projects a point of the stack: `x` along the front (0 in its middle), `y`
 * down from the cabinet's top, `w` back from the front.
 */
const project = (x: number, y: number, w: number): Point => {
  const X = CAMERA.x + x * cosYaw + w * sinYaw;
  const Z = CAMERA.z - x * sinYaw + w * cosYaw;
  const Y = CAMERA.y + y;
  return [CAMERA.cx + (CAMERA.f * X) / Z, CAMERA.cy + (CAMERA.f * Y) / Z];
};

type Polygon = [number, number][];
type Decoration = { fill: string; polygons: Polygon[] };

const COLORS = {
  front: "#2f2b57",
  side: "#262249",
  top: "#3d3873",
  edge: "#5a5596",
  handle: "#181530",
};

/** The highlighted front edges' color. */
export const EDGE_COLOR = COLORS.edge;

// The logo's front, in face coordinates.
const PANEL: Decoration = {
  fill: "#d3d5da",
  polygons: [
    [
      [0.202, 0.052],
      [0.798, 0.052],
      [0.928, 0.147],
      [0.928, 0.569],
      [0.071, 0.569],
      [0.071, 0.146],
    ],
  ],
};

/** The small high-frequency horn at the top of the panel. */
const HF_HORN: Decoration[] = [
  {
    fill: "#6c717e",
    polygons: [
      [
        [0.262, 0.078],
        [0.738, 0.078],
        [0.559, 0.114],
        [0.441, 0.114],
      ],
    ],
  },
  {
    fill: "#eef0f2",
    polygons: [
      [
        [0.262, 0.2],
        [0.738, 0.2],
        [0.559, 0.165],
        [0.441, 0.166],
      ],
    ],
  },
  {
    fill: "#a7abb4",
    polygons: [
      [
        [0.262, 0.078],
        [0.441, 0.114],
        [0.441, 0.166],
        [0.262, 0.2],
      ],
      [
        [0.738, 0.078],
        [0.559, 0.114],
        [0.559, 0.165],
        [0.738, 0.2],
      ],
    ],
  },
  {
    fill: "#110f21",
    polygons: [
      [
        [0.441, 0.114],
        [0.559, 0.114],
        [0.559, 0.165],
        [0.441, 0.166],
      ],
    ],
  },
];

/** The mid horn below it, with its lens. */
const MID_HORN: Decoration[] = [
  {
    fill: "#6c717e",
    polygons: [
      [
        [0.119, 0.233],
        [0.881, 0.233],
        [0.69, 0.302],
        [0.309, 0.302],
      ],
    ],
  },
  {
    fill: "#eef0f2",
    polygons: [
      [
        [0.119, 0.543],
        [0.881, 0.543],
        [0.69, 0.474],
        [0.309, 0.474],
      ],
    ],
  },
  {
    fill: "#a7abb4",
    polygons: [
      [
        [0.119, 0.233],
        [0.309, 0.302],
        [0.309, 0.474],
        [0.119, 0.543],
      ],
      [
        [0.881, 0.233],
        [0.69, 0.302],
        [0.69, 0.474],
        [0.881, 0.543],
      ],
    ],
  },
  {
    fill: "#110f21",
    polygons: [
      [
        [0.309, 0.302],
        [0.69, 0.302],
        [0.69, 0.474],
        [0.309, 0.474],
      ],
    ],
  },
  {
    fill: "#eef0f2",
    polygons: [
      [
        [0.226, 0.388],
        [0.357, 0.35],
        [0.643, 0.35],
        [0.774, 0.388],
      ],
    ],
  },
  {
    fill: "#6c717e",
    polygons: [
      [
        [0.226, 0.388],
        [0.357, 0.426],
        [0.643, 0.426],
        [0.774, 0.388],
      ],
    ],
  },
];

/** The bass horn's mouth at the bottom, with its bevels and braces. */
const BASS_MOUTH: Decoration[] = [
  {
    fill: "#110f21",
    polygons: [
      [
        [0.071, 0.621],
        [0.928, 0.621],
        [0.928, 0.948],
        [0.071, 0.948],
      ],
    ],
  },
  {
    fill: "#23203f",
    polygons: [
      [
        [0.071, 0.621],
        [0.928, 0.621],
        [0.833, 0.69],
        [0.166, 0.69],
      ],
      [
        [0.071, 0.621],
        [0.166, 0.69],
        [0.166, 0.948],
        [0.071, 0.948],
      ],
      [
        [0.928, 0.621],
        [0.833, 0.69],
        [0.833, 0.948],
        [0.928, 0.948],
      ],
    ],
  },
  {
    fill: COLORS.front,
    polygons: [
      [
        [0.476, 0.69],
        [0.524, 0.69],
        [0.524, 0.948],
        [0.476, 0.948],
      ],
      [
        [0.333, 0.759],
        [0.666, 0.759],
        [0.666, 0.786],
        [0.333, 0.786],
      ],
      [
        [0.166, 0.948],
        [0.166, 0.888],
        [0.25, 0.948],
      ],
      [
        [0.833, 0.948],
        [0.833, 0.888],
        [0.75, 0.948],
      ],
    ],
  },
];

/** A handle recessed into a side, u from the back (0) to the front (1). */
const handle = (v0: number, v1: number): Decoration => ({
  fill: COLORS.handle,
  polygons: [
    [
      [0.32, v0],
      [0.7, v0],
      [0.7, v1],
      [0.32, v1],
    ],
  ],
});

type Cabinet = {
  /** Width of the front and of the back (narrower for a tapered cabinet). */
  width: number;
  backWidth: number;
  height: number;
  depth: number;
  front: Decoration[];
  side: Decoration[];
};

/** The logo: a tapered full-range cabinet, horns over a bass horn. */
const MAIN: Cabinet = {
  width: 1,
  backWidth: 0.5229,
  height: 1.3652,
  depth: 0.8338,
  front: [PANEL, ...HF_HORN, ...MID_HORN, ...BASS_MOUTH],
  side: [handle(0.32, 0.38), handle(0.78, 0.84)],
};

/** How many of the logo's cabinets stand side by side in the full stack. */
const TOPS = 3;

/**
 * The bass bin the cabinets stand on, as wide as their row: one big horn,
 * its walls flaring out to a mouth over nearly all of its front.
 */
const BASS_BIN: Cabinet = {
  width: TOPS,
  backWidth: TOPS,
  height: 0.9,
  // Deep enough for the turned cabinets at the row's far end to stand on.
  depth: 1.25,
  front: [
    {
      fill: "#110f21",
      polygons: [
        [
          [0.03, 0.07],
          [0.97, 0.07],
          [0.97, 0.93],
          [0.03, 0.93],
        ],
      ],
    },
    {
      fill: "#3d3873",
      polygons: [
        [
          [0.03, 0.07],
          [0.97, 0.07],
          [0.78, 0.45],
          [0.22, 0.45],
        ],
      ],
    },
    {
      fill: "#23203f",
      polygons: [
        [
          [0.03, 0.07],
          [0.22, 0.45],
          [0.22, 0.93],
          [0.03, 0.93],
        ],
        [
          [0.97, 0.07],
          [0.78, 0.45],
          [0.78, 0.93],
          [0.97, 0.93],
        ],
      ],
    },
  ],
  side: [handle(0.4, 0.54)],
};

/** Where a cabinet stands: the middle of its front, and its turn from the logo's. */
type Pose = {
  x: number;
  w: number;
  /** In radians; positive turns the front's right end back. */
  angle: number;
};

/** Projects a point given along the front (x) and back (w) of a cabinet at `pose`. */
const placed = (pose: Pose) => {
  const cos = Math.cos(pose.angle);
  const sin = Math.sin(pose.angle);
  return (x: number, y: number, w: number) =>
    project(pose.x + x * cos - w * sin, y, pose.w + x * sin + w * cos);
};

/** Draws a cabinet standing at `pose`, its top `top` down from the logo's top. */
const drawCabinet = (
  cabinet: Cabinet,
  pose: Pose,
  top: number,
  opacity: number,
): CabinetDrawing => {
  const { width, backWidth, height, depth } = cabinet;
  const shapes: Shape[] = [];
  const bottom = top + height;
  const left = -width / 2;
  const right = width / 2;
  const backLeft = -backWidth / 2;
  const backRight = backWidth / 2;
  const at = placed(pose);
  const onFront = ([u, v]: [number, number]) =>
    at(left + u * width, top + v * height, 0);
  // u runs from the back-left edge to the front-left edge.
  const onSide = ([u, v]: [number, number]) =>
    at(backLeft + u * (left - backLeft), top + v * height, depth * (1 - u));
  const add = (points: Point[], fill: string) => shapes.push({ points, fill });

  add(
    [
      at(backLeft, top, depth),
      at(left, top, 0),
      at(left, bottom, 0),
      at(backLeft, bottom, depth),
    ],
    COLORS.side,
  );
  for (const { fill, polygons } of cabinet.side)
    for (const polygon of polygons) add(polygon.map(onSide), fill);
  add(
    [
      at(left, top, 0),
      at(right, top, 0),
      at(backRight, top, depth),
      at(backLeft, top, depth),
    ],
    COLORS.top,
  );
  add(
    [
      at(left, top, 0),
      at(right, top, 0),
      at(right, bottom, 0),
      at(left, bottom, 0),
    ],
    COLORS.front,
  );
  for (const { fill, polygons } of cabinet.front)
    for (const polygon of polygons) add(polygon.map(onFront), fill);
  return {
    shapes,
    edges: [at(left, bottom, 0), at(left, top, 0), at(right, top, 0)],
    opacity,
  };
};

const clamp01 = (x: number) => Math.min(1, Math.max(0, x));

/** How far along (0–1) a part is, over the volume range in which it appears. */
const stage = (level: number, from: number, to: number) =>
  clamp01((level - from) / (to - from));

/** How much each cabinet in the row turns back from the one before it. */
const SPLAY = (8 * Math.PI) / 180;

/**
 * The row's cabinets on a slight arc, as stacks are splayed: from the logo's,
 * each turns back a little more, its front's left corner on the right corner
 * of the one before.
 */
const arc = (): Pose[] => {
  const poses: Pose[] = [{ x: 0, w: 0, angle: 0 }];
  for (let i = 1; i < TOPS; i++) {
    const prev = poses[i - 1];
    const angle = i * SPLAY;
    const half = MAIN.width / 2;
    poses.push({
      x: prev.x + half * Math.cos(prev.angle) + half * Math.cos(angle),
      w: prev.w + half * Math.sin(prev.angle) + half * Math.sin(angle),
      angle,
    });
  }
  return poses;
};

/** A cabinet's front, where it pushes air out with the beats (`airWave`). */
export type Front = {
  pose: Pose;
  width: number;
  top: number;
  height: number;
  /** 0–1: as faded in as its cabinet. */
  opacity: number;
};

export type DanceStackDrawing = {
  /** In drawing order: each cabinet hides the parts of the ones before it behind it. */
  cabinets: CabinetDrawing[];
  /** The fronts of the cabinets drawn, the bass bins' first. */
  fronts: Front[];
};

/** How much a pressure wave widens on every side per cabinet width it travels. */
const WAVE_SPREAD = 0.35;
/** How far (in cabinet widths) a wave travels before it is round. */
const WAVE_ROUNDING = 0.8;
/** Points around a wave's outline. */
const WAVE_POINTS = 64;

/**
 * The outline of a pressure wave `distance` (in cabinet widths) in front of
 * `front`: it leaves as the front's rectangle, widens as it travels, and rounds
 * off into an ellipse (a superellipse in between), in the plane parallel to the
 * front, seen through the same camera as the stack.
 */
export const airWave = (front: Front, distance: number): Point[] => {
  const at = placed(front.pose);
  const a = front.width / 2 + WAVE_SPREAD * distance;
  const b = front.height / 2 + WAVE_SPREAD * distance;
  const middle = front.top + front.height / 2;
  // The superellipse's exponent: near 0 a rectangle, 1 an ellipse.
  const p = 0.15 + 0.85 * clamp01(distance / WAVE_ROUNDING);
  const power = (c: number) => Math.sign(c) * Math.abs(c) ** p;
  const points: Point[] = [];
  for (let i = 0; i < WAVE_POINTS; i++) {
    const t = (2 * Math.PI * i) / WAVE_POINTS;
    points.push(
      at(a * power(Math.cos(t)), middle + b * power(Math.sin(t)), -distance),
    );
  }
  return points;
};

/**
 * The stack at `level` (0–1, the volume): below a tenth the logo alone, then a
 * second cabinet beside it, a third, a bass bin under the three and a second
 * bin under that by nine tenths. Each fades in where it stands; nothing grows,
 * and the logo stays put.
 */
export const danceStack = (level: number): DanceStackDrawing => {
  const tops = [1, stage(level, 0.1, 0.25), stage(level, 0.3, 0.45)];
  const bins = [stage(level, 0.5, 0.65), stage(level, 0.75, 0.9)];

  // The bins bottom up, as each stands on the one below; then the cabinets from
  // the far end of the row (the camera is off the logo's end) to the logo.
  const cabinets: CabinetDrawing[] = [];
  const fronts: Front[] = [];
  // The bins stand square under the row's middle cabinet, so the row splays
  // out evenly to both ends over them.
  const poses = arc();
  const binPose = poses[(TOPS - 1) / 2];
  for (let i = bins.length - 1; i >= 0; i--)
    if (bins[i] > 0) {
      const top = MAIN.height + i * BASS_BIN.height;
      cabinets.push(drawCabinet(BASS_BIN, binPose, top, bins[i]));
      fronts.push({
        pose: binPose,
        width: BASS_BIN.width,
        top,
        height: BASS_BIN.height,
        opacity: bins[i],
      });
    }
  for (let i = TOPS - 1; i >= 0; i--)
    if (tops[i] > 0) {
      cabinets.push(drawCabinet(MAIN, poses[i], 0, tops[i]));
      fronts.push({
        pose: poses[i],
        width: MAIN.width,
        top: 0,
        height: MAIN.height,
        opacity: tops[i],
      });
    }
  return { cabinets, fronts };
};
