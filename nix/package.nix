{
  lib,
  stdenv,
  fetchurl,
  autoPatchelfHook,
}:

let
  version = "0.1.81";

  platforms = {
    "aarch64-darwin" = "macos-aarch64";
    "x86_64-darwin" = "macos-x86_64";
    "aarch64-linux" = "linux-aarch64";
    "x86_64-linux" = "linux-x86_64";
  };

  hashes = {
    "aarch64-darwin" = "sha256-vjneljP5SrFF0Ey/edg1FWOEAr/Et7k957wJbmtd/gA=";
    "x86_64-darwin" = "sha256-x2nH8hoBvxQ2gQPFb3F3SII2eE7BSgYPIjubVTyAhlU=";
    "aarch64-linux" = "sha256-XUr3oK1dnDyF0i0LbfUWnvo9R3mg6Rz7HEC+meva7GM=";
    "x86_64-linux" = "sha256-cWDtUDETrQPsN/2XgLqch//1HK18EXvqiD/oWJBivTc=";
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
