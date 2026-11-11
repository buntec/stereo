import { useEffect, useState } from "react";
import type { RequestReply } from "./Types.tsx";

const waveforms = new Map<string, number[] | null>();
/** Waveforms kept in memory, for going back and forth between tracks. */
const MAX_WAVEFORMS = 50;

/**
 * The waveform of track `id` once its audio is downloaded: `undefined` while
 * it is unknown or loading, `null` if the backend has none (it can't decode
 * the audio).
 */
export const useWaveform = (
  id: string | undefined,
  cached: boolean,
  requestReply: RequestReply,
): number[] | null | undefined => {
  const [loaded, setLoaded] = useState<{
    id: string;
    peaks: number[] | null;
  }>();

  useEffect(() => {
    if (!id || !cached || waveforms.has(id)) return;
    let cancelled = false;
    requestReply(
      { type: "get-waveform", yt_id: id },
      (msg) => {
        if (msg.type !== "waveform" || !("peaks" in msg)) {
          // Timed out or failed: try again next time.
          if (!cancelled) setLoaded({ id, peaks: null });
          return;
        }
        const peaks = msg.peaks?.length ? msg.peaks : null;
        waveforms.delete(id);
        waveforms.set(id, peaks);
        if (waveforms.size > MAX_WAVEFORMS) {
          waveforms.delete(waveforms.keys().next().value!);
        }
        if (!cancelled) setLoaded({ id, peaks });
      },
      30000,
    );
    return () => {
      cancelled = true;
    };
  }, [id, cached, requestReply]);

  if (!id) return undefined;
  if (waveforms.has(id)) return waveforms.get(id);
  return loaded?.id === id ? loaded.peaks : undefined;
};
