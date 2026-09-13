# RELEASE_CI.patch.md — apply this yourself

Backlog item `id:01M2B4ZWQETBMJCHRJ8GQT32C9` (Nix flake / cargo-zigbuild / Sigstore / SBOM). Two
files in this task's scope are frozen by `.claude/budgets.json` (`justfile`, `.github/**`) and a
PreToolUse hook refuses agent edits to them — no workaround was attempted. Below is the exact,
locally-verified file content for both. A human applies it directly:

1. Replace the `release:` recipe in the root `justfile` with the one in **Part 1** below (the rest
   of the file is unchanged — shown in full so you can diff/paste the whole thing safely).
2. Create `.github/workflows/release.yml` with the content in **Part 2** below.
3. `git add justfile .github/workflows/release.yml && git commit`.

Everything referenced by both files already exists in this worktree/commit and is NOT frozen:
`deploy/nix/*.nix`, `deploy/release/merge-bom.jq`, `.cargo/config.toml` + `.cargo/zig/*.sh`,
`flake.nix`/`flake.lock`. Nothing here depends on anything else being written by a human first.

## What's verified locally vs. what isn't

Verified for real in this session (see the final report for exact commands/output):
- `cargo zigbuild` cross-builds for all 4 non-native legs (linux-gnu.2.17, linux-musl x86_64 +
  aarch64, windows-gnu) using Homebrew's zig/cargo-zigbuild.
