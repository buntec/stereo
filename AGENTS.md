# AGENTS.md

Stereo is a music curation app for browsers and desktop. A collection is a SQLite file
of track metadata. The Rust backend serves the React UI, coordinates search across metadata
providers and matches tracks to YouTube. The frontend plays audio from `/audio/{yt_id}`,
downloaded and cached by the backend with yt-dlp.

## Commands

Development requires Rust, Bun, just and Git.

```sh
bun install              # install frontend dependencies (also via just init)
just dev                 # run Vite and the debug backend
just dev-tailscale       # build and serve the app locally and over Tailscale
just build               # build frontend then backend release binary
just desktop-dev         # run the Tauri desktop app with a debug backend (desktop/README.md)
just desktop-build       # package the desktop app with a release backend
just desktop-check       # clippy + tests for the desktop launcher
just install             # build and install the standalone binary
just test                # cargo test
just lint                # cargo clippy
just format              # format both Rust crates and frontend/src
bun run build            # tsc -b and Vite production build
bun run lint             # ESLint
bunx playwright install chromium webkit   # once; skip chromium if an executable is configured
bun run test:ui           # Playwright tests
```

`just dev` serves the UI with hot reload at `http://localhost:5174`. Vite proxies
`/ws` and `/audio` to the backend on port 8055 via the recipe's `STEREO_PORT`
override. Restart the backend after Rust changes. The dev data directory defaults to
`~/.local/share/stereo-dev` and can be set with `STEREO_HOME`. With `just dev-tailscale`,
open `http://localhost:8055` and use the Tailscale button's pairing QR code.

Run a single Rust test with `cargo test <test_name>`. The external-service tests in
`tests/scraper.rs` are ignored by default. Run them with `cargo test --test scraper -- --ignored`.
Playwright starts Vite and mocks the WebSocket and YouTube APIs. Set
`PLAYWRIGHT_CHROMIUM_EXECUTABLE` to use an existing Chromium. Tests tagged `@webkit`
also run in WebKit, the desktop app's engine. Tag tests of behavior that differs there.

Release builds embed `dist/` with `rust-embed` and `axum-embed`. `build.rs` builds
the frontend with Bun if `dist/index.html` is missing. In `--dev` mode Vite serves
the frontend separately.

## Commits

Keep commit messages ultra concise. Omit `Co-authored-by` trailers.

## Architecture

### Backend (`src/`, crate `stereo-backend`, binary `stereo`)

`main.rs` wires the axum router. Each client uses one WebSocket at `/ws` for app
operations. HTTP serves the frontend, `/audio/{yt_id}` and `/info`. The desktop app
reads the name, version and feature flags from `/info`. The CLI provides `version`
and macOS launchd commands `install`, `uninstall` and `restart`.

