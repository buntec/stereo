import { useEffect, useSyncExternalStore, type RefObject } from "react";
import type { PlaybackEngine } from "./Playback.ts";
import type { Beat } from "./Types.tsx";

/** How quickly a beat's pulse fades: its time constant in milliseconds. */
const DECAY_MS = 120;

const reducedMotion = "(prefers-reduced-motion: reduce)";
const subscribeReducedMotion = (onChange: () => void) => {
  const query = window.matchMedia(reducedMotion);
  query.addEventListener("change", onChange);
  return () => query.removeEventListener("change", onChange);
};
const prefersReducedMotion = () => window.matchMedia(reducedMotion).matches;

/** The index of the last beat at or before `ms`, or -1 before the first. */
export const lastBeatIndex = (beats: Beat[], ms: number): number => {
  let low = 0;
  let high = beats.length;
  while (low < high) {
    const mid = (low + high) >> 1;
    if (beats[mid][0] <= ms) low = mid + 1;
    else high = mid;
  }
  return low - 1;
};

/** The pulse (0–1) at `ms`: the last beat's strength, fading until the next. */
export const beatPulse = (beats: Beat[], ms: number): number => {
  const beat = beats[lastBeatIndex(beats, ms)];
  if (!beat) return 0;
  return (beat[1] / 255) * Math.exp(-(ms - beat[0]) / DECAY_MS);
};

/**
 * Pulses the element of `ref` with the beats of the playing track `id`: sets
 * its `--beat` CSS property to the pulse (0–1) on every animation frame, from
 * the playback position. Nothing unless `active` (playing, and the element is
 * shown: the effect starts when `active` turns true, so an element that mounts
 * later must turn it true with it), in the background (no animation frames) or
 * for those who prefer reduced motion.
 */
export function useBeatPulse(
  ref: RefObject<Element | null>,
  playback: PlaybackEngine | null,
  id: string | undefined,
  beats: Beat[] | undefined,
  active: boolean,
) {
  const still = useSyncExternalStore(
    subscribeReducedMotion,
    prefersReducedMotion,
  );
  useEffect(() => {
    const element = ref.current;
    if (!(element instanceof HTMLElement || element instanceof SVGElement))
      return;
    if (!playback || !id || !beats?.length || !active || still) return;
    let frame = requestAnimationFrame(function tick() {
      // The beats are those of `id`; the engine may already be loading another track.
      const pulse =
        playback.currentId() === id
          ? beatPulse(beats, playback.currentTime() * 1000)
          : 0;
      element.style.setProperty("--beat", pulse.toFixed(3));
      frame = requestAnimationFrame(tick);
    });
    return () => {
      cancelAnimationFrame(frame);
      element.style.removeProperty("--beat");
    };
  }, [ref, playback, id, beats, active, still]);
}
