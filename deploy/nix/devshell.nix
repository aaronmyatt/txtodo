# deploy/nix/devshell.nix — `nix develop`: the pinned 1.95.0 toolchain (rustc/cargo/rustfmt/
# clippy, matching rust-toolchain.toml via deploy/nix/overlay.nix) plus the release tooling from
# tasks/release-engineering/notes.md: zig (linker for cargo-zigbuild's cross targets), cosign
# (Sigstore keyless signing), cargo-cyclonedx (SBOM), and cargo-zigbuild itself.
#
# mkShell: https://nixos.org/manual/nixpkgs/stable/#sec-pkgs-mkShell
{ pkgs }:
pkgs.mkShell {
  name = "txtodo-dev";

  buildInputs = [
    pkgs.txtodoRustToolchain
    # https://github.com/rust-cross/cargo-zigbuild
    pkgs.cargo-zigbuild
    # https://ziglang.org/ — the C toolchain/linker cargo-zigbuild drives per target
    pkgs.zig
    # https://docs.sigstore.dev/ — cosign sign-blob / verify-blob
    pkgs.cosign
    # https://github.com/CycloneDX/cyclonedx-rust-cargo — `cargo cyclonedx` SBOM generation
    pkgs.cargo-cyclonedx
  ];
}
