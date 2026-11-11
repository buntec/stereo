import { useEffect, useMemo, useRef, useState } from "react";
import type {
  PlaybackEngine,
  PlaybackEvents,
  PlaybackStatus,
} from "./Playback.ts";

/** After this many tracks in a row fail to download, stop instead of skipping on. */
const MAX_CONSECUTIVE_FAILURES = 3;

const audioUrl = (id: string, key: string) =>
  `/audio/${encodeURIComponent(id)}?key=${encodeURIComponent(key)}`;

/**
 * Plays tracks from audio the backend downloads (`/audio/{id}`) in a single
 * `<audio>` element, which keeps playing in the background and with the
 * screen locked. A track whose audio can't be downloaded is skipped.
 *
 * `null` until the backend has handed out its audio key; without one (yt-dlp
 * isn't available) nothing can be played.
 */
export const useAudioPlayback = (
  audioKey: string | undefined,
  events: PlaybackEvents,
): PlaybackEngine | null => {
  const eventsRef = useRef<PlaybackEvents>(events);
  const keyRef = useRef(audioKey);
  /** The track the app asked for. */
  const idRef = useRef<string | undefined>(undefined);
  const prefetchedRef = useRef(new Set<string>());
  const failuresRef = useRef(0);
  const loadRef = useRef<(id: string) => void>(() => {});

  // One element for the whole session: iOS lets an element that is already
  // playing switch tracks while locked, but won't start a new one.
  const [audio] = useState(() => {
    const element = new Audio();
    element.preload = "auto";
    return element;
  });

  useEffect(() => {
    eventsRef.current = events;
  });

  useEffect(() => {
    if (keyRef.current !== audioKey) {
      keyRef.current = audioKey;
      // The backend restarted with a new key.
      prefetchedRef.current.clear();
    }
  }, [audioKey]);

  const emit = (status: PlaybackStatus, id = idRef.current) => {
    if (id) eventsRef.current.onStatusChange?.(status, id);
  };

  const advance = () => {
    const next = eventsRef.current.nextId?.();
    if (next) loadRef.current(next);
  };

  const playAudio = () => {
    audio.play().catch((e: unknown) => {
      // NotAllowedError: autoplay was blocked, the user has to press play.
      // AbortError: the source changed before playback started.
      if (e instanceof DOMException && e.name === "NotAllowedError") {
        emit("paused");
      }
    });
  };

  const load = (id: string) => {
    idRef.current = id;
    const key = keyRef.current;
    if (!key) return;
    audio.src = audioUrl(id, key);
    // Possibly a download first; don't rely on the element firing `waiting`.
    emit("buffering");
    playAudio();
  };
  useEffect(() => {
    loadRef.current = load;
  });

  // Audio element events. Registered once; everything they read is in refs.
  useEffect(() => {
    const onPlaying = () => {
      failuresRef.current = 0;
      emit("playing");
    };
    // `pause` also fires right before `ended`.
    const onPause = () => !audio.ended && emit("paused");
    const onWaiting = () => emit("buffering");
    const onEnded = () => {
      const id = idRef.current;
      // Start the next track right here: once this handler returns, a page in
      // the background may not get to run any code until it is woken up again.
      advance();
      emit("ended", id);
    };
    const onError = () => {
      const id = idRef.current;
      const key = keyRef.current;
      if (!id || !key || !audio.hasAttribute("src")) return;
      failuresRef.current += 1;
      const failures = failuresRef.current;
      const stop = failures >= MAX_CONSECUTIVE_FAILURES;
      // The element doesn't expose why the request failed; the backend's
      // response does (it remembers failed downloads, so this is quick).
      fetch(audioUrl(id, key))
        .then(async (res) => {
          const reason = res.ok ? undefined : (await res.text()).trim();
          await res.body?.cancel().catch(() => {});
          return reason;
        })
        .catch(() => undefined)
        .then((reason) => {
          const what = reason ? `: ${reason}` : "";
          eventsRef.current.onError?.(
            stop
              ? `Couldn't download the audio of ${failures} tracks in a row, stopping${what}`
              : `Couldn't download this track's audio, skipping it${what}`,
          );
        });
      if (stop) {
        failuresRef.current = 0;
        emit("paused", id);
      } else {
        advance();
        emit("ended", id);
      }
    };
    audio.addEventListener("playing", onPlaying);
    audio.addEventListener("pause", onPause);
    audio.addEventListener("waiting", onWaiting);
    audio.addEventListener("ended", onEnded);
    audio.addEventListener("error", onError);
    return () => {
      audio.removeEventListener("playing", onPlaying);
      audio.removeEventListener("pause", onPause);
      audio.removeEventListener("waiting", onWaiting);
      audio.removeEventListener("ended", onEnded);
      audio.removeEventListener("error", onError);
    };
  }, [audio]);

  useEffect(
    () => () => {
      audio.pause();
      audio.removeAttribute("src");
      audio.load();
    },
    [audio],
  );

  const ready = audioKey !== undefined;
  return useMemo(() => {
    if (!ready) return null;
    return {
      load: (id) => loadRef.current(id),
      play: () => playAudio(),
      pause: () => audio.pause(),
      stop: () => {
        audio.pause();
        audio.currentTime = 0;
      },
      seek: (seconds) => {
        audio.currentTime = seconds;
      },
      currentTime: () => audio.currentTime,
      duration: () => (Number.isFinite(audio.duration) ? audio.duration : 0),
      status: () => {
        if (!audio.hasAttribute("src")) return "unstarted";
        if (audio.ended) return "ended";
        if (audio.paused) return "paused";
        if (audio.readyState < HTMLMediaElement.HAVE_FUTURE_DATA)
          return "buffering";
        return "playing";
      },
      currentId: () => idRef.current,
      prefetch: (ids) => {
        const key = keyRef.current;
        if (!key) return;
        for (const id of ids) {
          if (prefetchedRef.current.has(id)) continue;
          prefetchedRef.current.add(id);
          // HEAD makes the backend download the track without sending it.
          fetch(audioUrl(id, key), { method: "HEAD" }).then(
            (res) => {
              // Retry a later prefetch unless the download itself failed.
              if (!res.ok && res.status !== 502)
                prefetchedRef.current.delete(id);
            },
            () => prefetchedRef.current.delete(id),
          );
        }
      },
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ready, audio]);
};
