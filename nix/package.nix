{
  lib,
  stdenv,
  fetchurl,
  autoPatchelfHook,
}:

let
  version = "0.1.80";

  platforms = {
    "aarch64-darwin" = "macos-aarch64";
    "x86_64-darwin" = "macos-x86_64";
    "aarch64-linux" = "linux-aarch64";
    "x86_64-linux" = "linux-x86_64";
  };

  hashes = {
    "aarch64-darwin" = "sha256-ic4VwQfZq5/FOe/arhYnKEv0jLGsFhWIbw9lBuL3Wt0=";
    "x86_64-darwin" = "sha256-9LXv2z0DLP0PyVe96Ccshho3s7jBtYAjyzMzctkAZUU=";
    "aarch64-linux" = "sha256-qu0ziI+GyXmhlZRgnJtnxHO2IZjXEfKIAr32uF+4I+0=";
    "x86_64-linux" = "sha256-A79xZS+V1pHzFnuU6Nq9POJ+CZsDR1UJZqSiabl+Jc4=";
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
