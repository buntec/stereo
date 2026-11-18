# Stereo 📻

Curate and play music in your browser. Search Beatport, Spotify, SoundCloud and more by artist,
label, BPM, key or release date, keep what you like in collections, rate and filter them, and
play them back. Collections sync across your machines with git, and saved views can be exported
as playlists to Spotify and YouTube.

No account, no subscription: a collection is just a small SQLite file of track metadata.

<p>
  <img src="https://github.com/user-attachments/assets/08e67c2d-58cc-4bee-9829-a7338e20255d" alt="Stereo with the dark theme" width="49%">
  <img src="https://github.com/user-attachments/assets/6e770cf6-3264-4cb5-a1bb-e5bafe0ceb27" alt="Stereo with the light theme" width="49%">
</p>

## Install

**Desktop app** (macOS, Windows, Linux): download it from the
[latest release](https://github.com/buntec/stereo/releases/latest). The macOS app isn't
notarized, so the first time, allow it under System Settings › Privacy & Security.

**Command line:** download the `stereo` binary for your platform from the same release, or build
it with [Rust](https://rustup.rs) and [Bun](https://bun.sh):

```
cargo install --git https://github.com/buntec/stereo
stereo
```

Then open `localhost:8005` in any modern browser. On macOS, `stereo install` runs it as a
service that starts when you log in.

**For playback**, install [yt-dlp](https://github.com/yt-dlp/yt-dlp), e.g. `brew install yt-dlp`
(if you install it some other way, also install [Deno](https://deno.com)), and keep it up to
date: YouTube changes often, and older versions stop working. Without them, Stereo still works for curating, but won't play anything.

## Using Stereo

Start by searching for music and adding it to a new collection, or import tracks from another
collection or a YouTube playlist. You can have as many collections as you like.

**Playback.** Stereo doesn't host any audio: it finds each track on YouTube and downloads the
audio with yt-dlp the first time you play it. Downloads are cached (10 GB by default, see
`stereo --help`) and upcoming tracks are fetched ahead of time. Playback keeps going in the
background and with a phone's screen locked.

**Sync.** Collections live in git repositories in `~/.local/share/stereo`. To sync them across
machines, create a repository with a remote (e.g. a private GitHub repo) and add it on each
machine with *Clone repository…* in the collection menu. From then on, every change is committed
and pushed a few minutes later.

**Playlists.** Save the current filter and sort as a playlist, and export it to Spotify or
YouTube; while Stereo is open, exports are kept up to date. Connect your accounts with the
buttons in the top bar. YouTube uses the login of a browser on your machine (on macOS, the first connect asks for
keychain access).

## On your phone

Start Stereo with `--tailscale` (also works with `stereo --tailscale install`) to reach it from
any device on your [tailnet](https://tailscale.com). Click the Tailscale icon in the top bar and
scan the QR code with your phone to open Stereo there, already signed in. This needs the
`tailscale` CLI, with [MagicDNS](https://tailscale.com/kb/1081/magicdns) and
[HTTPS certificates](https://tailscale.com/kb/1153/enabling-https) enabled for the tailnet.

Without Tailscale, Stereo only accepts connections from the same machine; see `stereo --help`
(`--host`, `--allowed-origin`, `--ws-token`) to change that.

## Where the metadata comes from

Searches query Beatport, SoundCloud, Spotify, Apple Music, Traxsource and Volumo, and match the
results against YouTube; Shazam finds similar songs. Most of these have no public API, so Stereo
uses the same endpoints as their websites: a search can break whenever a site changes, and heavy
use may get rate-limited. If Beatport searches fail, visit beatport.com once in your browser:
Stereo reuses its Cloudflare cookie. Cookies are read locally and only ever sent to the site they
belong to.

## Disclaimer

Stereo is a personal, non-commercial hobby project. It isn't affiliated with, endorsed by
or sponsored by YouTube, Google, Beatport, SoundCloud, Spotify, Shazam, Apple, Traxsource
or Volumo; all trademarks belong to their owners.

Stereo is meant to run locally, for your own use. Don't run it as a public service.
Downloading content from YouTube or using the other services through unofficial endpoints
may violate their terms of service, and depending on where you live, copyright or other
law. It's up to you to make sure your use is allowed, and to support the artists you
listen to. Using these services this way may also get your accounts with them restricted.

The software is provided as is, without warranty of any kind; see [LICENSE](LICENSE).

## Development

See [docs/development.md](docs/development.md).
