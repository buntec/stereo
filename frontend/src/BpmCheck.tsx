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
 * its audio deviates from its metadata, or to fill it in when it has none:
 * once per track and session, for tracks in the open collection, and never
 * again for a suggestion the user dismissed.
 */
export default function BpmCheck({
  track,
  tempo,
  isPlaying,
  requestReply,
  sendMsg,
}: {
  /** The playing track. */
  track?: ITrack;
  /** Its measured tempo. */
  tempo?: Tempo;
  isPlaying: boolean;
  requestReply: RequestReply;
  sendMsg: (msg: ClientMsg) => void;
}) {
  const toast = useToast();
  /** Tracks checked this session. */
  const checked = useRef(new Set<string>());

  const ytId = track?.yt_id;
  const title = track?.title;
  const bpm = track?.bpm;
  const suggestion = suggestedBpm(tempo, bpm);
  const measured = tempo && formatTempo(tempo, bpm);

  useEffect(() => {
    if (!isPlaying || !ytId || suggestion === undefined || !measured) return;
    if (checked.current.has(ytId)) return;
    const key = `${ytId}:${suggestion}`;
    if (readDismissed().includes(key)) return;
    checked.current.add(ytId);
    // Only a track in the open collection has metadata to update.
    requestReply({ type: "collection-contains-ids", yt_ids: [ytId] }, (msg) => {
      if (!("contained" in msg) || !msg.contained.includes(ytId)) return;
      let updated = false;
      const hide = toast({
        body: bpm
          ? `${title} plays at ${measured}, but its metadata says ${bpm} BPM.`
          : `${title} plays at ${measured}; its metadata has no BPM.`,
        endContent: (
          <Button
            label={`Set to ${suggestion}`}
            size="sm"
            variant="secondary"
            onClick={() => {
              updated = true;
              sendMsg({ type: "update-bpm", yt_id: ytId, bpm: suggestion });
              hide();
            }}
          />
        ),
        uniqueID: `bpm:${ytId}`,
        isAutoHide: true,
        autoHideDuration: TOAST_DURATION,
        onHide: (reason) => {
          if (reason === "manual" && !updated) dismiss(key);
        },
      });
    });
  }, [
    isPlaying,
    ytId,
    title,
    bpm,
    suggestion,
    measured,
    requestReply,
    sendMsg,
    toast,
  ]);

  return null;
}
