# deploy/nix/overlay.nix — nixpkgs overlay adding `txtodoRustToolchain`: the exact rustc/cargo
# pinned by ../../rust-toolchain.toml, sourced from fenix so a `nix build` and a contributor's
# plain `cargo build` share one toolchain (never nixpkgs' own, differently-versioned `rustc`).
#
# fenix fromToolchainFile: https://github.com/nix-community/fenix#fromtoolchainfile--attrs---derivation
# nixpkgs overlays: https://nixos.org/manual/nixpkgs/stable/#chap-overlays
{ fenix }:
final: _prev: {
  txtodoRustToolchain = fenix.packages.${final.system}.fromToolchainFile {
    file = ../../rust-toolchain.toml;
    # SHA-256 of the rustup channel manifest for 1.95.0 — a fixed-output derivation, required in
    # pure evaluation mode. To re-pin after rust-toolchain.toml changes: set this to
    # `final.lib.fakeSha256`, run `nix build`, and paste the "got:" hash from the error back in.
    sha256 = "sha256-gh/xTkxKHL4eiRXzWv8KP7vfjSk61Iq48x47BEDFgfk=";
  };
}
