/**
 * Source-agnostic playback interface: the app drives playback only through a
 * `PlaybackEngine`, implemented by `AudioPlayback.ts` (audio downloaded by the
 * backend, played in an `<audio>` element).
 *
 * Tracks are identified by their YouTube id.
 */

export type PlaybackStatus =
  "unstarted" | "buffering" | "playing" | "paused" | "ended";

export interface PlaybackEngine {
  /** Loads the track and starts playing it. */
  load(id: string): void;
  /**
   * Makes the track the current one without loading or playing it: `play`
   * starts it. Stops whatever was playing.
   */
  cue(id: string): void;
  /** Plays the current track, loading a cued one. */
  play(): void;
  pause(): void;
  stop(): void;
  /** Seeks to an absolute position in seconds. */
  seek(seconds: number): void;
  /** Playback position in seconds. */
  currentTime(): number;
  /** Duration of the loaded track in seconds; 0 when not known yet. */
  duration(): number;
  status(): PlaybackStatus;
  /** Id of the loaded track, if any. */
  currentId(): string | undefined;
  /** Hints that these tracks are likely to be played next. */
  prefetch(ids: string[]): void;
}

export interface PlaybackEvents {
  /** Called on every status change of the loaded track `id`. */
  onStatusChange?: (status: PlaybackStatus, id: string) => void;
  onError?: (message: string) => void;
  /**
   * The track to play after the current one ends. The engine starts it from
   * within the media element's `ended` event, which keeps a playlist going
   * while the page is in the background or the screen is locked; the app must
   * still advance its own state on `"ended"`.
   */
  nextId?: () => string | undefined;
}
