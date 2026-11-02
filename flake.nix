{
  description = "SDRmm software-defined radio receiver";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      nixpkgs,
      rust-overlay,
      ...
    }:
    let
      systems = [
        "aarch64-linux"
        "x86_64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          toolchainConfig = builtins.fromTOML (builtins.readFile ./rust-toolchain.toml);
          toolchain = pkgs.rust-bin.stable.${toolchainConfig.toolchain.channel}.minimal;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = toolchain;
            rustc = toolchain;
          };
          sdrmmDesktop = pkgs.callPackage ./packaging/nix/package.nix {
            inherit rustPlatform;
          };
        in
        {
          default = sdrmmDesktop;
          sdrmm = sdrmmDesktop;
          sdrmm-desktop = sdrmmDesktop;
        }
      );
    };
}
