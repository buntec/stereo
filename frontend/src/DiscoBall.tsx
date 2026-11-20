import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type RefObject,
} from "react";
import { usePrefersReducedMotion } from "./useBeatPulse.ts";
import type { AppearanceType } from "./Types.tsx";

/** The smallest radius the ball is drawn at, whatever its size setting, where the window has room, in px. */
const MIN_RADIUS = 20;
/** Where the window is too short for even this radius, in px, the ball isn't drawn at all. */
const SMALLEST_RADIUS = 8;
/**
 * How far above the window's middle the text of content centred in the window
 * (an empty state, a spinner) starts, about, in px: a ball reaching past it is
 * drawn fainter, as behind a grid's rows.
 */
const CENTRED_CONTENT_REACH = 60;
/** The cap its cord is tied to, in px. */
const CAP_WIDTH = 12;
const CAP_HEIGHT = 6;
/** Bands of mirror tiles from pole to pole, and tiles around the equator: square tiles. */
const BANDS = 44;
const EQUATOR_TILES = 88;
/** How much of a tile the (unlit) grout between tiles takes, as a fraction of its size. */
const GROUT = 0.2;
/** The ball hangs above us: tilted back by this much (radians), we see a little of its bottom. */
const TILT = -0.1;
/** How far each tile is off true, in radians: the patches of light break up unevenly. */
const WOBBLE = 0.07;
/** How much a tile's ink varies at random, for an engraving's grain. */
const GRAIN = 0.3;
/** How far a tile's corners are off, as a fraction of its size: engraved by hand. */
const ROUGH = 0.07;
/** Lighter than this, a tile is left out altogether. */
const BLANK = 0.93;
/** Inkier than this, a tile is solid; lighter, it's hatched with two lines, then one. */
const SOLID = 0.78;
const TWO_LINES = 0.4;

type Vec = [number, number, number];

const normalize = ([x, y, z]: Vec): Vec => {
  const l = Math.hypot(x, y, z);
  return [x / l, y / l, z / l];
};

/**
 * The room the mirrors reflect, as brightness by direction (view space: x
 * right, y up, z toward us): soft patches of light, which stay put while the
 * ball turns its tiles through them. Each is where it lies, how wide (a larger
 * spread: wider) and how bright.
 */
const PATCHES: { toward: Vec; spread: number; strength: number }[] = [
  // Behind us: a broad highlight in the ball's middle.
  { toward: normalize([0.2, 0.08, 1]), spread: 0.14, strength: 1.25 },
  // The ceiling's lights, along its top.
  { toward: normalize([-0.15, 1, 0.2]), spread: 0.18, strength: 0.75 },
  // Windows to the sides, and lamps low down.
  { toward: normalize([-0.9, 0.2, 0.4]), spread: 0.12, strength: 0.9 },
  { toward: normalize([0.85, 0.35, 0.4]), spread: 0.1, strength: 0.8 },
  { toward: normalize([0.5, -0.75, 0.45]), spread: 0.08, strength: 0.9 },
  { toward: normalize([-0.35, -0.85, 0.4]), spread: 0.1, strength: 0.6 },
  // Behind the ball: mirrors near its rim catch it, in streaks.
  { toward: normalize([0.1, 0.25, -1]), spread: 0.12, strength: 0.45 },
];

/** The room's light everywhere: most mirrors are hatched rather than solid. */
const AMBIENT = 0.04;

/** How light the room is in direction (`x`, `y`, `z`): 0 dark, 1 and more bright. */
const room = (x: number, y: number, z: number) => {
  let light = 0;
  for (const { toward, spread, strength } of PATCHES) {
    const facing = x * toward[0] + y * toward[1] + z * toward[2];
    light += strength * Math.exp((facing - 1) / spread);
  }
  // Swaths of light and shadow across the room, so the patches aren't round blobs.
  return (
    AMBIENT +
    light +
    0.32 * Math.sin(6 * x + 2 * y + 0.5) * Math.sin(4 * y - 5 * z + 1) +
    0.2 * Math.sin(11 * x - 7 * z + 2) * Math.sin(9 * y + 3 * x)
  );
};

