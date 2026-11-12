# Stereo 📻

Curate and play music in your browser.
Find tracks by their metadata (artist, label, BPM, key, release date), organize them into
collections, and keep those collections in sync across your machines with git.

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

## On your phone, via Tailscale

To use Stereo from your phone or other devices on your [tailnet](https://tailscale.com), start it
with `--tailscale` (also works with `stereo --tailscale install`):

```
stereo --tailscale
```

Stereo then runs `tailscale serve`, which serves it at `https://<machine>.<tailnet>.ts.net:8005`
with a Tailscale certificate, and removes that route again on shutdown. Click the Tailscale icon
in the top bar and scan the QR code with your phone: it opens Stereo there and signs it in. Each
code works once, for two minutes. `localhost:8005` keeps working as before, without a token.

This needs the `tailscale` CLI (on macOS, the app's bundled CLI also works),
[MagicDNS](https://tailscale.com/kb/1081/magicdns) and
[HTTPS certificates](https://tailscale.com/kb/1153/enabling-https) enabled for the tailnet.
Stereo never replaces another route on its port; if Tailscale isn't up yet, it keeps retrying.

Begin by searching for and adding music to a new collection.
Or by importing from another collection.
You can create as many collections as you want.
A collection is just a file on your local hard disk.
More precisely, it's a SQLite database containing track meta data only - no audio.

## Playback

Stereo doesn't include or host any audio. Playback relies on
[yt-dlp](https://github.com/yt-dlp/yt-dlp), which you install separately (e.g.
`brew install yt-dlp`), and you're responsible for using it in line with YouTube's terms and
the law where you live (see [Disclaimer](#disclaimer)). Without yt-dlp, Stereo still works for
curating collections, but won't play anything.

Stereo downloads each track's audio from YouTube when it is first played and plays it in the
browser, which keeps going in the background and with a phone's screen locked. Upcoming tracks
are downloaded ahead of time; a track that can't be downloaded is skipped.

Downloads are cached in `~/Library/Caches/stereo/audio` on macOS (`~/.cache/stereo/audio`
elsewhere, or `--cache-dir`), up to 10 GB by default (`--audio-cache-limit-gb`); the least
recently played tracks are evicted first. Pass `--yt-dlp <path>` if yt-dlp isn't on the `PATH`.
Keep yt-dlp up to date (`brew upgrade yt-dlp`): YouTube changes often, and older versions stop
working.

Work in progress! 🚧

<img width="1840" height="1195" alt="Screenshot 2026-01-03 at 18 04 40" src="https://github.com/user-attachments/assets/feb98b68-ef28-4d99-9c86-91da630edddd" />

## Connecting a YouTube account (for playlist export)

Stereo exports playlists to your YouTube account through YouTube's own web API, using
the YouTube login of the browser you run stereo in: no Google Cloud project, OAuth
client or API quota needed. Log in to youtube.com in that browser, then connect under
**Settings → Accounts → YouTube**. If several profiles of that browser are logged in,
stereo asks which one to use. On macOS, the first connect asks for keychain access
to the browser's cookie store.

Stereo only remembers which browser profile holds the login and reads its YouTube
cookies afresh for every export, so logging out of YouTube there disconnects stereo too.
The API is undocumented, so exports may break until stereo is updated.

## Where the metadata comes from

Searches query Beatport, SoundCloud, Traxsource, Volumo and Spotify for track metadata,
and match the results against YouTube search results. Shazam is used for "similar songs".
Most of these services have no public API, so Stereo uses the same endpoints as their
websites. That means a search can break whenever a site changes, and heavy use may get
rate-limited or blocked.

To get past Beatport's bot protection, Stereo reuses the Cloudflare cookie of your own
browser: visit beatport.com in that browser once if Beatport searches fail. Like the
YouTube connection above, it reads the browser's cookie store locally (on macOS, the first
read asks for keychain access). Cookies are never sent anywhere except to the site they
belong to.

# Disclaimer

Stereo is a personal, non-commercial hobby project. It isn't affiliated with, endorsed by
or sponsored by YouTube, Google, Beatport, SoundCloud, Spotify, Shazam, Apple, Traxsource
or Volumo; all trademarks belong to their owners.

Stereo is meant to run locally, for your own use. Don't run it as a public service.
Downloading content from YouTube or using the other services through unofficial endpoints
may violate their terms of service, and depending on where you live, copyright or other
law. It's up to you to make sure your use is allowed, and to support the artists you
listen to. Using these services this way may also get your accounts with them restricted.

The software is provided as is, without warranty of any kind; see [LICENSE](LICENSE).

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