- **`message.rs`** defines the protocol. See [Protocol sync](#protocol-sync) when changing it.
- **`server.rs`** owns per-connection `SessionState` in `handle_socket`, including the
  DB pool, active collection and repo, and in-flight tasks. `handle_msg` processes one
  message at a time. Spawn slow work and send an `InternalEvent` back to the session
  when it needs to update state. Handler errors produce a notification and, for requests
  with a `MsgClient::request_id`, an `ErrorReply`. `send_loop` drains up to 100
  available messages into a JSON array per frame.
- **`db.rs`** handles SQLite access via `sqlx`. Collections are single-table
  SQLite files. `get_rows`, `count_rows` and `get_row_index` implement pagination,
  sorting and filtering for ag-Grid's infinite row model. Protocol types
  `FilterModel` and `SortModelItem` mirror the grid's filter and sort shapes.
- **`scraper.rs`** — search backends. Beatport, SoundCloud, Volumo, Traxsource, Spotify (`spotify.rs`) and Apple Music (`apple_music.rs`) are queried in parallel and interleaved (`interleave`) into candidate `RawTrack`s with metadata (artist/label/bpm/key/release date), which are then matched against YouTube search results (`find_best_match`/`score_match`, resolved concurrently and de-duplicated by YouTube id in `stream_matched_tracks`) since actual playback comes from YouTube. A failing backend is reported to the client as a warning rather than silently yielding fewer results. Shazam provides "similar songs" searches (a collection track is found on Shazam through its Apple Music id, which Shazam maps to its own); `fixup_track` fills in missing metadata from Beatport/Traxsource/Volumo. Beatport is scraped via its embedded `__NEXT_DATA__` JSON (no official API); SoundCloud requires first fetching a client_id from SoundCloud's own JS bundle. `create_client()` uses `wreq` (a `reqwest`-like client with Chrome TLS fingerprint impersonation) specifically to get past Beatport's bot detection — a plain `reqwest` client gets blocked. Search results stream back to the client incrementally via `MsgServer::SearchResult`/`SearchComplete` over an mpsc channel, not as one batch response.
- **`git.rs`** manages repositories under `~/.local/share/stereo/gitrepos` by default.
  A repo may track a Git remote. `schedule_git_sync` in `server.rs` coalesces edits
  into a sync five minutes after the first change. Startup sync picks up uncommitted
  changes. The repo lock guards DB writes, commits and merges. A separate network
  lock serializes fetch and push, so background sync can drop the repo lock before
  waiting on the network. Both locks are process-local. `data_dir.rs` locks
  `stereo.lock` before any repo is opened and records the listening port in
  `stereo.port` for the desktop app.
- **`audio.rs`** — `GET`/`HEAD /audio/{yt_id}?key=…` downloads the track's audio with yt-dlp on first request and serves it from a size-limited cache with Range support; concurrent requests share one download, failures are remembered for a few minutes. Download progress (parsed from yt-dlp's `--progress-template` output) is broadcast to every WebSocket session as `MsgServer::AudioDownload`, with a snapshot of unfinished downloads on connect; while the playing track downloads, the footer's waveform shows it (filling up, or pulsing until the size is known). Without a runnable yt-dlp there's no audio key, and the client explains that playback needs it. The key is random per process and only sent to authenticated WebSocket clients in `BackendInfo`, since `<audio>` can't send the WS token; it also keeps other sites from triggering downloads. The cache lives in the OS cache dir (`Settings::audio_cache_dir`), not the data dir, which holds the synced git repos. Sessions also get the cached ids on connect (`AudioCached`) and evictions (`AudioEvicted`); `DownloadAudio` forces a download (retrying a remembered failure). `GetWaveform` returns a track's loudness overview, its beats (`beats.rs`: bass and high-band onsets, tempo by autocorrelation, beats by dynamic programming) and their tempo (`beats::tempo`, fitted over the whole track to a fraction of a BPM; computed from the cached beats on each request), decoded with Symphonia (`waveform.rs`, AAC/MP4 only; it skips the MP4 edit list's priming samples itself, which Symphonia doesn't) on first request and cached in the cache dir's `waveforms/`.
- **`settings.rs`** — CLI args via `clap`, also settable via env vars (`STEREO_HOME`, `STEREO_HOST`, `STEREO_PORT`, `STEREO_ALLOWED_ORIGINS`, `STEREO_WS_TOKEN`, `STEREO_CACHE_DIR`, `STEREO_AUDIO_CACHE_LIMIT_GB`, `STEREO_YT_DLP`, `STEREO_DEV`, `STEREO_TAILSCALE`). Binding to a non-loopback host or allowing a remote origin requires a WebSocket token.
- **`tailscale.rs`** — `--tailscale`: `main.rs` opens a second listener on an ephemeral loopback port and `tailscale serve --bg --https=<port>` proxies the tailnet's HTTPS name to it (TLS by Tailscale). Requests on that listener carry the `server::Tailnet` extension: its WebSockets must come from the tailnet origin and present the tailnet token (`--ws-token`, else one generated into the data dir's `tailnet-token`) or a pairing code, whatever the main listener allows — the listener, not a forwarded header, tells tailnet traffic apart, since `tailscale serve` connects from loopback. Pairing: `CreatePairingCode` issues a random code (single use, 2 min, at most 16 pending); the nav bar's `TailscaleButton` shows it as a QR code of `<tailnet url>/#stereo-pair=<code>`, the page connects with a `stereo.pair.<code>` subprotocol and is sent the token (`AuthToken`), which it keeps in localStorage. The page offers the code on every reconnect until the token arrives; a rejected code is told apart from an unreachable backend by an upgrade closed with code 4401 (`PAIRING_REJECTED`), since browsers can't see a failed handshake's status. The Serve config is machine-wide: stereo refuses a port whose root route serves anything else and removes only the root handler it created (`--set-path=/ off`, leaving other subpaths on the port alone). Each tailnet port has a lease, `tailscale-routes/<port>` in the data dir, locked while its stereo runs and recording the route it created: a crashed run's route (on an old ephemeral loopback port, or another tailnet port) is taken over, a running one's never, and a second stereo on the same port with the same data dir is refused. It rechecks every 30 s, and SIGINT/SIGTERM remove the route. Status (`TailscaleStatus`: starting/serving/error) goes to every session as `MsgServer::Tailscale`. Adapted from btmux's `tailscale.rs`.
- **`apple_music.rs`** — Apple Music search through Apple's public iTunes Search API (JSON, no key or login). No BPM, key or label, so it isn't used for fixups, nor for label searches (its search doesn't index labels); about 20 requests a minute per IP, so not for new releases either, which search many names at once.
- **`health.rs`** powers the manual search diagnostics in `HealthCheckButton.tsx`.
  Probes run concurrently with individual timeouts, independent of enabled sources
  or the open collection. A new `RunHealthCheck` replaces the session's previous
  task. The server sends `HealthCheckProgress` snapshots with `partial: true`
  followed by a final `HealthCheck` reply.
- **`spotify.rs`** searches anonymously. It reads persisted-query hashes and the
  newest version of the TOTP secret from Spotify's web-player bundle at runtime,
  with a cached or built-in fallback for the secret. Playlist writes require the
  user's imported browser session. Spotify ID lookup also supports backfills.
- **`youtube.rs`** exports through InnerTube (`/youtubei/v1/`) with `SAPISIDHASH`
  auth. The session keeps the chosen browser profile and channel. Cookies are
  re-read on export, and a selected brand channel is sent as `X-Goog-PageId`.
- **`playlists.rs`** and **`playlist_sync.rs`** manage dynamic playlists. A saved view
  stores a collection filename, grid filters and sort order, and an optional limit
  in the repo's `playlists.json`. Git force-stages that file because older repositories
  only allow `*.db`, and merges diverged edits per playlist. Exports replace the linked
  Spotify or YouTube playlist's contents in place. The data directory's
  `playlist_sync.json` records content hashes, times and errors per machine.
  An open tab requests automatic sync every few minutes. The backend skips unchanged
  views and throttles automatic checks to once an hour. The in-flight guard keeps
  two tabs from syncing the same playlist to the same service concurrently.
- **`beatport.rs`** / **`browser_cookies.rs`** — Beatport's Cloudflare clearance and YouTube's login are read straight from the user's browser cookie stores via `rookie`; `browser_cookies.rs` picks candidate browsers from the requesting tab's User-Agent and enumerates every profile's store. rookie opens databases as immutable, which skips the write-ahead log, so `browser_cookies::read` hands it a copy with the WAL checkpointed in when there is one.

### Frontend (`frontend/src/`, React 19 + TypeScript + Vite)

- **`App.tsx`** owns the main reducer, WebSocket connection and playback engine.
  The `Action` type in `Types.tsx` covers server messages and local actions. Return the
  same state object for ignored messages, such as heartbeat replies, to avoid
  restarting work across the UI.
- **Background and motion.** `.background-player` in `App.css` shows the current
  YouTube thumbnail. `App.tsx` applies the appearance settings as CSS variables,
  and `useBeatPulse.ts` updates `--beat` from the playback clock outside React.
  `DiscoBall.tsx` draws the hanging ball, hidden by default, in the theme's text
  color, filling one canvas `Path2D` for all tiles. It draws once when still, the
  default, and schedules redraws up to 60 Hz when rotating, including on
  high-refresh-rate displays. It fits the ball and cord below the title bar without
  changing the stored settings, and redraws when the device pixel ratio changes. CSS
  dims it behind grid rows and centered empty states. `SettingsMenu.tsx` exposes the
  artwork's visibility (also `b`), saturation, opacity, blur and pulse first, then the
  ball's visibility, rotation, size and cord length. Its Reset visual effects clears
  these and the speaker air, which are stored only as overrides of their defaults.
  Reduced motion disables rotation, pulsing and speaker air controls.
- **Playback.** `AudioPlayback.ts` implements `PlaybackEngine` (`Playback.ts`) with
  one reused `<audio>` element. Its `ended` handler advances playback, so
  `PlaybackEvents.nextId` must agree with the reducer's `play-next-track` logic.
  It prefetches with `HEAD` and stops after three consecutive playback failures.
  `VolumeControl` squares the stored slider level for a perceptual scale. On iOS
  the browser manages volume, so Stereo offers mute. `useCanSetVolume` reads the
  probe after WebKit has had time to revert it. Register Media Session handlers
  again on `playing` and when the page becomes visible, clearing `seekbackward`
  and `seekforward` each time so iOS shows previous and next. Keep playback on
  the audio element so a YouTube iframe cannot override these handlers in WebKit.
- **Speaker stacks.** `SpeakerStack.tsx` and `speakerStack.ts` fade in cabinets
  around the header's mirrored logos as volume increases. At zero they match the logo
  path for path. The optional beat-driven pressure waves use `useBeatPulse.ts`'s
  interpolated `playbackClock`. The canvas draws them outside React, using sine noise
  for turbulence to avoid repainting the SVG and running WebKit's software filter.
  `SettingsMenu.tsx` adjusts the effect, which defaults to off. The stacks overflow
  their fixed header boxes behind the UI. Preserve the stacking order (stacks at
  `z-index: -1`, artwork at `-2`) and `--speaker-stack-clearance` on wide screens.
  Header and toolbar controls use frosted glass over the stacks.
- **Playback UI.** `AudioCache.ts` is an external store, fed by `audio-*` messages
  in `App.tsx`'s `wsOnMessage`. Each `AudioCacheButton` subscribes to its own
  track. `PlaybackProgress.tsx` owns the footer's once-a-second position updates
  and wraps `Waveform.tsx` with data from `useWaveform.ts`. `rowProgress.ts` applies
  position inline to the playing row's elements, and `getRowStyle` to rows rendered
  in between, at zero for a track other than the one last sampled. Keeping these updates out of the reducer, off the document root and
  out of stylesheets (WebKit restyles the whole page on any rule change, some 90 ms)
  prevents beat animation and disco ball stalls. `App.css` sets `min-height: 0` on
  Astryx's layout to avoid relaying out the grid on every change.
- **Tempo.** `BpmCheck.tsx` offers to save the measured BPM when the stored value
  is missing or differs by more than 0.5 BPM, allowing for half or double time
  (`tempo.ts`). `get-collection-track` and `update-bpm` both name the open
  collection. Recheck after switching collections and dismiss the old toast.
  Dismissals persist in localStorage. `TapTempo.tsx` (also Shift+T) compares taps,
  measured tempo and metadata. The taps live in `tapTempo.ts`, an external store.
  It fits the taps by least squares and resets on a new track or a two-second pause.
- **`WebSocket.tsx`** reconnects, sends a heartbeat every 10 seconds and exposes
  fire-and-forget `sendMsg` and `requestReply`. Requests get an incrementing `id`.
  A matching reply with `partial: true` delivers progress and keeps the callback
  registered for the final reply.
- **`Grid.tsx`** wraps ag-Grid. `TracksGrid` uses the infinite row model and
  requests pages with `get-rows`. `App.tsx` looks up the playing row using
  `get-row-index`. `SearchResultsGrid` uses client-side rows and checks membership
  in the open collection with a batched `collection-contains-ids` request.
  `ChangeVideoDialog.tsx` searches YouTube via `search-youtube-videos` or looks up
  a pasted link. The `set-search-result-video` action replaces the match in
  client state and in a queue playing from the results. Multiple results can
  share a video. Keep row identity tied to `result_id` and the queue's position
  tied to its index (`current_index`, `queueIndex`). `GridScrollButtons.tsx`
  shows controls for the playing track and top of the grid, also bound to
  `c` and `t`. `useGridScrollState.ts` governs when the controls appear.
- **Dynamic playlists** — `PlaylistMenu` (collection toolbar) opens/saves views and shows when the grid differs from the active one ("modified"); `PlaylistsDialog` edits a playlist and its Spotify/YouTube exports. Opening a playlist switches the collection and applies its filter/sort to the grid (and to the saved grid state, for when the grid isn't mounted).
- **Theme** — `theme.ts`: Astryx's Neutral theme (`@astryxdesign/theme-neutral`) plus a few component overrides. Neutral names the Figtree font without loading it; it's self-hosted with Fontsource (`@fontsource-variable/figtree`), which registers it as `"Figtree Variable"`, so the theme's font stacks name that first.
- **Navigation.** `NavBarControls` groups account connections, Tailscale pairing,
  health checks, theme and settings. `CollectionSelector` handles collections and
  repository remotes. It shows routine Git sync state as a status dot and groups
  actionable states with the menu through `GitSyncStatus.tsx` and `gitSync.ts`.
  Sync can start immediately, retry, or open the conflict dialog according to the
  report. The dialog tracks the conflict and repo it was opened for and closes when
  that conflict clears or changes. `ImportMenu` handles collection and YouTube
  imports. `Keyboard` defines shortcuts.
- **Prefer standard Astryx components.** Before building or restyling a widget,
  run `bunx @astryxdesign/cli@0.7.14 search "<thing>"`. Use `ToggleButton` for an
  on/off control. A custom widget needs a concrete reason, documented where it is
  built. Icons come from Radix, with Lucide for glyphs Radix lacks.

### Desktop app (`desktop/`)

The `stereo-desktop` Tauri crate (`desktop/src-tauri/src/lib.rs`) loads the backend's
web UI. See [desktop/README.md](desktop/README.md) for packaging and launcher behavior.

- **Backend selection.** The launcher checks the process that owns the data directory,
  waiting for it to publish its port if needed. Otherwise it checks port 8005 and the
  launchd service's port, or uses `STEREO_PORT` for the port lookup when set. It recognizes
  Stereo by `/info`, with the page title as a fallback for older backends. If needed it
  starts the bundled sidecar, preferring the default or configured port when free.
- **Lifecycle.** A second launch focuses the existing window. The bundled backend's
  `--exit-with-stdin` flag ties it to the launcher even on a crash. On Unix, the
  launcher sends `SIGTERM` on quit and waits for cleanup, such as removal of a
  Tailscale route, before killing the process group. The justfile and release workflow
  copy the sidecar into `desktop/src-tauri/binaries/`. `just bump` versions both crates.
- **WebSocket auth.** The launcher checks whether the backend needs a token. It tries
  `STEREO_WS_TOKEN`, the saved start-page token, and the launchd token on macOS.
  If it needs a new token, the start-page form is intercepted in `on_navigation`.
  Once accepted, the launcher saves it and passes it in the UI URL's
  `#stereo-token=` fragment.
- **Title bar.** On macOS, `TitleBarStyle::Overlay` lets the page draw under the
  transparent title bar on the start page and backends with the `title-bar-overlay`
  feature. The launcher sets `data-title-bar="overlay"` on the root element, or
  `"hidden"` in full screen. `.stereo-app` reserves the strip. The script in
  `title_bar.rs` implements dragging and the system's double-click action with
  the only window commands that the page's IPC capability permits.
- **Browser integration.** External links open in the default browser via
  `on_navigation` or `on_new_window`. The app sets `window.__STEREO_DESKTOP__`
  for `inDesktopApp` in `Utils.tsx`. Beatport connects from Stereo in the user's
  browser via `openInBrowser`, which passes along the WebSocket token if needed.
  YouTube connects from the webview and searches all supported browsers for a
  login.

### Protocol sync

When changing the WS protocol, both sides need updating and must be kept structurally consistent by hand:

- Rust: `src/message.rs` (`MsgClient`/`MsgServer`), enum variant handled in `src/server.rs`'s `handle_msg`.
- TypeScript: `frontend/src/Types.tsx` (`ClientMsg`/`ServerMsg`), consumed in `App.tsx`'s reducer or wherever the message originates/is handled.

Serde tags use kebab-case (`#[serde(tag = "type", rename_all = "kebab-case")]`); TS message types mirror this with string literal `type` fields.

<!-- ASTRYX:START -->
Astryx v0.7.14 · 174 components
CLI: run every command as `bunx @astryxdesign/cli@0.7.14 <cmd>` (shown below as `astryx ...`).

SETUP (once, in app entry e.g. main.tsx) — without these, components render unstyled:
  import "@astryxdesign/core/reset.css";
  import "@astryxdesign/core/astryx.css";

WORKFLOW — start every page from a template, never from scratch:
1. `astryx build "<idea>"` — names the template to scaffold from and the parts it lacks.
2. `astryx template <name> <path>` — scaffold it; keep its frame, gap and padding; replace the content.
3. `astryx template <Block>` for parts it lacks; `astryx component <Name>` to read props before using one.
Changing an existing page? Keep it: skip step 2 and add blocks/components inside its sections.

RULES:
- No <div> — components handle all layout and spacing.
- Read `astryx docs layout` before changing a template's frame.
- Dense data = rows (Table, List/Item), not Card-wrapped lists. Badge = counts only.
- Style with component props first, then tokens (var(--color-*|--spacing-*|--radius-*)). No raw hex/px. No StyleX/Tailwind compiler here — don't use xstyle/utility classes directly. Some templates use StyleX and need a compiler plugin; see `astryx docs styling-overview`.
- Palettes and custom colors go through the theme, never :root overrides. Run `astryx theme add <slug>` to import; `theme eject <slug>` only forks source. See `astryx docs theme`.
- SELF-CHECK: re-read the file; replace any raw <div>/<span>, imported .css/@apply, or hardcoded #hex/px with the component or a token.

`astryx help` lists every command. Key ones beyond the workflow:
  search "<query>"   find any component / hook / doc / template
  discover <words>   integrations you could add
  docs <topic>       getting-started, principles, tokens, theme … (`astryx docs` lists all)
  docs cli           commands and API reference
  docs authoring     build an integration package, and every field it holds
  upgrade --from <old version> --apply   run after a dependency bump
<!-- ASTRYX:END -->
