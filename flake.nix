# flake.nix — reproducible builds for the txtodo workspace via crane (https://crane.dev/).
#
# Toolchain: pinned to rust-toolchain.toml's 1.95.0 through fenix (deploy/nix/overlay.nix), never
# nixpkgs' own `rustc` package — so `nix build` and a contributor's plain `cargo build` share
# exactly one compiler, never two silently-different ones.
#
# Packages: `txtodo` (crates/txtodo-cli), `txtodod` (crates/txtodo-daemon), `txtodo-tui`
# (crates/txtodo-tui) — package names follow each crate's [[bin]] name in Cargo.toml, not the
# crate directory name. Split into deploy/nix/*.nix per plan/design §12 (overlay/package/devShell
# modules), so this file only wires them together.
#
# Nix flakes reference: https://nixos.org/manual/nix/stable/command-ref/new-cli/nix3-flake.html
{
  description = "txtodo: Nix flake for reproducible builds, cross-compilation and dev shell (plan M10)";

  inputs = {
    # Pinned channel. Needs a default rustc >= 1.85 (edition2024) to build crane's own internal
    # `crane-utils` helper — nixos-24.11's default rustc (1.82) fails that build (verified locally:
    # "feature `edition2024` is required" from crane-utils' hashbrown dependency), so this tracks
    # 25.05 instead. https://github.com/NixOS/nixpkgs/tree/nixos-25.05
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    # crane: cargo workspaces as Nix derivations, with a separate dep-only caching pass.
    # https://crane.dev/
    crane.url = "github:ipetkov/crane";
    # fenix: exact upstream rustc/cargo pins (tracks rust-toolchain.toml exactly; nixpkgs' own
    # `rustc` follows its own release cadence and would drift). https://github.com/nix-community/fenix
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # flake-utils: eachDefaultSystem, the standard per-system flake boilerplate.
    # https://github.com/numtide/flake-utils
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, crane, fenix, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import ./deploy/nix/overlay.nix { inherit fenix; }) ];
        };

        # (crane.mkLib pkgs).overrideToolchain: https://crane.dev/API.html#craneliboverridetoolchain
        craneLib = (crane.mkLib pkgs).overrideToolchain pkgs.txtodoRustToolchain;

        packages = import ./deploy/nix/packages.nix { inherit pkgs craneLib; };
      in
      {
        packages = packages // {
          default = packages.txtodo;
        };

        devShells.default = import ./deploy/nix/devshell.nix { inherit pkgs; };

        # `nix flake check` builds every release package — a cheap smoke test that the flake
        # itself (not just cargo) still produces all three binaries.
        checks = packages;
      });
}
