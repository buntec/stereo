{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.stereo;
  svc = cfg.service;
  isLinux = pkgs.stdenv.hostPlatform.isLinux;
  isDarwin = pkgs.stdenv.hostPlatform.isDarwin;
  supportedPackageSystems = [
    "aarch64-darwin"
    "x86_64-darwin"
    "aarch64-linux"
    "x86_64-linux"
  ];
  supported = builtins.elem pkgs.stdenv.hostPlatform.system supportedPackageSystems;

  isLoopback =
    host:
    builtins.elem host [
      "localhost"
      "127.0.0.1"
      "::1"
      "[::1]"
    ];

  # Reads the token when the service starts, so that it stays out of the Nix store, the plist or
  # unit, and `ps`.
  tokenWrapper = pkgs.writeShellScript "stereo-service" ''
    STEREO_WS_TOKEN=$(< ${lib.escapeShellArg (toString svc.wsTokenFile)})
    export STEREO_WS_TOKEN
    exec "$@"
  '';

  # The port and data directory go in the environment (`location`) instead.
  programArguments =
    lib.optional (svc.wsTokenFile != null) "${tokenWrapper}"
    ++ [
      (lib.getExe cfg.package)
      "--host"
      svc.host
    ]
    ++ lib.optionals (svc.cacheDir != null) [
      "--cache-dir"
      (toString svc.cacheDir)
    ]
    ++ lib.optionals (svc.audioCacheLimitGb != null) [
      "--audio-cache-limit-gb"
      (toString svc.audioCacheLimitGb)
    ]
    ++ lib.optionals (cfg.ytDlpPackage != null) [
      "--yt-dlp"
      (lib.getExe cfg.ytDlpPackage)
    ]
    ++ lib.concatMap (origin: [
      "--allowed-origin"
      origin
    ]) svc.allowedOrigins
    ++ lib.optional svc.tailscale "--tailscale"
    ++ lib.optional (svc.verbosity > 0) "-${lib.concatStrings (lib.replicate svc.verbosity "v")}"
    ++ svc.extraArgs;

  # The service's port and data directory, as the CLI also reads them. The desktop app needs them
  # to find the service: from its own environment (`desktopPackage`), and on macOS, where apps
  # opened from the Finder don't get one, from the launchd plist's, since Home Manager wraps the
  # plist's `ProgramArguments` in a script.
  location = {
    STEREO_PORT = toString svc.port;
  }
  // lib.optionalAttrs (svc.home != null) { STEREO_HOME = toString svc.home; };

  serviceEnvironment = {
    HOME = config.home.homeDirectory;
    # Stereo runs git for collection sync, yt-dlp (unless `ytDlpPackage` is set) and tailscale.
    # The user's profiles come first; Nix's git is the fallback.
    PATH = lib.concatStringsSep ":" (
      lib.optional isLinux "/run/wrappers/bin"
      ++ [
        "${config.home.profileDirectory}/bin"
        "/etc/profiles/per-user/${config.home.username}/bin"
        "/run/current-system/sw/bin"
        "/nix/var/nix/profiles/default/bin"
        (lib.makeBinPath [ pkgs.git ])
        "/usr/local/bin"
        "/opt/homebrew/bin"
        "/usr/bin"
        "/bin"
        "/usr/sbin"
        "/sbin"
      ]
    );
  }
  // location
  // svc.environment;

  # The desktop app with the service's location, when that isn't the default one it looks at.
  desktopPackage =
    if !svc.enable || (svc.port == 8005 && svc.home == null) then
      cfg.desktop.package
    else
      pkgs.symlinkJoin {
        name = "stereo-desktop-${cfg.desktop.package.version or "wrapped"}";
        paths = [ cfg.desktop.package ];
        nativeBuildInputs = [ pkgs.makeWrapper ];
        postBuild = ''
          wrapProgram "$out/bin/stereo-desktop" ${
            lib.concatStringsSep " " (
              lib.mapAttrsToList (name: value: "--set-default ${name} ${lib.escapeShellArg value}") location
            )
          }
          entry="$out/share/applications/Stereo.desktop"
          if [[ -L "$entry" ]]; then
            target=$(readlink "$entry")
            rm "$entry"
            sed "s|^Exec=.*|Exec=$out/bin/stereo-desktop|" "$target" > "$entry"
          fi
        '';
        meta = cfg.desktop.package.meta or { };
      };

  # systemd's own quoting: double quotes with C escapes, and `%` (specifiers) doubled; `$` too in
  # command lines, where it would expand a variable.
  systemdQuote = s: ''"${lib.replaceStrings [ "\\" "\"" "\n" "%" ] [ "\\\\" "\\\"" "\\n" "%%" ] s}"'';
  systemdExecArgs =
    args:
    lib.concatMapStringsSep " " (arg: systemdQuote (lib.replaceStrings [ "$" ] [ "$$" ] arg)) args;

  hasToken = svc.wsTokenFile != null || svc.environment ? STEREO_WS_TOKEN;
