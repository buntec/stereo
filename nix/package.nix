{
  lib,
  stdenv,
  fetchurl,
  autoPatchelfHook,
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
    "aarch64-darwin" = "sha256-OdaetOP+Cg6WGvhNWwp3FBmJJhox2KfAsFducf7gdF8=";
    "x86_64-darwin" = "sha256-B14epagSPfNpA5finV2ZC35xXt0D6d2Y8A0kQ4SxY4c=";
    "aarch64-linux" = "sha256-OOUkPCYqvvpgeClYWPl0jgVRV18iSW+h47fBslb0BeA=";
    "x86_64-linux" = "sha256-USUN/N1mp3x39V78gq8KablYsSkS4Y9bfsSeW8ZKfk0=";
  };

  system = stdenv.hostPlatform.system;
  platform = platforms.${system} or (throw "stereo is not available on ${system}");
in
stdenv.mkDerivation {
  pname = "stereo";
  inherit version;

  # The release binary, with the frontend embedded (building it needs Bun and network access).
  src = fetchurl {
    url = "https://github.com/buntec/stereo/releases/download/v${version}/stereo-${platform}";
    hash = hashes.${system};
  };

  dontUnpack = true;
  dontConfigure = true;
  dontBuild = true;
  # The macOS binary is signed and notarized.
  dontStrip = true;

  nativeBuildInputs = lib.optional stdenv.hostPlatform.isLinux autoPatchelfHook;
  buildInputs = lib.optional stdenv.hostPlatform.isLinux stdenv.cc.cc.lib;

  installPhase = ''
    runHook preInstall
    install -Dm755 "$src" "$out/bin/stereo"
    runHook postInstall
  '';

  meta = {
    description = "Music curation app";
    homepage = "https://github.com/buntec/stereo";
    license = lib.licenses.mit;
    sourceProvenance = [ lib.sourceTypes.binaryNativeCode ];
    mainProgram = "stereo";
    platforms = lib.attrNames platforms;
  };
}
