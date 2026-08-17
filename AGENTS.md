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
- **`server.rs`** — `handle_socket` owns per-connection `SessionState` (current DB pool, active collection, active git repo, in-flight search task, git-push debounce state) and `handle_msg` is one large match over `MsgClient` dispatching to `db`/`git`/`scraper`/`utils`. Outgoing messages are batched and flushed by `send_loop` (up to 100 msgs / 100ms) rather than sent one-at-a-time.
- **`db.rs`** — all SQLite access via `sqlx`. Collections are single-table SQLite files; `get_rows`/`count_rows`/`get_row_index` implement server-side pagination, sorting and filtering compatible with ag-Grid's server-side row model (`FilterModel`/`SortModelItem` in `message.rs` mirror ag-Grid's filter/sort shape almost directly).
- **`scraper.rs`** — search backends. Beatport and SoundCloud are scraped in parallel (`interleave`) to produce candidate `RawTrack`s with metadata (artist/label/bpm/key/release date), which are then matched against YouTube search results (`find_best_match`/`score_match`) since actual playback comes from YouTube. Beatport is scraped via its embedded `__NEXT_DATA__` JSON (no official API); SoundCloud requires first fetching a client_id from SoundCloud's own JS bundle. `create_client()` uses `wreq` (a `reqwest`-like client with Chrome TLS fingerprint impersonation) specifically to get past Beatport's bot detection — a plain `reqwest` client gets blocked. Search results stream back to the client incrementally via `MsgServer::SearchResult`/`SearchComplete` over an mpsc channel, not as one batch response.
- **`git.rs`** — each git repo under the home dir (`~/.local/share/stereo` by default, or `--home`/`STEREO_HOME`) can optionally track a remote; collection `.db` files are auto-committed and pushed (`git_spawn` in `server.rs`, debounced to once per `GIT_PUSH_INTERVAL` = 5 min) whenever a track is added/edited/rated/deleted. This is how collections sync across machines.
- **`settings.rs`** — CLI args via `clap`, also settable via env vars (`STEREO_HOME`, `STEREO_HOST`, `STEREO_PORT`, `STEREO_DEV`).

### Frontend (`frontend/src/`, React 19 + TypeScript + Vite)

- **`App.tsx`** is the root component and largest file — holds most app state via a single `useReducer` (`Action` type in `Types.tsx` covers both server messages and local UI actions), owns the WebSocket connection (`WebSocket.tsx`'s `useWebSocket` hook) and the YouTube player (`YT.tsx`).
- **`WebSocket.tsx`** — thin reconnecting-WebSocket hook. Supports fire-and-forget `sendMsg` and a request/reply pattern (`requestReply`) that tags outgoing messages with an auto-incrementing `id` and resolves a callback when a reply with the same `id` comes back; also sends a heartbeat every 10s.
- **`Grid.tsx`** — ag-Grid wrappers (`TracksGrid`, `SearchResultsGrid`) using the server-side row model, calling back into the WS `get-rows`/`get-row-index` protocol for pagination/sort/filter.
- **`YT.tsx`** — wraps the YouTube IFrame Player API.
- Other components are single-purpose dialogs/widgets (`ImportDialog`, `ExportDropDown`, `GitRemoteDialog`, `YouTubePlaylistDialog`, `CollectionSelector`, `RepoSelector`, `EditDialog`, `Rating`, `SearchBox`, `NowPlayingMenu`, `CollectionSummary`, `Keyboard` for shortcuts, etc.) — UI is Radix UI (`@radix-ui/themes`).

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
