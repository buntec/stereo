import { useEffect, useState } from "react";
import type { RequestReply, ServerMsg } from "./Types.tsx";

const waveforms = new Map<string, number[] | null>();
/** Waveforms kept in memory, for going back and forth between tracks. */
const MAX_WAVEFORMS = 50;

/**
 * Forgets that tracks have no waveform once the backend reports their audio as
 * downloaded: it also replies with none for audio it doesn't have (e.g. evicted
 * just before the request), which a later download fixes. Called with every
 * server message, before the audio cache store sees it.
 */
export const forgetMissingWaveforms = (msg: ServerMsg) => {
  const forget = (id: string) => {
    if (waveforms.get(id) === null) waveforms.delete(id);
  };
  if (msg.type === "audio-download" && msg.status === "done") forget(msg.yt_id);
  else if (msg.type === "audio-cached") [...waveforms.keys()].forEach(forget);
};

/**
 * The waveform of track `id` once its audio is downloaded: `undefined` while
 * it is unknown or loading, `null` if the backend has none (it can't decode
 * the audio, or didn't have it).
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
