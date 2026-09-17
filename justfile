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
    # Same zig target as rust_target here (not a typo left in): windows-gnu has no
    # glibc-version-suffix convention, and "x86_64-windows-gnu" alone isn't a real target triple.
    build x86_64-pc-windows-gnu x86_64-pc-windows-gnu txtodo-tui
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
