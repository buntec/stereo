# AGENTS.md

This file provides guidance to AI agents when working with code in this repository.

## What this is

Stereo is a browser-based music curation app. There's no account/login/subscription — a "collection" is just a SQLite database file (track metadata only, no audio). The Rust backend serves a React SPA and a WebSocket API; playback happens client-side via the YouTube IFrame API. Tracks are found by searching Beatport/SoundCloud for metadata and matching against YouTube search results.

## Commands

Prerequisites: [Rust](https://rustup.rs), [Bun](https://bun.sh), [just](https://github.com/casey/just).

```
bun install              # install frontend deps (also: just init)
just dev                 # run backend (port 8055) + frontend dev server concurrently, hot reload both
just build               # build frontend then backend release binary
just install             # build + cargo install --path .
just test                # cargo test
just lint                # cargo clippy
just format              # cargo fmt + prettier on frontend/src
```

Single Rust test: `cargo test <test_name>` (e.g. `cargo test test_init_creates_table`).

Frontend-only commands (from `package.json`): `bun run dev`, `bun run build` (`tsc -b && vite build`), `bun run lint` (eslint).

`tests/scraper.rs` hits real external APIs (Beatport, YouTube, MusicBrainz) and is `#[ignore]`d by default — run explicitly with `cargo test -- --ignored`.

The backend binary embeds the built frontend (`dist/`) via `rust-embed`/`axum-embed` for release builds; in `--dev` mode (`just run-backend-dev`) it does not serve static assets and expects Vite to be running separately (the Vite dev server proxies `/ws` to the backend, see `vite.config.ts`).

## Architecture

### Backend (`src/`, crate `stereo-backend`, binary `stereo`)

Everything is driven over a **single WebSocket connection** (`/ws`, handled in `server.rs`). There is no REST API. `main.rs` wires up an axum `Router` with one route plus a static-file fallback; it also has CLI subcommands (`version`, `install`/`uninstall`/`restart`) for running as a macOS launchd service.

- **`message.rs`** — the entire client↔server protocol as two enums, `MsgClient` and `MsgServer`, serialized as JSON with a `type` tag (kebab-case). This is the source of truth for the protocol; the frontend's `Types.tsx` (`ClientMsg`/`ServerMsg`) must be kept in sync by hand — there's no codegen.
- **`server.rs`** — `handle_socket` owns per-connection `SessionState` (current DB pool, active collection, active git repo, in-flight search/sync/export tasks) and `handle_msg` is one large match over `MsgClient` dispatching to `db`/`git`/`scraper`/`utils`. Messages are handled one at a time, so anything slow (network, git) must run in a spawned task; tasks that need to change session state afterwards post an `InternalEvent` back into the session's queue. A handler error is sent to the client as a notification, plus an `ErrorReply` for request/reply messages (`MsgClient::request_id`). Outgoing messages are batched and flushed by `send_loop` (up to 100 msgs / 100ms) rather than sent one-at-a-time.
- **`db.rs`** — all SQLite access via `sqlx`. Collections are single-table SQLite files; `get_rows`/`count_rows`/`get_row_index` implement server-side pagination, sorting and filtering compatible with ag-Grid's server-side row model (`FilterModel`/`SortModelItem` in `message.rs` mirror ag-Grid's filter/sort shape almost directly).
- **`scraper.rs`** — search backends. Beatport, SoundCloud, Volumo, Traxsource and Spotify (`spotify.rs`) are queried in parallel and interleaved (`interleave`) into candidate `RawTrack`s with metadata (artist/label/bpm/key/release date), which are then matched against YouTube search results (`find_best_match`/`score_match`, resolved concurrently and de-duplicated by YouTube id in `stream_matched_tracks`) since actual playback comes from YouTube. A failing backend is reported to the client as a warning rather than silently yielding fewer results. Shazam provides "similar songs" searches; `fixup_track` fills in missing metadata from Beatport/Traxsource/Volumo. Beatport is scraped via its embedded `__NEXT_DATA__` JSON (no official API); SoundCloud requires first fetching a client_id from SoundCloud's own JS bundle. `create_client()` uses `wreq` (a `reqwest`-like client with Chrome TLS fingerprint impersonation) specifically to get past Beatport's bot detection — a plain `reqwest` client gets blocked. Search results stream back to the client incrementally via `MsgServer::SearchResult`/`SearchComplete` over an mpsc channel, not as one batch response.
- **`git.rs`** — each git repo under the home dir (`~/.local/share/stereo` by default, or `--home`/`STEREO_HOME`) can optionally track a remote; collection `.db` files are auto-committed and pushed whenever a track is added/edited/rated/deleted, debounced so a burst of changes becomes one sync after `GIT_SYNC_DELAY` = 5 min (`schedule_git_sync` in `server.rs`; uncommitted changes left at shutdown are picked up by the startup sync). This is how collections sync across machines. Two per-repo locks: the repo lock guards the working tree (DB writes, commits, merges); the network lock serializes fetch/push. Background syncs release the repo lock before the network phase so DB writes never wait on the network.
- **`settings.rs`** — CLI args via `clap`, also settable via env vars (`STEREO_HOME`, `STEREO_HOST`, `STEREO_PORT`, `STEREO_ALLOWED_ORIGINS`, `STEREO_WS_TOKEN`, `STEREO_DEV`). Binding to a non-loopback host or allowing a remote origin requires a WebSocket token.
- **`spotify.rs`** / **`youtube.rs`** — Spotify (session cookie) and YouTube accounts, used for playlist export and Spotify ID backfill. YouTube export goes through YouTube's internal web API (InnerTube, `/youtubei/v1/`), signed in with the login cookies of the user's browser (`SAPISIDHASH` auth, as YouTube's web client does); the session only records which browser profile holds the login (the user picks one when several profiles are logged in; a selected brand channel is forwarded as `X-Goog-PageId`), and cookies are re-read for every export since Google rotates them.
- **`beatport.rs`** / **`browser_cookies.rs`** — Beatport's Cloudflare clearance and YouTube's login are read straight from the user's browser cookie stores via `rookie`; `browser_cookies.rs` picks candidate browsers from the requesting tab's User-Agent and enumerates every profile's store. rookie opens databases as immutable, which skips the write-ahead log, so `browser_cookies::read` hands it a copy with the WAL checkpointed in when there is one.

### Frontend (`frontend/src/`, React 19 + TypeScript + Vite)

- **`App.tsx`** is the root component and largest file — holds most app state via a single `useReducer` (`Action` type in `Types.tsx` covers both server messages and local UI actions), owns the WebSocket connection (`WebSocket.tsx`'s `useWebSocket` hook) and the YouTube player (`YT.tsx`).
- **`WebSocket.tsx`** — thin reconnecting-WebSocket hook. Supports fire-and-forget `sendMsg` and a request/reply pattern (`requestReply`) that tags outgoing messages with an auto-incrementing `id` and resolves a callback when a reply with the same `id` comes back; also sends a heartbeat every 10s.
- **`Grid.tsx`** — ag-Grid wrappers (`TracksGrid`, `SearchResultsGrid`) using the server-side row model, calling back into the WS `get-rows`/`get-row-index` protocol for pagination/sort/filter. The search grid checks which results are already in the collection with one batched `collection-contains-ids` request.
- **`YT.tsx`** — wraps the YouTube IFrame Player API.
- Other components are single-purpose dialogs/widgets (`ImportDialog`, `ExportDropDown`, `GitRemoteDialog`, `YouTubePlaylistDialog`, `CollectionSelector` (collection + repo menu), `NavBarControls` (nav-bar version, account status buttons, appearance toggle), `EditDialog`, `Rating`, `SearchBox`, `NowPlayingMenu`, `CollectionSummary`, `Keyboard` for shortcuts, etc.) — UI is Astryx (`@astryxdesign/core`, see the Astryx notes below); icons come from `@radix-ui/react-icons`.

### Protocol sync

When changing the WS protocol, both sides need updating and must be kept structurally consistent by hand:

- Rust: `src/message.rs` (`MsgClient`/`MsgServer`), enum variant handled in `src/server.rs`'s `handle_msg`.
- TypeScript: `frontend/src/Types.tsx` (`ClientMsg`/`ServerMsg`), consumed in `App.tsx`'s reducer or wherever the message originates/is handled.

Serde tags use kebab-case (`#[serde(tag = "type", rename_all = "kebab-case")]`); TS message types mirror this with string literal `type` fields.

<!-- ASTRYX:START -->

Astryx v0.1.8 · 90+ components
CLI: run every command as `bunx @astryxdesign/cli <cmd>` (shown below as `astryx ...`).

SETUP (once, in your app entry e.g. main.tsx) — without these, components render unstyled:
import "@astryxdesign/core/reset.css";
import "@astryxdesign/core/astryx.css";

WORKFLOW — discover, don't guess. Before writing UI:

1. `astryx build "<idea>"` — START HERE: returns a kit (closest [page] + [block]s + [component]s). No args = full playbook.
2. `astryx template <name> [--skeleton]` — scaffold the [page]/[block]s it named, or study their layout. Templates are reference code.
3. `astryx component <Name>` — props + examples for every component you use.

RULES:

- No <div> — components do all layout/spacing. Full page → AppShell; sidebar nav → SideNav.
- Frame first: pick the shell (AppShell / Layout+LayoutPanel) and budget regions in px BEFORE writing content (`astryx docs layout`).
- Dense data = rows (Table, List/Item) edge-to-edge — never Card-wrapped list items. Card = dashboard widgets, galleries, settings groups only.
- Status → StatusDot/Token; Badge only for counts and enumerated states, never decoration.
- Custom styling: component props first; else style/className with tokens — var(--color-_|--spacing-_|--radius-\*). No raw hex/px. (No StyleX/Tailwind compiler here — don't use xstyle/utility classes.)
- Tokens for every value (`astryx docs tokens`). Brand/accent via `astryx theme` — never override --color-\* in :root.
- SELF-CHECK before you finish: re-read the file and replace any raw <div>/<span> layout, imported .css/@apply, or hardcoded value (#hex, 16px) with the component or a token (var(--color-_|--spacing-_|…)). If unsure a component/prop exists, run `astryx component <Name>` / `astryx search "<thing>"`; don't hand-roll CSS.

MORE CLI:
search "<query>" find any component / hook / doc / template / block
component --list 90+ components by category
template --list page + block recipes
docs <topic> color, elevation, icons, illustrations, internationalization, layout, migration, motion, principles, shape, spacing, styling, theme, tokens, typography
swizzle <Name> eject component source for deep customization
upgrade --apply run after any @astryxdesign/core bump

<!-- ASTRYX:END -->
