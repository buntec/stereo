import type { Tempo } from "./Types.tsx";

/** How far the measured BPM may be from the metadata's before it's worth correcting. */
export const BPM_TOLERANCE = 0.5;
/**
 * How close half or double the measured tempo must be to the metadata's to be
 * taken as its octave (as a fraction): the beat tracker can't tell the beat
 * from half or double time, e.g. drum & bass at 174 BPM measured as 87.
 */
const OCTAVE_TOLERANCE = 0.04;

/**
 * The measured tempo in BPM, at the octave of the metadata's `bpm` if half or
 * double of it is close to that.
 */
export const measuredBpm = (tempo: Tempo, bpm?: number | null): number => {
  if (!bpm) return tempo.bpm;
  const near = (value: number) => Math.abs(value / bpm - 1) <= OCTAVE_TOLERANCE;
  for (const factor of [2, 0.5]) {
    if (!near(tempo.bpm) && near(tempo.bpm * factor)) {
      return tempo.bpm * factor;
    }
  }
  return tempo.bpm;
};

/** The measured tempo for display, e.g. "124 BPM", "126.4 BPM" or "~128 BPM". */
export const formatTempo = (tempo: Tempo, bpm?: number | null): string => {
  const value = measuredBpm(tempo, bpm);
  if (!tempo.steady) return `~${Math.round(value)} BPM`;
  const rounded = Math.round(value);
  return Math.abs(value - rounded) < 0.05
    ? `${rounded} BPM`
    : `${value.toFixed(1)} BPM`;
};

/**
 * The BPM to suggest for a track with the metadata's `bpm`, from its measured
 * tempo: when that tempo is steady and the metadata has none or deviates by
 * more than {@link BPM_TOLERANCE}.
 */
export const suggestedBpm = (
  tempo: Tempo | undefined,
  bpm?: number | null,
): number | undefined => {
  if (!tempo?.steady) return undefined;
  const value = measuredBpm(tempo, bpm);
  const rounded = Math.round(value);
  if (rounded <= 0) return undefined;
  if (!bpm) return rounded;
  return Math.abs(value - bpm) > BPM_TOLERANCE && rounded !== bpm
    ? rounded
    : undefined;
};
