import { useSyncExternalStore } from "react";

/** A pause this long between taps starts a new count. */
export const TAP_RESET_MS = 2000;
/**
 * Taps closer together than this (300 BPM) are ignored: a held key's
 * auto-repeat, or a pointer and its click counted twice.
 */
const MIN_INTERVAL_MS = 200;
/** Only the latest taps count, so a long run follows a tempo change. */
const MAX_TAPS = 64;
/** From this many taps on, the BPM is shown with a decimal. */
export const PRECISE_TAPS = 9;

export type TapTempo = {
  /** The tapped tempo, from two taps on. */
  bpm?: number;
  /** Taps in the current count. */
  taps: number;
  /** When the latest tap was, on the `performance.now()` clock. */
  last?: number;
};

const EMPTY: TapTempo = { taps: 0 };

/**
 * The tempo of taps at `times` (ms, ascending): the slope of a least-squares
 * line through them, which weighs every tap rather than just the first and
 * last, so one early or late tap moves it little.
 */
export const tapBpm = (times: number[]): number | undefined => {
  const n = times.length;
  if (n < 2) return undefined;
  const meanIndex = (n - 1) / 2;
  const meanTime = times.reduce((sum, t) => sum + t, 0) / n;
  let numerator = 0;
  let denominator = 0;
  times.forEach((t, i) => {
    numerator += (i - meanIndex) * (t - meanTime);
    denominator += (i - meanIndex) ** 2;
  });
  const interval = numerator / denominator;
  return interval > 0 ? 60000 / interval : undefined;
};

/** The tapped BPM for display, e.g. "128" or, after enough taps, "127.8". */
export const formatTapBpm = (tapped: TapTempo): string | undefined =>
  tapped.bpm === undefined
    ? undefined
    : tapped.taps >= PRECISE_TAPS
      ? tapped.bpm.toFixed(1)
      : String(Math.round(tapped.bpm));

/**
 * The tempo the user taps along with the music (`TapTempo.tsx`), to check the
 * measured and the metadata's BPM against. Kept outside the app's reducer so a
 * tap re-renders only what shows it, not the whole app (see `AudioCache.ts`).
 */
class TapTempoStore {
  private times: number[] = [];
  private state: TapTempo = EMPTY;
  private listeners = new Set<() => void>();

  get = (): TapTempo => this.state;

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  /** A tap at `time` (`performance.now()` or an event's `timeStamp`). */
  tap = (time: number = performance.now()) => {
    const last = this.times.at(-1);
    if (last !== undefined) {
      const gap = time - last;
      if (gap < MIN_INTERVAL_MS && gap >= 0) return;
      if (gap > TAP_RESET_MS || gap < 0) this.times = [];
    }
    this.times.push(time);
    if (this.times.length > MAX_TAPS) this.times.shift();
    this.state = {
      bpm: tapBpm(this.times),
      taps: this.times.length,
      last: time,
    };
    this.changed();
  };

  /** Forgets the taps, e.g. for another track. */
  reset = () => {
    if (this.state === EMPTY) return;
    this.times = [];
    this.state = EMPTY;
    this.changed();
  };

  private changed() {
    this.listeners.forEach((listener) => listener());
  }
}

export const tapTempo = new TapTempoStore();

export const useTapTempo = (): TapTempo =>
  useSyncExternalStore(tapTempo.subscribe, tapTempo.get);
