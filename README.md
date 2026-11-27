# Stereo

Curate and play music in a browser or desktop app. Search Beatport, SoundCloud, Spotify,
Apple Music, Traxsource and Volumo, or find similar tracks with Shazam. Rate and filter
your finds in local SQLite collections that store track metadata. Stereo needs no account
or subscription.

<p>
  <img src="https://github.com/user-attachments/assets/2e984654-33e8-4264-af3a-1e95d9bb3092" alt="Stereo with the light theme" width="49%">
  <img src="https://github.com/user-attachments/assets/8614962e-b090-478d-b033-e481825d43ba" alt="Stereo with the dark theme" width="49%">
</p>

## Install

Install Git on the machine running Stereo. Download a standalone binary for macOS, Windows
or Linux from the [latest release](https://github.com/buntec/stereo/releases/latest).
For the desktop app, use a release installer when available or follow the
[desktop build instructions](desktop/README.md).

For browser use, run `stereo` and open `http://localhost:8005`. On macOS,
`stereo install` runs it as a login service. To build the binary from source with Rust and Bun:

```sh
cargo install --git https://github.com/buntec/stereo
```

For playback, install and keep yt-dlp up to date. Homebrew's `brew install yt-dlp`
includes Deno. For other installs, see the steps for Deno and the YouTube scripts in
the [yt-dlp setup guide](https://github.com/yt-dlp/yt-dlp/wiki/EJS).

### Nix

The flake packages the release binary (`packages.<system>.default`) and desktop app
(`.desktop`), and has a Home Manager module that runs Stereo as a launchd agent on macOS
or a systemd user service on Linux, with nixpkgs' yt-dlp:

```nix
{
  inputs.stereo.url = "github:buntec/stereo";

  # In your Home Manager configuration:
  imports = [ inputs.stereo.homeManagerModules.default ];
  programs.stereo = {
    enable = true;
    desktop.enable = true; # optional; opens the service's backend
    service.tailscale = true; # optional
  };
}
```

The service uses the same launchd label as `stereo install`, so `stereo restart` works, but
leave installing and uninstalling it to Home Manager. Set `service.wsTokenFile` to a secret
outside the Nix store to listen on a non-loopback `service.host`. See
[`nix/home-manager.nix`](nix/home-manager.nix) for all options.

## Use Stereo

- **Collect.** Search by track, artist or label, browse charts and new releases, or import
  tracks from a collection or YouTube playlist. Filter collections by BPM, key, release date
  and rating.
- **Play.** Stereo matches tracks to YouTube and caches the audio on demand (10 GB by default).
  Playback continues in the background and with your phone locked.
- **Sync collections.** Choose _Set remote…_ in the collection menu and point it to an empty
  Git repository. Use _Clone repository…_ on your other machines. Stereo syncs edits after
  about five minutes. The collection menu offers immediate sync and conflict resolution when
  available. Collections live under `~/.local/share/stereo/gitrepos` by default.
- **Export playlists.** Save a collection's filters and sort order as a playlist. Connect
  Spotify or YouTube from the toolbar to export it and keep the export up to date while
  Stereo is open. YouTube uses a browser login on the machine running Stereo.
- **Customize.** Use Settings to adjust the artwork and disco ball, or enable speaker
  animations that follow the beat.

If a search provider fails, run _Health check_ from the toolbar. 

## On your phone

Run `stereo --tailscale` and scan the QR code from the Tailscale button. This requires
the `tailscale` CLI and a tailnet with
[MagicDNS](https://tailscale.com/docs/features/magicdns) and
[HTTPS certificates](https://tailscale.com/docs/how-to/set-up-https-certificates) enabled.

## About

Stereo is a personal, non-commercial project for local use. It has no affiliation with
the services it uses. Unofficial integrations can break or be rate-limited. Follow the
services' terms and applicable law. See [LICENSE](LICENSE) for warranty terms.

## Development

For build and test commands, see [AGENTS.md](AGENTS.md#commands).
