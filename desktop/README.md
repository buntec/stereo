# Stereo desktop

A [Tauri](https://tauri.app) app around Stereo's web UI: a window showing the UI that a `stereo`
backend serves on loopback, like a browser tab would.

`just desktop-dev` builds the frontend and a debug backend, then opens the app. `just desktop-build`
packages it with a release backend under `desktop/src-tauri/target/release/bundle/`
(`macos/Stereo.app` on macOS). For a macOS disk image as well, run
`cd desktop && bun run build -- --bundles dmg` after `just desktop-build`. `just desktop-check`
lints and tests the launcher.

**Which backend.** On launch, the app looks for a Stereo running on port 8005, the CLI's default
(for example the launchd service from `stereo install`), and opens it: two backends on the same
data directory would race each other's git syncs. Otherwise it starts its bundled `stereo` (a
Tauri sidecar) on port 8005, or on a free port if something else holds 8005, with the default data
directory (`~/.local/share/stereo`), so it shows the same collections as the CLI. Its backend
stops when the app quits, or crashes (it's started with `--exit-with-stdin`), and changes not yet
synced with git are synced on its next start, as after Ctrl-C. While the app runs, its Stereo is
also at `http://localhost:8005` in a browser. `STEREO_PORT` picks another port, e.g. to try the
bundled backend in development while another Stereo runs on 8005; the backend also takes the
other `STEREO_*` variables (apps opened from the Finder or Dock don't see them).

A port that stays the same keeps the page's origin, and with it what the UI keeps in
localStorage (grid state, settings), across launches.

**PATH.** Apps opened from the Finder or Dock get a minimal `PATH`, without e.g. Homebrew's
yt-dlp and Deno, so on macOS the backend gets the `PATH` of the user's login shell.

**Links** to other sites, and new windows, open in the default browser.

**Beatport and YouTube** are connected from Stereo in a browser: both read the browser's cookies,
found by its User-Agent, which the app's webview doesn't share (Cloudflare's clearance only works
with the User-Agent of the browser that earned it). In the app, their dialogs open Stereo in the
browser instead, and the app picks up the connection when its window gets focus again. Spotify
connects from the app as from a browser.

The app sets `window.__STEREO_DESKTOP__`; the UI checks it with `inDesktopApp` (`Utils.tsx`).

Each target architecture needs its own bundled `stereo` binary. On macOS, sign and notarize the
bundle before distributing it to other users.

`app-icon.svg` is the icon's source, Stereo's logo on a tile. After editing it, run
`cd desktop && bunx tauri icon app-icon.svg -o src-tauri/icons` and delete the generated
`android` and `ios` folders.