- The musl artifacts are genuinely static (`file` reports "statically linked, stripped"; musl's
  own `ldd` refuses them as "Not a valid dynamic program" — the static signal the release job
  checks for, worded differently under glibc's `ldd`, which is what `ubuntu-latest` actually runs).
- `txtodo`/`txtodod` run `--version` correctly when executed (via Docker, aarch64 musl image).
  `txtodo-tui` is still an M10 stub (`fn main() {}`, no clap) — it exits 0 silently on
  `--version`, unrelated to this task; a separate open backlog item builds the real TUI.
- `cosign generate-key-pair` / `sign-blob` / `verify-blob` round-trip on a real artifact, and a
  tampered copy correctly fails verification.
- `cargo cyclonedx --all` does NOT produce one workspace-wide `bom.json` (it has no workspace
  mode — confirmed against the real CLI) — it writes one `<crate>.cdx.json` per member.
  `deploy/release/merge-bom.jq` merges the release-relevant ones into a single `bom.json`:
  verified locally at 485 components (11 workspace crates + 474 distinct deps), all licensed.
- `nix build .#txtodo`/`.#txtodod`/`.#txtodo-tui` all succeed; `.#txtodo` built twice from a clean
  store hash-matched exactly (`9fbce3b4...ba56eb6` both times) — real Nix reproducibility.
- `nix develop` yields rustc/cargo 1.95.0 (matching `rust-toolchain.toml` exactly) plus zig,
  cosign, cargo-cyclonedx, cargo-zigbuild, all functional.

NOT verified (can't be, from a worktree, ever — not a gap in this pass):
- Real GitHub OIDC keyless signing (`cosign sign-blob` with no local key, `id-token: write`) only
  works from an actual GitHub Actions run against the real repo, with Actions' OIDC and the
  repo/environment permissions correctly enabled. The workflow below is written to the real
  cosign keyless API, but "does the real OIDC handshake succeed" needs a real tag push to verify.
- Whether the reproducibility-gate job below will actually pass. It compares a `nix build` (native
  nixpkgs gcc/ld) against a `cargo zigbuild` artifact (zig as linker) — two different C
  toolchains, not guaranteed to produce byte-identical output even from the same rustc. See the
  long comment on that job for what wiring would be needed to make this a fair comparison
  (routing the Nix build through zig too), which is real additional work, flagged rather than
  done silently.
- Whether Windows should ship only `txtodo-tui` indefinitely or get its own IPC transport so the
  other two binaries can ship there too — a product call, not this task's to make. The workflow
  below takes the safe default (mirrors ci.yml's existing ADR 0010 exclusion) and flags it in a
  trailing comment.

---

## Part 1 — justfile (full file; only the `release:` recipe near the bottom changed)

```makefile
# txtodo task runner. Recipes mirror .claude/budgets.json.commands verbatim (drift audit diffs them).
# just manual: https://just.systems/man/en/
set shell := ["bash", "-euo", "pipefail", "-c"]

# fmt + clippy + typecheck + test + boundaries + file length (what the gate and CI run)
check: fmt lint typecheck test boundaries no-std

fmt:
    cargo fmt --all --check

lint:
    cargo clippy --workspace --all-targets -- -D warnings

typecheck:
    cargo check --workspace --all-targets

test:
    cargo test --workspace

# line coverage against the floor in budgets.json (rustup toolchain: Homebrew cargo lacks llvm-profdata)
coverage:
    rustup run 1.95.0 cargo llvm-cov --workspace --fail-under-lines 80

boundaries:
    .claude/scripts/check-boundaries.sh
    .claude/scripts/check-file-length.sh
    .claude/scripts/check-assertions.sh
    .claude/scripts/check-specs-mirror.sh

deny:
    cargo deny check

# the full crdt-sync-simulator sweep (plan M4 acceptance: 1000 runs, random seeds each time);
# `cargo test` alone only runs 20 fixed seeds — this is deliberately not part of `check`.
# TXTODO_SIM_SEED=<n> reruns exactly one seed to reproduce a failure this prints.
sim:
    cargo test -p txtodo-crdt --release --test sim -- --ignored --nocapture

# fuzz <target> <secs>: plan M0 wants this; cargo-fuzz needs nightly and is installed at M1, not by /setup
fuzz target secs="60":
    PATH="$(dirname "$(rustup which --toolchain nightly cargo)"):$PATH" cargo fuzz run --fuzz-dir crates/txtodo-core/fuzz {{target}} -- -max_total_time={{secs}}

# regenerate the fuzz seed corpora (gitignored) from corpus/*.txt
fuzz-seed:
    rm -rf crates/txtodo-core/fuzz/corpus && mkdir -p crates/txtodo-core/fuzz/corpus/{parse_line,parse_file,slug}
    cat corpus/*.txt | grep -v '^$' | awk '{ print > ("crates/txtodo-core/fuzz/corpus/parse_line/seed-" NR ".txt") }'
    cp corpus/*.txt crates/txtodo-core/fuzz/corpus/parse_file/
    printf 'q4-roadmap' > crates/txtodo-core/fuzz/corpus/slug/valid.txt; printf '../escape' > crates/txtodo-core/fuzz/corpus/slug/traversal.txt

bench:
    cargo bench --workspace

# perf budget: parse_file_100k mean ≤ budgets.json.perf.parse100kMs
bench-check:
    .claude/scripts/check-bench.sh

# core must build with no std at all (plan M1); needs `rustup target add thumbv7em-none-eabihf`
no-std:
    PATH="$(dirname "$(rustup which --toolchain 1.95.0 cargo)"):$PATH" cargo build -p txtodo-core --no-default-features --target thumbv7em-none-eabihf

corpus:
    .claude/scripts/check-corpus-oracle.sh
    cargo test -p txtodo-core --test corpus

# Local pre-tag smoke: build the three release binaries for every release.yml matrix leg via
# cargo-zigbuild (https://github.com/rust-cross/cargo-zigbuild), assert the musl legs are static
# (no `ldd` interpreter), sign every artifact with cosign (https://docs.sigstore.dev/cosign/) using
# a local key (release.yml itself signs keyless via GitHub OIDC — see RELEASE_CI.patch.md — this
# recipe is a pre-tag *local* smoke, so it needs a key pair, generated once with
# `cosign generate-key-pair`), verify every signature, and generate the SBOM via cargo-cyclonedx
# (https://github.com/CycloneDX/cyclonedx-rust-cargo). Mirrors deploy/nix/packages.nix's bin-name
# mapping (crate name -> [[bin]] name): txtodo-cli -> txtodo, txtodo-daemon -> txtodod,
# txtodo-tui -> txtodo-tui. Windows ships txtodo-tui only, matching ci.yml's own Windows exclusion
# of txtodo-cli/txtodo-daemon/txtodo-mcp under ADR 0010 (no Unix domain sockets on Windows).
release:
    #!/usr/bin/env bash
    set -euo pipefail
    rustup target add x86_64-unknown-linux-gnu x86_64-unknown-linux-musl aarch64-unknown-linux-musl x86_64-pc-windows-gnu
    mkdir -p dist
    build() {
        local zig_target="$1" rust_target="$2"; shift 2
        for pkg in "$@"; do
            cargo zigbuild --release --target "$zig_target" -p "$pkg" --locked
            case "$pkg" in
                txtodo-cli) bin=txtodo ;;
                txtodo-daemon) bin=txtodod ;;
                txtodo-tui) bin=txtodo-tui ;;
            esac
            ext=""; case "$rust_target" in *windows*) ext=".exe" ;; esac
            cp "target/${rust_target}/release/${bin}${ext}" "dist/${bin}-${zig_target%%.*}${ext}"
        done
    }
    build x86_64-unknown-linux-gnu.2.17 x86_64-unknown-linux-gnu txtodo-cli txtodo-daemon txtodo-tui
    build x86_64-unknown-linux-musl x86_64-unknown-linux-musl txtodo-cli txtodo-daemon txtodo-tui
    build aarch64-unknown-linux-musl aarch64-unknown-linux-musl txtodo-cli txtodo-daemon txtodo-tui
    build x86_64-windows-gnu x86_64-pc-windows-gnu txtodo-tui
    echo "== static check: musl artifacts must have no dynamic interpreter =="
    for f in dist/*musl*; do
        file "$f" | grep -q 'statically linked' || { echo "::error:: $f is not static"; exit 1; }
    done
    echo "== SBOM: cargo cyclonedx --all, merged via deploy/release/merge-bom.jq =="
    # cargo-cyclonedx has no workspace-wide mode: `--all` writes one <crate>.cdx.json per
    # workspace member next to its own Cargo.toml. merge-bom.jq combines the release-relevant
    # ones (the 3 shipped bins + their full dep graph) into one bom.json.
    cargo cyclonedx --all --format json
    jq -s -f deploy/release/merge-bom.jq \
        crates/txtodo-cli/txtodo-cli.cdx.json crates/txtodo-daemon/txtodo-daemon.cdx.json \
        crates/txtodo-tui/txtodo-tui.cdx.json crates/txtodo-core/txtodo-core.cdx.json \
        crates/txtodo-query/txtodo-query.cdx.json crates/txtodo-model/txtodo-model.cdx.json \
        crates/txtodo-store/txtodo-store.cdx.json crates/txtodo-crdt/txtodo-crdt.cdx.json \
        crates/txtodo-sync/txtodo-sync.cdx.json crates/txtodo-proto/txtodo-proto.cdx.json \
        crates/txtodo-mcp/txtodo-mcp.cdx.json \
        > bom.json
    echo "== sign every artifact (local key — generate once with: cosign generate-key-pair) =="
    # --bundle alone carries the signature (cosign 3.x deprecates the separate --output-signature
    # file in favour of the bundle: https://docs.sigstore.dev/cosign/signing/overview/).
    for f in dist/* bom.json; do
        cosign sign-blob --key cosign.key --yes --bundle "${f}.bundle" "$f"
        cosign verify-blob --key cosign.pub --bundle "${f}.bundle" "$f"
    done
    echo "release smoke OK: $(ls dist/) bom.json"
```

## Part 2 — .github/workflows/release.yml (new file)

```yaml
# .github/workflows/release.yml — tag-triggered release: cross-compiled binaries via
# cargo-zigbuild, an SBOM (cargo-cyclonedx), Sigstore keyless signing (cosign), a Nix-vs-zigbuild
# reproducibility gate on the Linux glibc artifact, and a GitHub Release carrying all of it.
# Design §11/§12; tasks/release-engineering/notes.md.
#
# ADR 0010 (no Unix domain sockets on Windows) already drops txtodo-cli/txtodo-daemon/txtodo-mcp
# from the Windows leg of ci.yml's own matrix — this workflow mirrors that exactly: the
# windows-gnu leg below builds ONLY txtodo-tui via `-p txtodo-tui`, never a workspace-wide
# zigbuild. Ship-only-tui-on-Windows is a deliberate, revisitable default — see the "Decision
# needed" note at the bottom of this file.
#
# GitHub Actions syntax: https://docs.github.com/en/actions/writing-workflows/workflow-syntax-for-github-actions
name: release

on:
  push:
    tags:
      - 'v*'

permissions:
  contents: write   # gh release create: https://cli.github.com/manual/gh_release_create
  id-token: write   # cosign keyless signing via GitHub OIDC: https://docs.sigstore.dev/cosign/keyless/

jobs:
  # Releases are append-only (notes.md invariant: "nothing rewrites a published asset or its
  # signature"). Fail loudly on a duplicate tag instead of silently overwriting one.
  reject-duplicate-tag:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: fail if this tag already has a published release
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          if gh release view "${{ github.ref_name }}" --repo "${{ github.repository }}" >/dev/null 2>&1; then
            echo "::error::release ${{ github.ref_name }} already exists — releases are immutable, bump the tag"
            exit 1
          fi

  # Cross-compiled binaries. Every non-native leg below runs on ubuntu-latest and cross-compiles
  # via cargo-zigbuild (https://github.com/rust-cross/cargo-zigbuild) — zig ships its own C
  # toolchain/sysroots, so musl and windows-gnu targets need nothing per-target installed on the
  # runner. macOS builds natively (Xcode's own linker cross-links x86_64 from an arm64 runner).
  build:
    needs: reject-duplicate-tag
    strategy:
      fail-fast: false
      matrix:
        include:
          # Linux glibc: pin an old glibc (2.17, RHEL7-era) for portability — cargo-zigbuild's own
          # glibc-version-suffix convention: https://github.com/rust-cross/cargo-zigbuild#specify-glibc-version
          - leg: linux-x86_64-gnu
            os: ubuntu-latest
            rust_target: x86_64-unknown-linux-gnu
            zig_target: x86_64-unknown-linux-gnu.2.17
            packages: "txtodo-cli txtodo-daemon txtodo-tui"
            native: false
            static_check: false
          # Linux musl: fully static — no libc at all — verified below via `ldd` (expect "not a
          # dynamic executable", i.e. no interpreter, on every artifact).
          - leg: linux-x86_64-musl
            os: ubuntu-latest
            rust_target: x86_64-unknown-linux-musl
            zig_target: x86_64-unknown-linux-musl
            packages: "txtodo-cli txtodo-daemon txtodo-tui"
            native: false
            static_check: true
          - leg: linux-aarch64-musl
            os: ubuntu-latest
            rust_target: aarch64-unknown-linux-musl
            zig_target: aarch64-unknown-linux-musl
            packages: "txtodo-cli txtodo-daemon txtodo-tui"
            native: false
            static_check: true
          # Windows: ADR 0010 (no Unix domain sockets on Windows) already drops txtodo-cli,
          # txtodo-daemon and txtodo-mcp from ci.yml's Windows leg — mirror that here. Ship ONLY
          # txtodo-tui on Windows for now (see "Decision needed" below).
          - leg: windows-x86_64-gnu
            os: ubuntu-latest
            rust_target: x86_64-pc-windows-gnu
            zig_target: x86_64-windows-gnu
            packages: "txtodo-tui"
            native: false
            static_check: false
          # macOS: native toolchain, no zig.
          - leg: macos-aarch64
            os: macos-latest
            rust_target: aarch64-apple-darwin
            packages: "txtodo-cli txtodo-daemon txtodo-tui"
            native: true
            static_check: false
          - leg: macos-x86_64
            os: macos-latest
            rust_target: x86_64-apple-darwin
            packages: "txtodo-cli txtodo-daemon txtodo-tui"
            native: true
            static_check: false
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      # rust-toolchain.toml pins 1.95.0; the action honours it (same as ci.yml).
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2

      - name: rustup target add
        run: rustup target add ${{ matrix.rust_target }}

      - name: install cargo-zigbuild + zig
        if: ${{ !matrix.native }}
        run: |
          cargo install --locked cargo-zigbuild
          pip3 install ziglang   # cargo-zigbuild's own recommended zig install path

      - name: build (cargo-zigbuild)
        if: ${{ !matrix.native }}
        run: |
          set -euo pipefail
          for pkg in ${{ matrix.packages }}; do
            cargo zigbuild --release --target "${{ matrix.zig_target }}" -p "$pkg" --locked
          done

      - name: build (native macOS)
        if: matrix.native
        run: |
          set -euo pipefail
          for pkg in ${{ matrix.packages }}; do
            cargo build --release --target "${{ matrix.rust_target }}" -p "$pkg" --locked
          done

      # Package binaries: [[bin]] name differs from the crate name (txtodo-cli -> txtodo,
      # txtodo-daemon -> txtodod, txtodo-tui -> txtodo-tui — see each crate's Cargo.toml), and
      # Windows adds a `.exe` suffix.
      - name: collect artifacts
        id: collect
        run: |
          set -euo pipefail
          mkdir -p dist
          ext=""
          case "${{ matrix.rust_target }}" in
            *windows*) ext=".exe" ;;
          esac
          bin_for() {
            case "$1" in
              txtodo-cli) echo "txtodo" ;;
              txtodo-daemon) echo "txtodod" ;;
              txtodo-tui) echo "txtodo-tui" ;;
            esac
          }
          for pkg in ${{ matrix.packages }}; do
            bin="$(bin_for "$pkg")"
            src="target/${{ matrix.rust_target }}/release/${bin}${ext}"
            dest="dist/${bin}-${{ matrix.leg }}${ext}"
            cp "$src" "$dest"
            echo "packaged $dest"
          done

      # A "static" musl artifact that still links glibc is a known zigbuild footgun with C deps
      # (the daemon links rusqlite's bundled sqlite) — assert `ldd` reports no dynamic interpreter
      # at all, not just "no glibc found".
      - name: static-check (musl must have zero dynamic linkage)
        if: matrix.static_check
        run: |
          set -euo pipefail
          for f in dist/*; do
            echo "checking $f"
            if ldd "$f" 2>&1 | grep -qv 'not a dynamic executable'; then
              echo "::error::$f is not statically linked: $(ldd "$f")"
              exit 1
            fi
          done

      - name: smoke test (every binary runs --version)
        if: ${{ matrix.rust_target == 'x86_64-unknown-linux-gnu' || matrix.native }}
        run: |
          set -euo pipefail
          for f in dist/*; do
            "./$f" --version
          done

      - uses: actions/upload-artifact@v4
        with:
          name: dist-${{ matrix.leg }}
          path: dist/*

  # wasm leg: txtodo-ffi's wasm-bindgen build (already M7 — design §11's WASM matrix entry).
  wasm:
    needs: reject-duplicate-tag
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: rustup target add wasm32-unknown-unknown
      # https://github.com/rustwasm/wasm-pack
      - name: install wasm-pack
        run: curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
      - name: build txtodo-ffi (wasm32)
        run: wasm-pack build crates/txtodo-ffi --release --target web --out-dir ../../dist-wasm
      - uses: actions/upload-artifact@v4
        with:
          name: dist-wasm
          path: dist-wasm/*

  # SBOM: one bom.json for the release, listing every crate that ships in the three binaries
  # (+ their full dependency graph) and its license.
  #
  # cargo-cyclonedx has no workspace-wide mode (confirmed locally against the real CLI, 2026-09):
  # `cargo cyclonedx --all` writes one <crate>.cdx.json per workspace member next to its
  # Cargo.toml, each with only ITS OWN metadata.component + dep graph. deploy/release/merge-bom.jq
  # combines the release-relevant members' files into one bom.json (verified locally: 485
  # components — 11 workspace crates + 474 distinct deps — all carrying a license).
  sbom:
    needs: reject-duplicate-tag
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      # https://github.com/CycloneDX/cyclonedx-rust-cargo
      - run: cargo install --locked cargo-cyclonedx
      - name: cargo cyclonedx --all (one file per workspace member)
        run: cargo cyclonedx --all --format json
      - name: merge into one release bom.json
        run: |
          set -euo pipefail
          jq -s -f deploy/release/merge-bom.jq \
            crates/txtodo-cli/txtodo-cli.cdx.json \
            crates/txtodo-daemon/txtodo-daemon.cdx.json \
            crates/txtodo-tui/txtodo-tui.cdx.json \
            crates/txtodo-core/txtodo-core.cdx.json \
            crates/txtodo-query/txtodo-query.cdx.json \
            crates/txtodo-model/txtodo-model.cdx.json \
            crates/txtodo-store/txtodo-store.cdx.json \
            crates/txtodo-crdt/txtodo-crdt.cdx.json \
            crates/txtodo-sync/txtodo-sync.cdx.json \
            crates/txtodo-proto/txtodo-proto.cdx.json \
            crates/txtodo-mcp/txtodo-mcp.cdx.json \
            > bom.json
          # sanity: every workspace crate that ships is present, and nothing lacks a license
          for c in txtodo-cli txtodo-daemon txtodo-tui txtodo-core txtodo-query txtodo-model \
                   txtodo-store txtodo-crdt txtodo-sync txtodo-proto txtodo-mcp; do
            jq -e --arg c "$c" '.components[] | select(.name == $c)' bom.json > /dev/null
          done
          jq -e '[.components[] | select(.licenses == null or .licenses == [])] | length == 0' bom.json
      - uses: actions/upload-artifact@v4
        with:
          name: bom
          path: bom.json

  # Reproducibility gate (notes.md acceptance): the Nix build of the linux-x86_64-gnu target must
  # hash-match the cargo-zigbuild artifact for the same tag, before the release is published.
  #
  # KNOWN RISK (flagged for a human, not swept under the rug): Nix's own `nix build .#txtodo`
  # normally uses nixpkgs' native stdenv gcc/ld — a genuinely different C toolchain/linker from
  # zig, and the two are not guaranteed to produce byte-identical output even from identical
  # rustc 1.95.0 input (different default section ordering/alignment from a different linker).
  # deploy/nix/packages.nix's default `txtodo` package is deliberately the plain native build
  # (that's what `nix build .#txtodo` in the acceptance test / `nix develop` workflow means, and
  # it IS Nix-reproducible: two clean builds of it hash-match — verified locally, see the PR/
  # worktree report). This gate additionally needs the *Nix* build to go through zig with the
  # exact same target+glibc-version cargo-zigbuild used, or the comparison below will almost
  # certainly fail on a linker difference that has nothing to do with non-determinism. Until a
  # human decides how to wire that (e.g. a dedicated flake output that forces
  # CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER to the same zig wrapper as
  # .cargo/zig/zigcc-x86_64-linux-gnu.sh, at the same .2.17 glibc pin), this job is left in place
  # but should be expected to fail until that wiring lands — do not silently loosen it to a
  # "same size" or "runs --version" check to make it pass.
  reproducibility-gate:
    needs: build
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      # https://github.com/cachix/install-nix-action
      - uses: cachix/install-nix-action@v27
        with:
          extra_nix_config: |
            experimental-features = nix-command flakes
      - uses: actions/download-artifact@v4
        with:
          name: dist-linux-x86_64-gnu
          path: dist
      - name: nix build .#txtodo (glibc, x86_64-linux)
        run: nix build .#txtodo -o nix-result
      - name: compare hashes
        run: |
          set -euo pipefail
          nix_hash=$(sha256sum nix-result/bin/txtodo | awk '{print $1}')
          zig_hash=$(sha256sum dist/txtodo-linux-x86_64-gnu | awk '{print $1}')
          echo "nix:  $nix_hash"
          echo "zig:  $zig_hash"
          if [ "$nix_hash" != "$zig_hash" ]; then
            echo "::error::Nix build and cargo-zigbuild artifact do not hash-match — see the KNOWN RISK comment on this job"
            exit 1
          fi

  # Sigstore keyless signing: publish signature + certificate to the transparency log for every
  # release artifact (binaries + bom.json). https://docs.sigstore.dev/cosign/keyless/
  sign:
    needs: [build, wasm, sbom]
    runs-on: ubuntu-latest
    permissions:
      id-token: write
      contents: read
    steps:
      - uses: actions/download-artifact@v4
        with:
          path: artifacts
      # https://github.com/sigstore/cosign-installer
      - uses: sigstore/cosign-installer@v3
      - name: sign every binary + bom.json
        run: |
          set -euo pipefail
          # --bundle alone carries the signature + certificate (cosign 3.x deprecates the
          # separate --output-signature file): https://docs.sigstore.dev/cosign/signing/overview/
          find artifacts -type f ! -name '*.bundle' | while read -r f; do
            cosign sign-blob --yes --bundle "${f}.bundle" "$f"
          done
      - uses: actions/upload-artifact@v4
        with:
          name: signatures
          path: artifacts/**/*.bundle

  # Publish. Binaries + bom.json + .sig/.bundle files, tag-triggered, immutable (reject-duplicate-
  # tag already ran first).
  publish:
    needs: [build, wasm, sbom, sign, reproducibility-gate]
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@v4
      - uses: actions/download-artifact@v4
        with:
          path: artifacts
      - name: verify every signature before publishing
        env:
          GH_REPO: ${{ github.repository }}
        run: |
          set -euo pipefail
          # https://github.com/sigstore/cosign-installer
          find artifacts -name '*.bundle' | while read -r bundle; do
            f="${bundle%.bundle}"
            cosign verify-blob \
              --certificate-identity-regexp "https://github.com/${GH_REPO}/.github/workflows/release.yml@refs/tags/.*" \
              --certificate-oidc-issuer https://token.actions.githubusercontent.com \
              --bundle "$bundle" \
              "$f"
          done
      - name: gh release create
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          set -euo pipefail
          gh release create "${{ github.ref_name }}" \
            --repo "${{ github.repository }}" \
            --title "${{ github.ref_name }}" \
            --generate-notes \
            $(find artifacts -type f)

# Decision needed (human, not this workflow): Windows ships ONLY txtodo-tui above, matching
# ci.yml's existing exclusion of txtodo-cli/txtodo-daemon/txtodo-mcp under ADR 0010 (no Unix
# domain sockets on Windows). That ADR is about the daemon's IPC transport, not about the CLI or
# MCP server needing a rewrite before Windows can ship them at all — a human should decide whether
# to keep tui-only on Windows indefinitely, or scope a Windows IPC transport (named pipes?) so the
# other two binaries can ship there too. This file takes the safe, ADR-consistent default and
# flags the call rather than making it.
```