/** A stable pseudo-random number in [0, 1) for each tile (and `k`), so each keeps its own quirks. */
const hash = (i: number, j: number, k: number) => {
  const s = Math.sin(i * 12.9898 + j * 78.233 + k * 37.719) * 43758.5453;
  return s - Math.floor(s);
};
/** The same, in [-1, 1). */
const jitter = (i: number, j: number, k: number) => 2 * hash(i, j, k) - 1;

const SIN_TILT = Math.sin(TILT);
const COS_TILT = Math.cos(TILT);

/** The ball's bands of tiles, each tile with its own quirks, computed once. */
const BAND_TILES = Array.from({ length: BANDS }, (_, i) => {
  const lat0 = -Math.PI / 2 + (i * Math.PI) / BANDS;
  const lat1 = lat0 + Math.PI / BANDS;
  const tiles = Math.max(
    4,
    Math.round(EQUATOR_TILES * Math.cos((lat0 + lat1) / 2)),
  );
  return {
    lat: (lat0 + lat1) / 2,
    height: lat1 - lat0,
    tiles,
    quirks: Array.from({ length: tiles }, (_, j) => ({
      // Off true, in latitude and longitude.
      dLat: jitter(i, j, 1) * WOBBLE,
      dLon: jitter(i, j, 2) * WOBBLE,
      grain: (jitter(i, j, 3) * GRAIN) / 2,
      // Its corners' offsets, clockwise from the top left, in tile sizes.
      rough: [4, 5, 6, 7].map((k) => jitter(i, j, k) * ROUGH),
    })),
  };
});

/** A path coordinate, to a twentieth of a px: short in the path's text, finer than a device pixel. */
const at = (v: number) => Math.round(v * 20) / 20;

/**
 * Draws the cord (`cord` px long), the cap and the ball turned by `angle`
 * (radians), all in `ink`, the ball's radius `radius`. Light is left blank, as
 * in an engraving: a tile catching light is hatched with two thin lines, then
 * one, a lit one is left out. All tiles go into one path, built as SVG path
 * text and filled once: a canvas call per corner would cost four times as much.
 */
const draw = (
  context: CanvasRenderingContext2D,
  width: number,
  height: number,
  radius: number,
  cord: number,
  angle: number,
  ink: string,
) => {
  const cx = width / 2;
  const cy = cord + CAP_HEIGHT + radius * COS_TILT;
  context.clearRect(0, 0, width, height);
  context.fillStyle = ink;
  context.strokeStyle = ink;
  context.lineWidth = 1.5;
  if (cord > 0) {
    context.beginPath();
    context.moveTo(cx, 0);
    context.lineTo(cx, cord);
    context.stroke();
  }
  context.beginPath();
  context.roundRect(cx - CAP_WIDTH / 2, cord, CAP_WIDTH, CAP_HEIGHT + 3, 2);
  context.fill();

  let path = "";
  // A point on the ball, tilted, onto the canvas: a corner of the path.
  const corner = (command: "M" | "L", lat: number, lon: number) => {
    const c = Math.cos(lat);
    const x = cx + radius * c * Math.sin(lon);
    const y =
      cy - radius * (Math.sin(lat) * COS_TILT - c * Math.cos(lon) * SIN_TILT);
    path += `${command}${at(x)} ${at(y)}`;
  };
  // A quad on the ball between latitudes `top`/`bottom`, longitudes `left`/`right`,
  // its corners off by `rough` (in `w` and `h`: the tile's width, and its or its
  // hatch line's height, so thin lines stay lines).
  const quad = (
    top: number,
    bottom: number,
    left: number,
    right: number,
    rough: number[],
    w: number,
    h: number,
  ) => {
    corner("M", top + rough[1] * h, left + rough[0] * w);
    corner("L", top + rough[2] * h, right + rough[1] * w);
    corner("L", bottom + rough[3] * h, right + rough[2] * w);
    corner("L", bottom + rough[0] * h, left + rough[3] * w);
    path += "Z";
  };

  for (const { lat, height: h, tiles, quirks } of BAND_TILES) {
    const w = (2 * Math.PI) / tiles;
    const cosLat = Math.cos(lat);
    const sinLat = Math.sin(lat);
    for (let j = 0; j < tiles; j++) {
      const lon = angle + (j + 0.5) * w;
      // Whether it faces us at all (its centre's depth, tilted).
      if (sinLat * SIN_TILT + cosLat * Math.cos(lon) * COS_TILT <= 0) continue;

      // Its mirror's normal, a little off true, and the view (0, 0, 1) reflected in it.
      const { dLat, dLon, grain, rough } = quirks[j];
      const c = Math.cos(lat + dLat);
      const ny0 = Math.sin(lat + dLat);
      const nz0 = c * Math.cos(lon + dLon);
      const nx = c * Math.sin(lon + dLon);
      const ny = ny0 * COS_TILT - nz0 * SIN_TILT;
      const nz = ny0 * SIN_TILT + nz0 * COS_TILT;
      const light = room(2 * nz * nx, 2 * nz * ny, 2 * nz * nz - 1) + grain;
      if (light >= BLANK) continue;

      const ink = Math.min(1, 1 - Math.max(0, light) / BLANK);
      const full = (1 - GROUT) / 2;
      if (ink >= SOLID) {
        quad(
          lat + full * h,
          lat - full * h,
          lon - full * w,
          lon + full * w,
          rough,
          w,
          h,
        );
        continue;
      }
      // Hatching: thin lines along the band, thinner and shorter the more light.
      const halfWidth = full * w * (0.55 + 0.45 * ink);
      if (ink >= TWO_LINES) {
        const line = full * h * ink * 0.5;
        const gap = full * h * 0.5;
        quad(
          lat + gap + line,
          lat + gap - line,
          lon - halfWidth,
          lon + halfWidth,
          rough,
          w,
          2 * line,
        );
        quad(
          lat - gap + line,
          lat - gap - line,
          lon - halfWidth,
          lon + halfWidth,
          rough,
          w,
          2 * line,
        );
      } else {
        const line = full * h * ink * 0.8;
        quad(
          lat + line,
          lat - line,
          lon - halfWidth,
          lon + halfWidth,
          rough,
          w,
          2 * line,
        );
      }
    }
  }
  context.fill(new Path2D(path));
};

