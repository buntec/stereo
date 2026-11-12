# Stereo desktop

A [Tauri](https://tauri.app) app around Stereo's web UI: a window showing the UI that a `stereo`
backend serves on loopback, like a browser tab would.

`just desktop-dev` builds the frontend and a debug backend, then opens the app. `just desktop-build`
packages it with a release backend under `desktop/src-tauri/target/release/bundle/`
(`macos/Stereo.app` on macOS). For a macOS disk image as well, run
`cd desktop && bun run build -- --bundles dmg` after `just desktop-build`. `just desktop-check`
lints and tests the launcher.

**Which backend.** Two backends on the same data directory would race each other's database
writes and git syncs, so a `stereo` locks its data directory (`stereo.lock` in it) before it
touches a collection, and once it listens writes its port to `stereo.port` there; another `stereo`
on that directory exits with an error. On launch, the app opens the Stereo that has locked its data
directory (`STEREO_HOME`, else `~/.local/share/stereo`), waiting for one that is still starting up.
Otherwise it looks for a Stereo running on port 8005, the CLI's default, or on the port of the
launchd service from `stereo install` (from its plist), and opens it. Otherwise it starts its
bundled `stereo` (a Tauri sidecar) on port 8005, or on a free port if something else holds 8005,
with the default data directory (`~/.local/share/stereo`), so it shows the same collections as the
CLI. Its backend stops when the app quits, or crashes (it's started with `--exit-with-stdin`, and
then ends its yt-dlp downloads too), and changes not yet synced with git are synced on its next
start, as after Ctrl-C. While the app runs, its Stereo is also at `http://localhost:8005` in a
browser. `STEREO_PORT` picks another port (and is then the only one looked at), and `STEREO_HOME`
another data directory, e.g. to try the bundled backend in development while another Stereo runs;
the backend also takes the other `STEREO_*` variables (apps opened from the Finder or Dock don't
see them).

A port that stays the same keeps the page's origin, and with it what the UI keeps in
localStorage (grid state, settings), across launches.

**One instance.** Launching the app again, e.g. a development build while a packaged one runs,
focuses the window of the one already running: the backend it started stops when it quits.

**Quitting** sends the backend `SIGTERM`, as Ctrl-C would, and waits up to 20 s for it to exit
(with `--tailscale` it removes its `tailscale serve` route first) before killing what is left of
its process group, such as yt-dlp downloads.

**WebSocket token.** A backend with `--ws-token` (`STEREO_WS_TOKEN`) wants it from the UI too, and
the webview's storage doesn't share the token a browser kept. The app checks, with a WebSocket
handshake, whether the backend wants one, and if so passes the UI the first it accepts of: the
app's own `STEREO_WS_TOKEN` (which its bundled backend inherits), the token last entered in the
app, and on macOS the launchd service's (from its plist). Failing those, the start page asks for
it; the app checks it and keeps it in its config directory.

**PATH.** Apps opened from the Finder or Dock get a minimal `PATH`, without e.g. Homebrew's
yt-dlp and Deno, so on macOS the backend gets the `PATH` of the user's login shell.

**Links** to other sites, and new windows, open in the default browser.

**Beatport** is connected from Stereo in a browser: it reads the browser's cookies, found by its
User-Agent, which the app's webview doesn't share, and Cloudflare's clearance only works with the
User-Agent of the browser that earned it. In the app, its dialog opens Stereo in the browser
instead (with the app's WebSocket token, if any), and the app picks up the connection when its
window gets focus again. **YouTube** connects from the app: with no browser to go by, the backend
looks for a login in every browser, and offers the ones it finds to pick from. Spotify connects
from the app as from a browser.

The app sets `window.__STEREO_DESKTOP__`; the UI checks it with `inDesktopApp` (`Utils.tsx`).

Each target architecture needs its own bundled `stereo` binary. On macOS, sign and notarize the
bundle before distributing it to other users.

`app-icon.svg` is the icon's source, Stereo's logo on a tile. After editing it, run
`cd desktop && bunx tauri icon app-icon.svg -o src-tauri/icons` and delete the generated
`android` and `ios` folders.
