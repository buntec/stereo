# Stereo 📻

Curate and play music in your browser.
No account, login or subscription required!

Works across platforms in any reasonably modern browser.
Tested with Firefox and Chrome.

# Getting started

All you need is [Rust](https://rustup.rs) and [Bun](https://bun.sh).

```
cargo install --git https://github.com/buntec/stereo
stereo
```

Navigate to `localhost:8005` in your browser.

Stereo only accepts same-origin WebSocket connections. If you deliberately listen on a
non-loopback interface, configure both the public browser origin and an authentication token:

```
STEREO_WS_TOKEN="choose-a-long-random-token" stereo \
  --host 0.0.0.0 \
  --allowed-origin http://stereo.local:8005
```

Then open `http://stereo.local:8005/#stereo-token=choose-a-long-random-token`. The URL fragment
is not sent in HTTP requests; the frontend presents an encoded form of it during the WebSocket
handshake. Missing-origin WebSocket clients are rejected.

Begin by searching for and adding music to a new collection.
Or by importing from another collection.
You can create as many collections as you want.
A collection is just a file on your local hard disk.
More precisely, it's a SQLite database containing track meta data only - no actual audio data is stored or downloaded!

For the best possible playback experience, a YouTube Premium subscription or a good ad-blocker is highly recommended.

Work in progress! 🚧

<img width="1840" height="1195" alt="Screenshot 2026-01-03 at 18 04 40" src="https://github.com/user-attachments/assets/feb98b68-ef28-4d99-9c86-91da630edddd" />

## Connecting a YouTube account (for playlist export)

Exporting tracks to a YouTube playlist requires a Google OAuth client, since stereo
doesn't ship its own (that would require bundling a client secret and passing Google's
verification for a public app). Set one up once and connect via the YouTube button in
the toolbar, next to the Spotify one:

1. Go to the [Google Cloud Console credentials page](https://console.cloud.google.com/apis/credentials)
   and select or create a project.
2. Under **APIs & Services → Enabled APIs**, enable the **YouTube Data API v3**.
3. Under **OAuth 2.0 Client IDs**, create a client of type **Desktop app**.
4. Add your stereo origin (e.g. `http://localhost:8005`) as an authorized redirect URI.
5. Copy the client ID and client secret into the YouTube login dialog in stereo.

If you forget your credentials later, they're always visible under the same
[credentials page](https://console.cloud.google.com/apis/credentials) — reset the
client secret there if needed.

# Development

Prerequisites: [Rust](https://rustup.rs), [Bun](https://bun.sh), [just](https://github.com/casey/just).

```
bun install        # install frontend dependencies
just dev           # run backend + frontend dev server concurrently
```

Frontend checks:

```sh
bun run build
bun run lint
bunx playwright install chromium  # once, for browser tests
bun run test:ui
```

Browser tests start Vite and mock the WebSocket and YouTube APIs; they do not read
or modify real collections. Set `PLAYWRIGHT_CHROMIUM_EXECUTABLE` to use an existing
Chromium executable instead of the Playwright browser download.
