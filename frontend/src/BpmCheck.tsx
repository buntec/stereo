import { useEffect, useRef } from "react";
import { useToast } from "@astryxdesign/core/Toast";
import { Button } from "@astryxdesign/core/Button";
import type { ClientMsg, ITrack, RequestReply, Tempo } from "./Types.tsx";
import { formatTempo, suggestedBpm } from "./tempo.ts";

/** Suggestions the user dismissed, as `{yt_id}:{bpm}`, so they aren't offered again. */
const DISMISSED_KEY = "stereo-bpm-dismissed";
const MAX_DISMISSED = 1000;
/** Long enough to read the toast and decide. */
const TOAST_DURATION = 15000;

const readDismissed = (): string[] => {
  try {
    const value = JSON.parse(
      window.localStorage.getItem(DISMISSED_KEY) ?? "[]",
    );
    return Array.isArray(value) ? value : [];
  } catch {
    return [];
  }
};

const dismiss = (suggestion: string) => {
  try {
    const dismissed = readDismissed().filter((item) => item !== suggestion);
    dismissed.push(suggestion);
    window.localStorage.setItem(
      DISMISSED_KEY,
      JSON.stringify(dismissed.slice(-MAX_DISMISSED)),
    );
  } catch {
    // Without storage, the suggestion is offered again next session.
  }
};

/**
 * Offers to correct the BPM of the playing track when the tempo measured from
 * its audio deviates from the BPM stored in the open collection, or to fill it
 * in when it has none: once per track and session, and never again for a
 * suggestion the user dismissed. A track that isn't in the collection (yet) is
 * checked again when the collection changes.
 */
export default function BpmCheck({
  track,
  tempo,
  isPlaying,
  collection,
  collectionVersion,
  requestReply,
  sendMsg,
}: {
  /** The playing track. */
  track?: ITrack;
  /** Its measured tempo. */
  tempo?: Tempo;
  isPlaying: boolean;
  /** The open collection's path. */
  collection?: string;
  /** Changes whenever the open collection's tracks may have. */
  collectionVersion?: number;
  requestReply: RequestReply;
  sendMsg: (msg: ClientMsg) => void;
}) {
  const toast = useToast();
  /** Tracks offered a suggestion this session. */
  const offered = useRef(new Set<string>());
  /** The suggestion on display, which only applies to the collection it was checked against. */
  const shown = useRef<{
    ytId: string;
    collection: string;
    withdraw: () => void;
  }>(undefined);
  const openCollection = useRef(collection);

  // A suggestion for another collection is withdrawn, to be offered again there.
  useEffect(() => {
    openCollection.current = collection;
    if (shown.current && shown.current.collection !== collection) {
      offered.current.delete(shown.current.ytId);
      shown.current.withdraw();
    }
  }, [collection]);

  const ytId = track?.yt_id;
  const bpm = tempo?.bpm;
  const steady = tempo?.steady;

  useEffect(() => {
    if (!isPlaying || !ytId || !collection || bpm === undefined || !steady)
      return;
    if (offered.current.has(ytId)) return;
    const tempo = { bpm, steady };
    let cancelled = false;
    // Compared with the metadata stored in the collection, which the playing
    // track's (e.g. a search result's) may differ from.
    requestReply(
      { type: "get-collection-track", yt_id: ytId, collection },
      (msg) => {
        if (cancelled || msg.type !== "collection-track" || !("track" in msg))
          return;
        const stored = msg.track;
        if (!stored || offered.current.has(ytId)) return;
        const suggestion = suggestedBpm(tempo, stored.bpm);
        if (suggestion === undefined) return;
        const key = `${ytId}:${suggestion}`;
        if (readDismissed().includes(key)) return;
        offered.current.add(ytId);
        const measured = formatTempo(tempo, stored.bpm);
        /** Whether the toast went away other than by the user dismissing it. */
        let settled = false;
        const hide = toast({
          body: stored.bpm
            ? `${stored.title} plays at ${measured}, but its metadata says ${stored.bpm} BPM.`
            : `${stored.title} plays at ${measured}; its metadata has no BPM.`,
          endContent: (
            <Button
              label={`Set to ${suggestion}`}
              size="sm"
              variant="secondary"
              onClick={() => {
                settled = true;
                if (openCollection.current === collection)
                  sendMsg({
                    type: "update-bpm",
                    yt_id: ytId,
                    bpm: suggestion,
                    collection,
                  });
                hide();
              }}
            />
          ),
          uniqueID: `bpm:${ytId}`,
          isAutoHide: true,
          autoHideDuration: TOAST_DURATION,
          onHide: (reason) => {
            if (shown.current?.withdraw === withdraw) shown.current = undefined;
            if (reason === "manual" && !settled) dismiss(key);
          },
        });
        const withdraw = () => {
          settled = true;
          hide();
        };
        shown.current = { ytId, collection, withdraw };
      },
    );
    return () => {
      cancelled = true;
    };
  }, [
    isPlaying,
    ytId,
    bpm,
    steady,
    collection,
    collectionVersion,
    requestReply,
    sendMsg,
    toast,
  ]);

  return null;
}
