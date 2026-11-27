{
  description = "A music curation app for browsers and desktop";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs =
    { self, nixpkgs }:
    let
      # nixpkgs-unstable has dropped x86_64-darwin; the packages still support it.
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (system: {
        default = nixpkgs.legacyPackages.${system}.callPackage ./nix/package.nix { };
        desktop = nixpkgs.legacyPackages.${system}.callPackage ./nix/desktop-package.nix { };
      });

      homeManagerModules = {
        stereo = ./nix/home-manager.nix;
        default = ./nix/home-manager.nix;
      };
    };
}
