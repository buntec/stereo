# Development

Prerequisites: [Rust](https://rustup.rs), [Bun](https://bun.sh), [just](https://github.com/casey/just).
For playback you also need [yt-dlp](https://github.com/yt-dlp/yt-dlp) and [Deno](https://deno.com).

```sh
bun install        # install frontend dependencies
just dev           # run the backend and the Vite dev server, both with hot reload
```

`just dev` serves the app on port 8055 and keeps its data in `~/.local/share/stereo-dev`
(or `$STEREO_HOME`), so it doesn't touch your real collections or clash with a running Stereo.
`just` on its own lists all recipes; the main ones:

```sh
just build             # frontend + release binary
just install           # build and cargo install
just dev-tailscale     # serve a dev build on your tailnet, e.g. to test on a phone
just desktop-dev       # run the desktop app with a debug backend (see desktop/README.md)
just desktop-build     # package the desktop app
just bump [part]       # bump the version, commit and tag it
```

## Checks

```sh
just test              # cargo test
just lint              # cargo clippy
just format            # cargo fmt + prettier
bun run build          # type-check and build the frontend
bun run lint           # eslint
```

`tests/scraper.rs` talks to the real services (Beatport, YouTube, …) and is ignored by default;
run it with `cargo test -- --ignored`.

Browser tests use Playwright. They start Vite and mock the WebSocket and YouTube APIs, so they
don't read or modify real collections:

```sh
bunx playwright install chromium   # once
bun run test:ui
```

Set `PLAYWRIGHT_CHROMIUM_EXECUTABLE` to use an existing Chromium instead of Playwright's download.

## Architecture

[AGENTS.md](../AGENTS.md) describes how the backend, frontend, protocol and desktop app fit
together.