in
{
  # Stereo is both a program and a long-running server: accept the service-shaped spelling too.
  imports = [
    (lib.mkAliasOptionModule [ "services" "stereo" ] [ "programs" "stereo" ])
  ];

  options.programs.stereo = {
    enable = lib.mkEnableOption "Stereo, a music curation app";

    package = lib.mkOption {
      type = with lib.types; nullOr package;
      default =
        if builtins.hasAttr "stereo" pkgs then
          pkgs.stereo
        else if supported then
          pkgs.callPackage ./package.nix { }
        else
          null;
      defaultText = lib.literalExpression "pkgs.stereo or ./nix/package.nix";
      example = lib.literalExpression "inputs.stereo.packages.\${pkgs.system}.default";
      description = ''
        The Stereo package to install and run. Set to `null` to manage Stereo outside Home
        Manager; this requires `service.enable = false`.
      '';
    };

    ytDlpPackage = lib.mkOption {
      type = with lib.types; nullOr package;
      default = pkgs.yt-dlp;
      defaultText = lib.literalExpression "pkgs.yt-dlp";
      description = ''
        yt-dlp, which the service downloads audio for playback with (`--yt-dlp`). YouTube changes
        often break older versions; set to `null` to use the `yt-dlp` on the service's `PATH`
        instead, e.g. one kept up to date with `yt-dlp -U` or Homebrew.
      '';
    };

    desktop = {
      enable = lib.mkEnableOption "the Stereo desktop app with its bundled backend";

      package = lib.mkOption {
        type = with lib.types; nullOr package;
        default = if supported then pkgs.callPackage ./desktop-package.nix { } else null;
        defaultText = lib.literalExpression "pkgs.callPackage ./nix/desktop-package.nix { }";
        example = lib.literalExpression "inputs.stereo.packages.\${pkgs.system}.desktop";
        description = ''
          Desktop app package to install when `desktop.enable` is set. The app opens the
          service's backend while the service runs.
        '';
      };
    };

    service = {
      enable = lib.mkEnableOption "the Stereo background service" // {
        default = true;
      };

      host = lib.mkOption {
        type = lib.types.str;
        default = "localhost";
        example = "0.0.0.0";
        description = ''
          Address on which Stereo listens. Any but a loopback address requires `wsTokenFile`.
        '';
      };

      port = lib.mkOption {
        type = lib.types.port;
        default = 8005;
        description = "TCP port on which Stereo listens.";
      };

      home = lib.mkOption {
        type = with lib.types; nullOr (either str path);
        default = null;
        example = "/Volumes/Music/stereo";
        description = ''
          Data directory, holding the collections' Git repositories. `null` keeps Stereo's
          default, {file}`~/.local/share/stereo`, which the desktop app and the CLI use too.
        '';
      };

      cacheDir = lib.mkOption {
        type = with lib.types; nullOr (either str path);
        default = null;
        description = ''
          Directory for cached data such as audio downloads. `null` keeps Stereo's default:
          {file}`~/Library/Caches/stereo` on macOS, {file}`$XDG_CACHE_HOME/stereo` elsewhere.
        '';
      };

      audioCacheLimitGb = lib.mkOption {
        type = with lib.types; nullOr ints.positive;
        default = null;
        example = 50;
        description = "Size limit of the audio cache in GB. `null` keeps Stereo's default (10).";
      };

      allowedOrigins = lib.mkOption {
        type = with lib.types; listOf str;
        default = [ ];
        example = [ "https://music.example" ];
        description = ''
          Additional browser origins allowed to open the WebSocket. A remote origin requires
          `wsTokenFile`.
        '';
      };

      wsTokenFile = lib.mkOption {
        type = with lib.types; nullOr (either str path);
        default = null;
        example = "/run/agenix/stereo-ws-token";
        description = ''
          File holding the WebSocket token (`--ws-token`), read when the service starts. Required
          when listening on a non-loopback address. Use a path outside the Nix store, such as a
          secret managed by agenix or sops-nix, since the store is world-readable. The desktop
          app can't read the token from the plist then, and asks for it once.
        '';
      };

      tailscale = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = ''
          Also serve Stereo over HTTPS on the tailnet with `tailscale serve`, on the same port.
          Requires the `tailscale` CLI, MagicDNS and HTTPS certificates for the tailnet.
        '';
      };

      verbosity = lib.mkOption {
        type = lib.types.ints.between 0 3;
        default = 0;
        description = "Log level: 0 for warnings, 1 for info, 2 for debug, 3 for trace.";
      };

      extraArgs = lib.mkOption {
        type = with lib.types; listOf str;
        default = [ ];
        description = "Additional command-line arguments passed to Stereo.";
      };

      environment = lib.mkOption {
        type = with lib.types; attrsOf str;
        default = { };
        example = {
          RUST_BACKTRACE = "1";
        };
        description = ''
          Environment variables for the service. `HOME` and `PATH` are set to match Home
          Manager and can be overridden here.
        '';
      };
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = !cfg.desktop.enable || cfg.desktop.package != null;
        message = "programs.stereo.desktop.package must be set on unsupported platforms";
      }
      {
        assertion = !svc.enable || cfg.package != null;
        message = "programs.stereo.package must be set when programs.stereo.service.enable is true";
      }
      {
        assertion = !svc.enable || isLinux || isDarwin;
        message = "programs.stereo.service is supported only on Linux and Darwin";
      }
      {
        assertion = !svc.enable || isLoopback svc.host || hasToken;
        message = "programs.stereo.service.wsTokenFile is required for a non-loopback host";
      }
      {
        assertion = !svc.enable || !svc.tailscale || isLoopback svc.host;
        message = "programs.stereo.service.tailscale requires a loopback host";
      }
    ];

    home.packages =
      lib.optional (cfg.package != null) cfg.package
      ++ lib.optional (cfg.desktop.enable && cfg.desktop.package != null) desktopPackage;

    # A new package or definition changes the unit, which Home Manager then restarts.
    systemd.user.services.stereo = lib.mkIf (svc.enable && isLinux) {
      Unit = {
        Description = "Stereo music curation app";
        After = [ "network.target" ];
      };

      Service = {
        ExecStart = systemdExecArgs programArguments;
        Environment = lib.mapAttrsToList (name: value: systemdQuote "${name}=${value}") serviceEnvironment;
        Restart = "on-failure";
        # Leaves time to remove the `tailscale serve` route on shutdown.
        TimeoutStopSec = 30;
      };

      Install.WantedBy = [ "default.target" ];
    };

    # Like `stereo install`. Home Manager reloads the agent when the plist changes, as it does for
    # a new package.
    launchd.agents.stereo = lib.mkIf (svc.enable && isDarwin) {
      enable = true;
      config = {
        # Same label and so plist path as `stereo install`: `stereo restart` works, and the desktop
        # app finds the service's port and token.
        Label = "com.stereo";
        ProgramArguments = programArguments;
        EnvironmentVariables = serviceEnvironment;
        RunAtLoad = true;
        KeepAlive = true;
        StandardOutPath = "${config.home.homeDirectory}/Library/Logs/stereo.log";
        StandardErrorPath = "${config.home.homeDirectory}/Library/Logs/stereo.log";
      };
    };
  };
}
