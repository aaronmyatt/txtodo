# Nix flake, cargo-zigbuild cross builds, Sigstore signing, SBOM (plan M10, design §11/§12)

## Goal

Reproducible Nix builds, `cargo-zigbuild` cross-compilation, Sigstore-signed releases, and an SBOM.
Design §11: "Nix flake for reproducible builds; `cargo-zigbuild` for cross-compilation; signed releases
via Sigstore; SBOM. CI matrix: macOS, Linux (glibc + musl), Windows, iOS simulator, Android emulator,
WASM." Design §12 places the nix files under `deploy/nix`.

## Design

### Nix flake (reproducible builds + dev shell)

```nix
# flake.nix — crane over the workspace; flake.lock pins nixpkgs + the 1.95 toolchain
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.11";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
  };
  outputs = { self, nixpkgs, crane, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system: {
      packages = {
        txtodo     = craneLib.buildPackage { src = craneLib.cleanCargoSource ./.; pname = "txtodo"; };
        txtodod    = ...;  # same crateSrc, bin = txtodod
        txtodo-tui = ...;
      };
      devShells.default = pkgs.mkShell { buildInputs = [ pkgs.cargo pkgs.rustc pkgs.zig
        pkgs.cosign pkgs.cargo-cyclonedx pkgs.cargo-zigbuild ]; };  # toolchain via fenix/rustup
    });
}
```

- `flake.nix` + `flake.lock` at the repo root; `deploy/nix/` holds the overlay/package split. Pins make
  the build byte-reproducible; the dev shell carries the exact toolchain (rust 1.95, zig, cosign,
  cargo-cyclonedx, cargo-zigbuild).
- Toolchain: honour the existing `rust-toolchain.toml` via `fenix` (or rustup in the shell) so the
  Nix build and the normal `cargo` build use the same 1.95 — never two toolchains.

### Cross builds (cargo-zigbuild)

```
cargo zigbuild --release --target x86_64-unknown-linux-gnu      # glibc
cargo zigbuild --release --target x86_64-unknown-linux-musl     # static musl
cargo zigbuild --release --target aarch64-unknown-linux-musl    # static musl, arm64
cargo zigbuild --release --target x86_64-pc-windows-gnu         # Windows (zig linker, no MSVC)
```

- `cargo-zigbuild` uses zig as the C toolchain/linker, so musl and windows-gnu targets build on a
  Linux CI runner with no per-target sysroots. `.cargo/config.toml` records the zig linker per target
  (new file — `.cargo/` is not frozen). macOS native, iOS-simulator, Android-emulator and WASM targets
  round out the design §11 matrix; WASM = `txtodo-ffi` wasm-bindgen build (already M7).

### Sigstore signing + SBOM

```
cargo cyclonedx --all --format json --output bom.json      # SBOM, CycloneDX JSON
cosign sign-blob --yes --bundle txtodo.bundle \
  --output-signature txtodo-x86_64-linux-musl.sig txtodo-x86_64-linux-musl
cosign verify-blob --bundle txtodo.bundle txtodo-x86_64-linux-musl
```

- SBOM via `cargo-cyclonedx` (CycloneDX JSON: full dependency graph + licenses) — one `bom.json`
  per release, committed as a release asset, itself signed.
- Sigstore keyless signing (`cosign sign-blob`) publishes the signature + certificate to the
  transparency log; `cosign verify-blob` is the release-consumer check. GitHub OIDC is the keyless
  identity (the release workflow acts as the signer).

### Release workflow (`.github/workflows/release.yml`)

On a `v*` tag: build matrix (macos native, linux glibc, linux musl, windows-gnu, wasm) via
`cargo-zigbuild` → generate `bom.json` → `cosign sign-blob` each artifact → `gh release create` with
the binaries, SBOM, and `.sig`/bundle files. Nix check (relaxed 2026-09-20, see Decision below): the Nix build of the same tag must succeed and
run `--version` before the release is published.

## Placement/dependencies

- `flake.nix`, `flake.lock`, `deploy/nix/` — new, none frozen (the frozen list covers manifests and
  `.github`, not a new flake). `.github/workflows/release.yml` and the ci.yml matrix change are FROZEN
  (ask). `.cargo/config.toml` for the zig linker is new and not frozen.
- No new Rust dependency (all tooling: nix, crane, cargo-zigbuild, cosign, cargo-cyclonedx). Each
  still needs human sign-off; `cargo deny` is unaffected (no Cargo.toml change).

## Edge cases & invariants

- musl static must actually be static: `file` + `ldd` (absent) assertions in the release job so a
  "static" artifact never ships with glibc linkage (a known zigbuild footgun with C deps — the daemon
  links rusqlite's bundled sqlite, so verify it).
- Keyless signing depends on GitHub OIDC availability: the release job requests the OIDC token
  (`id-token: write`); a fork push to a tag without the permission fails the job loudly, not silently.
- SBOM completeness: the release `bom.json` lists runtime deps only (dev-deps excluded); assert it
  lists every workspace crate and license.
- Invariant: the release is a tag-triggered, append-only event — nothing rewrites a published asset
  or its signature (immutable releases).

## Acceptance

- `nix develop` yields the pinned 1.95 toolchain + zig/cosign/cargo-cyclonedx; `nix build .#txtodo`
  succeeds and the artifact hash is stable across two clean builds (reproducibility).
- The release matrix builds linux glibc, linux musl (verified static via `ldd` absent), windows-gnu,
  macos native, and wasm; each binary runs `--version`.
- `bom.json` (CycloneDX) lists every workspace crate and its license; `cargo cyclonedx` exits 0.
- `cosign verify-blob` succeeds for every released artifact against the bundle produced at sign time.
- A `v0.1.0` tag publishes a GitHub Release carrying the binaries, `bom.json`, and the `.sig`/bundle
  files; re-tagging the same version is rejected (immutable).
- The Nix build of the same tag succeeds and its binary runs `--version`. No hash match is required.

## Frozen paths touched

- `.github/**` — new `release.yml` and the ci.yml matrix additions — frozen, ask.
- `justfile` — fill the existing `release` recipe (plan §2 lists it) — frozen, ask.
- No `Cargo.toml`/`Cargo.lock` change: tooling is external, not workspace deps.

## References

- plan M10 + §1 decision 10 (txtodo-implementation-plan.md), design §11/§12 (txtodo-design.md)
- Nix flakes: https://nixos.org/manual/nix/stable/command-ref/new-cli/nix3-flake · crane: https://crane.dev/
- cargo-zigbuild: https://github.com/rust-cross/cargo-zigbuild · CycloneDX: https://cyclonedx.org/
- Sigstore / cosign: https://docs.sigstore.dev/ · cosign sign-blob: https://github.com/sigstore/cosign

## Decision 2026-09-20: relax the Nix check (option B)

- The gate that the Nix build hash-matches the zigbuilt Linux glibc artifact is dropped. The check
  is now: `nix build .#txtodo` succeeds and the result runs `--version`.
- Why: the two builds use different C toolchains (Nix's gcc and ld, zig's linker). They are not
  guaranteed byte-identical even from the same rustc, and may never match.
- What stays: the two-clean-builds determinism check inside Nix already passed (both builds hashed
  9fbce3b483...ba56eb6). That is evidence the flake is reproducible; it is not a release gate.
- What is left: rewrite the job in `RELEASE_CI.patch.md`, a human applies it to `release.yml`
  (`.github/**` is frozen), then the test-tag dry run (full asset set, duplicate tag rejected).
