{
  lib,
  stdenv,
  fetchurl,
  undmg,
  dpkg,
  autoPatchelfHook,
  wrapGAppsHook3,
  makeWrapper,
  webkitgtk_4_1,
  libsoup_3,
  gtk3,
  glib-networking,
  gst_all_1,
}:

let
  version = "0.1.83";

  platforms = {
    "aarch64-darwin" = "macos-aarch64";
    "x86_64-darwin" = "macos-x86_64";
    "aarch64-linux" = "linux-aarch64";
    "x86_64-linux" = "linux-x86_64";
  };

  hashes = {
    "aarch64-darwin" = "sha256-1v/eT1kJYG10waX4KEGJHe9zRKMVQ3yXZvx9Y+apQyc=";
    "x86_64-darwin" = "sha256-EQEvr7W1Mj6X90QO07e6mZc5ZUiyC/KvZGpqmWZbDSs=";
    "aarch64-linux" = "sha256-9Jnl37w3yAkaggpG9zc+L7XC3uL7le/4L4RIlC46zOA=";
    "x86_64-linux" = "sha256-vzTFyjHCA7WPDtWDB7bgc/Xsc4+pHiWfSPD/On6ZCBk=";
  };

  system = stdenv.hostPlatform.system;
  platform = platforms.${system} or (throw "the Stereo desktop app is not available on ${system}");
  extension = if stdenv.hostPlatform.isDarwin then "dmg" else "deb";
in
stdenv.mkDerivation {
  pname = "stereo-desktop";
  inherit version;

  src = fetchurl {
    url = "https://github.com/buntec/stereo/releases/download/v${version}/stereo-desktop-${platform}.${extension}";
    hash = hashes.${system};
  };

  nativeBuildInputs = [
    makeWrapper
  ]
  ++ lib.optionals stdenv.hostPlatform.isDarwin [ undmg ]
  ++ lib.optionals stdenv.hostPlatform.isLinux [
    dpkg
    autoPatchelfHook
    wrapGAppsHook3
  ];

  buildInputs = lib.optionals stdenv.hostPlatform.isLinux [
    stdenv.cc.cc.lib
    webkitgtk_4_1
    libsoup_3
    gtk3
    glib-networking
    # WebKitGTK plays audio with GStreamer, whose plugins it leaves to the system. Stereo's audio
    # is AAC in MP4 (`audio.rs`): qtdemux is in -good, the AAC decoder in -libav. wrapGAppsHook3
    # puts them on GST_PLUGIN_SYSTEM_PATH_1_0.
    gst_all_1.gstreamer
    gst_all_1.gst-plugins-base
    gst_all_1.gst-plugins-good
    gst_all_1.gst-libav
  ];

  sourceRoot = ".";
  unpackPhase = ''
    runHook preUnpack
    ${if stdenv.hostPlatform.isDarwin then ''undmg "$src"'' else ''dpkg-deb -x "$src" .''}
    runHook postUnpack
  '';
  dontConfigure = true;
  dontBuild = true;
  dontWrapGApps = true;
  # The macOS app is signed and notarized.
  dontStrip = stdenv.hostPlatform.isDarwin;

  installPhase = ''
    runHook preInstall
    mkdir -p "$out/bin"
  ''
  + (
    if stdenv.hostPlatform.isDarwin then
      ''
        mkdir -p "$out/Applications"
        cp -R Stereo.app "$out/Applications/"
        makeWrapper "$out/Applications/Stereo.app/Contents/MacOS/stereo-desktop" "$out/bin/stereo-desktop"
      ''
    else
      ''
        # The launcher looks for its bundled backend (the sidecar) beside itself.
        mkdir -p "$out/libexec/stereo-desktop"
        cp usr/bin/stereo usr/bin/stereo-desktop "$out/libexec/stereo-desktop/"
        cp -R usr/share "$out/share"
        substituteInPlace "$out/share/applications/Stereo.desktop" \
          --replace-fail 'Exec=stereo-desktop' "Exec=$out/bin/stereo-desktop"
      ''
  )
  + ''
    runHook postInstall
  '';

  preFixup = lib.optionalString stdenv.hostPlatform.isLinux ''
    makeWrapper "$out/libexec/stereo-desktop/stereo-desktop" "$out/bin/stereo-desktop" "''${gappsWrapperArgs[@]}"
  '';

  meta = {
    description = "Desktop app for Stereo with a bundled backend";
    homepage = "https://github.com/buntec/stereo";
    license = lib.licenses.mit;
    sourceProvenance = [ lib.sourceTypes.binaryNativeCode ];
    mainProgram = "stereo-desktop";
    platforms = lib.attrNames platforms;
  };
}
