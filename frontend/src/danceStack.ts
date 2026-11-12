/**
 * The header's speaker, as a dance stack that grows with the volume: the logo's
 * cabinet alone when quiet, then bass bins appearing under it and a top box on
 * it. Pure geometry: `danceStack(level)` returns the SVG polygons, drawn by
 * `DanceStack.tsx`.
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
type Rect = [u0: number, v0: number, u1: number, v1: number];

const COLORS = {
  front: "#2f2b57",
  side: "#262249",
  top: "#3d3873",
  edge: "#5a5596",
  handle: "#181530",
};

/** The highlighted front edges' color. */
export const EDGE_COLOR = COLORS.edge;

/** Moves decorations drawn in the rect `from` of a face to the rect `to`. */
const remap = (decorations: Decoration[], from: Rect, to: Rect): Decoration[] =>
  decorations.map(({ fill, polygons }) => ({
    fill,
    polygons: polygons.map((polygon) =>
      polygon.map(([u, v]) => [
        to[0] + ((u - from[0]) / (from[2] - from[0])) * (to[2] - to[0]),
        to[1] + ((v - from[1]) / (from[3] - from[1])) * (to[3] - to[1]),
      ]),
    ),
  }));

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
const PANEL_RECT: Rect = [0.071, 0.052, 0.928, 0.569];

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
const MID_HORN_RECT: Rect = [0.119, 0.233, 0.881, 0.543];

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
const BASS_MOUTH_RECT: Rect = [0.071, 0.621, 0.928, 0.948];

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

/** A wide bass bin with two horn mouths, for under the cabinet. */
const BASS_BIN: Cabinet = {
  width: 1.3,
  backWidth: 1.3,
  height: 0.62,
  depth: 0.9,
  front: [
    ...remap(BASS_MOUTH, BASS_MOUTH_RECT, [0.05, 0.12, 0.49, 0.88]),
    ...remap(BASS_MOUTH, BASS_MOUTH_RECT, [0.51, 0.12, 0.95, 0.88]),
  ],
  side: [handle(0.42, 0.56)],
};

/** A top box with two mid horns side by side, for on top of the cabinet. */
const TOP_BOX: Cabinet = {
  width: 0.92,
  backWidth: 0.92,
  height: 0.42,
  depth: 0.7,
  front: [
    ...remap([PANEL], PANEL_RECT, [0.04, 0.1, 0.96, 0.9]),
    ...remap(MID_HORN, MID_HORN_RECT, [0.07, 0.18, 0.49, 0.82]),
    ...remap(MID_HORN, MID_HORN_RECT, [0.51, 0.18, 0.93, 0.82]),
  ],
  side: [handle(0.4, 0.58)],
};

/**
 * Draws a cabinet whose top is `top` (down from the main cabinet's top), as
 * tall as `height` (its own unless it's still rising).
 */
const drawCabinet = (
  cabinet: Cabinet,
  top: number,
  opacity: number,
  height = cabinet.height,
): CabinetDrawing => {
  const { width, backWidth, depth } = cabinet;
  const shapes: Shape[] = [];
  const bottom = top + height;
  const left = -width / 2;
  const right = width / 2;
  const backLeft = -backWidth / 2;
  const backRight = backWidth / 2;
  const onFront = ([u, v]: [number, number]) =>
    project(left + u * width, top + v * height, 0);
  // u runs from the back-left edge to the front-left edge.
  const onSide = ([u, v]: [number, number]) =>
    project(
      backLeft + u * (left - backLeft),
      top + v * height,
      depth * (1 - u),
    );
  const add = (points: Point[], fill: string) => shapes.push({ points, fill });

  add(
    [
      project(backLeft, top, depth),
      project(left, top, 0),
      project(left, bottom, 0),
      project(backLeft, bottom, depth),
    ],
    COLORS.side,
  );
  for (const { fill, polygons } of cabinet.side)
    for (const polygon of polygons) add(polygon.map(onSide), fill);
  add(
    [
      project(left, top, 0),
      project(right, top, 0),
      project(backRight, top, depth),
      project(backLeft, top, depth),
    ],
    COLORS.top,
  );
  add(
    [
      project(left, top, 0),
      project(right, top, 0),
      project(right, bottom, 0),
      project(left, bottom, 0),
    ],
    COLORS.front,
  );
  for (const { fill, polygons } of cabinet.front)
    for (const polygon of polygons) add(polygon.map(onFront), fill);
  return {
    shapes,
    edges: [
      project(left, bottom, 0),
      project(left, top, 0),
      project(right, top, 0),
    ],
    opacity,
  };
};

const clamp01 = (x: number) => Math.min(1, Math.max(0, x));

/** How far along (0–1) a part is, over the volume range in which it appears. */
const stage = (level: number, from: number, to: number) =>
  clamp01((level - from) / (to - from));

export type DanceStackDrawing = {
  /** Bottom up: from above, each cabinet hides the top of the one under it. */
  cabinets: CabinetDrawing[];
  /**
   * The drawing's top in image units (the logo's is 2): it rises with the top
   * box, so the stack can stay anchored at its top-left.
   */
  top: number;
  /** How much bigger than the plain logo the drawing is shown. */
  scale: number;
};

/**
 * The stack at `level` (0–1, the volume): below a tenth the logo alone, then
 * three bass bins one after another under it, and a top box on it by nine
 * tenths. The bins fade in where they stand; the top box rises out of the
 * cabinet (fading in as it does), pushing it down. The whole grows by half.
 */
export const danceStack = (level: number): DanceStackDrawing => {
  const cabinets: CabinetDrawing[] = [];
  const bins = [
    stage(level, 0.1, 0.3),
    stage(level, 0.3, 0.5),
    stage(level, 0.5, 0.7),
  ];
  const topBox = stage(level, 0.7, 0.9);

  let y = MAIN.height + bins.length * BASS_BIN.height;
  for (let i = bins.length - 1; i >= 0; i--) {
    y -= BASS_BIN.height;
    if (bins[i] > 0) cabinets.push(drawCabinet(BASS_BIN, y, bins[i]));
  }
  cabinets.push(drawCabinet(MAIN, 0, 1));
  const boxHeight = TOP_BOX.height * topBox;
  if (topBox > 0)
    cabinets.push(
      drawCabinet(TOP_BOX, -boxHeight, Math.min(1, 2 * topBox), boxHeight),
    );

  // The back corner is the highest point: the logo's, or the top box's.
  const top =
    topBox > 0
      ? project(TOP_BOX.backWidth / 2, -boxHeight, TOP_BOX.depth)[1]
      : project(MAIN.backWidth / 2, 0, MAIN.depth)[1];
  return {
    cabinets,
    top,
    scale: 1 + 0.5 * clamp01(level),
  };
};