const darkQuery = window.matchMedia("(prefers-color-scheme: dark)");
const subscribeDark = (onChange: () => void) => {
  darkQuery.addEventListener("change", onChange);
  return () => darkQuery.removeEventListener("change", onChange);
};
const prefersDark = () => darkQuery.matches;

const viewport = () => ({
  width: window.innerWidth,
  height: window.innerHeight,
  ratio: window.devicePixelRatio || 1,
});

/**
 * The window's size in CSS px and its device pixels per CSS px, kept up to
 * date. The ratio changes when the window moves to a display with another
 * scale, or the page is zoomed: browsers may tell with a resize, or only
 * through a media query on the current ratio, which stops matching.
 */
const useViewport = () => {
  const [state, setState] = useState(viewport);
  useEffect(() => {
    const update = () =>
      setState((old) => {
        const now = viewport();
        return now.width === old.width &&
          now.height === old.height &&
          now.ratio === old.ratio
          ? old
          : now;
      });
    const query = window.matchMedia(`(resolution: ${state.ratio}dppx)`);
    window.addEventListener("resize", update);
    query.addEventListener("change", update);
    return () => {
      window.removeEventListener("resize", update);
      query.removeEventListener("change", update);
    };
  }, [state.ratio]);
  return state;
};

/**
 * How far below the window's top the canvas starts (App.css: under the
 * desktop app's title bar), in CSS px; read again when the root element's
 * `data-title-bar` changes (it's `hidden` in full screen).
 */
const useTopInset = (canvasRef: RefObject<HTMLCanvasElement | null>) => {
  const [inset, setInset] = useState(0);
  useLayoutEffect(() => {
    const read = () => {
      const canvas = canvasRef.current;
      if (canvas) setInset(parseFloat(getComputedStyle(canvas).top) || 0);
    };
    read();
    const observer = new MutationObserver(read);
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-title-bar"],
    });
    return () => observer.disconnect();
  }, [canvasRef]);
  return inset;
};

/**
 * At most this many redraws a second while it turns: a 120 Hz display needn't
 * take twice the work. Each costs about 1.5ms of main thread; the compositor
 * also blurs the frosted controls over it again (their `backdrop-filter`).
 */
const MAX_FPS = 60;

