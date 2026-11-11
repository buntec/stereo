import { useSyncExternalStore } from "react";
import type { AudioDownload, ServerMsg } from "./Types.tsx";

/** What the client knows about a track's audio on the backend. */
export type TrackAudio = {
  /** The audio is downloaded: it plays right away and has a waveform. */
  cached: boolean;
  /** An unfinished or failed download. */
  download?: AudioDownload;
};

const UNKNOWN: TrackAudio = { cached: false };
const CACHED: TrackAudio = { cached: true };

/**
 * Which tracks' audio the backend has downloaded, and the downloads it is
 * working on, from its `audio-*` messages. Kept outside the app's reducer so a
 * download's progress updates re-render only what shows that track (e.g. its
 * grid row's icon), not the whole app.
 */
class AudioCacheStore {
  private tracks = new Map<string, TrackAudio>();
  private listeners = new Set<() => void>();

  get = (id: string | undefined): TrackAudio =>
    (id && this.tracks.get(id)) || UNKNOWN;

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  private set(id: string, audio: TrackAudio) {
    if (audio === UNKNOWN) this.tracks.delete(id);
    else this.tracks.set(id, audio);
  }

  private changed() {
    this.listeners.forEach((listener) => listener());
  }

  /** A track that plays is downloaded, even if its final update got lost. */
  markCached(id: string) {
    if (this.get(id) === CACHED) return;
    this.set(id, CACHED);
    this.changed();
  }

  /** Updates from a server message; other messages are ignored. */
  handle(msg: ServerMsg) {
    switch (msg.type) {
      case "backend-info":
        // A new connection: the backend sends what is cached next (unless it
        // has no audio downloads at all).
        this.tracks.clear();
        break;
      case "audio-cached":
        this.tracks.clear();
        for (const id of msg.yt_ids) this.tracks.set(id, CACHED);
        break;
      case "audio-evicted":
        for (const id of msg.yt_ids) this.tracks.delete(id);
        break;
      case "audio-download": {
        const { yt_id, status } = msg;
        if (status === "done") {
          this.set(yt_id, CACHED);
        } else {
          this.set(yt_id, {
            cached: false,
            download: {
              status,
              progress: msg.progress ?? undefined,
              error: msg.error ?? undefined,
            },
          });
        }
        break;
      }
      default:
        return;
    }
    this.changed();
  }
}

export const audioCache = new AudioCacheStore();

/** The audio state of track `id`, re-rendering when it changes. */
export const useTrackAudio = (id: string | undefined): TrackAudio =>
  useSyncExternalStore(audioCache.subscribe, () => audioCache.get(id));

/** Whether the download is still running (not failed). */
export const isDownloading = (download: AudioDownload | undefined) =>
  download !== undefined &&
  download.status !== "failed" &&
  download.status !== "done";