/**
 * A mirror ball, engraved in the theme's text colour, hanging on a cord from
 * the top of the window, behind the UI, turning at `speed` revolutions per
 * minute. Drawn on a canvas at most `MAX_FPS` times a second while it turns
 * (one path of a thousand-odd tiles, filled once); once when it's still, or
 * for those who prefer reduced motion. However long the cord and large the
 * ball, it's drawn to fit in the window (or not at all, where it's too short):
 * the settings keep what was asked for, for when the window grows again.
 */
function DiscoBall({
  speed,
  size,
  cord: cordSetting,
  appearance,
  behind,
}: {
  speed: number;
  /** Its diameter, in % of the window's shorter side. */
  size: number;
  /** How far below the window's top it hangs, in px. */
  cord: number;
  /** The app's appearance setting: when it changes, the theme's text colour does. */
  appearance: AppearanceType;
  /**
   * What it hangs behind: a grid's rows, or content centred in the window. Its
   * fine pattern, in the text's colour, is hard to read text past, so it's
   * fainter behind a grid, and behind centred content where it reaches it.
   */
  behind: "grid" | "centred";
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  // Kept across speed changes, so a new speed doesn't make the ball jump.
  const angle = useRef(0);
  const still = usePrefersReducedMotion();
  const systemDark = useSyncExternalStore(subscribeDark, prefersDark);
  const { width: windowWidth, height: windowHeight, ratio } = useViewport();
  const inset = useTopInset(canvasRef);

  // What the window leaves for the cord, cap and ball (with room around the
  // ball for its antialiased edge).
  const space = windowHeight - inset - CAP_HEIGHT - 4;
  const radius = Math.min(
    Math.max(
      MIN_RADIUS,
      Math.round((Math.min(windowWidth, windowHeight) * size) / 200),
    ),
    Math.floor(space / 2),
  );
  const isDrawn = radius >= SMALLEST_RADIUS;
  const cord = Math.max(0, Math.min(cordSetting, space - 2 * radius));
  const width = isDrawn ? 2 * radius + 4 : 0;
  const height = isDrawn ? cord + CAP_HEIGHT + 2 * radius + 4 : 0;
  const isSubdued =
    behind === "grid" ||
    inset + height > windowHeight / 2 - CENTRED_CONTENT_REACH;

  useEffect(() => {
    const canvas = canvasRef.current;
    const context = canvas?.getContext("2d");
    if (!canvas || !context || !isDrawn) return;
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(height * ratio);
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    // The theme's text colour (App.css), as it is for `appearance` and `systemDark`.
    const ink = getComputedStyle(canvas).color;
    const render = () =>
      draw(context, width, height, radius, cord, angle.current, ink);
    render();
    if (still || speed <= 0) return;
    const perMs = (speed * 2 * Math.PI) / 60_000;
    const frameMs = 1000 / MAX_FPS;
    let last: number | undefined;
    // When the next redraw is due. Paced against this schedule, not the last
    // redraw, so refresh rates that aren't a multiple of MAX_FPS (75, 90, 144 Hz)
    // still get close to it rather than every other frame; after a gap (a hidden
    // tab) it starts over.
    let due = 0;
    let frame = requestAnimationFrame(function tick(now) {
      frame = requestAnimationFrame(tick);
      // Frames come every 1000 / refresh rate ms, not quite evenly: allow for that.
      if (last !== undefined && now < due - 2) return;
      due =
        last === undefined || now - due > frameMs
          ? now + frameMs
          : due + frameMs;
      // Hidden tabs get no frames; don't make up for them in one leap.
      const elapsed = last === undefined ? 0 : Math.min(now - last, 100);
      last = now;
      angle.current = (angle.current + elapsed * perMs) % (2 * Math.PI);
      render();
    });
    return () => cancelAnimationFrame(frame);
  }, [
    isDrawn,
    speed,
    still,
    radius,
    cord,
    width,
    height,
    ratio,
    appearance,
    systemDark,
  ]);

  return (
    <canvas
      ref={canvasRef}
      className={isSubdued ? "disco-ball disco-ball-subdued" : "disco-ball"}
      aria-hidden
      // Kept in the page while hidden, so its offset from the window's top can still be read.
      hidden={!isDrawn}
      style={{ width, height }}
    />
  );
}

export default DiscoBall;
